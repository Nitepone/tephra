//! The layout tree apps return, and a small flexbox-like layout that draws it.
//!
//! Stacks (`column`, `row`, `box`) place their children one after another along their axis.
//! Children that set `grow` share whatever space is left over; otherwise `justify` places the
//! group. Across the axis, `align` places each child (default: stretch to the full width/height).

use super::font::TextStyle;
use super::{BLACK, Canvas, Rect, WHITE};
use image::GrayImage;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dir {
    Column,
    Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
    /// Rows only: children placed so their first lines of text share a baseline.
    Baseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
}

/// A width for each side, such as a box's border.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub const ZERO: Edges = Edges { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 };
    pub fn uniform(v: f32) -> Edges {
        Edges { top: v, right: v, bottom: v, left: v }
    }
    fn any(&self) -> bool {
        self.top > 0.0 || self.right > 0.0 || self.bottom > 0.0 || self.left > 0.0
    }
    /// With `padding` inside each side, in whole pixels: (left, top, right, bottom).
    fn inset(&self, padding: f32) -> (i32, i32, i32, i32) {
        let r = |v: f32| (padding + v).round() as i32;
        (r(self.left), r(self.top), r(self.right), r(self.bottom))
    }
}

/// A background: a gray, or a pattern of black on white.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill {
    Gray(u8),
    /// Diagonal lines.
    Hatch,
    /// Diagonal lines both ways.
    Cross,
    Dots,
}

/// What every node may set.
#[derive(Debug, Clone, Default)]
pub struct Common {
    /// Share of the parent's leftover space along its axis (0 = none).
    pub grow: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// A background, drawn before the node. Its content is drawn white on dark fills and black
    /// on light ones and patterns, unless `color` says otherwise.
    pub fill: Option<Fill>,
    /// The color of the node's text and lines, and its children's.
    pub color: Option<u8>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Stack {
        dir: Dir,
        children: Vec<Node>,
        gap: f32,
        align: Align,
        justify: Justify,
        /// Only set for `box`: padding, border widths and white-on-black.
        padding: f32,
        border: Edges,
        invert: bool,
        common: Common,
    },
    Text {
        text: String,
        style: TextStyle,
        wrap: bool,
        align: TextAlign,
        max_lines: Option<usize>,
        /// Shrink to fit: the smallest size the text may go down to, to fit without being cut.
        shrink_to: Option<f32>,
        common: Common,
    },
    /// The first child that fits the space uncut; the last if none does.
    First {
        children: Vec<Node>,
        common: Common,
    },
    /// Children drawn on top of each other, each over the whole space, in order.
    Layers {
        children: Vec<Node>,
        common: Common,
    },
    Image {
        img: Arc<GrayImage>,
        /// Where the image is see-through (below 128), for images with transparency kept.
        mask: Option<Arc<GrayImage>>,
        fit: Fit,
        dither: bool,
        /// Softens the image after scaling (a Gaussian blur of this many pixels), so coarse
        /// data such as radar comes out as smooth shapes.
        blur: f32,
        /// Black below this gray and white from it, after scaling: no dithering, no grays.
        threshold: Option<u8>,
        /// Light for dark and dark for light.
        invert: bool,
        common: Common,
    },
    Rule {
        thickness: f32,
        /// Dashes: (on, off) lengths in pixels.
        dash: Option<(f32, f32)>,
        common: Common,
    },
    Spacer {
        common: Common,
    },
}

/// Where one node was drawn, and what of it didn't fit. Collected when `Canvas::report` is set.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LayoutEntry {
    /// `root`, then the child indexes from it, counting from 0: `root/2/0`.
    pub path: String,
    pub kind: &'static str,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Text: the font size, and how many lines were drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<usize>,
    /// What didn't fit: `too_wide` (a line ended in "…"), `max_lines` (more lines than allowed),
    /// `too_tall` (lines below the bottom were left out), `word_split` (a word was broken),
    /// `no_space` (the node got no room at all), `missing_glyph` (characters no font has, listed in
    /// `missing`; they show as empty boxes).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
    /// `first`: which child fitted (counting from 0), and was drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen: Option<usize>,
    /// Containers: how many pixels the children needed beyond the space there was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow_px: Option<i32>,
    /// Images: the source size and the size drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<[u32; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Size {
    w: f32,
    h: f32,
}

impl Node {
    pub fn common(&self) -> &Common {
        match self {
            Node::Stack { common, .. }
            | Node::Text { common, .. }
            | Node::Image { common, .. }
            | Node::Layers { common, .. }
            | Node::First { common, .. }
            | Node::Rule { common, .. }
            | Node::Spacer { common } => common,
        }
    }

    /// A text node in the given style, for the server's own screens (errors, welcome, identify).
    pub fn text(text: impl Into<String>, style: &str) -> Node {
        Node::Text {
            text: text.into(),
            style: TextStyle::named(style).unwrap(),
            wrap: true,
            align: TextAlign::Left,
            max_lines: None,
            shrink_to: None,
            common: Common::default(),
        }
    }

    pub fn column(children: Vec<Node>, gap: f32) -> Node {
        Node::Stack {
            dir: Dir::Column,
            children,
            gap,
            align: Align::Stretch,
            justify: Justify::Start,
            padding: 0.0,
            border: Edges::ZERO,
            invert: false,
            common: Common::default(),
        }
    }

