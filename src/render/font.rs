//! Text: the bundled typefaces (assets/fonts, with their licenses), measuring, wrapping and
//! drawing. A character a face doesn't have is drawn from Noto Sans, and failing that from
//! DejaVu Sans, which has most symbols (● ✓ ★ → ▶ …). Text is put in Unicode's composed form
//! (NFC) first, so a letter and a separate accent become the one accented letter fonts have.

use super::Canvas;
use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use std::sync::OnceLock;

/// A typeface family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    #[default]
    Sans,
    Serif,
    Mono,
    Overpass,
}

impl Face {
    pub const NAMES: &'static str = "sans, serif, mono or overpass";

    pub fn parse(s: &str) -> Option<Face> {
        Some(match s {
            "sans" => Face::Sans,
            "serif" => Face::Serif,
            "mono" => Face::Mono,
            "overpass" => Face::Overpass,
            _ => return None,
        })
    }
}

/// (family, bold, italic, file). Missing combinations fall back: see `font`.
const FILES: &[(Face, bool, bool, &[u8])] = &[
    (Face::Sans, false, false, include_bytes!("../../assets/fonts/NotoSans-Regular.ttf")),
    (Face::Sans, true, false, include_bytes!("../../assets/fonts/NotoSans-Bold.ttf")),
    (Face::Serif, false, false, include_bytes!("../../assets/fonts/NotoSerif-Regular.ttf")),
    (Face::Serif, true, false, include_bytes!("../../assets/fonts/NotoSerif-Bold.ttf")),
    (Face::Serif, false, true, include_bytes!("../../assets/fonts/NotoSerif-Italic.ttf")),
    (Face::Mono, false, false, include_bytes!("../../assets/fonts/NotoSansMono-Regular.ttf")),
    (Face::Mono, true, false, include_bytes!("../../assets/fonts/NotoSansMono-Bold.ttf")),
    (Face::Overpass, false, false, include_bytes!("../../assets/fonts/overpass-regular.otf")),
    (Face::Overpass, true, false, include_bytes!("../../assets/fonts/overpass-bold.otf")),
];

/// The last resort, for symbols: DejaVu Sans, regular and bold.
const SYMBOLS: [&[u8]; 2] = [include_bytes!("../../assets/fonts/DejaVuSans.ttf"), include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf")];

fn symbols(bold: bool) -> &'static FontRef<'static> {
    static F: OnceLock<[FontRef<'static>; 2]> = OnceLock::new();
    &F.get_or_init(|| SYMBOLS.map(|d| FontRef::try_from_slice(d).expect("bundled font")))[bold as usize]
}

fn loaded() -> &'static Vec<(Face, bool, bool, FontRef<'static>)> {
    static F: OnceLock<Vec<(Face, bool, bool, FontRef<'static>)>> = OnceLock::new();
    F.get_or_init(|| FILES.iter().map(|(f, b, i, data)| (*f, *b, *i, FontRef::try_from_slice(data).expect("bundled font"))).collect())
}

/// The closest file: the exact style, then without italic, then without bold, then Noto Sans.
fn font(face: Face, bold: bool, italic: bool) -> &'static FontRef<'static> {
    let all = loaded();
    let find = |f: Face, b: bool, i: bool| all.iter().find(|(ff, bb, ii, _)| *ff == f && *bb == b && *ii == i).map(|t| &t.3);
    find(face, bold, italic)
        .or_else(|| find(face, bold, false))
        .or_else(|| find(face, false, italic))
        .or_else(|| find(face, false, false))
        .or_else(|| find(Face::Sans, bold, false))
        .expect("Noto Sans is bundled")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The line's height in pixels (ascent plus descent).
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub face: Face,
    /// Dither the edges instead of thresholding them: softer curves for large text.
    pub smooth: bool,
}

/// One glyph placed on a line.
struct Placed {
    font: &'static FontRef<'static>,
    scale: PxScale,
    id: GlyphId,
    x: f32,
}

/// The em size, in pixels, of `font` at `scale` (ab_glyph scales by ascent minus descent).
fn em_px(font: &FontRef, scale: PxScale) -> f32 {
    scale.y * font.units_per_em().unwrap_or(1000.0) / font.height_unscaled()
}

/// The scale at which `font`'s letters are as big as another font's at `em` pixels.
fn scale_for_em(font: &FontRef, em: f32) -> PxScale {
    PxScale::from(em * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0))
}

/// Whether `font` has a glyph for `c`.
fn has(font: &FontRef, c: char) -> bool {
    font.glyph_id(c).0 != 0
}

/// `text` in composed form (NFC), borrowed when it already is.
fn composed(text: &str) -> std::borrow::Cow<'_, str> {
    use unicode_normalization::{UnicodeNormalization, is_nfc_quick, IsNormalized};
    match is_nfc_quick(text.chars()) {
        IsNormalized::Yes => std::borrow::Cow::Borrowed(text),
        _ => std::borrow::Cow::Owned(text.nfc().collect()),
    }
}

