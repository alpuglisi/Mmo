//! A small RGB canvas for the film and the charts: filled shapes with
//! alpha, anti-aliased discs and lines, a 5×7 bitmap font, PNG output.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub type Rgb = [u8; 3];

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, bg: Rgb) -> Self {
        let mut px = vec![0u8; w * h * 3];
        for p in px.chunks_exact_mut(3) {
            p.copy_from_slice(&bg);
        }
        Canvas { w, h, px }
    }

    /// Blend `c` over the pixel at (x, y) with opacity `a` (0–1).
    #[inline]
    pub fn blend(&mut self, x: i64, y: i64, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || a <= 0.0 {
            return;
        }
        let k = (y as usize * self.w + x as usize) * 3;
        let a = a.min(1.0);
        for (p, &ci) in self.px[k..k + 3].iter_mut().zip(&c) {
            let v = *p as f32;
            *p = (v + (ci as f32 - v) * a).round() as u8;
        }
    }

    pub fn fill_rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: Rgb, a: f32) {
        for yy in y.max(0)..(y + h).min(self.h as i64) {
            for xx in x.max(0)..(x + w).min(self.w as i64) {
                self.blend(xx, yy, c, a);
            }
        }
    }

    /// A filled ellipse with a soft (anti-aliased) edge.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgb, a: f32) {
        let (x0, x1) = ((cx - rx - 1.0).floor() as i64, (cx + rx + 1.0).ceil() as i64);
        let (y0, y1) = ((cy - ry - 1.0).floor() as i64, (cy + ry + 1.0).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rx.max(0.1), (y as f32 + 0.5 - cy) / ry.max(0.1));
                let d = (dx * dx + dy * dy).sqrt();
                // About one pixel of fade at the rim.
                let edge = ((1.0 - d) * rx.min(ry) + 0.5).clamp(0.0, 1.0);
                self.blend(x, y, c, a * edge);
            }
        }
    }

    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, a: f32) {
        self.ellipse(cx, cy, r, r, c, a);
    }

    /// An anti-aliased line of width `w` (a capsule).
    pub fn line(&mut self, (ax, ay): (f32, f32), (bx, by): (f32, f32), w: f32, c: Rgb, a: f32) {
        let r = w / 2.0;
        let (x0, x1) = ((ax.min(bx) - r - 1.0).floor() as i64, (ax.max(bx) + r + 1.0).ceil() as i64);
        let (y0, y1) = ((ay.min(by) - r - 1.0).floor() as i64, (ay.max(by) + r + 1.0).ceil() as i64);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
                let d = (qx * qx + qy * qy).sqrt();
                let cov = (r - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(x, y, c, a * cov);
                }
            }
        }
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], w: f32, c: Rgb, a: f32) {
        for p in pts.windows(2) {
            self.line(p[0], p[1], w, c, a);
        }
    }

    /// Text in the bitmap font at `scale` pixels per font pixel; returns
    /// the width drawn. Letters are drawn upper case.
    pub fn text(&mut self, x: i64, y: i64, s: &str, scale: i64, c: Rgb, a: f32) -> i64 {
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0b10000 >> col) != 0 {
                        self.fill_rect(cx + col * scale, y + row as i64 * scale, scale, scale, c, a);
                    }
                }
            }
            cx += 6 * scale;
        }
        (cx - x - scale).max(0)
    }

    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        let file = BufWriter::new(File::create(path)?);
        let mut enc = png::Encoder::new(file, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        writer.write_image_data(&self.px).map_err(std::io::Error::other)
    }
}

/// Width in pixels of `s` in the bitmap font at `scale`.
pub fn text_width(s: &str, scale: i64) -> i64 {
    (s.chars().count() as i64 * 6 * scale - scale).max(0)
}

/// Rows (5 bits each, high bit left) of a character of the 5×7 font.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        ',' => [0, 0, 0, 0, 0x0C, 0x04, 0x08],
        ':' => [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0],
        '-' => [0, 0, 0, 0x1F, 0, 0, 0],
        '+' => [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
        '·' => [0, 0, 0, 0x0C, 0x0C, 0, 0],
        _ => [0; 7],
    }
}
