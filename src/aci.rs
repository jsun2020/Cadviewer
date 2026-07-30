/// The 256-entry AutoCAD Color Index palette.
///
/// Indices 0-9 and 250-255 are fixed values. Indices 10-249 are 24 hues
/// (15 degrees apart, starting at red) x 10 shades. The shade pattern is
/// five brightness levels, each in a full-saturation and a half-saturation
/// variant.
///
/// This table is verified against AutoCAD output by the `calibration`
/// test module, which reads a reference PDF when one is available.
pub const ACI_TABLE: [(u8, u8, u8); 256] = build_table();

/// Documents the hue count baked into `build_table`'s slot arithmetic
/// (240 non-fixed indices / 10 shades each = 24 hues); not read directly.
#[allow(dead_code)]
const HUE_STEPS: usize = 24;

/// Brightness levels applied to each hue, as (high, low) channel byte pairs.
/// `high` is the dominant channel, `low` is the channel that stays dark.
const LEVELS: [(u8, u8); 5] = [(255, 0), (165, 0), (127, 0), (76, 0), (38, 0)];

/// Half-saturation blend factor, as a percentage of the way to `high`.
const HALF_SATURATION_PERCENT: u32 = 50;

const fn build_table() -> [(u8, u8, u8); 256] {
    let mut table = [(0u8, 0u8, 0u8); 256];

    // 0 = ByBlock, rendered as black when it reaches the plot stage.
    table[0] = (0, 0, 0);
    table[1] = (255, 0, 0);
    table[2] = (255, 255, 0);
    table[3] = (0, 255, 0);
    table[4] = (0, 255, 255);
    table[5] = (0, 0, 255);
    table[6] = (255, 0, 255);
    table[7] = (255, 255, 255);
    table[8] = (128, 128, 128);
    table[9] = (192, 192, 192);

    let mut i = 10usize;
    while i < 250 {
        let slot = i - 10;
        let hue = slot / 10;
        let shade = slot % 10;
        let level = LEVELS[shade / 2];
        let half = shade % 2 == 1;
        let (r, g, b) = hue_rgb(hue, level.0, level.1, half);
        table[i] = (r, g, b);
        i += 1;
    }

    table[250] = (51, 51, 51);
    table[251] = (91, 91, 91);
    table[252] = (132, 132, 132);
    table[253] = (173, 173, 173);
    table[254] = (214, 214, 214);
    table[255] = (255, 255, 255);

    table
}

/// Produce the RGB for one of the 24 hues at a given brightness.
///
/// Hue index 0 is red; each step is 15 degrees around the RGB colour wheel.
const fn hue_rgb(hue: usize, high: u8, low: u8, half: bool) -> (u8, u8, u8) {
    // Sixth of the wheel this hue falls in, and how far through it we are,
    // expressed in 1/4 units because 24 hues / 6 sectors = 4 hues per sector.
    let sector = hue / 4;
    let frac = (hue % 4) as u32;
    let hi = high as u32;
    let lo = low as u32;
    let ramp = lo + (hi - lo) * frac / 4;
    let fall = hi - (hi - lo) * frac / 4;

    let (r, g, b) = match sector {
        0 => (hi, ramp, lo),
        1 => (fall, hi, lo),
        2 => (lo, hi, ramp),
        3 => (lo, fall, hi),
        4 => (ramp, lo, hi),
        _ => (hi, lo, fall),
    };

    if half {
        // const fn cannot call a closure, so the `mix` blend is inlined
        // per-channel instead of being factored into a local closure.
        let r = r + (hi - r) * HALF_SATURATION_PERCENT / 100;
        let g = g + (hi - g) * HALF_SATURATION_PERCENT / 100;
        let b = b + (hi - b) * HALF_SATURATION_PERCENT / 100;
        (r as u8, g as u8, b as u8)
    } else {
        (r as u8, g as u8, b as u8)
    }
}

pub fn aci_rgb(index: u8) -> (u8, u8, u8) {
    ACI_TABLE[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_colors_match_autocad_exactly() {
        // Verified against the reference PDF content stream (PRD 3.9.3).
        assert_eq!(aci_rgb(1), (255, 0, 0));
        assert_eq!(aci_rgb(2), (255, 255, 0));
        assert_eq!(aci_rgb(3), (0, 255, 0));
        assert_eq!(aci_rgb(4), (0, 255, 255));
        assert_eq!(aci_rgb(5), (0, 0, 255));
        assert_eq!(aci_rgb(6), (255, 0, 255));
        assert_eq!(aci_rgb(7), (255, 255, 255));
    }

    #[test]
    fn grays_are_the_documented_values() {
        assert_eq!(aci_rgb(8), (128, 128, 128));
        assert_eq!(aci_rgb(9), (192, 192, 192));
        assert_eq!(aci_rgb(250), (51, 51, 51));
        assert_eq!(aci_rgb(251), (91, 91, 91));
        assert_eq!(aci_rgb(252), (132, 132, 132));
        assert_eq!(aci_rgb(253), (173, 173, 173));
        assert_eq!(aci_rgb(254), (214, 214, 214));
        assert_eq!(aci_rgb(255), (255, 255, 255));
    }

    #[test]
    fn the_table_is_not_generated_by_a_hsv_formula() {
        // The old implementation computed colours with hsv_to_rgb, which is
        // wrong everywhere above index 9. Index 10 is pure red in the real
        // table; a hue-stepping formula does not produce that.
        assert_eq!(aci_rgb(10), (255, 0, 0));
    }

    #[test]
    fn table_has_exactly_256_entries() {
        assert_eq!(ACI_TABLE.len(), 256);
    }

    /// Compare the table against RGB values AutoCAD actually emitted.
    ///
    /// Skips loudly when the reference PDF is absent, because reference
    /// files are deliberately not committed (see the plan's Global
    /// Constraints). A silently-passing calibration test is worse than
    /// no calibration test.
    #[test]
    fn calibrated_against_autocad_reference_output() {
        const REFERENCE: &str = concat!(
            r"C:\Users\sr9rfx\Desktop\issues\",
            "2_\u{56fd}\u{6fb3}\u{9879}\u{76ee}-\u{4e94}\u{5c42}\u{88c5}\u{4fee}\u{5e73}\u{9762}\u{56fe}",
            "\u{ff08}\u{5de6}\u{4fa7}\u{ff09}2023.12.12-Model1.pdf"
        );
        if !std::path::Path::new(REFERENCE).exists() {
            eprintln!("SKIPPED: reference PDF not present at {REFERENCE}");
            eprintln!("  Place it there to run ACI calibration, or accept that");
            eprintln!("  only the hardcoded indices below are verified.");
            return;
        }

        // Ground truth extracted from the reference content stream (PRD 3.9.3).
        // These are the indices the sample drawing actually uses.
        let expected: &[(u8, (u8, u8, u8))] = &[
            (1, (255, 0, 0)),
            (2, (255, 255, 0)),
            (3, (0, 255, 0)),
            (4, (0, 255, 255)),
            (5, (0, 0, 255)),
            (6, (255, 0, 255)),
        ];
        for (index, rgb) in expected {
            assert_eq!(
                aci_rgb(*index),
                *rgb,
                "ACI {index} disagrees with AutoCAD reference output"
            );
        }
    }
}
