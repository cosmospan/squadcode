//! Peek with ears: the crew drawn as dark rounded blobs with their own ears
//! and eye colour.
//!
//! Every bot has the same dark body. What tells them apart is the shape of
//! their ears (oval, pointed, round, long, floppy, antenna, notched, tufted)
//! and the colour their eyes glow, so the crew still reads in greyscale and at
//! avatar sizes. Peek, the crew's lead, has oval ears lit lime inside and lime
//! eyes.
//!
//! It takes the same [`FinPose`] as the outline Peek, so moods, gaze and clock
//! offsets carry over. Like the rest of the crew, a frame is a pure function of
//! time. The ears carry most of the acting: they perk up, droop, fold back and
//! twitch with the mood, while the body breathes, hops and shakes.

use vello_cpu::kurbo::{
    Affine, BezPath, Cap, Circle, Ellipse, Join, Point, RoundedRect, Shape, Stroke,
};
use vello_cpu::peniko::{
    color::{AlphaColor, Srgb},
    ColorStop, ColorStops, Gradient, LinearGradientPosition,
};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::bubble::Tint;
use crate::fin::{FinPose, State, OFFSET_STEP};
use crate::look::Rgb;
use crate::Framing;

const TOP: Rgb = Rgb(0x34343C);
const BOTTOM: Rgb = Rgb(0x0E0E11);
const STALK: Rgb = Rgb(0x5A5B62);
const WHITE: Rgb = Rgb(0xFFFFFF);
const INK: Rgb = Rgb(0x101116);
const GREY: Rgb = Rgb(0x9A9AA2);
const AMBER: Rgb = Rgb(0xFFB020);
const DROP: Rgb = Rgb(0x6FB4FF);
/// Peek, the crew's lead, keeps the brand lime.
pub const LIME: Rgb = Rgb(0xC8F04B);

/// The body: a soft rounded blob, wider at the base, leaning into its face.
const BODY: &str = "M14 94C10 62 18 22 52 15C79 10 93 34 90 60C88 80 82 94 80 94Z";

/// The shape of a bot's ears. Each crew colour has its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ear {
    Oval,
    Point,
    Round,
    Long,
    Flop,
    Antenna,
    Notch,
    Tuft,
}

/// The eye colour and ear shape for a bot of `tint`; the lead is lime with
/// oval ears.
pub fn kit(tint: Tint, lead: bool) -> (Rgb, Ear) {
    if lead {
        return (LIME, Ear::Oval);
    }
    match tint {
        Tint::ORANGE => (Rgb(0xFF9A55), Ear::Point),
        Tint::RED => (Rgb(0xFF7070), Ear::Flop),
        Tint::AMBER => (Rgb(0xFFC54F), Ear::Notch),
        Tint::GREEN => (Rgb(0x4FE38E), Ear::Round),
        Tint::VIOLET => (Rgb(0xB6A0FF), Ear::Tuft),
        Tint::BLUE => (Rgb(0x73AEFF), Ear::Long),
        Tint::TEAL => (Rgb(0x40E3D0), Ear::Antenna),
        other => (other.0[0], Ear::Oval),
    }
}

/// How the bot moves: each state borrows one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Idle,
    Think,
    Work,
    Jump,
    Perk,
    Shake,
    Sleep,
    Wave,
}

/// What the eyes do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Open,
    Up,
    Narrow,
    Happy,
    Wide,
    Tilted,
    Closed,
}

/// The little sign beside the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sign {
    None,
    Thought,
    Busy,
    Ask,
    Zzz,
    Sweat,
}

fn act(s: State) -> (Move, Look, Sign) {
    use State::*;
    match s {
        Idle | Shy | Curious | Wink => (Move::Idle, Look::Open, Sign::None),
        Focused => (Move::Work, Look::Narrow, Sign::None),
        Think | Confused => (Move::Think, Look::Up, Sign::Thought),
        Listen | Surprised => (Move::Perk, Look::Wide, Sign::None),
        Work => (Move::Work, Look::Narrow, Sign::Busy),
        Done | Love | Proud => (Move::Jump, Look::Happy, Sign::None),
        Needs => (Move::Perk, Look::Wide, Sign::Ask),
        Error | Sorry | Blocked => (Move::Shake, Look::Tilted, Sign::Sweat),
        Sleep => (Move::Sleep, Look::Closed, Sign::Zzz),
        Hello => (Move::Wave, Look::Open, Sign::None),
    }
}

