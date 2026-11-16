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

/// What every node may set.
#[derive(Debug, Clone, Default)]
pub struct Common {
    /// Share of the parent's leftover space along its axis (0 = none).
    pub grow: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Stack {
        dir: Dir,
        children: Vec<Node>,
        gap: f32,
        align: Align,
        justify: Justify,
        /// Only set for `box`: padding, border width and white-on-black.
        padding: f32,
        border: f32,
        invert: bool,
        common: Common,
    },
    Text {
        text: String,
        style: TextStyle,
        wrap: bool,
        align: TextAlign,
        max_lines: Option<usize>,
        common: Common,
    },
    Image {
        img: Arc<GrayImage>,
        fit: Fit,
        dither: bool,
        common: Common,
    },
    Rule {
        thickness: f32,
        common: Common,
    },
    Spacer {
        common: Common,
    },
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
            border: 0.0,
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
                let lines = text_lines(text, *style, *wrap, *max_lines, max_w);
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
            Node::Stack { dir, children, gap, padding, border, .. } => {
                let inset = 2.0 * (padding + border);
                let (iw, ih) = ((max_w - inset).max(0.0), (max_h - inset).max(0.0));
                let sizes = measure_children(*dir, children, *gap, iw, ih);
                let n = children.len().max(1) as f32;
                let (main, cross) = sizes.iter().fold((gap * (n - 1.0), 0.0f32), |(m, x), s| {
                    let (sm, sx) = split(*dir, *s);
                    (m + sm, x.max(sx))
                });
                let (w, h) = join(*dir, main, cross);
                Size { w: w + inset, h: h + inset }
            }
        };
        Size {
            w: c.width.unwrap_or(s.w).min(max_w),
            h: c.height.unwrap_or(s.h).min(max_h),
        }
    }

    /// Draws this node into `r`.
    pub fn draw(&self, cv: &mut Canvas, r: Rect, ink: u8, parent: Dir) {
        if r.w <= 0 || r.h <= 0 {
            return;
        }
        let clip = cv.push_clip(r);
        match self {
            Node::Text { text, style, wrap, align, max_lines, .. } => {
                let lh = style.line_height();
                let lines = text_lines(text, *style, *wrap, *max_lines, r.w as f32);
                // Only whole lines; at least the first.
                let fit = ((r.h as f32 / lh).floor() as usize).max(1);
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
            Node::Image { img, fit, dither, .. } => {
                let (iw, ih) = (img.width() as f32, img.height() as f32);
                let scale = match fit {
                    Fit::Contain => (r.w as f32 / iw).min(r.h as f32 / ih),
                    Fit::Cover => (r.w as f32 / iw).max(r.h as f32 / ih),
                };
                let (w, h) = ((iw * scale).round().max(1.0) as u32, (ih * scale).round().max(1.0) as u32);
                let resized = image::imageops::resize(img.as_ref(), w, h, image::imageops::FilterType::Triangle);
                let x = r.x + (r.w - w as i32) / 2;
                let y = r.y + (r.h - h as i32) / 2;
                cv.draw_gray(&resized, x, y, *dither);
            }
            Node::Rule { thickness, .. } => {
                let t = thickness.round() as i32;
                match parent {
                    Dir::Column => cv.fill(Rect::new(r.x, r.y + (r.h - t) / 2, r.w, t), ink),
                    Dir::Row => cv.fill(Rect::new(r.x + (r.w - t) / 2, r.y, t, r.h), ink),
                }
            }
            Node::Spacer { .. } => {}
            Node::Stack { dir, children, gap, align, justify, padding, border, invert, .. } => {
                let outer = ink;
                let mut ink = ink;
                if *invert {
                    cv.fill(r, outer);
                    ink = if outer == BLACK { WHITE } else { BLACK };
                }
                if *border > 0.0 {
                    cv.border(r, border.round() as i32, outer);
                }
                let inner = r.inset((padding + border).round() as i32);
                draw_children(cv, inner, ink, *dir, children, *gap, *align, *justify);
            }
        }
        cv.pop_clip(clip);
    }
}

fn text_lines(text: &str, style: TextStyle, wrap: bool, max_lines: Option<usize>, max_w: f32) -> Vec<String> {
    let mut lines = if wrap {
        style.wrap(text, max_w)
    } else {
        text.split('\n').map(|l| style.truncate(l, max_w)).collect()
    };
    if let Some(n) = max_lines.filter(|&n| n > 0 && n < lines.len()) {
        lines.truncate(n);
        let last = lines.pop().unwrap_or_default();
        lines.push(style.truncate(&format!("{last}…"), max_w));
    }
    lines
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

#[allow(clippy::too_many_arguments)]
fn draw_children(cv: &mut Canvas, r: Rect, ink: u8, dir: Dir, children: &[Node], gap: f32, align: Align, justify: Justify) {
    let (main_total, cross_total) = split(dir, Size { w: r.w as f32, h: r.h as f32 });
    let gaps = gap * children.len().saturating_sub(1) as f32;

    // Fixed children first, in order, each with what is left; growing ones share the rest.
    let mut mains = vec![0.0f32; children.len()];
    let mut avail = main_total - gaps;
    for (i, ch) in children.iter().enumerate() {
        if ch.common().grow <= 0.0 {
            let (cw, chh) = join(dir, avail.max(0.0), cross_total);
            mains[i] = split(dir, ch.measure(cw, chh, dir)).0.min(avail.max(0.0));
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

    for (ch, &main) in children.iter().zip(&mains) {
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
        };
        let (x, y) = join(dir, pos, cross_pos);
        let (w, h) = join(dir, main, cross);
        let cr = Rect::new(r.x + x.round() as i32, r.y + y.round() as i32, w.round() as i32, h.round() as i32);
        ch.draw(cv, cr, ink, dir);
        pos += main + gap + extra_gap;
    }
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
    fn nothing_is_drawn_outside_the_rect() {
        let long = Node::text("word ".repeat(200), "huge");
        let mut cv = Canvas::new(300, 300);
        long.draw(&mut cv, Rect::new(50, 50, 100, 100), BLACK, Dir::Column);
        assert_eq!(black_in(&cv, Rect::new(0, 0, 300, 50)), 0);
        assert_eq!(black_in(&cv, Rect::new(0, 150, 300, 150)), 0);
        assert_eq!(black_in(&cv, Rect::new(150, 0, 150, 300)), 0);
    }
}
