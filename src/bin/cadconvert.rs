use std::path::PathBuf;
use std::process::ExitCode;

use cadviewer::converter::convert_to_pdf;
use cadviewer::plot::style::ColorMode;

const USAGE: &str = "用法：Cadconvert.exe <input.dwg|input.dxf> <output.pdf> [--mono]";

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
    let mode = if args.any(|arg| arg == "--mono") {
        ColorMode::Monochrome
    } else {
        ColorMode::Color
    };

    match convert_to_pdf(&PathBuf::from(input), &PathBuf::from(output), mode) {
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