/// The ears' resting pose for a move: left turn, left size, right turn, right
/// size. Turns are in degrees; positive folds the ear outwards and down.
fn ear_pose(m: Move) -> [f64; 4] {
    match m {
        Move::Idle => [0.0, 1.0, 0.0, 1.0],
        Move::Think => [0.0, 1.0, 38.0, 0.96],
        Move::Work => [24.0, 0.9, 24.0, 0.9],
        Move::Jump => [-8.0, 1.18, -8.0, 1.18],
        Move::Perk => [-12.0, 1.14, -12.0, 1.14],
        Move::Shake => [52.0, 0.94, 96.0, 0.92],
        Move::Sleep => [72.0, 0.94, 72.0, 0.94],
        Move::Wave => [-8.0, 1.08, -14.0, 1.1],
    }
}

/// A transform part: lift (negative is up), width, height and turn in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tf {
    y: f64,
    sx: f64,
    sy: f64,
    rot: f64,
}

const REST: Tf = Tf {
    y: 0.0,
    sx: 1.0,
    sy: 1.0,
    rot: 0.0,
};

const fn tf(y: f64, sx: f64, sy: f64, rot: f64) -> Tf {
    Tf { y, sx, sy, rot }
}

fn ease(x: f64) -> f64 {
    x * x * (3.0 - 2.0 * x)
}

/// Plays `keys` (share of the loop, value) over `period` seconds at `t`,
/// easing in and out between keys.
fn play<T: Copy>(t: f64, period: f64, keys: &[(f64, T)], mix: impl Fn(T, T, f64) -> T) -> T {
    let ph = (t / period).rem_euclid(1.0);
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if ph >= a.0 && ph <= b.0 {
            let k = if b.0 > a.0 {
                (ph - a.0) / (b.0 - a.0)
            } else {
                0.0
            };
            return mix(a.1, b.1, ease(k));
        }
    }
    keys.last().map(|k| k.1).unwrap_or_else(|| keys[0].1)
}

