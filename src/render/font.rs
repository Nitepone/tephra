//! Text: the two bundled faces (Noto Sans Regular and Bold, OFL), measuring, wrapping and drawing.

use super::Canvas;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use std::sync::OnceLock;

static REGULAR: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");

struct Faces {
    regular: FontRef<'static>,
    bold: FontRef<'static>,
}

fn faces() -> &'static Faces {
    static F: OnceLock<Faces> = OnceLock::new();
    F.get_or_init(|| Faces {
        regular: FontRef::try_from_slice(REGULAR).expect("bundled font"),
        bold: FontRef::try_from_slice(BOLD).expect("bundled font"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub bold: bool,
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
        Some(Self { size, bold })
    }

    fn font(&self) -> &'static FontRef<'static> {
        let f = faces();
        if self.bold { &f.bold } else { &f.regular }
    }

    pub fn ascent(&self) -> f32 {
        self.font().as_scaled(PxScale::from(self.size)).ascent()
    }

    pub fn line_height(&self) -> f32 {
        let s = self.font().as_scaled(PxScale::from(self.size));
        (s.ascent() - s.descent() + s.line_gap()).ceil()
    }

    pub fn width(&self, text: &str) -> f32 {
        let s = self.font().as_scaled(PxScale::from(self.size));
        let mut w = 0.0;
        let mut prev = None;
        for c in text.chars() {
            let id = s.glyph_id(c);
            if let Some(p) = prev {
                w += s.kern(p, id);
            }
            w += s.h_advance(id);
            prev = Some(id);
        }
        w
    }

    /// Breaks `text` into lines no wider than `max_w`: at newlines, then between words, and
    /// inside a word only when the word alone is too wide.
    pub fn wrap(&self, text: &str, max_w: f32) -> Vec<String> {
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
                        line.pop();
                        lines.push(std::mem::take(&mut line));
                        line.push(c);
                    }
                }
            }
            lines.push(line);
        }
        lines
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

    /// Draws one line with its top at `y`. Glyph coverage is thresholded, so text stays crisp.
    pub fn draw(&self, c: &mut Canvas, x: f32, y: f32, text: &str, ink: u8) {
        let font = self.font();
        let s = font.as_scaled(PxScale::from(self.size));
        let baseline = y + s.ascent();
        let mut pen = x;
        let mut prev = None;
        for ch in text.chars() {
            let id = s.glyph_id(ch);
            if let Some(p) = prev {
                pen += s.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(self.size), point(pen, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    if cov >= 0.45 {
                        c.set(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, ink);
                    }
                });
            }
            pen += s.h_advance(id);
            prev = Some(id);
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
}