    pub fn centered(mut self) -> Node {
        if let Node::Text { align, .. } = &mut self {
            *align = TextAlign::Center;
        }
        if let Node::Stack { justify, .. } = &mut self {
            *justify = Justify::Center;
        }
        self
    }

    /// The size this node wants within `max_w` x `max_h`, laid out in a parent running along `parent`.
    fn measure(&self, max_w: f32, max_h: f32, parent: Dir) -> Size {
        let c = self.common();
        let (max_w, max_h) = (c.width.unwrap_or(max_w).min(max_w), c.height.unwrap_or(max_h).min(max_h));
        let s = match self {
            Node::Text { text, style, wrap, max_lines, .. } => {
                let lines = text_lines(text, *style, *wrap, *max_lines, max_w).lines;
                let w = lines.iter().map(|l| style.width(l)).fold(0.0, f32::max);
                Size { w: w.ceil(), h: style.line_height() * lines.len() as f32 }
            }
            Node::Image { img, .. } => {
                let (iw, ih) = (img.width() as f32, img.height() as f32);
                let scale = (max_w / iw).min(max_h / ih).clamp(0.0, 1.0);
                Size { w: (iw * scale).floor(), h: (ih * scale).floor() }
            }
            Node::Rule { thickness, .. } => match parent {
                Dir::Column => Size { w: 0.0, h: *thickness },
                Dir::Row => Size { w: *thickness, h: 0.0 },
            },
            Node::Spacer { .. } => Size { w: 0.0, h: 0.0 },
            Node::Layers { children, .. } => children.iter().fold(Size { w: 0.0, h: 0.0 }, |acc, ch| {
                let s = ch.measure(max_w, max_h, parent);
                Size { w: acc.w.max(s.w), h: acc.h.max(s.h) }
            }),
            Node::First { children, .. } => match children.get(self.choose(max_w, max_h)) {
                Some(ch) => ch.measure(max_w, max_h, parent),
                None => Size { w: 0.0, h: 0.0 },
            },
            Node::Stack { dir, children, gap, padding, border, align, .. } => {
                // Rounded as `draw` rounds it.
                let (l, t, r, b) = border.inset(*padding);
                let (inset_w, inset_h) = ((l + r) as f32, (t + b) as f32);
                let (iw, ih) = ((max_w - inset_w).max(0.0), (max_h - inset_h).max(0.0));
                let sizes = measure_children(*dir, children, *gap, iw, ih);
                let n = children.len().max(1) as f32;
                // Growing children share the leftover space equally (by `grow`), whatever their
                // own size, so the space must give each its share of the one that needs most.
                let total_grow: f32 = children.iter().map(|c| c.common().grow.max(0.0)).sum();
                let (mut fixed, mut per_grow, mut cross) = (0.0f32, 0.0f32, 0.0f32);
                for (ch, s) in children.iter().zip(&sizes) {
                    let (sm, sx) = split(*dir, *s);
                    let g = ch.common().grow;
                    if g > 0.0 { per_grow = per_grow.max(sm / g) } else { fixed += sm }
                    cross = cross.max(sx);
                }
                let main = (gap * (n - 1.0) + fixed + per_grow * total_grow).min(split(*dir, Size { w: iw, h: ih }).0.max(fixed));
                if *dir == Dir::Row && *align == Align::Baseline {
                    let (above, below) = baseline_extents(children, &sizes);
                    cross = cross.max(above + below);
                }
                let (w, h) = join(*dir, main, cross);
                Size { w: w + inset_w, h: h + inset_h }
            }
        };
        // Whole pixels, rounded up: a node given what it measured never comes up short.
        Size {
            w: c.width.unwrap_or(s.w).ceil().min(max_w),
            h: c.height.unwrap_or(s.h).ceil().min(max_h),
        }
    }

    /// For `First`: which child to draw in `w` x `h` (either may be unlimited).
    fn choose(&self, w: f32, h: f32) -> usize {
        let Node::First { children, .. } = self else { return 0 };
        for (i, ch) in children.iter().enumerate().take(children.len().saturating_sub(1)) {
            let s = ch.measure(w, h, Dir::Column);
            let (tw, th) = (if w.is_finite() { w } else { s.w }, if h.is_finite() { h } else { s.h });
            if ch.trial(tw, th).0.iter().all(|c| *c == "missing_glyph") {
                return i;
            }
        }
        children.len().saturating_sub(1)
    }

    /// Lays this node out in `w` x `h` exactly as a render would, on a scratch canvas: what was
    /// cut, sorted (with `overflow` for children that needed more room), and the characters no
    /// font has.
    pub fn trial(&self, w: f32, h: f32) -> (Vec<&'static str>, String) {
        let (w, h) = (w.clamp(1.0, 4000.0), h.clamp(1.0, 4000.0));
        let mut cv = Canvas::new(w as u32, h as u32);
        cv.report = Some(Vec::new());
        self.draw(&mut cv, Rect::new(0, 0, w as i32, h as i32), BLACK, Dir::Column);
        let report = cv.report.take().unwrap_or_default();
        let mut cut: Vec<&'static str> = report.iter().flat_map(|e| e.cut.iter().copied()).collect();
        if report.iter().any(|e| e.overflow_px.is_some()) {
            cut.push("overflow");
        }
        cut.sort();
        cut.dedup();
        (cut, report.iter().filter_map(|e| e.missing.as_deref()).collect())
    }

