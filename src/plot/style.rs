use std::collections::HashMap;

use crate::aci::aci_rgb;
use crate::dxfnew::entities::RawEntity;
use crate::dxfnew::tables::{LayerRecord, LtypeRecord};
use crate::encoding::Codepage;
use crate::plot::Rgb;
use crate::plot::{DEFAULT_WIDTH_MM, HAIRLINE_MM};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Monochrome,
}

/// ByBlock context handed down through INSERT recursion. An entity inside a
/// block that says "ByBlock" takes these values from the INSERT that
/// referenced its block.
#[derive(Clone, Debug)]
pub struct Inherited {
    pub color: Rgb,
    pub lineweight: i16,
    pub linetype: String,
}

impl Default for Inherited {
    fn default() -> Self {
        Self {
            color: Rgb::BLACK,
            lineweight: -3,
            linetype: "CONTINUOUS".to_owned(),
        }
    }
}

const ACI_BYBLOCK: i32 = 0;
const ACI_BYLAYER: i32 = 256;

fn truecolor_to_rgb(value: u32) -> Rgb {
    Rgb::new(
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    )
}

/// Resolve one entity's plotted colour.
pub fn resolve_color(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited: Rgb,
    mode: ColorMode,
) -> Rgb {
    if mode == ColorMode::Monochrome {
        return Rgb::BLACK;
    }

    let raw = entity_color(entity, layer, inherited);

    // On white paper AutoCAD plots white as black, otherwise the drawing
    // would be invisible. Verified in the reference PDF (PRD 3.9.3).
    if raw == Rgb::new(255, 255, 255) {
        Rgb::BLACK
    } else {
        raw
    }
}

fn entity_color(entity: &RawEntity, layer: Option<&LayerRecord>, inherited: Rgb) -> Rgb {
    let true_color = entity.int(420, 0);
    if true_color > 0 {
        return truecolor_to_rgb(true_color as u32);
    }

    match entity.int(62, ACI_BYLAYER) {
        ACI_BYBLOCK => inherited,
        ACI_BYLAYER => layer_color(layer),
        aci if (1..=255).contains(&aci) => rgb_from_aci(aci),
        // Negative ACI marks a layer that is turned off; treat the absolute
        // value as the colour and let visibility be handled elsewhere.
        aci => rgb_from_aci(aci.abs().clamp(1, 255)),
    }
}

fn layer_color(layer: Option<&LayerRecord>) -> Rgb {
    match layer {
        Some(l) => match l.true_color {
            Some(tc) => truecolor_to_rgb(tc),
            None => rgb_from_aci(l.aci.abs().clamp(1, 255) as i32),
        },
        None => Rgb::BLACK,
    }
}

fn rgb_from_aci(index: i32) -> Rgb {
    let (r, g, b) = aci_rgb(index.clamp(0, 255) as u8);
    Rgb::new(r, g, b)
}

pub const LW_BYLAYER: i16 = -1;
pub const LW_BYBLOCK: i16 = -2;
pub const LW_DEFAULT: i16 = -3;

/// Resolve one entity's plotted line width in millimetres.
///
/// Group code 370 carries hundredths of a millimetre, plus three sentinels.
/// Note that `0` is a real value meaning hairline, not an absent one, so the
/// chain must distinguish "absent" from "zero" — which is why the raw i16 is
/// threaded through rather than an Option.
pub fn resolve_width_mm(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited_lw: i16,
    celweight: i16,
) -> f32 {
    let entity_lw = entity.int(370, LW_BYLAYER as i32) as i16;
    let resolved = resolve_raw(entity_lw, layer, inherited_lw, celweight);
    hundredths_to_mm(resolved)
}

fn resolve_raw(
    entity_lw: i16,
    layer: Option<&LayerRecord>,
    inherited_lw: i16,
    celweight: i16,
) -> i16 {
    match entity_lw {
        LW_BYLAYER => {
            let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
            if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
        }
        LW_BYBLOCK => {
            if inherited_lw >= 0 {
                inherited_lw
            } else {
                let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
                if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
            }
        }
        LW_DEFAULT => {
            let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
            if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
        }
        explicit => explicit,
    }
}

fn fallback(celweight: i16) -> i16 {
    if celweight >= 0 {
        celweight
    } else {
        // Sentinel meaning "use DEFAULT_WIDTH_MM"; -100 cannot collide with
        // a real 1/100 mm value because those are non-negative here.
        -100
    }
}

fn hundredths_to_mm(raw: i16) -> f32 {
    match raw {
        0 => HAIRLINE_MM,
        v if v > 0 => v as f32 / 100.0,
        _ => DEFAULT_WIDTH_MM,
    }
}

