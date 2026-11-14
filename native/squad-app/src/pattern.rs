//! The window's background pattern: a fine motif in the theme's ink, warming into the
//! accent toward the top-left corner, where the lit rim's light comes from. Every motif
//! sits on one grid in window coordinates, so every piece lines up as one sheet, and a
//! motif is only ever drawn whole.
//!
//! Where it lies is up to the user, by area: the content (the page and the terminal),
//! the side panels, and the frame round the islands (the top bar, the rail, the gutters
//! and the status bar). With all three on and glass on, it also lies behind the islands,
//! between the blurred desktop and the content.

use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    Off,
    /// A dot in each cell.
    Dots,
    /// Hairlines along each cell's edges.
    Grid,
    /// A faint grid with every fourth line stronger, like drafting paper.
    Blueprint,
    /// A small plus in each cell.
    Crosses,
    /// A small ring in each cell.
    Rings,
    /// A short dash in each cell, every other row shifted half a cell, like stitching.
    Stitches,
}

impl Pattern {
    pub const ALL: [Pattern; 7] = [
        Pattern::Off,
        Pattern::Dots,
        Pattern::Grid,
        Pattern::Blueprint,
        Pattern::Crosses,
        Pattern::Rings,
        Pattern::Stitches,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Pattern::Off => "off",
            Pattern::Dots => "dots",
            Pattern::Grid => "grid",
            Pattern::Blueprint => "blueprint",
            Pattern::Crosses => "crosses",
            Pattern::Rings => "rings",
            Pattern::Stitches => "stitches",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Pattern::Off => "None",
            Pattern::Dots => "Dot matrix",
            Pattern::Grid => "Grid",
            Pattern::Blueprint => "Blueprint",
            Pattern::Crosses => "Crosses",
            Pattern::Rings => "Rings",
            Pattern::Stitches => "Stitches",
        }
    }
}

/// The parts of the window a pattern can lie on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Area {
    /// The page and the terminal: the islands on the content's fill.
    Content = 1,
    /// The sidebar and the panels beside the content, such as changes and files.
    Panels = 2,
    /// The top bar, the rail, the gutters and the status bar.
    Frame = 4,
}

static PATTERN: AtomicU8 = AtomicU8::new(0);
static AREAS: AtomicU8 = AtomicU8::new(Area::Content as u8);

pub fn set(id: &str, content: bool, panels: bool, frame: bool) {
    let ix = Pattern::ALL.iter().position(|p| p.id() == id).unwrap_or(0);
    PATTERN.store(ix as u8, Ordering::Relaxed);
    let areas = [
        (content, Area::Content),
        (panels, Area::Panels),
        (frame, Area::Frame),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |m, (_, a)| m | a as u8);
    AREAS.store(areas, Ordering::Relaxed);
}

pub fn current() -> Pattern {
    Pattern::ALL[PATTERN.load(Ordering::Relaxed) as usize % Pattern::ALL.len()]
}

/// Whether the pattern lies on `area`.
pub fn shows(area: Area) -> bool {
    current() != Pattern::Off && AREAS.load(Ordering::Relaxed) & area as u8 != 0
}

/// The pattern on a piece of the frame (a bar, a gutter), over its fill.
pub fn chrome() -> Option<AnyElement> {
    surface(Area::Frame)
}

/// The pattern on an island of `area`, over its fill and under what it holds.
pub fn surface(area: Area) -> Option<AnyElement> {
    shows(area).then(|| layer(|b, window| paint(b, None, FRAME, window)))
}

fn layer(f: impl FnOnce(Bounds<Pixels>, &mut Window) + 'static) -> AnyElement {
    canvas(|_, _, _| (), move |b, _, window, _| f(b, window))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
}

/// How strong the pattern is on a surface laid straight over it.
pub const FRAME: f32 = 1.;

/// The grid's pitch.
pub const STEP: f32 = 16.;

/// The Squad Code window's page area's top-left corner, in window pixels, as last
/// painted: the sidebar lines its pattern up with it, so the two read as one grid.
static PAGE_X: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PAGE_Y: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