fn lerp(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

fn mix_tf(a: Tf, b: Tf, k: f64) -> Tf {
    tf(
        lerp(a.y, b.y, k),
        lerp(a.sx, b.sx, k),
        lerp(a.sy, b.sy, k),
        lerp(a.rot, b.rot, k),
    )
}

fn play_tf(t: f64, period: f64, keys: &[(f64, Tf)]) -> Tf {
    play(t, period, keys, mix_tf)
}

fn play_f(t: f64, period: f64, keys: &[(f64, f64)]) -> f64 {
    play(t, period, keys, lerp)
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
    /// The whole body, about the middle of its base.
    body: Tf,
    /// The ears: left turn, left size, right turn, right size.
    ears: [f64; 4],
    /// The shadow's size and darkness.
    shadow: (f64, f64),
    /// How open the eyes are, for blinks.
    open: f64,
}

/// Seconds between idle blinks.
const BLINK: f64 = 4.2;

fn rig(m: Move, t: f64) -> Rig {
    let mut r = Rig {
        body: REST,
        ears: ear_pose(m),
        shadow: (1.0, 0.45),
        open: 1.0,
    };
    let blink = |t: f64| {
        let ph = t.rem_euclid(BLINK) / BLINK;
        play_f(
            ph,
            1.0,
            &[(0.0, 1.0), (0.9, 1.0), (0.94, 0.1), (0.98, 1.0), (1.0, 1.0)],
        )
    };
    match m {
        Move::Idle => {
            r.body = play_tf(
                t,
                3.2,
                &[(0.0, REST), (0.5, tf(0.0, 0.99, 1.02, 0.0)), (1.0, REST)],
            );
            // Now and then one ear gives a quick twitch.
            r.ears[0] += 14.0 * bump(t, 5.3, 2.4, 0.3);
            r.ears[2] += 14.0 * bump(t, 5.3, 0.0, 0.3);
            r.open = blink(t);
        }
        Move::Think => {
            r.body = play_tf(
                t,
                3.0,
                &[
                    (0.0, REST),
                    (0.3, tf(0.0, 1.0, 1.0, -3.0)),
                    (0.7, tf(0.0, 1.0, 1.0, 3.0)),
                    (1.0, REST),
                ],
            );
            r.ears[2] += 8.0 * (t * std::f64::consts::TAU / 3.0).sin();
            r.open = blink(t + 1.3);
        }
        Move::Work => {
            r.body = play_tf(
                t,
                0.55,
                &[(0.0, REST), (0.5, tf(-2.0, 0.99, 1.02, 0.0)), (1.0, REST)],
            );
            // The ears flick in turn, as if keeping time.
            r.ears[0] += 6.0 * bump(t, 1.1, 0.0, 0.25);
            r.ears[2] += 6.0 * bump(t, 1.1, 0.55, 0.25);
        }
        Move::Jump => {
            const P: f64 = 1.6;
            r.body = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.12, tf(0.0, 1.1, 0.88, 0.0)),
                    (0.22, tf(-6.0, 0.92, 1.1, 0.0)),
                    (0.40, tf(-18.0, 1.0, 1.0, -3.0)),
                    (0.58, tf(-5.0, 0.96, 1.05, 0.0)),
                    (0.66, tf(0.0, 1.12, 0.87, 0.0)),
                    (0.76, tf(0.0, 0.98, 1.03, 0.0)),
                    (0.86, REST),
                    (1.0, REST),
                ],
            );
            // The ears trail the jump: back on the way up, up at the top.
            let lag = play_f(
                t,
                P,
                &[
                    (0.0, 0.0),
                    (0.22, 16.0),
                    (0.40, -10.0),
                    (0.66, 12.0),
                    (0.86, 0.0),
                    (1.0, 0.0),
                ],
            );
            r.ears[0] += lag;
            r.ears[2] += lag;
            r.shadow.0 = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.12, 1.08),
                    (0.4, 0.6),
                    (0.66, 1.1),
                    (0.86, 1.0),
                    (1.0, 1.0),
                ],
            );
            r.shadow.1 = play_f(t, P, &[(0.0, 0.45), (0.4, 0.22), (0.66, 0.5), (1.0, 0.45)]);
        }
        Move::Perk => {
            const P: f64 = 1.6;
            r.body = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.08, tf(-6.0, 0.96, 1.05, 0.0)),
                    (0.18, REST),
                    (0.26, tf(-4.0, 0.97, 1.03, 0.0)),
                    (0.36, REST),
                    (1.0, REST),
                ],
            );
            let up = play_f(
                t,
                P,
                &[
                    (0.0, 0.0),
                    (0.08, 0.1),
                    (0.2, 0.0),
                    (0.28, 0.08),
                    (0.4, 0.0),
                    (1.0, 0.0),
                ],
            );
            r.ears[1] += up;
            r.ears[3] += up;
        }
        Move::Shake => {
            const P: f64 = 2.2;
            r.body = play_tf(
                t,
                P,
                &[
                    (0.0, tf(1.0, 1.0, 0.98, 0.0)),
                    (0.08, tf(0.0, 1.0, 1.0, -5.0)),
                    (0.16, tf(0.0, 1.0, 1.0, 5.0)),
                    (0.24, tf(0.0, 1.0, 1.0, -3.0)),
                    (0.32, tf(0.0, 1.0, 1.0, 2.0)),
                    (0.40, tf(1.0, 1.0, 0.98, 0.0)),
                    (1.0, tf(1.0, 1.0, 0.98, 0.0)),
                ],
            );
        }
        Move::Sleep => {
            r.body = play_tf(
                t,
                4.0,
                &[
                    (0.0, tf(1.0, 1.0, 0.98, 0.0)),
                    (0.5, tf(2.0, 1.03, 0.95, 0.0)),
                    (1.0, tf(1.0, 1.0, 0.98, 0.0)),
                ],
            );
            r.ears[0] += 4.0 * (t * std::f64::consts::TAU / 4.0).sin();
            r.ears[2] += 4.0 * (t * std::f64::consts::TAU / 4.0).sin();
        }
        Move::Wave => {
            const P: f64 = 2.2;
            r.body = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.10, tf(0.0, 1.06, 0.92, 0.0)),
                    (0.22, tf(-8.0, 0.95, 1.06, -3.0)),
                    (0.34, tf(0.0, 1.06, 0.92, 0.0)),
                    (0.44, REST),
                    (1.0, REST),
                ],
            );
            // The right ear waves hello.
            let ph = t.rem_euclid(P) / P;
            if ph < 0.7 {
                r.ears[2] += 22.0 * (ph / 0.7 * std::f64::consts::TAU * 2.0).sin();
            }
            r.shadow.0 = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.22, 0.75),
                    (0.34, 1.06),
                    (0.44, 1.0),
                    (1.0, 1.0),
                ],
            );
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Drawing, on a 100-unit grid: the body sits on y 94, the ears reach above 0.