/// Resolve one entity's dash pattern, already converted to plotted
/// millimetres.
///
/// AutoCAD itself explodes linetypes into individual segments and emits no
/// PDF dash operator at all (PRD 3.9.4). We use native PDF dashes instead:
/// visually equivalent and dramatically smaller output. R-LT-5 records this
/// as a deliberate deviation that the visual-diff harness must confirm.
pub fn resolve_dash_mm(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    ltypes: &HashMap<String, LtypeRecord>,
    inherited_ltype: &str,
    ltscale: f64,
    plot_scale: f64,
    cp: Codepage,
) -> Option<Vec<f32>> {
    let name = linetype_name(entity, layer, inherited_ltype, cp);
    let record = ltypes.get(&name)?;
    if record.pattern.is_empty() {
        return None;
    }

    let celtscale = {
        let v = entity.f64(48, 1.0);
        if v > 0.0 { v } else { 1.0 }
    };
    let factor = ltscale.abs().max(f64::EPSILON) * celtscale * plot_scale;

    let dashes: Vec<f32> = record
        .pattern
        .iter()
        .map(|v| (v.abs() * factor) as f32)
        .collect();

    // A pattern whose lengths all round to zero would render as an
    // invisible line rather than a dashed one.
    if dashes.iter().all(|v| *v <= f32::EPSILON) {
        return None;
    }
    Some(dashes)
}

