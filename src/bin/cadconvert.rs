use cadviewer::{converter, pdf};
use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("Usage: Cadconvert.exe <input.dwg|input.dxf> <output.pdf>");
        std::process::exit(2);
    }

    let input = Path::new(&args[0]);
    let output = Path::new(&args[1]);
    let result = converter::convert_to_svg(input)
        .and_then(|converted| pdf::svg_to_pdf(&converted.pdf_svg, output));
    if let Err(error) = result {
        eprintln!("Cadconvert: {error}");
        std::process::exit(1);
    }
}