type Color = AlphaColor<Srgb>;

struct Pen<'a> {
    ctx: &'a mut RenderContext,
    xf: Affine,
}

impl Pen<'_> {
    fn at(&mut self, t: Affine, f: impl FnOnce(&mut Self)) {
        let old = self.xf;
        self.xf = old * t;
        f(self);
        self.xf = old;
    }

    fn color(c: Rgb, a: f64) -> Color {
        let a = a.clamp(0.0, 1.0);
        Color::from_rgba8(c.r(), c.g(), c.b(), (a * 255.0).round() as u8)
    }

    fn fill(&mut self, shape: &impl Shape, c: Rgb, a: f64) {
        if a <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx.set_paint(Self::color(c, a));
        self.ctx.fill_path(&shape.to_path(0.05));
    }

    /// Fills with the body's top-to-bottom shade, from y 10 to y 95.
    fn fill_body(&mut self, shape: &impl Shape) {
        let stops: Vec<ColorStop> = [(0.0f32, TOP), (1.0, BOTTOM)]
            .iter()
            .map(|&(o, c)| ColorStop::from((o, Self::color(c, 1.0))))
            .collect();
        let g = Gradient {
            kind: LinearGradientPosition {
                start: Point::new(50.0, 10.0),
                end: Point::new(50.0, 95.0),
            }
            .into(),
            stops: ColorStops::from(stops.as_slice()),
            ..Default::default()
        };
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx.set_paint(g);
        self.ctx.fill_path(&shape.to_path(0.05));
    }

    fn stroke(&mut self, shape: &impl Shape, w: f64, c: Rgb, a: f64) {
        if a <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx
            .set_stroke(Stroke::new(w).with_caps(Cap::Round).with_join(Join::Round));
        self.ctx.set_paint(Self::color(c, a));
        self.ctx.stroke_path(&shape.to_path(0.05));
    }
}

fn svg(d: &str) -> BezPath {
    BezPath::from_svg(d).unwrap_or_default()
}

/// `t` applied about `c`.
fn about(c: (f64, f64), t: Affine) -> Affine {
    Affine::translate(c) * t * Affine::translate((-c.0, -c.1))
}

/// A [`Tf`] as an affine about the pivot `c`.
fn place(c: (f64, f64), p: Tf) -> Affine {
    Affine::translate((0.0, p.y))
        * about(
            c,
            Affine::rotate(p.rot.to_radians()) * Affine::scale_non_uniform(p.sx, p.sy),
        )
}

/// A curved eyelid: smiling (`up`) or closed.
fn arc(x: f64, y: f64, r: f64, up: bool) -> BezPath {
    let mut p = BezPath::new();
    let (y0, cy) = if up {
        (y + r * 0.5, y - r * 1.1)
    } else {
        (y - r * 0.2, y + r * 1.1)
    };
    p.move_to((x - r, y0));
    p.quad_to((x, cy), (x + r, y0));
    p
}

