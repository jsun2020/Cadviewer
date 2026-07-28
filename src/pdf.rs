use crate::fonts;
use std::fs;
use std::path::Path;

pub fn svg_to_pdf(svg: &str, output: &Path) -> Result<(), String> {
    let mut options = svg2pdf::usvg::Options::default();
    fonts::configure(&mut options, svg);
    let tree = svg2pdf::usvg::Tree::from_str(svg, &options)
        .map_err(|error| format!("无法解析矢量图：{error}"))?;
    let pdf = svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions { dpi: 96.0 },
    )
    .map_err(|error| format!("PDF 生成失败：{error:?}"))?;
    fs::write(output, pdf).map_err(|error| format!("无法写入 PDF：{error}"))
}
