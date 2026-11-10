//! Pixel calculation for the transparent clock window.
//! No Windows calls. All functions have tests.

/// Convert a text image into the pixel format of a transparent window.
///
/// Input: 32-bit pixels where white text was drawn on a black background.
/// The brightness of each pixel is the text coverage (0 = no text, 255 = text).
///
/// Output: white text on a black background with opacity `background_alpha`,
/// as premultiplied BGRA pixels.
pub fn compose_white_text(pixels: &mut [u32], background_alpha: u8) {
    let bg = u32::from(background_alpha);
    for p in pixels.iter_mut() {
        let b = *p & 0xFF;
        let g = (*p >> 8) & 0xFF;
        let r = (*p >> 16) & 0xFF;
        let coverage = r.max(g).max(b);
        // alpha = coverage + (1 - coverage) * background alpha
        let alpha = coverage + ((255 - coverage) * bg + 127) / 255;
        // White text, black background: each premultiplied color value is the coverage.
        *p = (alpha << 24) | (coverage << 16) | (coverage << 8) | coverage;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_pixel_gets_background_alpha() {
        let mut px = [0x0000_0000u32];
        compose_white_text(&mut px, 160);
        assert_eq!(px[0], 160 << 24);
    }

    #[test]
    fn full_text_pixel_is_opaque_white() {
        let mut px = [0x00FF_FFFFu32];
        compose_white_text(&mut px, 160);
        assert_eq!(px[0], 0xFFFF_FFFF);
    }

    #[test]
    fn edge_pixel_is_between_and_premultiplied() {
        let mut px = [0x0080_8080u32];
        compose_white_text(&mut px, 0);
        // With a fully transparent background, alpha equals coverage.
        assert_eq!(px[0], 0x8080_8080);
    }

    #[test]
    fn color_never_exceeds_alpha() {
        for c in 0..=255u32 {
            let mut px = [(c << 16) | (c << 8) | c];
            compose_white_text(&mut px, 100);
            let a = px[0] >> 24;
            assert!((px[0] & 0xFF) <= a);
        }
    }
}