/// One ear, standing on (0, 0) and pointing up; `inner` lights its inside.
fn ear(p: &mut Pen, shape: Ear, inner: Rgb, inner_a: f64, rim: bool) {
    let edge = |p: &mut Pen, s: &BezPath| {
        p.fill_body(s);
        if rim {
            p.stroke(s, 2.0, WHITE, 0.22);
        }
    };
    let ell =
        |cx: f64, cy: f64, rx: f64, ry: f64| Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(0.05);
    match shape {
        Ear::Oval => {
            edge(p, &ell(0.0, -13.0, 9.0, 14.5));
            p.fill(&ell(0.0, -14.5, 4.0, 8.5), inner, inner_a);
        }
        Ear::Point => {
            edge(p, &svg("M-12 3Q-7 -18 0 -31Q7 -18 12 3Z"));
            p.fill(&svg("M-4 -2Q-2.5 -12 0 -18Q2.5 -12 4 -2Z"), inner, inner_a);
        }
        Ear::Round => {
            edge(p, &Circle::new((0.0, -8.0), 10.5).to_path(0.05));
            p.fill(&Circle::new((0.0, -9.0), 5.0), inner, inner_a);
        }
        Ear::Long => {
            edge(p, &ell(0.0, -16.0, 7.5, 17.5));
            p.fill(&ell(0.0, -17.5, 3.2, 11.5), inner, inner_a);
        }
        Ear::Flop => edge(
            p,
            &svg("M-8 2Q-12 -14 -2 -22Q10 -28 20 -18Q24 -12 18 -10Q8 -16 6 -6L8 2Z"),
        ),
        Ear::Antenna => {
            p.stroke(&svg("M0 0V-20"), 3.4, STALK, 1.0);
            edge(p, &Circle::new((0.0, -23.0), 5.5).to_path(0.05));
            p.fill(&Circle::new((-1.6, -24.6), 1.8), WHITE, 0.55);
        }
        Ear::Notch => edge(p, &svg("M-10 2L-9 -22L-2 -15L5 -26L10 2Z")),
        Ear::Tuft => edge(
            p,
            &svg("M-9 2Q-12 -14 -4 -24Q-2 -16 1 -27Q4 -17 8 -22Q12 -10 9 2Z"),
        ),
    }
}

fn ears(p: &mut Pen, shape: Ear, pose: [f64; 4], inner: (Rgb, f64), rim: bool) {
    // (base x, base y, lean, turn, size, which way a positive turn goes, mirrored)
    let sides = [
        (37.0, 30.0, -30.0, pose[0], pose[1], -1.0, true),
        (68.0, 24.0, 20.0, pose[2], pose[3], 1.0, false),
    ];
    for (x, y, lean, turn, size, dir, flip) in sides {
        let xf = Affine::translate((x, y))
            * Affine::rotate((lean + dir * turn).to_radians())
            * Affine::scale_non_uniform(if flip { -size } else { size }, size);
        p.at(xf, |p| ear(p, shape, inner.0, inner.1, rim));
    }
}

fn eyes(p: &mut Pen, look: Look, col: Rgb, gaze: (f64, f64), open: f64) {
    let (w, h, y) = (9.0, 18.0, 46.0 + gaze.1);
    for (i, x0) in [56.0, 72.0].into_iter().enumerate() {
        let x = x0 + gaze.0;
        match look {
            Look::Happy => p.stroke(&arc(x, y, 6.8, true), 5.0, col, 1.0),
            Look::Closed => p.stroke(&arc(x, y + 3.6, 6.8, false), 4.5, col, 1.0),
            _ => {
                let (mut ww, mut hh, mut cy) = (w, h, y);
                let mut rot = 14.0;
                match look {
                    Look::Up => cy -= 4.0,
                    Look::Narrow => {
                        hh = h * 0.5;
                        cy += 2.0;
                    }
                    Look::Wide => {
                        hh = h * 1.15;
                        ww = w * 1.12;
                    }
                    Look::Tilted => {
                        hh = h * 0.78;
                        cy += 2.0;
                        rot = if i == 0 { 24.0 } else { -24.0 };
                    }
                    _ => {}
                }
                // Blinks squash the pill about its middle.
                let k = open.max(0.08);
                let hk = hh * k;
                let shape = RoundedRect::new(
                    x - ww / 2.0,
                    cy - hk / 2.0,
                    x + ww / 2.0,
                    cy + hk / 2.0,
                    (ww / 2.0).min(hk / 2.0),
                );
                p.at(about((x, cy), Affine::rotate(f64::to_radians(rot))), |p| {
                    p.fill(&shape, col, 1.0)
                });
            }
        }
    }
    if look == Look::Tilted {
        p.stroke(&svg("M59 67Q65 62 71 67"), 2.8, WHITE, 0.9);
    }
}

