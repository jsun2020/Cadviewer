use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::doc::Document;
use crate::plot::PaperSize;
use crate::plot::build::{BuildReport, PlotRequest, build, model_extents};
use crate::plot::style::ColorMode;
use crate::plot::PlotScene;
use crate::render::pdf::write_pdf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const DEFAULT_MARGIN_MM: f64 = 10.0;

#[derive(Debug)]
pub struct LoadedDrawing {
    pub doc: Document,
    pub warnings: String,
}

pub fn load(input: &Path) -> Result<LoadedDrawing, String> {
    if !input.exists() {
        return Err(format!("文件不存在：{}", input.display()));
    }
    let extension = input
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (bytes, warnings, _guard) = match extension.as_str() {
        "dxf" => (
            fs::read(input).map_err(|error| format!("无法读取 DXF：{error}"))?,
            String::new(),
            None,
        ),
        "dwg" => {
            let dir = tempfile::tempdir().map_err(|error| format!("无法创建临时目录：{error}"))?;
            let out = dir.path().join("drawing.dxf");
            let converter = locate_converter()?;

            // ASCII DXF, deliberately: LibreDWG's binary writer stores name
            // fields (group codes 2 and 8) as UTF-16LE, so a null-terminated
            // read truncates every block and layer name to its first
            // character. Counts stay right while names silently rot -- on the
            // reference drawing ASCII finds all 26 title-block inserts and
            // binary finds none. Do not add `-b` without re-proving that.
            let mut command = Command::new(&converter);
            command
                .arg("-y")
                .arg("-o")
                .arg(&out)
                .arg(input)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            command.creation_flags(CREATE_NO_WINDOW);

            let output = command
                .output()
                .map_err(|error| format!("无法启动 LibreDWG：{error}"))?;
            let warnings = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if !output.status.success() || !out.exists() {
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
            let bytes = fs::read(&out).map_err(|error| format!("无法读取中间 DXF：{error}"))?;
            (bytes, warnings, Some(dir))
        }
        _ => return Err("仅支持 .dwg 和 .dxf 文件".to_owned()),
    };

    let doc = Document::parse(&bytes)?;
    Ok(LoadedDrawing { doc, warnings })
}

/// Build the single scene that both the viewer and the PDF writer consume,
/// so what is on screen and what is exported can never drift apart.
///
/// Phase 3 replaces this with one scene per detected sheet.
pub fn build_scene(doc: &Document, mode: ColorMode) -> Result<(PlotScene, BuildReport), String> {
    let window = model_extents(doc);
    if !window.valid() {
        return Err("图纸中没有可打印的二维实体".to_owned());
    }
    let request = PlotRequest {
        window,
        paper: PaperSize::fit(window.width(), window.height()),
        margin_mm: DEFAULT_MARGIN_MM,
        mode,
    };
    let (scene, report) = build(doc, &request);
    if scene.items.is_empty() {
        return Err("图纸中没有可打印的二维实体".to_owned());
    }
    Ok((scene, report))
}

/// Convert to a single-page PDF covering model extents, returning the page
/// count.
pub fn convert_to_pdf(input: &Path, output: &Path, mode: ColorMode) -> Result<usize, String> {
    let loaded = load(input)?;
    let (scene, _report) = build_scene(&loaded.doc, mode)?;
    let bytes = write_pdf(&[scene]);
    fs::write(output, bytes).map_err(|error| format!("无法写入 PDF：{error}"))?;
    Ok(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nL\n 62\n1\n370\n35\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nL\n 10\n0.0\n 20\n0.0\n 11\n100.0\n 21\n50.0\n  0\nENDSEC\n  0\nEOF\n";

    #[test]
    fn build_scene_produces_items_for_a_drawing_with_geometry() {
        let doc = Document::parse(SRC).unwrap();
        let (scene, report) = build_scene(&doc, ColorMode::Color).unwrap();
        assert_eq!(report.items, 1);
        assert_eq!(scene.items.len(), 1);
    }

    #[test]
    fn build_scene_rejects_a_drawing_with_no_drawable_entities() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\n3DSOLID\n  8\n0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        assert!(build_scene(&doc, ColorMode::Color).is_err());
    }

    #[test]
    fn unsupported_extensions_are_rejected_before_any_work() {
        let error = load(Path::new("Cargo.toml")).unwrap_err();
        assert!(error.contains("dwg"), "unexpected error: {error}");
    }
}
