//! Ink whose width varies along each stroke, as a pressure-sensitive pen draws it.
//!
//! The drawing stays a standard `/Ink` annotation: `/InkList` holds the strokes and `/BS /W` the
//! widest point's width, so any viewer shows the strokes. `/PCInkWidths` keeps each point's width
//! as a fraction (0–1] of `/BS /W`, one array per stroke, and the appearance stream draws the
//! strokes as filled outlines of those widths. Moving, resizing and restyling keep the fractions;
//! erasing cuts them with the strokes. A `/PCInkWidths` that no longer matches `/InkList` point
//! for point (another editor changed the strokes) is ignored, and the strokes are drawn at
//! `/BS /W` as before.

use super::create::{add_annotation, num_array};
use super::lookup::annot_ref;
use super::types::{Meta, NewAnnotation, Shape};
use super::{AnnotError, Dict, Document, Object, set_appearance};

/// The key holding each point's width fraction.
pub const WIDTHS_KEY: &[u8] = b"PCInkWidths";

/// Thinnest a point may be, as a fraction of the widest: hairlines vanish when printed.
const MIN_FRACTION: f64 = 0.05;

/// Add freehand ink (`Shape::Ink`, `FreehandHighlight` or `Signature`) whose point `j` of stroke
/// `i` is `widths[i][j]` × `style.width` wide. Fractions are clamped to [0.05, 1]. Returns the
/// annotation's index on its page, as [`add_annotation`] does.
pub fn add_pressure_ink(doc: &mut Document, new: &NewAnnotation, widths: &[Vec<f64>], meta: &Meta) -> Result<usize, AnnotError> {
    let strokes = match &new.shape {
        Shape::Ink { strokes } | Shape::FreehandHighlight { strokes } | Shape::Signature { strokes } => strokes,
        _ => return Err(AnnotError::Invalid("only freehand ink has widths per point".into())),
    };
    if strokes.iter().any(Vec::is_empty) {
        return Err(AnnotError::Invalid("a stroke has no points".into()));
    }
    if strokes.len() != widths.len() || strokes.iter().zip(widths).any(|(s, w)| s.len() != w.len()) {
        return Err(AnnotError::Invalid("the widths don't match the strokes point for point".into()));
    }
    if widths.iter().flatten().any(|w| !w.is_finite()) {
        return Err(AnnotError::Invalid("invalid stroke width".into()));
    }
    let index = add_annotation(doc, new, meta)?;
    let (_, r) = annot_ref(doc, new.page, index)?;
    let fractions: Vec<Object> = widths.iter().map(|w| num_array(&w.iter().map(|f| f.clamp(MIN_FRACTION, 1.0)).collect::<Vec<_>>())).collect();
    doc.update_dict(r, |d| d.set(WIDTHS_KEY.to_vec(), Object::Array(fractions)))?;
    set_appearance(doc, r)?;
    Ok(index)
}

/// The width fractions of an ink annotation, when they match `/InkList` point for point.
pub(crate) fn fractions(d: &Dict, ink_list: &[Object]) -> Option<Vec<Vec<f64>>> {
    let list = d.get(WIDTHS_KEY)?.as_array()?;
    if list.len() != ink_list.len() {
        return None;
    }
    let mut out = Vec::with_capacity(list.len());
    for (w, s) in list.iter().zip(ink_list) {
        let w: Vec<f64> = w.as_array()?.iter().map(Object::as_f64).collect::<Option<_>>()?;
        if w.len() * 2 != s.as_array()?.len() || w.iter().any(|f| !f.is_finite()) {
            return None;
        }
        out.push(w.into_iter().map(|f| f.clamp(MIN_FRACTION, 1.0)).collect());
    }
    Some(out)
}

/// Content-stream path operators filling the outline of a stroke through `pts` whose point `i`
/// is `radii[i]` wide on each side: a disc at every point and a quadrilateral joining each pair,
/// all wound the same way so a nonzero fill paints their union once (translucent ink doesn't
/// darken where the pieces overlap).
pub(crate) fn outline_path(pts: &[(f64, f64)], radii: &[f64], n: impl Fn(f64) -> String) -> String {
    let mut c = String::new();
    for (&(x, y), &r) in pts.iter().zip(radii) {
        disc(&mut c, x, y, r, &n);
    }
    for (a, b) in pts.windows(2).zip(radii.windows(2)) {
        let ((x0, y0), (x1, y1)) = (a[0], a[1]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = dx.hypot(dy);
        if len < 1e-9 {
            continue;
        }
        // Unit normal to the left of the direction of travel.
        let (nx, ny) = (-dy / len, dx / len);
        let (r0, r1) = (b[0], b[1]);
        // Counter-clockwise: right side forward, left side back.
        let quad = [(x0 - nx * r0, y0 - ny * r0), (x1 - nx * r1, y1 - ny * r1), (x1 + nx * r1, y1 + ny * r1), (x0 + nx * r0, y0 + ny * r0)];
        c.push_str(&format!("{} {} m\n", n(quad[0].0), n(quad[0].1)));
        for p in &quad[1..] {
            c.push_str(&format!("{} {} l\n", n(p.0), n(p.1)));
        }
        c.push_str("h\n");
    }
    c
}

/// A counter-clockwise circle of four Bézier arcs.
fn disc(c: &mut String, x: f64, y: f64, r: f64, n: &impl Fn(f64) -> String) {
    const K: f64 = 0.552_284_749_8;
    let k = r * K;
    c.push_str(&format!("{} {} m\n", n(x + r), n(y)));
    let arcs = [
        (x + r, y + k, x + k, y + r, x, y + r),
        (x - k, y + r, x - r, y + k, x - r, y),
        (x - r, y - k, x - k, y - r, x, y - r),
        (x + k, y - r, x + r, y - k, x + r, y),
    ];
    for (a, b, cc, d, e, f) in arcs {
        c.push_str(&format!("{} {} {} {} {} {} c\n", n(a), n(b), n(cc), n(d), n(e), n(f)));
    }
    c.push_str("h\n");
}