fn sign(p: &mut Pen, s: Sign, t: f64) {
    match s {
        Sign::None => {}
        Sign::Thought => {
            p.fill(&Circle::new((83.0, 13.0), 3.0), GREY, 1.0);
            p.fill(&Circle::new((92.0, 5.0), 4.2), GREY, 1.0);
        }
        Sign::Busy => {
            for i in 0..3 {
                let a = 0.4 + 0.6 * ((t * 3.0 - i as f64 * 0.33).rem_euclid(1.0));
                p.fill(&Circle::new((80.0 + i as f64 * 8.0, 8.0), 2.6), GREY, a);
            }
        }
        Sign::Ask => {
            let c = Circle::new((90.0, 12.0), 9.0);
            p.fill(&c, AMBER, 1.0);
            p.stroke(&c, 3.0, INK, 1.0);
        }
        Sign::Zzz => {
            p.stroke(&svg("M82 6H90L82 16H90"), 2.4, GREY, 1.0);
            p.stroke(&svg("M92 -2H97L92 4H97"), 1.8, GREY, 1.0);
        }
        Sign::Sweat => {
            let d = svg("M89 2C89 2 83 10 83 14A6 6 0 0 0 95 14C95 10 89 2 89 2Z");
            p.fill(&d, DROP, 1.0);
            p.stroke(&d, 2.4, INK, 1.0);
        }
    }
}

/// The part of the grid a framing shows: `(x0, y0, side)`.
fn view(framing: Framing) -> (f64, f64, f64) {
    match framing {
        // Room above for the ears and a jump, and beside the head for signs.
        Framing::Full => (-14.0, -30.0, 128.0),
        // Avatars: the body and ears, with a little room to hop.
        Framing::Bust => (-8.0, -18.0, 116.0),
    }
}

/// Draws the bot into `ctx` at time `t`, scaled so the framing fills `px` pixels.
fn draw_into(ctx: &mut RenderContext, pose: &FinPose, t: f64, framing: Framing, px: u16) {
    let (x0, y0, side) = view(framing);
    let k = px as f64 / side;
    let xf = Affine::scale(k) * Affine::translate((-x0, -y0));
    let t = t + pose.offset as f64 * OFFSET_STEP;
    let small = px < 36;
    let lead = pose.fin.ears;
    let (col, shape) = kit(pose.fin.tint, lead);
    let (mv, look, sg) = act(pose.state);
    let mut r = rig(mv, t);
    if framing == Framing::Bust {
        // Avatars sit in tight spots: keep the hops small.
        r.body.y *= 0.35;
    }
    let gaze = (pose.gaze.0 as f64 * 0.35, pose.gaze.1 as f64 * 0.3);
    let mut p = Pen { ctx, xf };
    if framing == Framing::Full {
        let sh = Ellipse::new((50.0, 97.0), (34.0 * r.shadow.0, 3.4 * r.shadow.0), 0.0);
        p.fill(&sh, Rgb(0x000000), r.shadow.1);
    }
    // Lime inside Peek's ears; a soft lift inside everyone else's.
    let inner = if lead { (LIME, 1.0) } else { (WHITE, 0.14) };
    p.at(place((50.0, 94.0), r.body), |p| {
        ears(p, shape, r.ears, inner, true);
        let body = svg(BODY);
        p.fill_body(&body);
        p.stroke(&body, 2.0, WHITE, 0.22);
        if !small {
            p.at(
                about((40.0, 30.0), Affine::rotate(f64::to_radians(-28.0))),
                |p| {
                    p.fill(&Ellipse::new((40.0, 30.0), (13.0, 6.0), 0.0), WHITE, 0.08);
                    p.fill(&Ellipse::new((40.0, 30.0), (8.0, 3.6), 0.0), WHITE, 0.1);
                },
            );
            if !matches!(look, Look::Happy | Look::Closed) {
                p.fill(&Ellipse::new((64.0, 46.0), (19.0, 14.0), 0.0), col, 0.16);
            }
        }
        eyes(p, look, col, gaze, r.open);
        if !small && (framing == Framing::Full || px >= 40) {
            sign(p, sg, t);
        }
    });
}