impl TextStyle {
    /// The type scale apps choose from by name.
    pub fn named(name: &str) -> Option<Self> {
        let (size, bold) = match name {
            "small" => (16.0, false),
            "body" => (22.0, false),
            "label" => (16.0, true),
            "title" => (26.0, true),
            "large" => (44.0, true),
            "huge" => (96.0, true),
            _ => return None,
        };
        Some(Self { size, bold, italic: false, face: Face::Sans, smooth: false })
    }

    fn primary(&self) -> &'static FontRef<'static> {
        font(self.face, self.bold, self.italic)
    }

    fn scale(&self) -> PxScale {
        PxScale::from(self.size)
    }

    pub fn ascent(&self) -> f32 {
        self.primary().as_scaled(self.scale()).ascent()
    }

    /// A line's height: the size, whatever extra spacing a font asks for.
    pub fn line_height(&self) -> f32 {
        let s = self.primary().as_scaled(self.scale());
        (s.ascent() - s.descent()).ceil()
    }

    /// The fonts tried for each character, in order, with the scale that matches their letter
    /// size to the face's.
    fn chain(&self) -> [(&'static FontRef<'static>, PxScale); 3] {
        let primary = self.primary();
        let em = em_px(primary, self.scale());
        let sans = font(Face::Sans, self.bold, false);
        let sym = symbols(self.bold);
        [(primary, self.scale()), (sans, scale_for_em(sans, em)), (sym, scale_for_em(sym, em))]
    }

    /// Places each character, from the first font in `chain` that has it.
    fn place(&self, text: &str) -> (Vec<Placed>, f32) {
        let text = composed(text);
        let chain = self.chain();
        let mut out = Vec::with_capacity(text.len());
        let mut pen = 0.0;
        let mut prev: Option<(&FontRef, GlyphId)> = None;
        for c in text.chars() {
            let (f, scale, id) = chain
                .iter()
                .find(|(f, _)| has(f, c))
                .map(|(f, s)| (*f, *s, f.glyph_id(c)))
                .unwrap_or((chain[0].0, chain[0].1, GlyphId(0)));
            let s = f.as_scaled(scale);
            if let Some((pf, pid)) = prev
                && std::ptr::eq(pf, f)
            {
                pen += s.kern(pid, id);
            }
            out.push(Placed { font: f, scale, id, x: pen });
            pen += s.h_advance(id);
            prev = Some((f, id));
        }
        (out, pen)
    }

    /// The characters of `text` that no bundled font has, each once.
    pub fn missing(&self, text: &str) -> String {
        let chain = self.chain();
        let mut out = String::new();
        for c in composed(text).chars() {
            if !c.is_whitespace() && !c.is_control() && chain.iter().all(|(f, _)| !has(f, c)) && !out.contains(c) {
                out.push(c);
            }
        }
        out
    }

    pub fn width(&self, text: &str) -> f32 {
        self.place(text).1
    }

    /// Breaks `text` into lines no wider than `max_w`: at newlines, then between words, and
    /// inside a word only when the word alone is too wide.
    pub fn wrap(&self, text: &str, max_w: f32) -> Vec<String> {
        self.wrap_reporting(text, max_w).0
    }

    /// `wrap`, and whether a word had to be split because it was wider than the line.
    pub fn wrap_reporting(&self, text: &str, max_w: f32) -> (Vec<String>, bool) {
        let mut split_word = false;
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split_whitespace() {
                let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if self.width(&candidate) <= max_w {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                // The word alone: split it if it still doesn't fit.
                for c in word.chars() {
                    line.push(c);
                    if self.width(&line) > max_w && line.chars().count() > 1 {
                        split_word = true;
                        line.pop();
                        lines.push(std::mem::take(&mut line));
                        line.push(c);
                    }
                }
            }
            lines.push(line);
        }
        (lines, split_word)
    }

    /// `text` cut to fit `max_w`, ending in an ellipsis when it was cut.
    pub fn truncate(&self, text: &str, max_w: f32) -> String {
        if self.width(text) <= max_w {
            return text.to_string();
        }
        let mut s: String = text.to_string();
        while !s.is_empty() {
            s.pop();
            let t = format!("{}…", s.trim_end());
            if self.width(&t) <= max_w {
                return t;
            }
        }
        String::new()
    }

