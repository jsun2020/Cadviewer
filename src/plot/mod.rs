use crate::geom::PathGeom;

pub mod build;
pub mod flatten;
pub mod style;

/// PDF lineweight 0 means "thinnest line the device can draw". AutoCAD uses
/// it for every hairline entity, and it is the most common width in real
/// drawings (PRD 3.9.2: 42,134 occurrences in the reference sheet).
pub const HAIRLINE_MM: f32 = 0.0;

/// Fallback when entity, layer and `$CELWEIGHT` all say "default".
pub const DEFAULT_WIDTH_MM: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub color: Rgb,
    /// Plotted width in millimetres. The point of the whole IR: no stage
    /// downstream of `plot` ever reasons about drawing units again.
    pub width_mm: f32,
    /// Dash pattern in millimetres, already scaled for the plot.
    /// `None` is a solid line.
    pub dash_mm: Option<Vec<f32>>,
}

#[derive(Clone, Debug)]
pub enum PlotItem {
    Path { geom: PathGeom, style: StrokeStyle },
    Fill { geom: PathGeom, color: Rgb },
}

#[derive(Clone, Copy, Debug)]
pub struct PaperSize {
    pub width_mm: f64,
    pub height_mm: f64,
}

/// ISO A-series in portrait orientation, largest first.
const A_SERIES: [(f64, f64); 5] = [
    (841.0, 1189.0),
    (594.0, 841.0),
    (420.0, 594.0),
    (297.0, 420.0),
    (210.0, 297.0),
];

impl PaperSize {
    pub fn a4_landscape() -> Self {
        Self { width_mm: 297.0, height_mm: 210.0 }
    }

    /// Smallest A-series sheet containing the frame, matching its
    /// orientation. Oversized frames get A0.
    pub fn fit(width_mm: f64, height_mm: f64) -> Self {
        let landscape = width_mm >= height_mm;
        let oriented = |short: f64, long: f64| {
            if landscape {
                PaperSize { width_mm: long, height_mm: short }
            } else {
                PaperSize { width_mm: short, height_mm: long }
            }
        };
        let mut best = None;
        for (short, long) in A_SERIES {
            let sheet = oriented(short, long);
            if width_mm <= sheet.width_mm && height_mm <= sheet.height_mm {
                best = Some(sheet);
            }
        }
        best.unwrap_or_else(|| oriented(A_SERIES[0].0, A_SERIES[0].1))
    }
}

#[derive(Clone, Debug)]
pub struct PlotScene {
    pub paper: PaperSize,
    pub items: Vec<PlotItem>,
}

impl PlotScene {
    pub fn new(paper: PaperSize) -> Self {
        Self { paper, items: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_landscape_is_297_by_210() {
        let p = PaperSize::a4_landscape();
        assert!((p.width_mm - 297.0).abs() < 0.01);
        assert!((p.height_mm - 210.0).abs() < 0.01);
    }

    #[test]
    fn fit_picks_the_smallest_sheet_that_contains_the_frame() {
        let p = PaperSize::fit(400.0, 280.0);
        assert!((p.width_mm - 420.0).abs() < 0.01, "got {}", p.width_mm);
        assert!((p.height_mm - 297.0).abs() < 0.01, "got {}", p.height_mm);
    }

    #[test]
    fn fit_returns_portrait_for_a_tall_frame() {
        let p = PaperSize::fit(200.0, 290.0);
        assert!(p.height_mm > p.width_mm, "expected portrait, got {p:?}");
    }

    #[test]
    fn oversized_frames_fall_back_to_the_largest_sheet() {
        let p = PaperSize::fit(5000.0, 3000.0);
        assert!((p.width_mm - 1189.0).abs() < 0.01, "got {}", p.width_mm);
    }
}
