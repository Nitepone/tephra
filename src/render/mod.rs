//! Drawing: a 1-bit canvas, text, images, the layout tree, and the PNG the device downloads.
//!
//! Everything on the canvas is pure black or pure white: text is thresholded and images are
//! dithered as they are drawn, so the PNG is a plain 1-bit image the OG draws with a fast refresh.

pub mod font;
pub mod layout;

use anyhow::Context;

pub const WHITE: u8 = 255;
pub const BLACK: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w: w.max(0), h: h.max(0) }
    }
    pub fn inset(self, d: i32) -> Self {
        Self::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
    pub fn intersect(self, o: Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<u8>,
    clip: Rect,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        let (w, h) = (w as i32, h as i32);
        Self { w, h, px: vec![WHITE; (w * h) as usize], clip: Rect::new(0, 0, w, h) }
    }

    /// Restricts drawing to `r` (within the current clip) until the returned clip is restored.
    pub fn push_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = old.intersect(r);
        old
    }
    pub fn pop_clip(&mut self, old: Rect) {
        self.clip = old;
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32, v: u8) {
        let c = self.clip;
        if x >= c.x && y >= c.y && x < c.x + c.w && y < c.y + c.h {
            self.px[(y * self.w + x) as usize] = v;
        }
    }

    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.px[(y * self.w + x) as usize]
    }

    pub fn fill(&mut self, r: Rect, v: u8) {
        let r = r.intersect(self.clip);
        for y in r.y..r.y + r.h {
            let row = (y * self.w) as usize;
            self.px[row + r.x as usize..row + (r.x + r.w) as usize].fill(v);
        }
    }

    /// A `t`-pixel border drawn inside `r`.
    pub fn border(&mut self, r: Rect, t: i32, v: u8) {
        self.fill(Rect::new(r.x, r.y, r.w, t), v);
        self.fill(Rect::new(r.x, r.y + r.h - t, r.w, t), v);
        self.fill(Rect::new(r.x, r.y, t, r.h), v);
        self.fill(Rect::new(r.x + r.w - t, r.y, t, r.h), v);
    }

    /// Draws a grayscale image (0 black .. 255 white) at (x, y), dithered (Atkinson) or thresholded.
    pub fn draw_gray(&mut self, img: &image::GrayImage, x: i32, y: i32, dither: bool) {
        let (iw, ih) = (img.width() as usize, img.height() as usize);
        let mut err: Vec<f32> = img.as_raw().iter().map(|&v| v as f32).collect();
        for j in 0..ih {
            for i in 0..iw {
                let old = err[j * iw + i];
                let new = if old >= 128.0 { 255.0 } else { 0.0 };
                self.set(x + i as i32, y + j as i32, new as u8);
                if !dither {
                    continue;
                }
                // Atkinson spreads 6/8 of the error, which keeps contrast on a 1-bit panel.
                let e = (old - new) / 8.0;
                for (di, dj) in [(1, 0), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)] {
                    let (ni, nj) = (i as isize + di, j + dj);
                    if ni >= 0 && (ni as usize) < iw && nj < ih {
                        err[nj * iw + ni as usize] += e;
                    }
                }
            }
        }
    }

    /// The canvas as a 1-bit grayscale PNG (bit set = white).
    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        let (w, h) = (self.w as usize, self.h as usize);
        let stride = w.div_ceil(8);
        let mut packed = vec![0u8; stride * h];
        for y in 0..h {
            for x in 0..w {
                if self.px[y * w + x] >= 128 {
                    packed[y * stride + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::One);
        enc.set_compression(png::Compression::High);
        let mut writer = enc.write_header().context("encoding PNG")?;
        writer.write_image_data(&packed).context("encoding PNG")?;
        writer.finish().context("encoding PNG")?;
        Ok(out)
    }
}

/// Decodes an image file to grayscale, flattening any transparency onto white.
pub fn decode_gray(bytes: &[u8]) -> anyhow::Result<image::GrayImage> {
    let rgba = image::load_from_memory(bytes).context("decoding image")?.to_rgba8();
    let mut gray = image::GrayImage::new(rgba.width(), rgba.height());
    for (p, q) in rgba.pixels().zip(gray.pixels_mut()) {
        let [r, g, b, a] = p.0;
        let l = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        let a = a as f32 / 255.0;
        q.0[0] = (l * a + 255.0 * (1.0 - a)).round() as u8;
    }
    Ok(gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_1_bit_and_decodes_back() {
        let mut c = Canvas::new(800, 480);
        c.fill(Rect::new(10, 10, 100, 50), BLACK);
        let png = c.to_png().unwrap();
        assert!(png.len() < 90_000);
        let img = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!((img.width(), img.height()), (800, 480));
        assert_eq!(img.get_pixel(20, 20).0[0], 0);
        assert_eq!(img.get_pixel(200, 200).0[0], 255);
        let dec = png::Decoder::new(std::io::Cursor::new(&png)).read_info().unwrap();
        assert_eq!(dec.info().bit_depth, png::BitDepth::One);
    }

    #[test]
    fn a_dithered_photo_stays_under_the_og_limit() {
        // Noise is the worst case for compression.
        let mut img = image::GrayImage::new(800, 480);
        let mut s: u32 = 1;
        for p in img.pixels_mut() {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            p.0[0] = (s >> 16) as u8;
        }
        let mut c = Canvas::new(800, 480);
        c.draw_gray(&img, 0, 0, true);
        assert!(c.to_png().unwrap().len() < 90_000);
    }
}