    /// How far below the top of this node, drawn `w` wide, its first line of text sits on its
    /// baseline; None if it has no text.
    fn baseline(&self, w: f32, h: f32) -> Option<f32> {
        match self {
            Node::Text { text, style, wrap, max_lines, shrink_to, .. } => {
                let st = match shrink_to {
                    Some(min) => shrunk(text, *style, *wrap, *max_lines, *min, Rect::new(0, 0, w as i32, h as i32)),
                    None => *style,
                };
                Some(st.ascent())
            }
            Node::Stack { dir, children, gap, padding, border, .. } => {
                let (l, t, r, b) = border.inset(*padding);
                let inset = t as f32;
                let (iw, ih) = ((w - (l + r) as f32).max(0.0), (h - (t + b) as f32).max(0.0));
                let sizes = measure_children(*dir, children, *gap, iw, ih);
                let mut offset = 0.0;
                for (ch, s) in children.iter().zip(&sizes) {
                    if let Some(b) = ch.baseline(if *dir == Dir::Row { s.w } else { iw }, s.h) {
                        return Some(inset + offset + b);
                    }
                    if *dir == Dir::Column {
                        offset += s.h + gap;
                    }
                }
                None
            }
            Node::Layers { children, .. } => children.iter().find_map(|c| c.baseline(w, h)),
            Node::First { children, .. } => children.get(self.choose(w, h)).and_then(|c| c.baseline(w, h)),
            _ => None,
        }
    }

    /// The size this node takes within `max_w` x `max_h`, and for text the number of lines.
    pub fn measured(&self, max_w: f32, max_h: f32) -> (f32, f32, Option<usize>) {
        let s = self.measure(max_w, max_h, Dir::Column);
        let lines = match self {
            Node::Text { text, style, wrap, max_lines, .. } => Some(text_lines(text, *style, *wrap, *max_lines, max_w).lines.len()),
            _ => None,
        };
        (s.w, s.h, lines)
    }

