//! The program icon, calculated as pixels. No image file and no Windows calls.
//!
//! The icon is a blue disc with a white ring and two white clock hands.
//! It is visible on a light taskbar and on a dark taskbar.

const SAMPLES: i32 = 4;

/// Shortest distance from point (px, py) to the line segment (ax, ay)-(bx, by).
fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Color of one point of the icon. Coordinates are 0.0 to 1.0.
/// Returns (red, green, blue, alpha), each 0.0 to 1.0.
fn sample(x: f32, y: f32) -> (f32, f32, f32, f32) {
    let (cx, cy) = (0.5, 0.5);
    let radius = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    if radius > 0.48 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let white = (1.0, 1.0, 1.0, 1.0);
    // Ring.
    if radius > 0.39 {
        return white;
    }
    // Minute hand: from the center up. Hour hand: from the center to the right.
    let hand = 0.055;
    if distance_to_segment(x, y, cx, cy, cx, 0.20) < hand
        || distance_to_segment(x, y, cx, cy, 0.70, cy) < hand
    {
        return white;
    }
    (0.10, 0.37, 0.71, 1.0)
}

/// Icon pixels for a square of `size` pixels: premultiplied BGRA, first row at the top.
pub fn clock_icon(size: usize) -> Vec<u32> {
    let mut pixels = Vec::with_capacity(size * size);
    let scale = 1.0 / (size as f32 * SAMPLES as f32);
    for row in 0..size {
        for column in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = ((column as i32 * SAMPLES + sx) as f32 + 0.5) * scale;
                    let y = ((row as i32 * SAMPLES + sy) as f32 + 0.5) * scale;
                    let (sr, sg, sb, sa) = sample(x, y);
                    // Premultiplied sum.
                    r += sr * sa;
                    g += sg * sa;
                    b += sb * sa;
                    a += sa;
                }
            }
            let n = (SAMPLES * SAMPLES) as f32;
            let to_byte = |v: f32| ((v / n) * 255.0 + 0.5) as u32;
            pixels.push((to_byte(a) << 24) | (to_byte(r) << 16) | (to_byte(g) << 8) | to_byte(b));
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_the_correct_size() {
        assert_eq!(clock_icon(16).len(), 256);
        assert_eq!(clock_icon(32).len(), 1024);
    }

    #[test]
    fn corners_are_transparent_and_center_is_opaque() {
        let size = 32;
        let px = clock_icon(size);
        assert_eq!(px[0] >> 24, 0);
        assert_eq!(px[size * size - 1] >> 24, 0);
        let center = px[(size / 2) * size + size / 2];
        assert_eq!(center >> 24, 255);
    }

    #[test]
    fn pixels_are_premultiplied() {
        for p in clock_icon(24) {
            let a = p >> 24;
            assert!((p & 0xFF) <= a && ((p >> 8) & 0xFF) <= a && ((p >> 16) & 0xFF) <= a);
        }
    }
}
