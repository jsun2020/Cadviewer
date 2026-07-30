use crate::aci::aci_rgb;
use crate::dxfnew::entities::RawEntity;
use crate::dxfnew::tables::LayerRecord;
use crate::plot::Rgb;

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
}
