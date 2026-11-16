//! Peek as a viewfinder: every bot on the crew is a lens pointed at your code.
//!
//! Four corner marks frame two slits of light, like a camera finding focus.
//! There is no body, mouth or blush: the corners are the body language and
//! the eyes do the expressing. At rest the corners breathe; while thinking the
//! lens swings round and a point of light orbits it; while working the
//! corners snap in to lock focus and a scan line sweeps; when a job is done
//! they close into a full frame, like a shot being taken.
//!
//! Each crew member grinds its own lens, so the crew reads by silhouette
//! alone, even in greyscale: round, sharp, double, notched, bold, long, dotted
//! or chamfered corners. Their eyes glow in the bot's colour. Peek, the lead,
//! keeps the round lens, lime eyes and a small red record light.
//!
//! It takes the same [`FinPose`] as the outline Peek, so moods, gaze and clock
//! offsets carry over, and like the rest of the crew a frame is a pure
//! function of time.

use vello_cpu::kurbo::{Affine, BezPath, Cap, Circle, Join, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::bubble::Tint;
use crate::fin::{FinPose, State, OFFSET_STEP};
use crate::look::Rgb;
use crate::Framing;

/// The corner marks at rest when the pose names no ink: a soft white, for a
/// dark ground. In the app the corners take the theme's text colour instead
/// (see [`FinPose::ink`]), so they read on light themes too.
const LIGHT: Rgb = Rgb(0xE4E4E8);
/// How lit the corners are while thinking: a step dimmer, so the orbiting
/// light leads.
const MIST: f64 = 0.8;
/// How lit the corners are when stuck: faded towards the ground.
const DULL: f64 = 0.4;
/// Stuck eyes, the stuck corner and the lead's record light.
const ALERT: Rgb = Rgb(0xFF6B6B);
const REC: Rgb = Rgb(0xFF5C5C);
const GREY: Rgb = Rgb(0x9A9AA2);
/// Peek, the crew's lead, keeps the brand lime.
pub const LIME: Rgb = Rgb(0xC8F04B);

/// How a bot's corner marks are cut. Each crew colour has its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lens {
    /// Rounded ends and joins: Peek's.
    Round,
    /// Square ends and a mitred vertex.
    Sharp,
    /// Two thin nested marks.
    Double,
    /// Two arms with a gap at the vertex and a dot in it.
    Notch,
    /// Short and heavy.
    Bold,
    /// Long and fine.
    Long,
    /// A dot at the vertex and two ticks.
    Dot,
    /// The vertex cut off at 45°.
    Chamfer,
}

/// Generates `ALL`, `id`, `name` and `from_id` for a picker enum.
macro_rules! choices {
    ($t:ty { $($v:ident => $id:literal, $name:literal;)* }) => {
        impl $t {
            pub const ALL: &'static [$t] = &[$(<$t>::$v),*];

            /// The saved form.
            pub fn id(self) -> &'static str {
                match self { $(<$t>::$v => $id),* }
            }

            /// What the picker calls it.
            pub fn name(self) -> &'static str {
                match self { $(<$t>::$v => $name),* }
            }

            pub fn from_id(id: &str) -> Option<$t> {
                match id { $($id => Some(<$t>::$v),)* _ => None }
            }
        }
    };
}

choices!(Lens {
    Round => "round", "Round";
    Sharp => "sharp", "Sharp";
    Double => "double", "Double";
    Notch => "notch", "Notched";
    Bold => "bold", "Bold";
    Long => "long", "Long";
    Dot => "dot", "Dotted";
    Chamfer => "chamfer", "Chamfered";
});

/// The shape of a bot's eyes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Eyes {
    /// Two tall pills: the default.
    #[default]
    Pill,
    /// Two round dots.
    Dot,
    /// Two squared-off blocks.
    Block,
    /// Two diamonds.
    Diamond,
    /// One wide visor across both.
    Visor,
}

choices!(Eyes {
    Pill => "pill", "Pills";
    Dot => "dot", "Dots";
    Block => "block", "Blocks";
    Diamond => "diamond", "Diamonds";
    Visor => "visor", "Visor";
});

/// The small light inside the top-right corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Light {
    /// The lead's red record light; none for the rest.
    #[default]
    Auto,
    Off,
    /// A red record light.
    Rec,
    /// A light in the bot's own colour.
    Glow,
}