std::thread_local! {
    /// The bars and headers laid over the page this frame, in window pixels: the
    /// pattern stays out from under them and starts again on a whole cell below.
    static CHROME: std::cell::RefCell<Vec<Bounds<Pixels>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Forgets last frame's bars and headers; called as each frame starts.
pub fn clear_chrome() {
    CHROME.with(|c| c.borrow_mut().clear());
}

/// Notes a bar or header over the page, as it's laid out.
pub fn add_chrome(b: Bounds<Pixels>) {
    CHROME.with(|c| c.borrow_mut().push(b));
}

fn bars_over_page() -> Vec<Bounds<Pixels>> {
    CHROME.with(|c| c.borrow().clone())
}

/// Notes where the page area's pattern starts.
pub fn set_page_origin(at: Point<Pixels>) {
    PAGE_X.store(f32::from(at.x).to_bits(), Ordering::Relaxed);
    PAGE_Y.store(f32::from(at.y).to_bits(), Ordering::Relaxed);
}

/// Where the page area's pattern starts, once it has been painted.
pub fn page_origin() -> Option<Point<Pixels>> {
    let (x, y) = (
        f32::from_bits(PAGE_X.load(Ordering::Relaxed)),
        f32::from_bits(PAGE_Y.load(Ordering::Relaxed)),
    );
    (x > 0. || y > 0.).then(|| point(px(x), px(y)))
}

/// Paints the motifs whose whole shape lies in `area` and clear of `skip`, `gain` times
/// their strength on the frame.
pub fn paint(area: Bounds<Pixels>, skip: Option<Bounds<Pixels>>, gain: f32, window: &mut Window) {
    paint_in(
        current(),
        area,
        skip,
        None,
        gain,
        (c_text(), c_blue(), is_dark()),
        window,
    );
}

/// Paints `pattern` in `area`, in `ink` warming into `accent` toward the top-left, on a
/// dark or light ground. The Squad Code window passes its own theme's colours.
pub fn paint_in(
    pattern: Pattern,
    area: Bounds<Pixels>,
    skip: Option<Bounds<Pixels>>,
    anchor: Option<Point<Pixels>>,
    gain: f32,
    (ink, accent, dark): (u32, u32, bool),
    window: &mut Window,
) {
    if pattern == Pattern::Off {
        return;
    }
    let view = window.viewport_size();
    let (w, h) = (f32::from(view.width), f32::from(view.height));
    let diag = (w * w + h * h).sqrt();
    let strength = gain * if dark { 0.16 } else { 0.12 };
    let (left, top) = (f32::from(area.origin.x), f32::from(area.origin.y));
    let (right, bottom) = (f32::from(area.right()), f32::from(area.bottom()));
    let rect =
        |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    // Where a cell's corner falls: on the window's own grid, or on `anchor`, so the
    // pattern starts with a whole cell at an area's edge and areas that share an
    // anchor line up across their boundary.
    let (ox, mut oy) = match anchor {
        Some(a) => (f32::from(a.x), f32::from(a.y)),
        None => (0., 0.),
    };
    // Bars and headers over the page: nothing under them, and a header across the
    // page's top moves the grid's first row to just below it.
    let bars: Vec<Bounds<Pixels>> = if anchor.is_some() {
        bars_over_page()
            .into_iter()
            .filter(|b| b.intersects(&area))
            .collect()
    } else {
        Vec::new()
    };
    let mut top = top;
    for b in &bars {
        let (bt, bb) = (f32::from(b.top()), f32::from(b.bottom()));
        if (bt - top).abs() < 2. && f32::from(b.size.width) >= 0.6 * (right - left) {
            top = bb;
            oy = bb;
        }
    }
    let inside = |b: &Bounds<Pixels>| {
        f32::from(b.left()) >= left
            && f32::from(b.right()) <= right
            && f32::from(b.top()) >= top
            && f32::from(b.bottom()) <= bottom
            && !skip.is_some_and(|s| s.intersects(b))
            && !bars.iter().any(|bar| bar.intersects(b))
    };
    let start = |from: f32, o: f32| ((from - o) / STEP).floor() * STEP + o;
    let mut gy = start(top, oy);
    while gy < bottom {
        let row = ((gy - oy) / STEP).round() as i64;
        let mut gx = start(left, ox);
        while gx < right {
            let col = ((gx - ox) / STEP).round() as i64;
            let (x, y) = (gx + STEP / 2., gy + STEP / 2.);
            let (cell_x, cell_y) = (gx, gy);
            gx += STEP;
            // The corner's light: 1 at the top-left, nothing past half the diagonal.
            let light = (1. - (x * x + y * y).sqrt() / (diag * 0.5))
                .clamp(0., 1.)
                .powi(2);
            // Calm in the middle of the window, a little stronger toward its edges.
            let (cx, cy) = ((x / w - 0.5) * 2., (y / h - 0.5) * 2.);
            let edge = (cx * cx + cy * cy).sqrt().min(1.);
            let color = |weight: f32| {
                let mut c: Hsla = rgb(mix(accent, ink, (light * 1.4).min(1.))).into();
                c.a = (weight * strength * (0.6 + 0.4 * edge + 1.4 * light)).min(1.);
                c
            };
            let put = |q: PaintQuad, window: &mut Window| {
                if inside(&q.bounds) {
                    window.paint_quad(q);
                }
            };
            match pattern {
                Pattern::Off => {}
                Pattern::Dots => {
                    let r = 0.85 + 0.35 * light;
                    let dot = rect(x - r, y - r, 2. * r, 2. * r);
                    put(fill(dot, color(1.)).corner_radii(px(r)), window);
                }
                Pattern::Grid | Pattern::Blueprint => {
                    // Lines are long and thin, so they read far stronger than a dot.
                    let weight = |line: i64| match pattern {
                        Pattern::Blueprint if line % 4 == 0 => 0.6,
                        Pattern::Blueprint => 0.22,
                        _ => 0.35,
                    };
                    // Lines are cut at the area's edge rather than left out, so the
                    // grid runs right up to it on every side.
                    let clip = |b: Bounds<Pixels>| {
                        let (x0, y0) = (f32::from(b.left()).max(left), f32::from(b.top()).max(top));
                        let x1 = f32::from(b.right()).min(right);
                        let y1 = f32::from(b.bottom()).min(bottom);
                        // A line right on the area's top or left edge would sit
                        // against the border there and read as a double line: the
                        // border stands in for it.
                        let on_edge = anchor.is_some()
                            && ((y1 - y0 <= 1. && y0 < top + 0.5)
                                || (x1 - x0 <= 1. && x0 < left + 0.5));
                        (x1 > x0 && y1 > y0 && !on_edge).then(|| rect(x0, y0, x1 - x0, y1 - y0))
                    };
                    for (line, w) in [
                        (rect(cell_x, cell_y, STEP, 1.), weight(row)),
                        (rect(cell_x, cell_y + 1., 1., STEP - 1.), weight(col)),
                    ] {
                        if let Some(r) = clip(line) {
                            put(fill(r, color(w)), window);
                        }
                    }
                }
                Pattern::Crosses => {
                    let arm = 2.5 + light;
                    let c = color(0.5);
                    put(fill(rect(x - arm, y - 0.5, 2. * arm, 1.), c), window);
                    put(fill(rect(x - 0.5, y - arm, 1., arm - 0.5), c), window);
                    put(fill(rect(x - 0.5, y + 0.5, 1., arm - 0.5), c), window);
                }
                Pattern::Rings => {
                    let r = 2.5 + 0.5 * light;
                    let ring = rect(x - r, y - r, 2. * r, 2. * r);
                    put(
                        outline(ring, color(0.45), BorderStyle::Solid).corner_radii(px(r)),
                        window,
                    );
                }
                Pattern::Stitches => {
                    let shift = if row % 2 == 0 { 0. } else { STEP / 2. };
                    let len = 5. + light;
                    let dash = rect(cell_x + shift + (STEP - len) / 2., y - 0.5, len, 1.);
                    put(fill(dash, color(0.55)).corner_radii(px(0.5)), window);
                }
            }
        }
        gy += STEP;
    }
}
