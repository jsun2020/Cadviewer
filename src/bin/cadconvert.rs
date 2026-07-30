use std::path::PathBuf;
use std::process::ExitCode;

use cadviewer::converter::convert_to_pdf;
use cadviewer::plot::style::ColorMode;

const USAGE: &str = "用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono] [--sheet N]";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let Some(output) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let rest: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
    let mode = if rest.iter().any(|a| a == "--mono") {
        ColorMode::Monochrome
    } else {
        ColorMode::Color
    };
    let sheet = rest
        .iter()
        .position(|a| a == "--sheet")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok());
    let options = cadviewer::converter::ConvertOptions { mode, sheet };

    match convert_to_pdf(&PathBuf::from(input), &PathBuf::from(output), &options) {
        Ok(pages) => {
            println!("已导出 {pages} 页");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::from(1)
        }
    }
}