    /// Draws one line with its top at `y`. Glyph coverage is thresholded, so text stays crisp;
    /// with `smooth`, it is dithered instead.
    pub fn draw(&self, c: &mut Canvas, x: f32, y: f32, text: &str, ink: u8) {
        let baseline = y + self.ascent();
        let outlines: Vec<_> = self
            .place(text)
            .0
            .into_iter()
            .filter_map(|p| p.font.outline_glyph(p.id.with_scale_and_position(p.scale, point(x + p.x, baseline))))
            .collect();
        if !self.smooth {
            for o in &outlines {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    if cov >= 0.45 {
                        c.set(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, ink);
                    }
                });
            }
            return;
        }
        // Gather the line's coverage, then diffuse it (Atkinson, like images).
        let Some((x0, y0, x1, y1)) = outlines.iter().map(|o| o.px_bounds()).fold(None, |acc: Option<(f32, f32, f32, f32)>, b| {
            Some(match acc {
                None => (b.min.x, b.min.y, b.max.x, b.max.y),
                Some((a, bb, cc, d)) => (a.min(b.min.x), bb.min(b.min.y), cc.max(b.max.x), d.max(b.max.y)),
            })
        }) else {
            return;
        };
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        let (w, h) = ((x1.ceil() as i32 - x0).max(1) as usize, (y1.ceil() as i32 - y0).max(1) as usize);
        let mut cov = vec![0f32; w * h];
        for o in &outlines {
            let b = o.px_bounds();
            let (ox, oy) = (b.min.x as i32 - x0, b.min.y as i32 - y0);
            o.draw(|gx, gy, v| {
                let (px, py) = (ox + gx as i32, oy + gy as i32);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let cell = &mut cov[py as usize * w + px as usize];
                    *cell = (*cell + v).min(1.0);
                }
            });
        }
        for j in 0..h {
            for i in 0..w {
                let old = cov[j * w + i];
                if old <= 0.0 {
                    continue;
                }
                let on = old >= 0.5;
                if on {
                    c.set(x0 + i as i32, y0 + j as i32, ink);
                }
                let e = (old - if on { 1.0 } else { 0.0 }) / 8.0;
                for (di, dj) in [(1isize, 0usize), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)] {
                    let (ni, nj) = (i as isize + di, j + dj);
                    if ni >= 0 && (ni as usize) < w && nj < h {
                        cov[nj * w + ni as usize] += e;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_between_words_and_inside_long_ones() {
        let st = TextStyle::named("body").unwrap();
        let lines = st.wrap("the quick brown fox jumps over the lazy dog", 120.0);
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| st.width(l) <= 120.0));
        let long = st.wrap("Supercalifragilisticexpialidocious", 80.0);
        assert!(long.len() > 1 && long.iter().all(|l| st.width(l) <= 80.0));
    }

    #[test]
    fn truncates_with_an_ellipsis() {
        let st = TextStyle::named("body").unwrap();
        let t = st.truncate("a fairly long line of text", 100.0);
        assert!(t.ends_with('…') && st.width(&t) <= 100.0);
    }

    #[test]
    fn faces_differ_and_mono_is_monospaced() {
        let mut st = TextStyle::named("body").unwrap();
        let sans = st.width("Illuminati");
        st.face = Face::Mono;
        assert_eq!(st.width("iiii"), st.width("WWWW"));
        assert_ne!(st.width("Illuminati"), sans);
        st.face = Face::Serif;
        st.italic = true;
        assert!(st.width("Illuminati") > 0.0);
    }

    #[test]
    fn symbols_come_from_dejavu_and_line_height_is_the_size() {
        for face in [Face::Sans, Face::Serif, Face::Mono, Face::Overpass] {
            let st = TextStyle { face, ..TextStyle::named("title").unwrap() };
            assert_eq!(st.missing("● ○ ✓ ▶ ★ ━ → São"), "", "{face:?}");
            assert_eq!(st.line_height(), 26.0, "{face:?}");
        }
        let st = TextStyle::named("body").unwrap();
        assert_eq!(st.missing("ok 日本 \u{E000}"), "日本\u{E000}");
    }

    #[test]
    fn decomposed_accents_are_composed() {
        let st = TextStyle { face: Face::Overpass, ..TextStyle::named("large").unwrap() };
        assert_eq!(st.width("SA\u{303}O"), st.width("S\u{C3}O"));
    }

    #[test]
    fn missing_characters_come_from_noto_sans() {
        let overpass = font(Face::Overpass, false, false);
        let sans = font(Face::Sans, false, false);
        let c = ['→', 'Ж', 'Ω', '₹', '≈']
            .into_iter()
            .find(|&c| overpass.glyph_id(c).0 == 0 && sans.glyph_id(c).0 != 0)
            .expect("a character Overpass lacks and Noto Sans has");
        let mut st = TextStyle::named("title").unwrap();
        st.face = Face::Overpass;
        let mut cv = Canvas::new(200, 60);
        st.draw(&mut cv, 10.0, 10.0, &c.to_string(), super::super::BLACK);
        let ink = (0..60).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| cv.get(x, y) == 0).count();
        assert!(ink > 20, "the arrow was drawn ({ink} pixels)");
    }
}