fn linetype_name(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited_ltype: &str,
    cp: Codepage,
) -> String {
    let raw = entity
        .text(6, cp)
        .unwrap_or_else(|| "BYLAYER".to_owned());
    match raw.to_ascii_uppercase().as_str() {
        "BYLAYER" => layer
            .map(|l| l.linetype.clone())
            .unwrap_or_else(|| "CONTINUOUS".to_owned()),
        "BYBLOCK" => inherited_ltype.to_owned(),
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxfnew::entities::RawEntity;
    use crate::dxfnew::lexer::Value;
    use crate::dxfnew::tables::LayerRecord;

    fn entity(codes: &[(i32, i32)]) -> RawEntity {
        RawEntity {
            kind: "LINE".to_owned(),
            codes: codes.iter().map(|(c, v)| (*c, Value::I32(*v))).collect(),
        }
    }

    fn layer(aci: i16, true_color: Option<u32>) -> LayerRecord {
        LayerRecord {
            name: "L".to_owned(),
            aci,
            true_color,
            lineweight: -3,
            linetype: "CONTINUOUS".to_owned(),
        }
    }

    #[test]
    fn entity_truecolor_wins_over_everything() {
        let e = entity(&[(420, 0x00FF7F00), (62, 1)]);
        let got = resolve_color(&e, Some(&layer(3, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 127, 0));
    }

    #[test]
    fn explicit_aci_uses_the_lookup_table() {
        let e = entity(&[(62, 4)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(0, 255, 255), "ACI 4 is cyan");
    }

    #[test]
    fn aci_256_means_bylayer() {
        let e = entity(&[(62, 256)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::new(9, 9, 9), ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 0, 0), "should take the layer's red");
    }

    #[test]
    fn aci_0_means_byblock_and_takes_the_inherited_colour() {
        let e = entity(&[(62, 0)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::new(0, 0, 255), ColorMode::Color);
        assert_eq!(got, Rgb::new(0, 0, 255));
    }

    #[test]
    fn absent_colour_code_defaults_to_bylayer() {
        let e = entity(&[]);
        let got = resolve_color(&e, Some(&layer(2, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 255, 0), "ACI 2 is yellow");
    }

    #[test]
    fn white_plots_as_black_on_white_paper() {
        // R-COL-3, verified against the reference PDF (PRD 3.9.3): AutoCAD
        // emits 0 0 0 RG for ACI 7.
        let e = entity(&[(62, 7)]);
        let got = resolve_color(&e, Some(&layer(7, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::BLACK);
    }

    #[test]
    fn monochrome_forces_black_but_colour_mode_does_not() {
        let e = entity(&[(62, 1)]);
        assert_eq!(
            resolve_color(&e, None, Rgb::BLACK, ColorMode::Monochrome),
            Rgb::BLACK
        );
        assert_eq!(
            resolve_color(&e, None, Rgb::BLACK, ColorMode::Color),
            Rgb::new(255, 0, 0)
        );
    }

    #[test]
    fn explicit_entity_lineweight_converts_hundredths_to_millimetres() {
        let e = entity(&[(370, 35)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), -3, -3), 0.35);
        let e = entity(&[(370, 100)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), -3, -3), 1.00);
    }

    #[test]
    fn zero_is_hairline_not_missing_data() {
        // 55% of strokes in the reference sheet are hairline (PRD 3.9.2).
        let e = entity(&[(370, 0)]);
        assert_eq!(
            resolve_width_mm(&e, Some(&layer(7, None)), -3, -3),
            HAIRLINE_MM
        );
    }

    #[test]
    fn bylayer_takes_the_layer_lineweight() {
        let mut l = layer(7, None);
        l.lineweight = 15;
        let e = entity(&[(370, -1)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), 0.15);
    }

    #[test]
    fn byblock_takes_the_inherited_lineweight() {
        let e = entity(&[(370, -2)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), 50, -3), 0.50);
    }

    #[test]
    fn default_falls_through_layer_to_celweight() {
        let mut l = layer(7, None);
        l.lineweight = -3;
        let e = entity(&[(370, -3)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, 25), 0.25);
    }

    #[test]
    fn default_falls_all_the_way_to_the_fallback_width() {
        let mut l = layer(7, None);
        l.lineweight = -3;
        let e = entity(&[(370, -3)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), DEFAULT_WIDTH_MM);
    }

    #[test]
    fn absent_lineweight_code_behaves_as_bylayer() {
        let mut l = layer(7, None);
        l.lineweight = 40;
        let e = entity(&[]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), 0.40);
    }

    #[test]
    fn covers_every_width_observed_in_the_autocad_reference() {
        // PRD 3.9.2: the 13 widths AutoCAD emitted for this drawing.
        let cases: [(i32, f32); 13] = [
            (0, 0.0),
            (9, 0.09),
            (13, 0.13),
            (15, 0.15),
            (18, 0.18),
            (20, 0.20),
            (25, 0.25),
            (30, 0.30),
            (35, 0.35),
            (40, 0.40),
            (50, 0.50),
            (60, 0.60),
            (100, 1.00),
        ];
        for (raw, expected) in cases {
            let e = entity(&[(370, raw)]);
            let got = resolve_width_mm(&e, Some(&layer(7, None)), -3, -3);
            assert!(
                (got - expected).abs() < 1e-6,
                "370={raw} gave {got} mm, expected {expected} mm"
            );
        }
    }

    use crate::dxfnew::tables::LtypeRecord;
    use std::collections::HashMap;

    fn ltypes() -> HashMap<String, LtypeRecord> {
        let mut m = HashMap::new();
        m.insert(
            "HIDDEN".to_owned(),
            LtypeRecord { name: "HIDDEN".to_owned(), pattern: vec![6.35, -3.175] },
        );
        m.insert(
            "CONTINUOUS".to_owned(),
            LtypeRecord { name: "CONTINUOUS".to_owned(), pattern: vec![] },
        );
        m
    }

    fn entity_with(codes: &[(i32, i32)], strings: &[(i32, &str)]) -> RawEntity {
        let mut e = entity(codes);
        for (c, s) in strings {
            e.codes.push((*c, Value::Str(s.as_bytes().to_vec())));
        }
        e
    }

    #[test]
    fn solid_linetypes_produce_no_dash_pattern() {
        let e = entity_with(&[], &[(6, "CONTINUOUS")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert_eq!(got, None);
    }

    #[test]
    fn dash_lengths_are_scaled_into_millimetres() {
        // 6.35 drawing units * LTSCALE 10 * plot scale 0.01 mm/unit = 0.635 mm
        let e = entity_with(&[], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 10.0, 0.01, Codepage::Latin1,
        )
        .expect("HIDDEN should dash");
        assert!((got[0] - 0.635).abs() < 1e-4, "got {got:?}");
        assert!((got[1] - 0.3175).abs() < 1e-4, "got {got:?}");
    }

    #[test]
    fn gaps_become_positive_lengths() {
        // PDF dash arrays alternate on/off as positive numbers; the DXF
        // sign convention (negative = gap) must not leak through.
        let e = entity_with(&[], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        )
        .unwrap();
        assert!(got.iter().all(|v| *v >= 0.0), "got {got:?}");
    }

    #[test]
    fn celtscale_multiplies_the_pattern() {
        let e = entity_with(&[(48, 2)], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        )
        .unwrap();
        assert!((got[0] - 12.70).abs() < 1e-4, "got {got:?}");
    }

    #[test]
    fn bylayer_linetype_is_taken_from_the_layer() {
        let mut l = layer(7, None);
        l.linetype = "HIDDEN".to_owned();
        let e = entity_with(&[], &[(6, "BYLAYER")]);
        let got = resolve_dash_mm(
            &e, Some(&l), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert!(got.is_some(), "should have inherited HIDDEN from the layer");
    }

    #[test]
    fn degenerate_patterns_do_not_produce_an_invisible_line() {
        // An all-zero pattern would make a PDF dash array that renders
        // nothing at all. Treat it as solid.
        let mut m = ltypes();
        m.insert(
            "ZERO".to_owned(),
            LtypeRecord { name: "ZERO".to_owned(), pattern: vec![0.0, 0.0] },
        );
        let e = entity_with(&[], &[(6, "ZERO")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &m, "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert_eq!(got, None);
    }
}