/// Draws the eared crew into premultiplied RGBA8, reusing buffers between frames.
pub struct EaredRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
}

impl EaredRenderer {
    pub fn new(px: u16) -> Self {
        EaredRenderer {
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

    fn coverage(px: &[u8]) -> usize {
        px.chunks(4).filter(|p| p[3] > 128).count()
    }

    fn pose_of(tint: Tint, lead: bool, state: State) -> FinPose {
        let mut p = Fin {
            tint,
            line: Line::default(),
            ears: lead,
            outfit: Outfit::PLAIN,
        }
        .pose(Mood::Idle);
        p.state = state;
        p
    }

    fn pose(state: State) -> FinPose {
        pose_of(Tint::ORANGE, false, state)
    }

    #[test]
    fn every_state_draws_at_every_size() {
        for px in [22u16, 48, 96] {
            let mut r = EaredRenderer::new(px);
            for s in State::ALL {
                for framing in [Framing::Full, Framing::Bust] {
                    let n = coverage(r.draw(&pose(s), 0.7, framing));
                    let min = (px as usize * px as usize) / 8;
                    assert!(n > min, "{s:?} at {px}px {framing:?} drew {n}");
                }
            }
        }
    }

    #[test]
    fn the_jump_leaves_the_ground() {
        let mut r = EaredRenderer::new(96);
        let p = pose(State::Done);
        let rest = r.draw(&p, 0.0, Framing::Full).to_vec();
        let top = r.draw(&p, 1.6 * 0.4, Framing::Full).to_vec();
        assert_ne!(rest, top);
        // At the top of the jump the bottom of the body has lifted.
        let row = |px: &[u8], y: usize| (0..96).filter(|x| px[(y * 96 + x) * 4 + 3] > 200).count();
        let floor = ((92.0 + 30.0) / 128.0 * 96.0) as usize;
        assert!(row(&rest, floor) > row(&top, floor));
    }

    #[test]
    fn same_time_same_frame() {
        let mut r = EaredRenderer::new(64);
        let p = pose(State::Work);
        let a = r.draw(&p, 2.5, Framing::Bust).to_vec();
        let b = r.draw(&p, 2.5, Framing::Bust).to_vec();
        assert_eq!(a, b);
    }

    #[test]
    fn each_crew_colour_has_its_own_ears() {
        let mut seen = vec![Ear::Oval];
        for tint in [
            Tint::ORANGE,
            Tint::RED,
            Tint::AMBER,
            Tint::GREEN,
            Tint::VIOLET,
            Tint::BLUE,
            Tint::TEAL,
        ] {
            let (_, e) = kit(tint, false);
            assert!(!seen.contains(&e), "{tint:?}");
            seen.push(e);
        }
        assert_eq!(kit(Tint::VIOLET, true), (LIME, Ear::Oval));
    }

    #[test]
    fn ear_shapes_change_the_drawing() {
        let mut r = EaredRenderer::new(64);
        let a = r
            .draw(
                &pose_of(Tint::ORANGE, false, State::Idle),
                0.1,
                Framing::Bust,
            )
            .to_vec();
        let b = r
            .draw(&pose_of(Tint::BLUE, false, State::Idle), 0.1, Framing::Bust)
            .to_vec();
        assert_ne!(a, b);
    }

    #[test]
    fn moods_look_different() {
        let mut r = EaredRenderer::new(64);
        let mut frames: Vec<Vec<u8>> = Vec::new();
        for s in [
            State::Idle,
            State::Work,
            State::Done,
            State::Needs,
            State::Error,
            State::Sleep,
        ] {
            frames.push(r.draw(&pose(s), 0.05, Framing::Bust).to_vec());
        }
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "{i} and {j} match");
            }
        }
    }
}