choices!(Light {
    Auto => "auto", "Auto";
    Off => "off", "Off";
    Rec => "rec", "Record";
    Glow => "glow", "Own colour";
});

/// What colour the corner marks are at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Corners {
    /// The theme's text colour: dark on light themes, light on dark ones.
    #[default]
    Light,
    /// The bot's own colour.
    Tinted,
}

choices!(Corners {
    Light => "light", "Theme";
    Tinted => "tinted", "Own colour";
});

/// The person's choices for one bot, on top of its colour. The default is
/// what the colour (and being the lead) gives on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Kit {
    /// `None` keeps the colour's own lens.
    pub lens: Option<Lens>,
    pub eyes: Eyes,
    pub light: Light,
    pub corners: Corners,
}

impl Kit {
    /// `lens.eyes.light.corners`, with `auto` for the colour's own lens.
    pub fn code(self) -> String {
        format!(
            "{}.{}.{}.{}",
            self.lens.map_or("auto", Lens::id),
            self.eyes.id(),
            self.light.id(),
            self.corners.id()
        )
    }

    /// Reads [`Kit::code`]; anything it doesn't know is left at its default.
    pub fn from_code(code: &str) -> Kit {
        let mut it = code.split('.');
        let mut next = || it.next().unwrap_or("");
        Kit {
            lens: Lens::from_id(next()),
            eyes: Eyes::from_id(next()).unwrap_or_default(),
            light: Light::from_id(next()).unwrap_or_default(),
            corners: Corners::from_id(next()).unwrap_or_default(),
        }
    }

    /// The lens a bot of `tint` wears with this kit.
    pub fn lens_for(self, tint: Tint, lead: bool) -> Lens {
        self.lens.unwrap_or(kit(tint, lead).1)
    }
}

/// The eye colour and lens for a bot of `tint`; the lead is lime with the
/// round lens.
pub fn kit(tint: Tint, lead: bool) -> (Rgb, Lens) {
    if lead {
        return (LIME, Lens::Round);
    }
    match tint {
        Tint::ORANGE => (Rgb(0xFF9A55), Lens::Sharp),
        Tint::RED => (Rgb(0xFF7070), Lens::Double),
        Tint::AMBER => (Rgb(0xFFC54F), Lens::Notch),
        Tint::GREEN => (Rgb(0x4FE38E), Lens::Bold),
        Tint::VIOLET => (Rgb(0xB6A0FF), Lens::Long),
        Tint::BLUE => (Rgb(0x73AEFF), Lens::Dot),
        Tint::TEAL => (Rgb(0x40E3D0), Lens::Chamfer),
        other => (other.0[0], Lens::Round),
    }
}

/// How the lens moves: each state borrows one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Idle,
    Think,
    Work,
    Ask,
    Done,
    Stuck,
    Sleep,
    Hello,
}

/// What the eyes do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Open,
    Up,
    Narrow,
    Wide,
    Happy,
    Flat,
    Closed,
}

fn act(s: State) -> (Move, Look) {
    use State::*;
    match s {
        Idle | Shy | Curious | Wink => (Move::Idle, Look::Open),
        Think | Confused => (Move::Think, Look::Up),
        Work | Focused => (Move::Work, Look::Narrow),
        Listen | Surprised | Needs => (Move::Ask, Look::Wide),
        Done | Love | Proud => (Move::Done, Look::Happy),
        Error | Sorry | Blocked => (Move::Stuck, Look::Flat),
        Sleep => (Move::Sleep, Look::Closed),
        Hello => (Move::Hello, Look::Open),
    }
}