    fn kind(&self) -> &'static str {
        match self {
            Node::Stack { invert, border, padding, .. } if *invert || border.any() || *padding > 0.0 => "box",
            Node::Stack { dir: Dir::Row, .. } => "row",
            Node::Stack { .. } => "column",
            Node::Text { .. } => "text",
            Node::Image { .. } => "image",
            Node::Layers { .. } => "layers",
            Node::First { .. } => "first",
            Node::Rule { .. } => "rule",
            Node::Spacer { .. } => "spacer",
        }
    }

    /// Adds this node to the canvas's report, if it keeps one; its index there.
    fn report(&self, cv: &mut Canvas, r: Rect) -> Option<usize> {
        let path = std::iter::once("root".to_string()).chain(cv.path.iter().map(|i| i.to_string())).collect::<Vec<_>>().join("/");
        let report = cv.report.as_mut()?;
        let mut e = LayoutEntry {
            path,
            kind: self.kind(),
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            text: None,
            size: None,
            lines: None,
            cut: Vec::new(),
            missing: None,
            chosen: None,
            overflow_px: None,
            image: None,
        };
        if let Node::Text { text, style, .. } = self {
            e.text = Some(text.chars().take(80).collect());
            e.size = Some(style.size);
        }
        if (r.w <= 0 || r.h <= 0) && !matches!(self, Node::Spacer { .. } | Node::Rule { .. }) {
            e.cut.push("no_space");
        }
        report.push(e);
        Some(report.len() - 1)
    }

    fn entry(cv: &mut Canvas, i: Option<usize>) -> Option<&mut LayoutEntry> {
        cv.report.as_mut().and_then(|r| r.get_mut(i?))
    }

    /// Draws this node into `r`.
    pub fn draw(&self, cv: &mut Canvas, r: Rect, ink: u8, parent: Dir) {
        let me = self.report(cv, r);
        if r.w <= 0 || r.h <= 0 {
            return;
        }
        let mut ink = ink;
        let c = self.common();
        match c.fill {
            Some(Fill::Gray(f)) => {
                let f = cv.quantize(f as f32);
                cv.fill(r, f);
                ink = if f < 128 { WHITE } else { BLACK };
            }
            Some(pattern) => {
                cv.fill(r, WHITE);
                // On the canvas's own grid, so neighbouring patterns line up.
                let on = |x: i32, y: i32| match pattern {
                    Fill::Hatch => (x + y).rem_euclid(6) == 0,
                    Fill::Cross => (x + y).rem_euclid(6) == 0 || (x - y).rem_euclid(6) == 0,
                    Fill::Dots => x.rem_euclid(4) == 0 && y.rem_euclid(4) == 0,
                    Fill::Gray(_) => false,
                };
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        if on(x, y) {
                            cv.set(x, y, BLACK);
                        }
                    }
                }
                ink = BLACK;
            }
            None => {}
        }
        if let Some(color) = c.color {
            ink = cv.quantize(color as f32);
        }
        // What a node may draw on. Text may reach a little above and below its box, as accents
        // and descenders do in fonts whose lines leave them no room; never sideways. Containers
        // don't clip: their children are always placed inside them, and an accent at the top of
        // a row must be able to rise above it. (The slot itself is clipped by `screen::compose`.)
        let clip = match self {
            Node::Text { style, .. } => {
                let m = (style.size * 0.3).ceil() as i32;
                cv.push_clip(Rect::new(r.x, r.y - m, r.w, r.h + 2 * m))
            }
            Node::Stack { .. } => cv.push_clip(Rect::new(i32::MIN / 4, i32::MIN / 4, i32::MAX / 2, i32::MAX / 2)),
            _ => cv.push_clip(r),
        };
        match self {
            Node::Text { text, style, wrap, align, max_lines, shrink_to, .. } => {
                let style = &match shrink_to {
                    Some(min) => shrunk(text, *style, *wrap, *max_lines, *min, r),
                    None => *style,
                };
                let lh = style.line_height();
                let tl = text_lines(text, *style, *wrap, *max_lines, r.w as f32);
                let lines = tl.lines;
                // Only whole lines; at least the first.
                let fit = ((r.h as f32 / lh).floor() as usize).max(1);
                if let Some(e) = Self::entry(cv, me) {
                    e.size = Some(style.size);
                    e.lines = Some(lines.len().min(fit));
                    for (cut, why) in [(tl.too_wide, "too_wide"), (tl.max_lines, "max_lines"), (lines.len() > fit, "too_tall"), (tl.split_word, "word_split")] {
                        if cut {
                            e.cut.push(why);
                        }
                    }
                    let missing = style.missing(text);
                    if !missing.is_empty() {
                        e.cut.push("missing_glyph");
                        e.missing = Some(missing);
                    }
                }
                for (i, line) in lines.iter().take(fit).enumerate() {
                    let line = if i + 1 == fit && lines.len() > fit { style.truncate(&format!("{line}…"), r.w as f32) } else { line.clone() };
                    let lw = style.width(&line);
                    let x = match align {
                        TextAlign::Left => r.x as f32,
                        TextAlign::Center => r.x as f32 + (r.w as f32 - lw) / 2.0,
                        TextAlign::Right => r.x as f32 + r.w as f32 - lw,
                    };
                    style.draw(cv, x.round(), r.y as f32 + i as f32 * lh, &line, ink);
                }
            }
            Node::Image { img, mask, fit, dither, blur, threshold, invert, .. } => {
                let (iw, ih) = (img.width() as f32, img.height() as f32);
                let scale = match fit {
                    Fit::Contain => (r.w as f32 / iw).min(r.h as f32 / ih),
                    Fit::Cover => (r.w as f32 / iw).max(r.h as f32 / ih),
                };
                let (w, h) = ((iw * scale).round().max(1.0) as u32, (ih * scale).round().max(1.0) as u32);
                if let Some(e) = Self::entry(cv, me) {
                    e.image = Some([img.width(), img.height(), w, h]);
                }
                let soften = |i: image::GrayImage| if *blur > 0.0 { image::imageops::blur(&i, *blur) } else { i };
                let mut resized = soften(image::imageops::resize(img.as_ref(), w, h, image::imageops::FilterType::Triangle));
                if *invert || threshold.is_some() {
                    for p in resized.pixels_mut() {
                        let v = if *invert { 255 - p.0[0] } else { p.0[0] };
                        p.0[0] = match threshold {
                            Some(t) => if v >= *t { WHITE } else { BLACK },
                            None => v,
                        };
                    }
                }
                let mask = mask.as_ref().map(|m| soften(image::imageops::resize(m.as_ref(), w, h, image::imageops::FilterType::Triangle)));
                let x = r.x + (r.w - w as i32) / 2;
                let y = r.y + (r.h - h as i32) / 2;
                cv.draw_gray_masked(&resized, mask.as_ref(), x, y, *dither);
            }
            Node::Rule { thickness, dash, .. } => {
                let t = thickness.round() as i32;
                let line = match parent {
                    Dir::Column => Rect::new(r.x, r.y + (r.h - t) / 2, r.w, t),
                    Dir::Row => Rect::new(r.x + (r.w - t) / 2, r.y, t, r.h),
                };
                match dash {
                    None => cv.fill(line, ink),
                    Some((on, off)) => {
                        let (on, off) = (on.round().max(1.0) as i32, off.round().max(1.0) as i32);
                        let len = if parent == Dir::Column { line.w } else { line.h };
                        let mut at = 0;
                        while at < len {
                            let n = on.min(len - at);
                            cv.fill(
                                if parent == Dir::Column { Rect::new(line.x + at, line.y, n, line.h) } else { Rect::new(line.x, line.y + at, line.w, n) },
                                ink,
                            );
                            at += on + off;
                        }
                    }
                }
            }
            Node::Spacer { .. } => {}
            Node::Layers { children, .. } => {
                for (i, ch) in children.iter().enumerate() {
                    cv.path.push(i);
                    ch.draw(cv, r, ink, parent);
                    cv.path.pop();
                }
            }
            Node::First { children, .. } => {
                let i = self.choose(r.w as f32, r.h as f32);
                if let Some(e) = Self::entry(cv, me) {
                    e.chosen = Some(i);
                }
                if let Some(ch) = children.get(i) {
                    cv.path.push(i);
                    ch.draw(cv, r, ink, parent);
                    cv.path.pop();
                }
            }
            Node::Stack { dir, children, gap, align, justify, padding, border, invert, .. } => {
                let outer = ink;
                let mut ink = ink;
                if *invert {
                    cv.fill(r, outer);
                    ink = if outer == BLACK { WHITE } else { BLACK };
                }
                let side = |v: f32| v.round() as i32;
                let (bl, bt, br, bb) = (side(border.left), side(border.top), side(border.right), side(border.bottom));
                cv.fill(Rect::new(r.x, r.y, r.w, bt), outer);
                cv.fill(Rect::new(r.x, r.y + r.h - bb, r.w, bb), outer);
                cv.fill(Rect::new(r.x, r.y, bl, r.h), outer);
                cv.fill(Rect::new(r.x + r.w - br, r.y, br, r.h), outer);
                let (l, t, rr, b) = border.inset(*padding);
                let inner = Rect::new(r.x + l, r.y + t, r.w - l - rr, r.h - t - b);
                let overflow = draw_children(cv, inner, ink, *dir, children, *gap, *align, *justify);
                if overflow >= 1.0
                    && let Some(e) = Self::entry(cv, me)
                {
                    e.overflow_px = Some(overflow.round() as i32);
                }
            }
        }
        cv.pop_clip(clip);
    }
}

