//! Drawing: a canvas, text, images, the layout tree, and the PNG the device downloads.
//!
//! A canvas has 2 grays (black and white) or 4 (with dark and light gray between). Text is
//! thresholded and images are dithered to those levels as they are drawn. A screen with no gray
//! on it is sent as a 1-bit PNG, which the OG draws with a fast refresh; one with gray as a 2-bit
//! PNG, which the OG draws in its 4-gray mode with a full refresh.

pub mod font;
pub mod layout;

use anyhow::Context;

pub const WHITE: u8 = 255;
pub const BLACK: u8 = 0;
/// The largest image an OG (no PSRAM) will download.
pub const OG_IMAGE_LIMIT: usize = 90_000;

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
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
    /// 2 (black and white) or 4 (also 85 and 170).
    pub grays: u8,
    /// When set, every node drawn adds where it went and what didn't fit (see `layout::LayoutEntry`).
    pub report: Option<Vec<layout::LayoutEntry>>,
    /// The child indexes leading to the node being drawn, for the report.
    pub(crate) path: Vec<usize>,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        let (w, h) = (w as i32, h as i32);
        Self { w, h, px: vec![WHITE; (w * h) as usize], clip: Rect::new(0, 0, w, h), grays: 2, report: None, path: Vec::new() }
    }

    pub fn with_grays(mut self, grays: u8) -> Self {
        self.grays = if grays >= 4 { 4 } else { 2 };
        self
    }

    /// The nearest level this canvas can show to `v` (0 black .. 255 white).
    #[inline]
    pub fn quantize(&self, v: f32) -> u8 {
        if self.grays >= 4 {
            ((v / 85.0).round().clamp(0.0, 3.0) * 85.0) as u8
        } else if v >= 128.0 {
            WHITE
        } else {
            BLACK
        }
    }

    /// Whether anything on it is neither black nor white.
    pub fn has_gray(&self) -> bool {
        self.px.iter().any(|&v| v != BLACK && v != WHITE)
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

    /// The pixel at (x, y), or white outside the canvas.
    pub fn get_or_white(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && y >= 0 && x < self.w && y < self.h { self.get(x, y) } else { WHITE }
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

    /// Draws a grayscale image (0 black .. 255 white) at (x, y), dithered (Atkinson) or
    /// thresholded to the canvas's grays.
    pub fn draw_gray(&mut self, img: &image::GrayImage, x: i32, y: i32, dither: bool) {
        let (iw, ih) = (img.width() as usize, img.height() as usize);
        let mut err: Vec<f32> = img.as_raw().iter().map(|&v| v as f32).collect();
        for j in 0..ih {
            for i in 0..iw {
                let old = err[j * iw + i];
                let new = self.quantize(old) as f32;
                self.set(x + i as i32, y + j as i32, new as u8);
                if !dither {
                    continue;
                }
                // Atkinson spreads 6/8 of the error, which keeps contrast on a panel with few grays.
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

    /// Copies `other` onto this canvas with its top left at (x, y).
    pub fn blit(&mut self, other: &Canvas, x: i32, y: i32) {
        for j in 0..other.h {
            for i in 0..other.w {
                self.set(x + i, y + j, other.get(i, j));
            }
        }
    }

    /// The canvas as a PNG: 1-bit when it is all black and white, else 2-bit.
    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        self.encode(if self.has_gray() { 2 } else { 1 })
    }

    /// The PNG to send a device that downloads at most `limit` bytes. A gray screen too big as
    /// 2-bit is dithered to black and white and sent as 1-bit instead.
    pub fn to_device_png(&self, limit: usize) -> anyhow::Result<Vec<u8>> {
        let png = self.to_png()?;
        if png.len() <= limit || !self.has_gray() {
            return Ok(png);
        }
        let img = image::GrayImage::from_raw(self.w as u32, self.h as u32, self.px.clone()).context("copying the canvas")?;
        let mut bw = Canvas::new(self.w as u32, self.h as u32);
        bw.draw_gray(&img, 0, 0, true);
        bw.encode(1)
    }

    /// Gray PNG at 1 bit (bit set = white) or 2 bits (0 black .. 3 white) per pixel.
    fn encode(&self, bits: usize) -> anyhow::Result<Vec<u8>> {
        let (w, h) = (self.w as usize, self.h as usize);
        let stride = (w * bits).div_ceil(8);
        let per_byte = 8 / bits;
        let mut packed = vec![0u8; stride * h];
        for y in 0..h {
            for x in 0..w {
                let v = self.px[y * w + x];
                let level = if bits == 1 { (v >= 128) as u8 } else { ((v as u16 + 42) / 85).min(3) as u8 };
                let shift = 8 - bits * (x % per_byte + 1);
                packed[y * stride + x / per_byte] |= level << shift;
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(if bits == 1 { png::BitDepth::One } else { png::BitDepth::Two });
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

    fn noise() -> image::GrayImage {
        let mut img = image::GrayImage::new(800, 480);
        let mut s: u32 = 1;
        for p in img.pixels_mut() {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            p.0[0] = (s >> 16) as u8;
        }
        img
    }

    fn depth(png: &[u8]) -> png::BitDepth {
        png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap().info().bit_depth
    }

    #[test]
    fn four_grays_make_a_2_bit_png_only_when_used() {
        let mut c = Canvas::new(800, 480).with_grays(4);
        c.fill(Rect::new(0, 0, 100, 100), BLACK);
        assert_eq!(depth(&c.to_png().unwrap()), png::BitDepth::One);
        let ramp = image::GrayImage::from_fn(256, 10, |x, _| image::Luma([x as u8]));
        c.draw_gray(&ramp, 200, 200, false);
        let png = c.to_png().unwrap();
        assert_eq!(depth(&png), png::BitDepth::Two);
        let back = image::load_from_memory(&png).unwrap().to_luma8();
        let levels: std::collections::BTreeSet<u8> = (200..456).map(|x| back.get_pixel(x, 205).0[0]).collect();
        assert_eq!(levels.into_iter().collect::<Vec<_>>(), vec![0, 85, 170, 255]);
        assert_eq!(back.get_pixel(50, 50).0[0], 0);
    }

    #[test]
    fn a_gray_screen_too_big_for_the_og_goes_as_1_bit() {
        let mut c = Canvas::new(800, 480).with_grays(4);
        c.draw_gray(&noise(), 0, 0, true);
        assert!(c.to_png().unwrap().len() > OG_IMAGE_LIMIT);
        let png = c.to_device_png(OG_IMAGE_LIMIT).unwrap();
        assert_eq!(depth(&png), png::BitDepth::One);
        assert!(png.len() <= OG_IMAGE_LIMIT);
    }

    #[test]
    fn a_dithered_photo_stays_under_the_og_limit() {
        // Noise is the worst case for compression.
        let img = noise();
        let mut c = Canvas::new(800, 480);
        c.draw_gray(&img, 0, 0, true);
        assert!(c.to_png().unwrap().len() < 90_000);
    }
}
