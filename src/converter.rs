use crate::dxf;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
pub struct ConvertedDrawing {
    pub svg: String,
    pub pdf_svg: String,
    pub warnings: String,
    pub entity_count: usize,
}

pub fn convert_to_svg(input: &Path) -> Result<ConvertedDrawing, String> {
    if !input.exists() {
        return Err(format!("文件不存在：{}", input.display()));
    }

    let extension = input
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut warnings = String::new();
    let (dxf_text, _temp_dir) = if extension == "dxf" {
        (
            fs::read_to_string(input).map_err(|error| format!("无法读取 DXF：{error}"))?,
            None,
        )
    } else if extension == "dwg" {
        let temp_dir = tempfile::tempdir().map_err(|error| format!("无法创建临时目录：{error}"))?;
        let output_path = temp_dir.path().join("drawing.dxf");
        let converter = locate_converter()?;

        let mut command = Command::new(&converter);
        command
            .arg("-y")
            .arg("-o")
            .arg(&output_path)
            .arg(input)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let output = command
            .output()
            .map_err(|error| format!("无法启动 LibreDWG：{error}"))?;
        warnings = String::from_utf8_lossy(&output.stderr).trim().to_owned();

        if !output.status.success() || !output_path.exists() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if warnings.is_empty() {
                stdout.trim()
            } else {
                warnings.as_str()
            };
            return Err(format!(
                "LibreDWG 转换失败（退出码 {}）{}{}",
                output.status.code().unwrap_or(-1),
                if detail.is_empty() { "" } else { "：" },
                detail
            ));
        }

        let bytes = fs::read(&output_path).map_err(|error| format!("无法读取中间 DXF：{error}"))?;
        (String::from_utf8_lossy(&bytes).into_owned(), Some(temp_dir))
    } else {
        return Err("仅支持 .dwg 和 .dxf 文件".to_owned());
    };

    let scene = dxf::parse(&dxf_text)?;
    let entity_count = scene.primitives.len();
    if entity_count == 0 {
        return Err("图纸中没有可显示的二维实体".to_owned());
    }

    Ok(ConvertedDrawing {
        svg: scene.to_svg(),
        pdf_svg: scene.to_pdf_svg(),
        warnings,
        entity_count,
    })
}

fn locate_converter() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Some(custom) = std::env::var_os("CADVIEWER_LIBREDWG") {
        let custom = PathBuf::from(custom);
        candidates.push(if custom.is_dir() {
            custom.join("dwg2dxf.exe")
        } else {
            custom
        });
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(directory) = exe.parent()
    {
        candidates.push(directory.join("runtime").join("dwg2dxf.exe"));
        candidates.push(directory.join("dwg2dxf.exe"));
    }

    if let Ok(directory) = std::env::current_dir() {
        candidates.push(directory.join("runtime").join("dwg2dxf.exe"));
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "缺少 runtime\\dwg2dxf.exe。请重新解压完整的 Cadviewer 便携包。".to_owned())
}
