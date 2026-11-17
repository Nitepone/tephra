//! A device's screen: the preset layouts, where their slots fall on a panel, and composing the
//! apps' trees into one image.

use crate::render::layout::{Dir, Node};
use crate::render::{BLACK, Canvas, Rect, WHITE};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// Each slot as fractions of the panel (x, y, w, h), in reading order.
    pub slots: &'static [(f32, f32, f32, f32)],
    /// The size name apps see for each slot (`full`, `large`, `half`, `small`).
    pub sizes: &'static [&'static str],
}

const T: f32 = 1.0 / 3.0;

pub const PRESETS: &[Preset] = &[
    Preset { id: "full", name: "Full", slots: &[(0.0, 0.0, 1.0, 1.0)], sizes: &["full"] },
    Preset { id: "halves", name: "Left | right", slots: &[(0.0, 0.0, 0.5, 1.0), (0.5, 0.0, 0.5, 1.0)], sizes: &["half", "half"] },
    Preset { id: "stacked", name: "Top / bottom", slots: &[(0.0, 0.0, 1.0, 0.5), (0.0, 0.5, 1.0, 0.5)], sizes: &["half", "half"] },
    Preset { id: "two-thirds", name: "⅔ | ⅓", slots: &[(0.0, 0.0, 2.0 * T, 1.0), (2.0 * T, 0.0, T, 1.0)], sizes: &["large", "small"] },
    Preset {
        id: "two-thirds-split",
        name: "⅔ | ⅓ split",
        slots: &[(0.0, 0.0, 2.0 * T, 1.0), (2.0 * T, 0.0, T, 0.5), (2.0 * T, 0.5, T, 0.5)],
        sizes: &["large", "small", "small"],
    },
    Preset {
        id: "quadrants",
        name: "Quadrants",
        slots: &[(0.0, 0.0, 0.5, 0.5), (0.5, 0.0, 0.5, 0.5), (0.0, 0.5, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)],
        sizes: &["small", "small", "small", "small"],
    },
];

pub fn preset(id: &str) -> &'static Preset {
    PRESETS.iter().find(|p| p.id == id).unwrap_or(&PRESETS[0])
}

const GAP: i32 = 8; // between slots and around them, on multi-slot screens
const BORDER: i32 = 2;
const PADDING: i32 = 14;
const TITLE_H: i32 = 30;

/// Where one slot falls on the panel.
#[derive(Debug, Clone, Copy)]
pub struct SlotGeom {
    pub outer: Rect,
    /// What the app draws into.
    pub content: Rect,
    pub title: Option<Rect>,
    pub size: &'static str,
}

pub fn geometry(p: &Preset, w: u32, h: u32, title_bars: &[bool]) -> Vec<SlotGeom> {
    let (w, h) = (w as i32, h as i32);
    let single = p.slots.len() == 1;
    p.slots
        .iter()
        .zip(p.sizes)
        .enumerate()
        .map(|(i, (&(fx, fy, fw, fh), &size))| {
            let outer = if single {
                Rect::new(0, 0, w, h)
            } else {
                // Fractions of the area inside the outer gap; each slot gives up half a gap per side.
                let (aw, ah) = ((w - GAP) as f32, (h - GAP) as f32);
                let x0 = GAP / 2 + (fx * aw).round() as i32;
                let y0 = GAP / 2 + (fy * ah).round() as i32;
                let x1 = GAP / 2 + ((fx + fw) * aw).round() as i32;
                let y1 = GAP / 2 + ((fy + fh) * ah).round() as i32;
                Rect::new(x0, y0, x1 - x0, y1 - y0).inset(GAP / 2)
            };
            let frame = if single { 0 } else { BORDER };
            let inner = outer.inset(frame);
            let title_bar = title_bars.get(i).copied().unwrap_or(false);
            let title = title_bar.then(|| Rect::new(inner.x, inner.y + inner.h - TITLE_H, inner.w, TITLE_H));
            let body = Rect::new(inner.x, inner.y, inner.w, inner.h - if title_bar { TITLE_H } else { 0 });
            SlotGeom { outer, content: body.inset(PADDING), title, size }
        })
        .collect()
}

/// One slot, ready to draw.
pub struct SlotDraw {
    pub geom: SlotGeom,
    /// The app's tree, an error to show instead, or nothing (an empty slot).
    pub content: Option<Result<Node, String>>,
    pub title: String,
}

pub fn compose(w: u32, h: u32, multi: bool, slots: &[SlotDraw]) -> Canvas {
    let mut cv = Canvas::new(w, h);
    compose_into(&mut cv, multi, slots);
    cv
}

pub fn compose_into(cv: &mut Canvas, multi: bool, slots: &[SlotDraw]) {
    let cv = &mut *cv;
    for s in slots {
        if multi {
            cv.border(s.geom.outer, BORDER, BLACK);
        }
        if let Some(t) = s.geom.title {
            cv.fill(t, BLACK);
            let title = Node::text(s.title.clone(), "label");
            title.draw(cv, Rect::new(t.x + PADDING, t.y + 5, t.w - 2 * PADDING, t.h - 5), WHITE, Dir::Column);
        }
        match &s.content {
            Some(Ok(node)) => node.draw(cv, s.geom.content, BLACK, Dir::Column),
            Some(Err(e)) => error_node(&s.title, e).draw(cv, s.geom.content, BLACK, Dir::Column),
            None => {}
        }
    }
}

pub fn error_node(title: &str, err: &str) -> Node {
    Node::column(vec![Node::text(format!("{title} failed"), "title"), Node::text(err.to_string(), "small")], 6.0)
}

/// A full screen of centered text: the welcome screen of an unconfigured device, identify.
pub fn message_screen(w: u32, h: u32, lines: Vec<Node>) -> Canvas {
    let mut cv = Canvas::new(w, h);
    let body = Node::column(lines.into_iter().map(Node::centered).collect(), 12.0).centered();
    body.draw(&mut cv, Rect::new(0, 0, w as i32, h as i32).inset(40), BLACK, Dir::Column);
    cv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_tile_the_og_panel_without_overlap() {
        for p in PRESETS {
            let g = geometry(p, 800, 480, &vec![true; p.slots.len()]);
            assert_eq!(g.len(), p.slots.len());
            for (i, a) in g.iter().enumerate() {
                assert!(a.outer.x >= 0 && a.outer.y >= 0 && a.outer.x + a.outer.w <= 800 && a.outer.y + a.outer.h <= 480, "{}", p.id);
                assert!(a.content.w > 100 && a.content.h > 80, "{} slot {i} too small: {:?}", p.id, a.content);
                for b in &g[i + 1..] {
                    assert_eq!(a.outer.intersect(b.outer).w * a.outer.intersect(b.outer).h, 0, "{} overlaps", p.id);
                }
            }
        }
    }
}
