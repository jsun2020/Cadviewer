//! The application icon, decoded for the window manager.
//!
//! Two different consumers need it and they read it from different places.
//! Explorer, the taskbar and Alt+Tab read the `.ico` embedded in the
//! executable as a Win32 resource (see `build.rs`); the window itself is
//! given raw pixels at startup. Only the second one lives here.

/// Straight RGBA8 pixels, row-major, no padding.
pub struct Rgba8 {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The 256×256 application icon.
///
/// `None` only if the embedded PNG cannot be decoded, which would mean the
/// asset was replaced with something malformed — the window then opens with
/// the platform default rather than failing to open at all.
pub fn app_icon() -> Option<Rgba8> {
    decode(include_bytes!("../assets/app-icon-256.png"))
}

fn decode(bytes: &[u8]) -> Option<Rgba8> {
    let mut decoder = png::Decoder::new(bytes);
    // Normalise whatever the artist exported — palette, grayscale, no alpha
    // — into the RGBA8 the window manager wants.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    pixels.truncate(info.buffer_size());
    Some(Rgba8 { pixels, width: info.width, height: info.height })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_to_square_rgba() {
        let icon = app_icon().expect("assets/app-icon-256.png must decode");
        assert_eq!(icon.width, 256, "the window icon should be the 256 px asset");
        assert_eq!(icon.height, 256);
        assert_eq!(
            icon.pixels.len() as u32,
            icon.width * icon.height * 4,
            "four bytes per pixel, no row padding"
        );
    }

    /// An icon that decodes but is blank looks exactly like a working icon
    /// to every structural check — the drawing shows nothing and nobody is
    /// told. Assert there is something visible in it (LL-002: validate the
    /// pixels, not the file size).
    #[test]
    fn the_icon_is_not_blank_or_fully_transparent() {
        let icon = app_icon().unwrap();
        let opaque = icon.pixels.chunks_exact(4).filter(|p| p[3] > 16).count();
        assert!(
            opaque > icon.pixels.len() / 4 / 10,
            "only {opaque} pixels carry any alpha; the icon is effectively empty"
        );
        let first = &icon.pixels[..3];
        let varied = icon
            .pixels
            .chunks_exact(4)
            .any(|p| p[..3] != *first);
        assert!(varied, "every pixel is the same colour; this is a flat swatch");
    }

    /// Malformed input must degrade to the platform default, not panic
    /// during window creation.
    #[test]
    fn a_corrupt_png_is_none_rather_than_a_panic() {
        assert!(decode(b"not a png at all").is_none());
        assert!(decode(&[]).is_none());
    }
}