fn ease(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

fn lerp(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

/// Plays `keys` (share of the loop, value) over `period` seconds at `t`,
/// easing in and out between keys.
fn play(t: f64, period: f64, keys: &[(f64, f64)]) -> f64 {
    let ph = (t / period).rem_euclid(1.0);
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if ph >= a.0 && ph <= b.0 {
            let k = if b.0 > a.0 {
                (ph - a.0) / (b.0 - a.0)
            } else {
                0.0
            };
            return lerp(a.1, b.1, ease(k));
        }
    }
    keys.last().map(|k| k.1).unwrap_or(keys[0].1)
}

/// A quick there-and-back bump, 0 → 1 → 0, lasting `len` seconds and
/// starting every `every` seconds (offset by `at`).
fn bump(t: f64, every: f64, at: f64, len: f64) -> f64 {
    let k = (t - at).rem_euclid(every);
    if k < len {
        (k / len * std::f64::consts::PI).sin()
    } else {
        0.0
    }
}

/// Where every moving part is at `t`.
#[derive(Clone, Copy, Debug)]
struct Rig {
    /// How far in from the edge of the grid the corner vertices sit.
    inset: f64,
    /// Arm length at rest.
    arm: f64,
    /// How far the corners have closed into a full frame, 0 to 1.
    shut: f64,
    /// The lens's turn about its middle, in degrees.
    turn: f64,
    /// Each corner's own push outwards, for the stuck jitter.
    jitter: [f64; 4],
    /// Corner colour and how lit it is.
    ink: Rgb,
    ink_a: f64,
    /// How open the eyes are, for blinks.
    open: f64,
    /// The eyes' own glance, in grid units.
    glance: (f64, f64),
    /// A soft fill inside a closed frame, 0 to 1.
    flash: f64,
    /// The whole drawing's opacity.
    alpha: f64,
}

/// Seconds between idle blinks.
const BLINK: f64 = 4.2;

fn rig(m: Move, t: f64, col: Rgb, base: Rgb) -> Rig {
    let mut r = Rig {
        inset: 15.0,
        arm: 16.0,
        shut: 0.0,
        turn: 0.0,
        jitter: [0.0; 4],
        ink: base,
        ink_a: 1.0,
        open: 1.0,
        glance: (0.0, 0.0),
        flash: 0.0,
        alpha: 1.0,
    };
    let blink = |t: f64| {
        play(
            t.rem_euclid(BLINK) / BLINK,
            1.0,
            &[(0.0, 1.0), (0.9, 1.0), (0.94, 0.1), (0.98, 1.0), (1.0, 1.0)],
        )
    };
    match m {
        Move::Idle => {
            // Breathing: the corners drift out and back.
            r.inset = play(t, 3.6, &[(0.0, 15.0), (0.5, 13.6), (1.0, 15.0)]);
            r.open = blink(t);
            r.glance.0 = play(
                t,
                7.0,
                &[
                    (0.0, 0.0),
                    (0.3, 0.0),
                    (0.36, -2.2),
                    (0.52, -2.2),
                    (0.58, 2.2),
                    (0.74, 2.2),
                    (0.8, 0.0),
                    (1.0, 0.0),
                ],
            );
        }
        Move::Think => {
            r.inset = 17.0;
            r.ink_a = MIST;
            // The lens swings a quarter turn in two beats, resting at 45°.
            r.turn = play(
                t,
                3.2,
                &[
                    (0.0, 0.0),
                    (0.3, 45.0),
                    (0.5, 45.0),
                    (0.8, 90.0),
                    (1.0, 90.0),
                ],
            );
            r.open = blink(t + 1.3);
        }
        Move::Work => {
            // Locked in, with a small focus hunt now and then.
            r.inset = 22.0 - 1.6 * bump(t, 1.8, 0.0, 0.3);
        }
        Move::Ask => {
            r.ink = col;
            let k = play(t, 1.4, &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
            r.inset = 15.0 - 2.2 * k;
            r.ink_a = 0.75 + 0.25 * k;
        }
        Move::Done => {
            r.ink = col;
            // Snap shut into a full frame, flash, hold, open again.
            let shut = play(
                t,
                2.6,
                &[(0.0, 0.0), (0.12, 1.0), (0.8, 1.0), (0.95, 0.0), (1.0, 0.0)],
            );
            r.inset = lerp(15.0, 13.0, shut);
            r.shut = shut;
            r.flash = play(t, 2.6, &[(0.0, 0.0), (0.12, 1.0), (0.4, 0.25), (1.0, 0.0)]);
        }
        Move::Stuck => {
            r.inset = 11.0;
            r.ink_a = DULL;
            // Bursts of jitter, each corner its own way.
            let burst = bump(t, 2.2, 0.0, 0.5);
            for (i, j) in r.jitter.iter_mut().enumerate() {
                *j = burst * 1.8 * (t * 47.0 + i as f64 * 1.7).sin();
            }
        }
        Move::Sleep => {
            r.alpha = 0.45;
            r.inset = play(t, 4.4, &[(0.0, 16.0), (0.5, 17.2), (1.0, 16.0)]);
        }
        Move::Hello => {
            // The corners spring out and back, twice, then rest.
            r.inset = play(
                t,
                2.2,
                &[
                    (0.0, 15.0),
                    (0.1, 9.0),
                    (0.22, 16.0),
                    (0.32, 11.0),
                    (0.44, 15.0),
                    (1.0, 15.0),
                ],
            );
            r.open = blink(t + 3.0);
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Drawing, on a 100-unit grid centred on (50, 50).

type Color = AlphaColor<Srgb>;

struct Pen<'a> {
    ctx: &'a mut RenderContext,
    xf: Affine,
    alpha: f64,
}

impl Pen<'_> {
    fn at(&mut self, t: Affine, f: impl FnOnce(&mut Self)) {
        let old = self.xf;
        self.xf = old * t;
        f(self);
        self.xf = old;
    }

    fn color(&self, c: Rgb, a: f64) -> Color {
        let a = (a * self.alpha).clamp(0.0, 1.0);
        Color::from_rgba8(c.r(), c.g(), c.b(), (a * 255.0).round() as u8)
    }

    fn fill(&mut self, shape: &impl Shape, c: Rgb, a: f64) {
        if a <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        let paint = self.color(c, a);
        self.ctx.set_paint(paint);
        self.ctx.fill_path(&shape.to_path(0.05));
    }

    fn stroke_with(&mut self, shape: &impl Shape, s: Stroke, c: Rgb, a: f64) {
        if a <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx.set_stroke(s);
        let paint = self.color(c, a);
        self.ctx.set_paint(paint);
        self.ctx.stroke_path(&shape.to_path(0.05));
    }

    fn stroke(&mut self, shape: &impl Shape, w: f64, c: Rgb, a: f64) {
        self.stroke_with(shape, round(w), c, a)
    }
}

fn round(w: f64) -> Stroke {
    Stroke::new(w).with_caps(Cap::Round).with_join(Join::Round)
}

fn path(points: &[(f64, f64)]) -> BezPath {
    let mut p = BezPath::new();
    for (i, &pt) in points.iter().enumerate() {
        if i == 0 {
            p.move_to(pt);
        } else {
            p.line_to(pt);
        }
    }
    p
}

/// `t` applied about `c`.
fn about(c: (f64, f64), t: Affine) -> Affine {
    Affine::translate(c) * t * Affine::translate((-c.0, -c.1))
}

/// The top-left corner mark of `lens`, its vertex at (`a`, `a`), arms `l`
/// long and `w` thick, closed `shut` of the way into a full frame. The other
/// three are this one turned about the middle.
#[allow(clippy::too_many_arguments)]
fn corner(p: &mut Pen, lens: Lens, a: f64, l: f64, shut: f64, w: f64, c: Rgb, ca: f64) {
    // Short lenses and long ones all meet at the middle of each side once shut.
    let reach = |f: f64| lerp(l * f, 50.0 - a, shut);
    let l = reach(1.0);
    let ell = |l: f64| path(&[(a, a + l), (a, a), (a + l, a)]);
    match lens {
        Lens::Round => p.stroke(&ell(l), w, c, ca),
        Lens::Sharp => p.stroke_with(
            &ell(l),
            Stroke::new(w).with_caps(Cap::Butt).with_join(Join::Miter),
            c,
            ca,
        ),
        Lens::Double => {
            let t = w * 0.48;
            p.stroke(&ell(l), t, c, ca);
            let g = w * 0.85;
            p.stroke(
                &path(&[(a + g, a + l), (a + g, a + g), (a + l, a + g)]),
                t,
                c,
                ca,
            );
        }
        Lens::Notch => {
            let g = w * 1.15;
            p.stroke(&path(&[(a, a + g), (a, a + l)]), w, c, ca);
            p.stroke(&path(&[(a + g, a), (a + l, a)]), w, c, ca);
            p.fill(&Circle::new((a, a), w * 0.44), c, ca);
        }
        Lens::Bold => p.stroke(&ell(reach(0.75)), w * 1.55, c, ca),
        Lens::Long => p.stroke(&ell(reach(1.45)), w * 0.65, c, ca),
        Lens::Dot => {
            p.fill(&Circle::new((a, a), w * 0.73), c, ca);
            let g = w * 1.65;
            p.stroke(&path(&[(a, a + g), (a, a + l)]), w, c, ca);
            p.stroke(&path(&[(a + g, a), (a + l, a)]), w, c, ca);
        }
        Lens::Chamfer => {
            let cut = w * 1.1;
            p.stroke(
                &path(&[(a, a + l), (a, a + cut), (a + cut, a), (a + l, a)]),
                w,
                c,
                ca,
            );
        }
    }
}

/// A smiling `^` or a closed `‿` eye.
fn arc(x: f64, y: f64, r: f64, up: bool) -> BezPath {
    let mut p = BezPath::new();
    if up {
        p.move_to((x - r, y + r * 0.5));
        p.line_to((x, y - r * 0.6));
        p.line_to((x + r, y + r * 0.5));
    } else {
        p.move_to((x - r, y));
        p.quad_to((x, y + r), (x + r, y));
    }
    p
}

/// The two eyes, centred on `at`, with a soft glow round them when `glow`.
#[allow(clippy::too_many_arguments)]
fn eyes(
    p: &mut Pen,
    shape: Eyes,
    look: Look,
    col: Rgb,
    at: (f64, f64),
    open: f64,
    glow: f64,
    big: bool,
) {
    let k = if big { 1.3 } else { 1.0 };
    let visor = shape == Eyes::Visor && !matches!(look, Look::Happy | Look::Closed);
    let sides: &[f64] = if visor { &[0.0] } else { &[-1.0, 1.0] };
    for &side in sides {
        let x = at.0 + side * 6.0 * k;
        let y = at.1;
        match look {
            Look::Happy => p.stroke(&arc(x, y, 4.2 * k, true), 3.2 * k, col, 1.0),
            Look::Closed => p.stroke(&arc(x, y, 4.0 * k, false), 2.8 * k, col, 0.7),
            _ => {
                let (w, h, dx, dy): (f64, f64, f64, f64) = match look {
                    Look::Up => (6.5, 10.0, 2.0, -4.0),
                    Look::Narrow => (10.0, 3.6, 0.0, 0.0),
                    Look::Wide => (7.6, 13.4, 0.0, 0.0),
                    Look::Flat => (9.0, 3.2, 0.0, 1.0),
                    _ => (7.0, 12.0, 0.0, 0.0),
                };
                // Each shape reshapes the open eye; slits stay slits.
                let tall = h > w;
                let (w, h, corner) = match shape {
                    Eyes::Pill => (w, h, 0.5),
                    Eyes::Dot if tall => {
                        let d = (w + h) * 0.42;
                        (d, d, 0.5)
                    }
                    Eyes::Dot => (w, h, 0.5),
                    Eyes::Block => (w * 1.1, h * 0.9, 0.16),
                    Eyes::Diamond if tall => (w * 1.35, h * 0.95, 0.0),
                    Eyes::Diamond => (w, h, 0.5),
                    Eyes::Visor => (w + 12.0, (h * 0.55).max(3.0), 0.5),
                };
                let (w, h) = (w * k, (h * k * open.max(0.1)).max(1.4));
                let (cx, cy) = (x + dx, y + dy);
                let pill = |g: f64| {
                    RoundedRect::new(
                        cx - w / 2.0 - g,
                        cy - h / 2.0 - g,
                        cx + w / 2.0 + g,
                        cy + h / 2.0 + g,
                        w.min(h) * corner + g,
                    )
                };
                for (g, a) in [(5.0, 0.06), (3.4, 0.08), (1.8, 0.12)] {
                    p.fill(&pill(g), col, a * glow);
                }
                if shape == Eyes::Diamond && tall {
                    let d = path(&[
                        (cx, cy - h / 2.0),
                        (cx + w / 2.0, cy),
                        (cx, cy + h / 2.0),
                        (cx - w / 2.0, cy),
                    ]);
                    p.fill(&d, col, 1.0);
                } else {
                    p.fill(&pill(0.0), col, 1.0);
                }
            }
        }
    }
}

/// The part of the grid a framing shows: `(x0, y0, side)`.
fn view(framing: Framing) -> (f64, f64, f64) {
    match framing {
        // Room round the lens for its breathing, the orbit and the z.
        Framing::Full => (-10.0, -10.0, 120.0),
        // Avatars: the lens fills the square.
        Framing::Bust => (6.0, 6.0, 88.0),
    }
}

/// Draws the bot into `ctx` at time `t`, scaled so the framing fills `px` pixels.
fn draw_into(ctx: &mut RenderContext, pose: &FinPose, t: f64, framing: Framing, px: u16) {
    let (x0, y0, side) = view(framing);
    let xf = Affine::scale(px as f64 / side) * Affine::translate((-x0, -y0));
    let t = t + pose.offset as f64 * OFFSET_STEP;
    // Below 36px fine detail turns to mush: heavier corners, bigger eyes,
    // no glow, orbit or signs.
    let small = px < 36;
    let lead = pose.fin.ears;
    let style = pose.fin.kit;
    let col = kit(pose.fin.tint, lead).0;
    let lens = style.lens_for(pose.fin.tint, lead);
    let (mv, look) = act(pose.state);
    let base = pose.ink.unwrap_or(LIGHT);
    let mut r = rig(mv, t, col, base);
    if style.corners == Corners::Tinted && r.ink == base && mv != Move::Stuck {
        r.ink = col;
        r.ink_a = 1.0;
    }
    let mut p = Pen {
        ctx,
        xf,
        alpha: r.alpha,
    };
    let (w, arm) = if small { (8.5, 18.0) } else { (5.5, r.arm) };
    let mid = (50.0, 50.0);

    if r.flash > 0.0 {
        let a = r.inset;
        let frame = RoundedRect::new(a, a, 100.0 - a, 100.0 - a, 4.0);
        p.fill(&frame, col, 0.14 * r.flash);
    }
    p.at(about(mid, Affine::rotate(r.turn.to_radians())), |p| {
        for i in 0..4 {
            let a = r.inset - r.jitter[i];
            // Stuck: one corner gives the game away in red.
            let (ink, ink_a) = if mv == Move::Stuck && i == 1 {
                (ALERT, 1.0)
            } else {
                (r.ink, r.ink_a)
            };
            let turn = Affine::rotate(f64::to_radians(90.0 * i as f64));
            p.at(about(mid, turn), |p| {
                corner(p, lens, a, arm, r.shut, w, ink, ink_a)
            });
        }
    });

    // The lead's record light, inside the top-right corner: steady at rest,
    // blinking while it works, off while it sleeps.
    let light = match style.light {
        Light::Auto if lead => Some(REC),
        Light::Rec => Some(REC),
        Light::Glow => Some(col),
        _ => None,
    };
    if let Some(lc) =
        light.filter(|_| !small && !matches!(mv, Move::Stuck | Move::Done | Move::Sleep))
    {
        let on = if mv == Move::Work {
            if t.rem_euclid(1.0) < 0.6 {
                1.0
            } else {
                0.15
            }
        } else {
            1.0
        };
        let c = (100.0 - r.inset - 7.0, r.inset + 7.0);
        p.fill(&Circle::new(c, 5.5), lc, 0.18 * on);
        p.fill(&Circle::new(c, 3.0), lc, on);
    }

    let gaze = (
        pose.gaze.0 as f64 * 0.3 + r.glance.0,
        pose.gaze.1 as f64 * 0.25 + r.glance.1,
    );
    let eye_col = if mv == Move::Stuck { ALERT } else { col };
    let glow = if small {
        0.0
    } else if pose.shine {
        1.6
    } else {
        1.0
    };
    eyes(
        &mut p,
        style.eyes,
        look,
        eye_col,
        (50.0 + gaze.0, 50.0 + gaze.1),
        r.open,
        glow,
        small,
    );

    if small {
        return;
    }
    match mv {
        Move::Think => {
            // A point of light orbits the lens.
            let a = (t / 3.2) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
            let c = (50.0 + 31.0 * a.cos(), 50.0 + 31.0 * a.sin());
            p.fill(&Circle::new(c, 4.6), col, 0.2);
            p.fill(&Circle::new(c, 2.4), col, 1.0);
        }
        Move::Work => {
            // The scan line sweeps the frame.
            let y = 50.0 + 14.0 * (t * std::f64::consts::TAU / 1.6).sin();
            p.stroke(&path(&[(30.0, y), (70.0, y)]), 1.6, col, 0.55);
        }
        Move::Sleep if framing == Framing::Full || px >= 48 => {
            let k = (t / 3.0).rem_euclid(1.0);
            let (x, y) = (76.0 + 3.0 * k, 26.0 - 8.0 * k);
            let a = (1.0 - k) * 1.6;
            let z = path(&[(x, y), (x + 6.0, y), (x, y + 7.0), (x + 6.0, y + 7.0)]);
            p.stroke(&z, 2.2, GREY, a.min(1.0));
        }
        _ => {}
    }
}

/// Draws the viewfinder crew into premultiplied RGBA8, reusing buffers between frames.
pub struct ViewfinderRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
}

impl ViewfinderRenderer {
    pub fn new(px: u16) -> Self {
        ViewfinderRenderer {
            ctx: RenderContext::new(px, px),
            resources: Resources::new(),
            pixmap: Pixmap::new(px, px),
        }
    }

    pub fn size(&self) -> u16 {
        self.ctx.width()
    }

    /// Draws `pose` at time `t`; premultiplied RGBA8, `size()` pixels square.
    pub fn draw(&mut self, pose: &FinPose, t: f64, framing: Framing) -> &[u8] {
        let px = self.ctx.width();
        self.ctx.reset();
        draw_into(&mut self.ctx, pose, t, framing, px);
        self.ctx.flush();
        self.pixmap = Pixmap::new(px, px);
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        self.pixmap.data_as_u8_slice()
    }
}

/// Busy enough to need a quick frame rate.
pub fn lively(pose: &FinPose) -> bool {
    pose.shine || !matches!(act(pose.state).0, Move::Idle | Move::Sleep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fin::{Fin, Line, Outfit};
    use crate::look::Mood;

    /// Pixels clearly drawn; asleep the lens is dimmed to under half.
    fn coverage(px: &[u8]) -> usize {
        px.chunks(4).filter(|p| p[3] > 64).count()
    }

    fn pose_of(tint: Tint, lead: bool, state: State) -> FinPose {
        let mut p = Fin {
            tint,
            line: Line::default(),
            ears: lead,
            outfit: Outfit::PLAIN,
            kit: Kit::default(),
        }
        .pose(Mood::Idle);
        p.state = state;
        p
    }

    fn pose(state: State) -> FinPose {
        pose_of(Tint::ORANGE, false, state)
    }

    #[test]
    fn corners_take_the_theme_ink() {
        // Opaque pixels averaged: dark ink on a light theme, light on a dark one.
        let mean = |ink: Rgb| {
            let mut r = ViewfinderRenderer::new(96);
            let px = r.draw(&pose(State::Idle).inked(ink), 0.5, Framing::Bust);
            let solid: Vec<_> = px.chunks(4).filter(|p| p[3] == 255).collect();
            assert!(!solid.is_empty());
            solid
                .iter()
                .map(|p| p[0] as usize + p[1] as usize + p[2] as usize)
                .sum::<usize>()
                / (3 * solid.len())
        };
        assert!(mean(Rgb(0x1A1A17)) < 90, "dark ink drew light corners");
        assert!(mean(Rgb(0xEDEDE6)) > 160, "light ink drew dark corners");
        // With no ink the corners stay the soft white they were.
        let mut r = ViewfinderRenderer::new(64);
        let plain = r.draw(&pose(State::Idle), 0.5, Framing::Bust).to_vec();
        let white = r
            .draw(&pose(State::Idle).inked(LIGHT), 0.5, Framing::Bust)
            .to_vec();
        assert_eq!(plain, white);
    }

    #[test]
    fn every_state_draws_at_every_size() {
        for px in [22u16, 48, 96] {
            let mut r = ViewfinderRenderer::new(px);
            for s in State::ALL {
                for framing in [Framing::Full, Framing::Bust] {
                    let n = coverage(r.draw(&pose(s), 0.7, framing));
                    // A lens is mostly open space: the corners and eyes alone.
                    let min = (px as usize * px as usize) / 30;
                    assert!(n > min, "{s:?} at {px}px {framing:?} drew {n}");
                }
            }
        }
    }

    #[test]
    fn done_closes_the_frame() {
        let mut r = ViewfinderRenderer::new(100);
        let p = pose(State::Done);
        let open = r.draw(&p, 2.6 * 0.97, Framing::Bust).to_vec();
        let shut = r.draw(&p, 2.6 * 0.5, Framing::Bust).to_vec();
        // Halfway down the left edge, where the arms only meet once shut.
        let side = 100.0 / 88.0;
        let (x, y) = (
            ((13.0 - 6.0) * side) as usize,
            ((50.0 - 6.0) * side) as usize,
        );
        let lit = |px: &[u8]| (x.saturating_sub(2)..x + 3).any(|x| px[(y * 100 + x) * 4 + 3] > 128);
        assert!(!lit(&open));
        assert!(lit(&shut));
    }

    #[test]
    fn kit_codes_round_trip() {
        let k = Kit {
            lens: Some(Lens::Chamfer),
            eyes: Eyes::Visor,
            light: Light::Glow,
            corners: Corners::Tinted,
        };
        assert_eq!(Kit::from_code(&k.code()), k);
        assert_eq!(Kit::from_code(&Kit::default().code()), Kit::default());
        assert_eq!(Kit::from_code("nonsense"), Kit::default());
    }

    #[test]
    fn every_choice_changes_the_drawing() {
        let mut r = ViewfinderRenderer::new(64);
        let base = pose(State::Idle);
        let plain = r.draw(&base, 0.1, Framing::Bust).to_vec();
        let mut kits = vec![
            Kit {
                light: Light::Glow,
                ..Kit::default()
            },
            Kit {
                corners: Corners::Tinted,
                ..Kit::default()
            },
        ];
        for &l in Lens::ALL.iter().filter(|&&l| l != Lens::Sharp) {
            kits.push(Kit {
                lens: Some(l),
                ..Kit::default()
            });
        }
        for &e in Eyes::ALL.iter().filter(|&&e| e != Eyes::Pill) {
            kits.push(Kit {
                eyes: e,
                ..Kit::default()
            });
        }
        for k in kits {
            let mut p = base;
            p.fin.kit = k;
            assert_ne!(r.draw(&p, 0.1, Framing::Bust), &plain[..], "{k:?}");
        }
    }

    #[test]
    fn same_time_same_frame() {
        let mut r = ViewfinderRenderer::new(64);
        let p = pose(State::Work);
        let a = r.draw(&p, 2.5, Framing::Bust).to_vec();
        let b = r.draw(&p, 2.5, Framing::Bust).to_vec();
        assert_eq!(a, b);
    }

    #[test]
    fn each_crew_colour_has_its_own_lens() {
        let mut seen = vec![Lens::Round];
        for tint in [
            Tint::ORANGE,
            Tint::RED,
            Tint::AMBER,
            Tint::GREEN,
            Tint::VIOLET,
            Tint::BLUE,
            Tint::TEAL,
        ] {
            let (_, l) = kit(tint, false);
            assert!(!seen.contains(&l), "{tint:?}");
            seen.push(l);
        }
        assert_eq!(kit(Tint::VIOLET, true), (LIME, Lens::Round));
    }

    #[test]
    fn lenses_change_the_drawing() {
        let mut r = ViewfinderRenderer::new(64);
        let mut frames: Vec<Vec<u8>> = Vec::new();
        for tint in [Tint::ORANGE, Tint::RED, Tint::AMBER, Tint::BLUE] {
            frames.push(
                r.draw(&pose_of(tint, false, State::Idle), 0.1, Framing::Bust)
                    .to_vec(),
            );
        }
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "{i} and {j} match");
            }
        }
    }

    #[test]
    fn moods_look_different() {
        let mut r = ViewfinderRenderer::new(64);
        let mut frames: Vec<Vec<u8>> = Vec::new();
        for s in [
            State::Idle,
            State::Think,
            State::Work,
            State::Done,
            State::Needs,
            State::Error,
            State::Sleep,
        ] {
            frames.push(r.draw(&pose(s), 0.6, Framing::Bust).to_vec());
        }
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "{i} and {j} match");
            }
        }
    }
}