/// The largest size from the style's down to `min` at which the text fits `r` uncut. `min` if
/// none does.
fn shrunk(text: &str, style: TextStyle, wrap: bool, max_lines: Option<usize>, min: f32, r: Rect) -> TextStyle {
    let fits = |size: f32| {
        let st = TextStyle { size, ..style };
        let tl = text_lines(text, st, wrap, max_lines, r.w as f32);
        !tl.too_wide && !tl.max_lines && !tl.split_word && tl.lines.len() as f32 * st.line_height() <= r.h as f32
    };
    let (mut lo, mut hi) = (min.min(style.size).max(1.0), style.size);
    if fits(hi) {
        return style;
    }
    // Whole pixels, by bisection: fits(lo) is assumed (or nothing does, and lo is the floor).
    while hi - lo > 0.5 {
        let mid = ((lo + hi) / 2.0).floor().max(lo);
        if mid <= lo {
            break;
        }
        if fits(mid) { lo = mid } else { hi = mid }
    }
    TextStyle { size: lo, ..style }
}

/// Text broken into the lines it is drawn as, and how it had to be cut to get there.
struct TextLines {
    lines: Vec<String>,
    too_wide: bool,
    max_lines: bool,
    split_word: bool,
}

fn text_lines(text: &str, style: TextStyle, wrap: bool, max_lines: Option<usize>, max_w: f32) -> TextLines {
    let (mut lines, too_wide, split_word) = if wrap {
        let (lines, split) = style.wrap_reporting(text, max_w);
        (lines, false, split)
    } else {
        let lines: Vec<String> = text.split('\n').map(|l| style.truncate(l, max_w)).collect();
        let too_wide = text.split('\n').zip(&lines).any(|(a, b)| a != b);
        (lines, too_wide, false)
    };
    let mut cut_lines = false;
    if let Some(n) = max_lines.filter(|&n| n > 0 && n < lines.len()) {
        lines.truncate(n);
        let last = lines.pop().unwrap_or_default();
        lines.push(style.truncate(&format!("{last}…"), max_w));
        cut_lines = true;
    }
    TextLines { lines, too_wide, max_lines: cut_lines, split_word }
}

/// (main, cross) for a size along `dir`.
fn split(dir: Dir, s: Size) -> (f32, f32) {
    match dir {
        Dir::Column => (s.h, s.w),
        Dir::Row => (s.w, s.h),
    }
}
/// (w, h) for a main and cross extent along `dir`.
fn join(dir: Dir, main: f32, cross: f32) -> (f32, f32) {
    match dir {
        Dir::Column => (cross, main),
        Dir::Row => (main, cross),
    }
}

/// For a row aligned on baselines: how far its children reach above the shared baseline and
/// below it. Children without text are placed at the top.
fn baseline_extents(children: &[Node], sizes: &[Size]) -> (f32, f32) {
    let mut above = 0.0f32;
    let mut below = 0.0f32;
    for (ch, s) in children.iter().zip(sizes) {
        match ch.baseline(s.w, s.h) {
            Some(b) => {
                above = above.max(b);
                below = below.max(s.h - b);
            }
            None => below = below.max(s.h - above.min(s.h)),
        }
    }
    (above, below)
}

/// Each child's size, measured in order with the space the earlier ones left.
fn measure_children(dir: Dir, children: &[Node], gap: f32, w: f32, h: f32) -> Vec<Size> {
    let (mut avail, cross) = split(dir, Size { w, h });
    avail -= gap * (children.len().saturating_sub(1)) as f32;
    children
        .iter()
        .map(|ch| {
            let (cw, chh) = join(dir, avail.max(0.0), cross);
            let s = ch.measure(cw, chh, dir);
            avail -= split(dir, s).0;
            s
        })
        .collect()
}

