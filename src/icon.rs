//! Procedurally-generated application icon.
//!
//! Drawn in code (no binary asset) as a rounded dark badge carrying the app's
//! bandwidth motif: a green download arrow and a blue upload arrow, the same
//! green/blue used for Down/Up throughout the UI. Returned as RGBA for the
//! window/taskbar icon, and serializable to a `.ico` for the executable.

/// Icon edge length in pixels.
pub const SIZE: u32 = 64;

// Palette (matches gui.rs COLOR_DOWN / COLOR_UP and the dark panel background).
const BG: [u8; 3] = [0x1e, 0x24, 0x30];
const BG_EDGE: [u8; 3] = [0x2b, 0x33, 0x42];
const DOWN: [u8; 3] = [0x4c, 0xc2, 0x5c]; // green
const UP: [u8; 3] = [0x4a, 0x9e, 0xff]; // blue

/// Generate the icon as `SIZE x SIZE` RGBA8 pixels (row-major, top-left origin).
pub fn rgba() -> Vec<u8> {
    let s = SIZE as f32;
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];

    let radius = s * 0.22; // rounded-corner radius
    let cx_left = s * 0.34;
    let cx_right = s * 0.66;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // Rounded-square background with a soft antialiased edge.
            let cover = rounded_rect_coverage(fx, fy, s, radius);
            if cover <= 0.0 {
                continue; // transparent outside the badge
            }

            // Vertical gradient for a bit of depth.
            let t = fy / s;
            let mut rgb = lerp3(BG_EDGE, BG, t);

            // Left glyph: download arrow (points down). Right glyph: upload
            // arrow (points up). Each occupies its half of the badge.
            if in_arrow(fx, fy, cx_left, s, true) {
                rgb = DOWN;
            } else if in_arrow(fx, fy, cx_right, s, false) {
                rgb = UP;
            }

            let idx = ((y * SIZE + x) * 4) as usize;
            px[idx] = rgb[0];
            px[idx + 1] = rgb[1];
            px[idx + 2] = rgb[2];
            px[idx + 3] = (cover * 255.0) as u8;
        }
    }
    px
}

/// Signed coverage in [0,1] of a rounded square centred in an `s`-sized canvas.
fn rounded_rect_coverage(x: f32, y: f32, s: f32, radius: f32) -> f32 {
    let margin = s * 0.06;
    let half = s * 0.5 - margin;
    let cx = s * 0.5;
    let cy = s * 0.5;
    // Distance to a rounded box (standard SDF).
    let dx = (x - cx).abs() - (half - radius);
    let dy = (y - cy).abs() - (half - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let dist = outside + inside - radius;
    // Antialias across ~1px.
    (0.5 - dist).clamp(0.0, 1.0)
}

/// Whether point `(x,y)` is inside a chunky arrow glyph centred at `cx`.
/// `down = true` points the arrow downward (download), else upward (upload).
fn in_arrow(x: f32, y: f32, cx: f32, s: f32, down: bool) -> bool {
    // Normalise y so the arrow spans a fixed vertical band.
    let top = s * 0.24;
    let bot = s * 0.76;
    // Flip vertically for the upload arrow.
    let ny = if down { y } else { s - y };
    if ny < top || ny > bot {
        return false;
    }
    let f = (ny - top) / (bot - top); // 0 at top .. 1 at bottom

    let stem_half = s * 0.055; // half-width of the shaft
    let head_half = s * 0.16; // half-width of the arrowhead base
    let head_start = 0.55; // where the head begins along f

    let dx = (x - cx).abs();
    if f < head_start {
        dx <= stem_half
    } else {
        // Triangular head narrowing to the tip.
        let ht = (f - head_start) / (1.0 - head_start); // 0..1 down the head
        let w = head_half * (1.0 - ht);
        dx <= w
    }
}

fn lerp3(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

/// Encode the icon as a single-image `.ico` (32-bit BGRA DIB) for the Windows
/// executable resource. Kept dependency-free.
pub fn ico_bytes() -> Vec<u8> {
    let w = SIZE as usize;
    let h = SIZE as usize;
    let rgba = rgba();

    // BITMAPINFOHEADER (40 bytes) + XOR (BGRA, bottom-up) + AND mask.
    let and_stride = w.div_ceil(32) * 4; // row-padded to 32 bits
    let dib_size = 40 + w * h * 4 + and_stride * h;

    let mut ico = Vec::new();
    // ICONDIR
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    ico.extend_from_slice(&1u16.to_le_bytes()); // image count
    // ICONDIRENTRY
    ico.push(if w >= 256 { 0 } else { w as u8 });
    ico.push(if h >= 256 { 0 } else { h as u8 });
    ico.push(0); // palette
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // color planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bpp
    ico.extend_from_slice(&(dib_size as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes()); // offset to DIB

    // BITMAPINFOHEADER, height doubled (XOR + AND).
    ico.extend_from_slice(&40u32.to_le_bytes());
    ico.extend_from_slice(&(w as i32).to_le_bytes());
    ico.extend_from_slice(&((h * 2) as i32).to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    ico.extend_from_slice(&0u32.to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0i32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());
    ico.extend_from_slice(&0u32.to_le_bytes());

    // XOR bitmap: BGRA, bottom-up.
    for y in (0..h).rev() {
        for x in 0..w {
            let i = (y * w + x) * 4;
            ico.push(rgba[i + 2]); // B
            ico.push(rgba[i + 1]); // G
            ico.push(rgba[i]); // R
            ico.push(rgba[i + 3]); // A
        }
    }
    // AND mask: fully opaque (alpha already carries transparency).
    ico.extend(std::iter::repeat_n(0u8, h * and_stride));
    ico
}
