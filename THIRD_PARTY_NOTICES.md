# Third-party notices

## GNU LibreDWG 0.14

- Project: https://github.com/LibreDWG/libredwg
- License: GNU General Public License version 3 or later
- Usage: the portable runtime invokes `dwg2dxf.exe` to decode DWG files
- Corresponding source: `source/libredwg-0.14.tar.xz` in the portable package

## Rust dependencies

The GUI and rendering pipeline use eframe/egui, tiny-skia, pdf-writer,
ttf-parser, subsetter, flate2, encoding_rs, rfd and tempfile. Their
transitive license metadata is recorded by Cargo in `Cargo.lock`; release
packaging retains this notice and the project source.

No font is bundled. SHX stroke fonts are Autodesk/third-party licensed
assets and TrueType faces belong to their vendors; every font this program
uses is located on the user's own machine at runtime. Subsets of a user's
TrueType faces are embedded in the PDFs that user exports, which is the
ordinary embedding any CAD or office application performs on their behalf.