/// Draws a container's children; returns how many pixels more they needed than there was.
#[allow(clippy::too_many_arguments)]
fn draw_children(cv: &mut Canvas, r: Rect, ink: u8, dir: Dir, children: &[Node], gap: f32, align: Align, justify: Justify) -> f32 {
    let (main_total, cross_total) = split(dir, Size { w: r.w as f32, h: r.h as f32 });
    let gaps = gap * children.len().saturating_sub(1) as f32;

    // Fixed children first, in order, each with what is left; growing ones share the rest.
    let mut mains = vec![0.0f32; children.len()];
    let mut avail = main_total - gaps;
    let mut overflow = (-avail).max(0.0);
    for (i, ch) in children.iter().enumerate() {
        if ch.common().grow <= 0.0 {
            let (cw, chh) = join(dir, avail.max(0.0), cross_total);
            mains[i] = split(dir, ch.measure(cw, chh, dir)).0.min(avail.max(0.0));
            // What it would take with no limit, to report overflow. Only down a column: across a
            // row, wrapping text would ask for its whole length on one line. Text that shrinks
            // to fit takes what it gets.
            if dir == Dir::Column && !matches!(ch, Node::Text { shrink_to: Some(_), .. }) {
                let wanted = ch.measure(cross_total, f32::INFINITY, dir).h;
                overflow += (wanted - mains[i]).max(0.0);
            }
            avail -= mains[i];
        }
    }
    let leftover = avail.max(0.0);
    let total_grow: f32 = children.iter().map(|c| c.common().grow.max(0.0)).sum();
    for (i, ch) in children.iter().enumerate() {
        if ch.common().grow > 0.0 {
            mains[i] = leftover * ch.common().grow / total_grow;
        }
    }

    let used: f32 = mains.iter().sum::<f32>() + gaps;
    let free = if total_grow > 0.0 { 0.0 } else { (main_total - used).max(0.0) };
    let n = children.len() as f32;
    let (mut pos, extra_gap) = match justify {
        Justify::Start => (0.0, 0.0),
        Justify::Center => (free / 2.0, 0.0),
        Justify::End => (free, 0.0),
        Justify::Between if n > 1.0 => (0.0, free / (n - 1.0)),
        Justify::Between => (0.0, 0.0),
    };

    // On a shared baseline: each child's own, and the lowest-reaching one's, from the top.
    let baseline = (dir == Dir::Row && align == Align::Baseline).then(|| {
        let own: Vec<Option<f32>> = children
            .iter()
            .zip(&mains)
            .map(|(ch, &main)| {
                let s = ch.measure(main, cross_total, dir);
                ch.baseline(main, s.h)
            })
            .collect();
        let line = own.iter().flatten().fold(0.0f32, |a, &b| a.max(b));
        (own, line)
    });

    for (i, (ch, &main)) in children.iter().zip(&mains).enumerate() {
        let cross = if align == Align::Stretch || matches!(ch, Node::Rule { .. }) {
            cross_total
        } else {
            let (cw, chh) = join(dir, main, cross_total);
            split(dir, ch.measure(cw, chh, dir)).1.min(cross_total)
        };
        let cross_pos = match align {
            Align::Start | Align::Stretch => 0.0,
            Align::Center => (cross_total - cross) / 2.0,
            Align::End => cross_total - cross,
            Align::Baseline => match &baseline {
                Some((own, line)) => own[i].map(|b| (line - b).max(0.0)).unwrap_or(0.0),
                None => 0.0,
            },
        };
        let (x, y) = join(dir, pos, cross_pos);
        let (w, h) = join(dir, main, cross);
        // Round the edges, not the position and size apart, so no pixels are lost along the way.
        let (x0, y0) = ((r.x as f32 + x).round() as i32, (r.y as f32 + y).round() as i32);
        let (x1, y1) = ((r.x as f32 + x + w).round() as i32, (r.y as f32 + y + h).round() as i32);
        let cr = Rect::new(x0, y0, x1 - x0, y1 - y0);
        cv.path.push(i);
        ch.draw(cv, cr, ink, dir);
        cv.path.pop();
        pos += main + gap + extra_gap;
    }
    overflow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn black_in(cv: &Canvas, r: Rect) -> usize {
        let mut n = 0;
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                if cv.get(x, y) == BLACK {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn grow_pushes_the_last_child_to_the_bottom() {
        let col = Node::column(vec![Node::text("top", "body"), Node::Spacer { common: Common { grow: 1.0, ..Default::default() } }, Node::text("bottom", "body")], 0.0);
        let mut cv = Canvas::new(200, 400);
        col.draw(&mut cv, Rect::new(0, 0, 200, 400), BLACK, Dir::Column);
        assert!(black_in(&cv, Rect::new(0, 0, 200, 40)) > 0, "first line at the top");
        assert_eq!(black_in(&cv, Rect::new(0, 60, 200, 280)), 0, "nothing in the middle");
        assert!(black_in(&cv, Rect::new(0, 360, 200, 40)) > 0, "last line at the bottom");
    }

    #[test]
    fn centered_text_is_centered() {
        let t = Node::text("x", "large").centered();
        let mut cv = Canvas::new(400, 100);
        t.draw(&mut cv, Rect::new(0, 0, 400, 100), BLACK, Dir::Column);
        assert_eq!(black_in(&cv, Rect::new(0, 0, 150, 100)), 0);
        assert!(black_in(&cv, Rect::new(150, 0, 100, 100)) > 0);
        assert_eq!(black_in(&cv, Rect::new(250, 0, 150, 100)), 0);
    }

    #[test]
    fn the_report_says_what_did_not_fit() {
        let t = |s: &str| Node::text(s, "body");
        let mut nowrap = t("a line far too long to fit in this narrow column");
        if let Node::Text { wrap, .. } = &mut nowrap {
            *wrap = false;
        }
        let tree = Node::column(vec![t("fits"), nowrap, t("many words ".repeat(40).as_str()), t("pushed out")], 4.0);
        let mut cv = Canvas::new(200, 200);
        cv.report = Some(Vec::new());
        tree.draw(&mut cv, Rect::new(0, 0, 200, 200), BLACK, Dir::Column);
        let r = cv.report.unwrap();
        let by_path = |p: &str| r.iter().find(|e| e.path == p).unwrap();
        assert_eq!(by_path("root").kind, "column");
        assert!(by_path("root").overflow_px.unwrap() > 0, "{:?}", by_path("root"));
        assert!(by_path("root/0").cut.is_empty());
        assert_eq!(by_path("root/1").cut, vec!["too_wide"]);
        assert!(by_path("root/2").cut.contains(&"too_tall"), "{:?}", by_path("root/2"));
        assert_eq!(by_path("root/3").cut, vec!["no_space"]);
    }

    #[test]
    fn shrink_finds_the_largest_size_that_fits() {
        let mut t = Node::text("Abu Dhabi Grand Prix", "huge");
        if let Node::Text { wrap, shrink_to, .. } = &mut t {
            *wrap = false;
            *shrink_to = Some(10.0);
        }
        let mut cv = Canvas::new(400, 200);
        cv.report = Some(Vec::new());
        t.draw(&mut cv, Rect::new(0, 0, 300, 200), BLACK, Dir::Column);
        let e = &cv.report.unwrap()[0];
        let size = e.size.unwrap();
        assert!(e.cut.is_empty(), "{e:?}");
        assert!((10.0..96.0).contains(&size), "{size}");
        let st = TextStyle { size, ..TextStyle::named("huge").unwrap() };
        assert!(st.width("Abu Dhabi Grand Prix") <= 300.0);
        let bigger = TextStyle { size: size + 1.0, ..st };
        assert!(bigger.width("Abu Dhabi Grand Prix") > 300.0, "it is the largest that fits");
    }

    /// A node drawn at the size it measured has nothing cut, fractional gaps and sizes or not.
    #[test]
    fn the_measured_size_is_enough() {
        let t = |s: &str, st: &str| { let mut n = Node::text(s, st); if let Node::Text { wrap, style, .. } = &mut n { *wrap = false; style.size += 0.7; } n };
        let boxed = |c: Vec<Node>| Node::Stack { dir: Dir::Column, children: c, gap: 6.5, align: Align::Start, justify: Justify::Start, padding: 10.6, border: Edges::uniform(2.0), invert: false, common: Common::default() };
        let row = |c: Vec<Node>| Node::Stack { dir: Dir::Row, children: c, gap: 12.5, align: Align::Start, justify: Justify::Start, padding: 0.0, border: Edges::ZERO, invert: false, common: Common::default() };
        let trees = vec![
            ("text", t("Interlagos 4.309 km", "body")),
            ("box(text)", boxed(vec![t("Interlagos 4.309 km", "body")])),
            ("box(box(text))", boxed(vec![boxed(vec![t("Autódromo José Carlos Pace", "title")])])),
            ("box(row(t,t))", boxed(vec![row(vec![t("FP1", "label"), t("Fri 7 Nov 11:30", "body")])])),
            ("box(box(row(t,t)),t)", boxed(vec![boxed(vec![row(vec![t("Quali", "label"), t("Sat 8 Nov 15:00", "body")])]), t("São Paulo", "large")])),
        ];
        for (name, n) in trees {
            let (w, h, _) = n.measured(f32::INFINITY, f32::INFINITY);
            let (w, h) = (w.ceil() as i32, h.ceil() as i32);
            let mut cv = Canvas::new(1000, 600);
            cv.report = Some(Vec::new());
            n.draw(&mut cv, Rect::new(0, 0, w, h), BLACK, Dir::Column);
            let bad: Vec<_> = cv.report.unwrap().into_iter().filter(|e| !e.cut.is_empty() || e.overflow_px.is_some()).map(|e| format!("{} {:?} {:?} w{}", e.path, e.cut, e.overflow_px, e.w)).collect();
            assert!(bad.is_empty(), "{name}: measured {w}x{h}: {bad:?}");
        }
    }

    fn one_line(s: &str, st: &str) -> Node {
        let mut n = Node::text(s, st);
        if let Node::Text { wrap, .. } = &mut n {
            *wrap = false;
        }
        n
    }
    fn row(children: Vec<Node>, align: Align) -> Node {
        Node::Stack { dir: Dir::Row, children, gap: 12.0, align, justify: Justify::Start, padding: 0.0, border: Edges::ZERO, invert: false, common: Common::default() }
    }
    fn growing(mut n: Node) -> Node {
        if let Node::Text { common, .. } | Node::Stack { common, .. } = &mut n {
            common.grow = 1.0;
        }
        n
    }

    #[test]
    fn a_row_of_growing_children_measures_what_it_draws() {
        // Growing children get equal shares, so the row needs twice the longer one.
        let r = row(vec![growing(one_line("Q1", "body")), growing(one_line("Fastest lap of the race", "body"))], Align::Start);
        let (w, h, _) = r.measured(f32::INFINITY, f32::INFINITY);
        assert!(r.trial(w, h).0.is_empty(), "{:?} at {w}x{h}", r.trial(w, h).0);
        // And a limit still limits it.
        assert!(r.measured(100.0, f32::INFINITY).0 <= 100.0);
    }

    #[test]
    fn first_draws_the_first_child_that_fits() {
        let pick = Node::First {
            children: vec![one_line("Max Verstappen", "huge"), one_line("Max Verstappen", "title"), one_line("VER", "title")],
            common: Common::default(),
        };
        let chosen = |w: i32| {
            let mut cv = Canvas::new(w as u32, 120);
            cv.report = Some(Vec::new());
            pick.draw(&mut cv, Rect::new(0, 0, w, 120), BLACK, Dir::Column);
            cv.report.unwrap()[0].chosen
        };
        assert_eq!(chosen(800), Some(0));
        assert_eq!(chosen(250), Some(1));
        assert_eq!(chosen(60), Some(2));
        // Measuring picks the same one, and what it measures fits.
        let (w, h, _) = pick.measured(250.0, f32::INFINITY);
        assert!(w <= 250.0 && pick.trial(250.0, h).0.is_empty());
    }

    #[test]
    fn baseline_alignment_lines_up_mixed_sizes() {
        let r = row(vec![one_line("72°", "huge"), one_line("Sunny", "body"), one_line("feels 70", "small")], Align::Baseline);
        let (w, h, _) = r.measured(f32::INFINITY, f32::INFINITY);
        let mut cv = Canvas::new(w as u32, h as u32);
        cv.report = Some(Vec::new());
        r.draw(&mut cv, Rect::new(0, 0, w as i32, h as i32), BLACK, Dir::Column);
        let report = cv.report.unwrap();
        let baselines: Vec<f32> = report[1..]
            .iter()
            .map(|e| e.y as f32 + TextStyle::named(match e.size.unwrap() as i32 { 96 => "huge", 22 => "body", _ => "small" }).unwrap().ascent())
            .collect();
        assert!(baselines.iter().all(|b| (b - baselines[0]).abs() < 1.01), "{baselines:?}");
        assert!(report[1..].iter().all(|e| e.cut.is_empty()));
    }

    #[test]
    fn borders_by_side_dashes_and_patterns() {
        let mut cv = Canvas::new(100, 40);
        let b = Node::Stack {
            dir: Dir::Column,
            children: vec![],
            gap: 0.0,
            align: Align::Stretch,
            justify: Justify::Start,
            padding: 0.0,
            border: Edges { left: 3.0, ..Edges::ZERO },
            invert: false,
            common: Common::default(),
        };
        b.draw(&mut cv, Rect::new(0, 0, 100, 40), BLACK, Dir::Column);
        assert!((0..3).all(|x| cv.get(x, 20) == BLACK) && cv.get(3, 20) == WHITE && cv.get(99, 20) == WHITE && cv.get(50, 0) == WHITE);

        let mut cv = Canvas::new(100, 10);
        let rule = Node::Rule { thickness: 2.0, dash: Some((6.0, 4.0)), common: Common::default() };
        rule.draw(&mut cv, Rect::new(0, 0, 100, 10), BLACK, Dir::Column);
        let row: Vec<bool> = (0..100).map(|x| cv.get(x, 5) == BLACK).collect();
        let dashes = row.windows(2).filter(|w| !w[0] && w[1]).count() + usize::from(row[0]);
        assert_eq!(dashes, 10);

        let mut cv = Canvas::new(60, 60);
        let hatched = Node::Spacer { common: Common { fill: Some(Fill::Hatch), ..Common::default() } };
        hatched.draw(&mut cv, Rect::new(0, 0, 60, 60), BLACK, Dir::Column);
        let black = (0..60).flat_map(|y| (0..60).map(move |x| (x, y))).filter(|&(x, y)| cv.get(x, y) == BLACK).count();
        assert_eq!(black, 600, "one pixel in six");
    }

    #[test]
    fn images_can_be_thresholded_and_inverted() {
        let ramp = Arc::new(GrayImage::from_fn(256, 4, |x, _| image::Luma([x as u8])));
        let img = |threshold, invert| Node::Image { img: ramp.clone(), mask: None, fit: Fit::Contain, dither: true, blur: 0.0, threshold, invert, common: Common::default() };
        let mut cv = Canvas::new(256, 4).with_grays(4);
        img(Some(200), false).draw(&mut cv, Rect::new(0, 0, 256, 4), BLACK, Dir::Column);
        assert!((0..256).all(|x| cv.get(x, 1) == if x >= 200 { WHITE } else { BLACK }));
        img(Some(128), true).draw(&mut cv, Rect::new(0, 0, 256, 4), BLACK, Dir::Column);
        assert_eq!((cv.get(10, 1), cv.get(250, 1)), (WHITE, BLACK));
    }

    #[test]
    fn accents_may_rise_above_a_row() {
        // Overpass puts accents on capitals above the top of its line.
        let mut t = Node::text("ÃÕÑ", "title");
        if let Node::Text { style, .. } = &mut t {
            style.face = crate::render::font::Face::Overpass;
        }
        let row = Node::Stack { dir: Dir::Row, children: vec![t], gap: 0.0, align: Align::Start, justify: Justify::Start, padding: 0.0, border: Edges::ZERO, invert: false, common: Common::default() };
        let mut cv = Canvas::new(200, 100);
        row.draw(&mut cv, Rect::new(10, 40, 180, 26), BLACK, Dir::Column);
        assert!(black_in(&cv, Rect::new(0, 0, 200, 40)) > 0, "the accents are drawn above the row");
    }

    #[test]
    fn nothing_is_drawn_outside_the_rect() {
        let long = Node::text("word ".repeat(200), "huge");
        let mut cv = Canvas::new(300, 300);
        long.draw(&mut cv, Rect::new(50, 50, 100, 100), BLACK, Dir::Column);
        assert_eq!(black_in(&cv, Rect::new(0, 0, 300, 50)), 0);
        assert_eq!(black_in(&cv, Rect::new(0, 150, 300, 150)), 0);
        assert_eq!(black_in(&cv, Rect::new(150, 0, 150, 300)), 0);
    }
}
