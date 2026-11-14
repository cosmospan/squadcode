//! Termi: the crew drawn as a little retro computer on a stand.
//!
//! A matte graphite monitor with two pill eyes on its screen, a springy neck
//! and a flat base. Every bot is the same Termi: only the screen colour and
//! the shape of the light on its antenna change, so the crew still tells
//! apart in greyscale and at avatar sizes.
//!
//! It takes the same [`FinPose`] as Peek, so moods, gaze and clock offsets
//! carry over. Like the rest of the crew, a frame is a pure function of time.
//! Three parts move on their own: the whole bot hops with squash and stretch,
//! the screen tilts and shakes, and the neck stretches like a spring.

use vello_cpu::kurbo::{Affine, BezPath, Cap, Circle, Ellipse, Join, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::bubble::Tint;
use crate::fin::{FinPose, State, OFFSET_STEP};
use crate::look::Rgb;
use crate::Framing;

const BODY: Rgb = Rgb(0x1C1D22);
const EDGE: Rgb = Rgb(0x3A3C44);
const SCREEN: Rgb = Rgb(0x08090B);
const NECK: Rgb = Rgb(0x24262C);
const BASE: Rgb = Rgb(0x33353D);
const STALK: Rgb = Rgb(0x2A2C33);
const OFF: Rgb = Rgb(0x4A4C55);
const WHITE: Rgb = Rgb(0xFFFFFF);
const INK: Rgb = Rgb(0x101116);
const GREY: Rgb = Rgb(0x9A9AA2);
const AMBER: Rgb = Rgb(0xFFB020);
const DROP: Rgb = Rgb(0x6FB4FF);
/// Peek, the crew's lead, keeps the brand lime.
pub const LIME: Rgb = Rgb(0xC8F04B);

/// The light on top of the antenna. Each crew colour has its own shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Ball,
    Hex,
    Plus,
    Square,
    Diamond,
    Ring,
    Star,
    Arrow,
}

/// The screen colour and antenna light for a bot of `tint`; the lead is lime.
pub fn kit(tint: Tint, lead: bool) -> (Rgb, Light) {
    if lead {
        return (LIME, Light::Ball);
    }
    match tint {
        Tint::ORANGE => (Rgb(0xFF9A55), Light::Hex),
        Tint::RED => (Rgb(0xFF7070), Light::Plus),
        Tint::AMBER => (Rgb(0xFFC54F), Light::Square),
        Tint::GREEN => (Rgb(0x4FE38E), Light::Diamond),
        Tint::VIOLET => (Rgb(0xB6A0FF), Light::Ring),
        Tint::BLUE => (Rgb(0x73AEFF), Light::Star),
        Tint::TEAL => (Rgb(0x40E3D0), Light::Arrow),
        other => (other.0[0], Light::Ball),
    }
}

/// How the bot moves: each state borrows one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Idle,
    Think,
    Work,
    Jump,
    Pop,
    Shake,
    Sleep,
    Hello,
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

/// The little sign beside the screen.
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
        Focused => (Move::Idle, Look::Narrow, Sign::Busy),
        Think | Confused => (Move::Think, Look::Up, Sign::Thought),
        Listen => (Move::Pop, Look::Wide, Sign::None),
        Work => (Move::Work, Look::Narrow, Sign::Busy),
        Done | Love | Proud => (Move::Jump, Look::Happy, Sign::None),
        Needs => (Move::Pop, Look::Wide, Sign::Ask),
        Surprised => (Move::Pop, Look::Wide, Sign::None),
        Error | Sorry | Blocked => (Move::Shake, Look::Tilted, Sign::Sweat),
        Sleep => (Move::Sleep, Look::Closed, Sign::Zzz),
        Hello => (Move::Hello, Look::Open, Sign::None),
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

/// Where every moving part is at `t`.
#[derive(Clone, Copy, Debug)]
struct Rig {
    /// The whole bot, about the middle of the base.
    hop: Tf,
    /// The monitor, about the bottom of its case.
    mon: Tf,
    /// How far the neck is stretched.
    neck: f64,
    /// The shadow's size and darkness.
    shadow: (f64, f64),
    /// The antenna light's size.
    light: f64,
    /// How open the eyes are, for blinks.
    open: f64,
}

/// Seconds between idle blinks.
const BLINK: f64 = 4.2;

fn rig(m: Move, t: f64) -> Rig {
    let mut r = Rig {
        hop: REST,
        mon: REST,
        neck: 1.0,
        shadow: (1.0, 0.5),
        light: 1.0,
        open: 1.0,
    };
    match m {
        Move::Idle => {
            r.mon = play_tf(
                t,
                3.2,
                &[(0.0, REST), (0.5, tf(-1.6, 1.0, 1.0, 0.0)), (1.0, REST)],
            );
            r.neck = play_f(t, 3.2, &[(0.0, 1.0), (0.5, 1.13), (1.0, 1.0)]);
            let ph = t.rem_euclid(BLINK) / BLINK;
            r.open = play_f(
                ph,
                1.0,
                &[(0.0, 1.0), (0.9, 1.0), (0.94, 0.1), (0.98, 1.0), (1.0, 1.0)],
            );
        }
        Move::Think => {
            r.mon = play_tf(
                t,
                3.0,
                &[
                    (0.0, REST),
                    (0.3, tf(0.0, 1.0, 1.0, -8.0)),
                    (0.7, tf(0.0, 1.0, 1.0, 6.0)),
                    (1.0, REST),
                ],
            );
        }
        Move::Work => {
            r.hop = play_tf(
                t,
                0.55,
                &[
                    (0.0, REST),
                    (0.3, tf(0.0, 1.05, 0.94, 0.0)),
                    (0.6, tf(-5.0, 0.97, 1.04, 0.0)),
                    (0.85, tf(0.0, 1.03, 0.97, 0.0)),
                    (1.0, REST),
                ],
            );
            r.shadow.0 = play_f(t, 0.55, &[(0.0, 1.0), (0.6, 0.8), (1.0, 1.0)]);
        }
        Move::Jump => {
            const P: f64 = 1.7;
            r.hop = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.12, tf(0.0, 1.12, 0.84, 0.0)),
                    (0.22, tf(-8.0, 0.9, 1.12, 0.0)),
                    (0.40, tf(-26.0, 1.0, 1.0, 0.0)),
                    (0.58, tf(-6.0, 0.95, 1.06, 0.0)),
                    (0.66, tf(0.0, 1.14, 0.84, 0.0)),
                    (0.76, tf(0.0, 0.97, 1.04, 0.0)),
                    (0.86, REST),
                    (1.0, REST),
                ],
            );
            r.mon = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.22, REST),
                    (0.34, tf(0.0, 1.0, 1.0, -7.0)),
                    (0.46, tf(0.0, 1.0, 1.0, 6.0)),
                    (0.58, REST),
                    (1.0, REST),
                ],
            );
            r.shadow.0 = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.12, 1.1),
                    (0.4, 0.5),
                    (0.66, 1.12),
                    (0.86, 1.0),
                    (1.0, 1.0),
                ],
            );
            r.shadow.1 = play_f(
                t,
                P,
                &[
                    (0.0, 0.5),
                    (0.4, 0.22),
                    (0.66, 0.55),
                    (0.86, 0.5),
                    (1.0, 0.5),
                ],
            );
        }
        Move::Pop => {
            const P: f64 = 1.6;
            r.mon = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.10, tf(-12.0, 1.0, 1.0, 0.0)),
                    (0.18, tf(-8.0, 1.0, 1.0, 0.0)),
                    (0.26, tf(-11.0, 1.0, 1.0, 0.0)),
                    (0.40, tf(-10.0, 1.0, 1.0, 0.0)),
                    (0.70, tf(-10.0, 1.0, 1.0, 0.0)),
                    (0.84, tf(1.0, 1.04, 0.96, 0.0)),
                    (0.92, REST),
                    (1.0, REST),
                ],
            );
            r.neck = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.10, 1.95),
                    (0.18, 1.62),
                    (0.26, 1.85),
                    (0.40, 1.77),
                    (0.70, 1.77),
                    (0.84, 0.95),
                    (0.92, 1.0),
                    (1.0, 1.0),
                ],
            );
            r.light = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.2, 1.35),
                    (0.4, 1.0),
                    (0.6, 1.35),
                    (0.8, 1.0),
                    (1.0, 1.0),
                ],
            );
        }
        Move::Shake => {
            const P: f64 = 2.2;
            r.mon = play_tf(
                t,
                P,
                &[
                    (0.0, tf(2.0, 1.0, 1.0, 0.0)),
                    (0.08, tf(0.0, 1.0, 1.0, -6.0)),
                    (0.16, tf(0.0, 1.0, 1.0, 6.0)),
                    (0.24, tf(0.0, 1.0, 1.0, -4.0)),
                    (0.32, tf(0.0, 1.0, 1.0, 3.0)),
                    (0.40, tf(2.0, 1.0, 1.0, 0.0)),
                    (1.0, tf(2.0, 1.0, 1.0, 0.0)),
                ],
            );
            r.neck = play_f(
                t,
                P,
                &[
                    (0.0, 0.86),
                    (0.08, 1.0),
                    (0.32, 1.0),
                    (0.4, 0.86),
                    (1.0, 0.86),
                ],
            );
        }
        Move::Sleep => {
            r.mon = play_tf(
                t,
                4.0,
                &[
                    (0.0, tf(3.0, 1.0, 1.0, 0.0)),
                    (0.5, tf(4.5, 1.03, 0.96, 0.0)),
                    (1.0, tf(3.0, 1.0, 1.0, 0.0)),
                ],
            );
            r.neck = play_f(t, 4.0, &[(0.0, 0.72), (0.5, 0.62), (1.0, 0.72)]);
            r.light = 1.0;
        }
        Move::Hello => {
            const P: f64 = 2.2;
            r.hop = play_tf(
                t,
                P,
                &[
                    (0.0, REST),
                    (0.10, tf(0.0, 1.1, 0.88, 0.0)),
                    (0.22, tf(-12.0, 0.94, 1.08, -4.0)),
                    (0.34, tf(0.0, 1.1, 0.88, 0.0)),
                    (0.44, REST),
                    (1.0, REST),
                ],
            );
            r.shadow.0 = play_f(
                t,
                P,
                &[
                    (0.0, 1.0),
                    (0.22, 0.7),
                    (0.34, 1.08),
                    (0.44, 1.0),
                    (1.0, 1.0),
                ],
            );
            r.light = play_f(t, P, &[(0.0, 1.0), (0.22, 1.35), (0.44, 1.0), (1.0, 1.0)]);
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Drawing, on a 100-unit grid: the base sits at y 98, the antenna top near 0.

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
        if a * self.alpha <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx.set_paint(self.color(c, a));
        self.ctx.fill_path(&shape.to_path(0.05));
    }

    fn stroke(&mut self, shape: &impl Shape, w: f64, c: Rgb, a: f64) {
        if a * self.alpha <= 0.004 {
            return;
        }
        self.ctx.set_transform(self.xf);
        self.ctx.reset_paint_transform();
        self.ctx
            .set_stroke(Stroke::new(w).with_caps(Cap::Round).with_join(Join::Round));
        self.ctx.set_paint(self.color(c, a));
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

fn rrect(x: f64, y: f64, w: f64, h: f64, r: f64) -> RoundedRect {
    RoundedRect::new(x, y, x + w, y + h, r)
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

fn eyes(p: &mut Pen, look: Look, col: Rgb, cy: f64, gaze: (f64, f64), open: f64) {
    let (w, h) = (9.0, 17.0);
    for (i, x0) in [40.0, 60.0].into_iter().enumerate() {
        let x = x0 + gaze.0;
        let y = cy + gaze.1;
        match look {
            Look::Happy => p.stroke(&arc(x, y, w * 0.8, true), w * 0.6, col, 1.0),
            Look::Closed => p.stroke(&arc(x, y + h * 0.15, w * 0.8, false), w * 0.55, col, 1.0),
            _ => {
                let (mut ww, mut hh, mut yy) = (w, h, y - h / 2.0);
                let mut rot = 0.0;
                match look {
                    Look::Up => yy -= h * 0.28,
                    Look::Narrow => {
                        hh = w * 0.72;
                        yy = y - hh / 2.0 + 2.0;
                    }
                    Look::Wide => {
                        hh = h * 1.14;
                        ww = w * 1.12;
                        yy = y - hh / 2.0;
                    }
                    Look::Tilted => {
                        hh = h * 0.74;
                        yy = y - hh / 2.0 + 1.5;
                        rot = if i == 0 { 22.0 } else { -22.0 };
                    }
                    _ => {}
                }
                // Blinks squash the pill about its middle.
                let mid = yy + hh / 2.0;
                let k = open.max(0.08);
                let shape = rrect(
                    x - ww / 2.0,
                    mid - hh * k / 2.0,
                    ww,
                    hh * k,
                    (ww / 2.0).min(hh * k / 2.0),
                );
                p.at(about((x, y), Affine::rotate(f64::to_radians(rot))), |p| {
                    p.fill(&shape, col, 1.0)
                });
            }
        }
    }
}

fn light(p: &mut Pen, shape: Light, col: Rgb) {
    p.fill(&Circle::new((50.0, 6.0), 6.4), STALK, 1.0);
    // The shapes are drawn about (50, 8.5) and lifted onto the ball.
    p.at(Affine::translate((0.0, -2.5)), |p| match shape {
        Light::Ball => p.fill(&Circle::new((50.0, 8.5), 3.6), col, 1.0),
        Light::Hex => p.fill(&svg("M50 4.3L53.7 6.4V10.6L50 12.7L46.3 10.6V6.4Z"), col, 1.0),
        Light::Plus => p.stroke(&svg("M50 4.6V12.4M46.1 8.5H53.9"), 2.6, col, 1.0),
        Light::Square => p.fill(&rrect(46.6, 5.1, 6.8, 6.8, 1.4), col, 1.0),
        Light::Diamond => p.fill(&svg("M50 4.2L54.3 8.5L50 12.8L45.7 8.5Z"), col, 1.0),
        Light::Ring => p.stroke(&Circle::new((50.0, 8.5), 3.0), 2.2, col, 1.0),
        Light::Star => p.fill(
            &svg("M50 4.2L51.3 7.2L54.4 7.4L52 9.4L52.8 12.5L50 10.8L47.2 12.5L48 9.4L45.6 7.4L48.7 7.2Z"),
            col,
            1.0,
        ),
        Light::Arrow => p.fill(&svg("M50 4.4L54 9.8H51.6V12.6H48.4V9.8H46Z"), col, 1.0),
    });
}

fn sign(p: &mut Pen, s: Sign, t: f64) {
    match s {
        Sign::None => {}
        Sign::Thought => {
            p.fill(&Circle::new((85.0, 13.0), 3.0), GREY, 1.0);
            p.fill(&Circle::new((94.0, 5.0), 4.2), GREY, 1.0);
        }
        Sign::Busy => {
            for i in 0..3 {
                let a = 0.4 + 0.6 * ((t * 3.0 - i as f64 * 0.33).rem_euclid(1.0));
                p.fill(&Circle::new((82.0 + i as f64 * 8.0, 7.0), 2.6), GREY, a);
            }
        }
        Sign::Ask => {
            let c = Circle::new((92.0, 12.0), 9.0);
            p.fill(&c, AMBER, 1.0);
            p.stroke(&c, 3.0, INK, 1.0);
        }
        Sign::Zzz => {
            p.stroke(&svg("M84 6H92L84 16H92"), 2.4, GREY, 1.0);
            p.stroke(&svg("M94 -2H99L94 4H99"), 1.8, GREY, 1.0);
        }
        Sign::Sweat => {
            let d = svg("M91 2C91 2 85 10 85 14A6 6 0 0 0 97 14C97 10 91 2 91 2Z");
            p.fill(&d, DROP, 1.0);
            p.stroke(&d, 2.4, INK, 1.0);
        }
    }
}

/// The monitor: antenna, case, screen and face.
/// What the face shows: its look, whether it's asleep and where it's looking.
#[derive(Clone, Copy)]
struct Face {
    look: Look,
    asleep: bool,
    gaze: (f64, f64),
}

fn monitor(p: &mut Pen, (col, shape): (Rgb, Light), face: &Face, r: &Rig, small: bool) {
    let Face { look, asleep, gaze } = *face;
    let (x, y, w, h, rad) = (12.0, 16.0, 76.0, 70.0, 24.0);
    p.fill(&rrect(48.0, 7.0, 4.0, 12.0, 2.0), STALK, 1.0);
    let lit = if asleep { OFF } else { col };
    p.at(about((50.0, 6.0), Affine::scale(1.3 * r.light)), |p| {
        light(p, shape, lit)
    });
    let case = rrect(x, y, w, h, rad);
    p.fill(&case, BODY, 1.0);
    p.stroke(
        &rrect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, rad - 1.0),
        2.0,
        EDGE,
        1.0,
    );
    let (sx, sy, sw, sh, sr) = (x + 9.0, y + 9.0, w - 18.0, 44.0, 16.0);
    p.fill(
        &rrect(sx - 1.5, sy - 1.5, sw + 3.0, sh + 3.0, sr + 1.5),
        SCREEN,
        1.0,
    );
    eyes(p, look, col, sy + sh / 2.0, gaze, r.open);
    if !small {
        let ly = y + h - 7.5;
        p.fill(
            &Circle::new((x + 15.0, ly), 2.6),
            col,
            if asleep { 0.3 } else { 0.95 },
        );
        p.fill(&rrect(x + 21.0, ly - 1.4, 14.0, 2.8, 1.4), WHITE, 0.18);
        for k in 0..3 {
            p.fill(
                &rrect(x + w - 29.0 + k as f64 * 6.0, ly - 3.5, 2.6, 7.0, 1.3),
                WHITE,
                0.16,
            );
        }
    }
}

/// The part of the grid a framing shows: `(x0, y0, side)`.
fn view(framing: Framing) -> (f64, f64, f64) {
    match framing {
        // Room above for the jump and beside the screen for the signs.
        Framing::Full => (-12.0, -26.0, 124.0),
        // Avatars: the bot itself, with a little room to hop.
        Framing::Bust => (-3.0, -6.0, 106.0),
    }
}

/// Draws Termi into `ctx` at time `t`, scaled so the framing fills `px` pixels.
fn draw_into(ctx: &mut RenderContext, pose: &FinPose, t: f64, framing: Framing, px: u16) {
    let (x0, y0, side) = view(framing);
    let k = px as f64 / side;
    let xf = Affine::scale(k) * Affine::translate((-x0, -y0));
    let t = t + pose.offset as f64 * OFFSET_STEP;
    let small = px < 36;
    let (col, shape) = kit(pose.fin.tint, pose.fin.ears);
    let (mv, look, sg) = act(pose.state);
    let mut r = rig(mv, t);
    if framing == Framing::Bust {
        // Avatars sit in tight spots: keep the hops small.
        r.hop.y *= 0.3;
        r.mon.y *= 0.5;
        r.neck = 1.0 + (r.neck - 1.0) * 0.5;
    }
    let gaze = (pose.gaze.0 as f64 * 0.35, pose.gaze.1 as f64 * 0.3);
    let mut p = Pen {
        ctx,
        xf,
        alpha: 1.0,
    };
    if framing == Framing::Full {
        let sh = Ellipse::new((50.0, 100.0), (27.0 * r.shadow.0, 3.2 * r.shadow.0), 0.0);
        p.fill(&sh, Rgb(0x000000), r.shadow.1);
    }
    let hop = place((50.0, 98.5), r.hop);
    p.at(hop, |p| {
        p.fill(&rrect(26.0, 92.0, 48.0, 6.5, 3.25), BASE, 1.0);
        p.at(
            about((50.0, 93.0), Affine::scale_non_uniform(1.0, r.neck)),
            |p| p.fill(&svg("M42 80L39.5 93H60.5L58 80Z"), NECK, 1.0),
        );
        p.at(place((50.0, 86.0), r.mon), |p| {
            monitor(
                p,
                (col, shape),
                &Face {
                    look,
                    asleep: mv == Move::Sleep,
                    gaze,
                },
                &r,
                small,
            )
        });
        if !small && (framing == Framing::Full || px >= 40) {
            sign(p, sg, t);
        }
    });
}

/// Draws Termi into premultiplied RGBA8, reusing buffers between frames.
pub struct TermiRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
}

impl TermiRenderer {
    pub fn new(px: u16) -> Self {
        TermiRenderer {
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

/// Busy enough to need a quick frame rate. Termi always moves a little.
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

    fn termi(tint: Tint, lead: bool) -> Fin {
        Fin {
            tint,
            line: Line::default(),
            ears: lead,
            outfit: Outfit::PLAIN,
        }
    }

    fn pose(state: State) -> FinPose {
        let mut p = termi(Tint::ORANGE, false).pose(Mood::Idle);
        p.state = state;
        p
    }

    #[test]
    fn every_state_draws_at_every_size() {
        for px in [22u16, 48, 96] {
            let mut r = TermiRenderer::new(px);
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
        let mut r = TermiRenderer::new(96);
        let p = pose(State::Done);
        let rest = r.draw(&p, 0.0, Framing::Full).to_vec();
        let top = r.draw(&p, 1.7 * 0.4, Framing::Full).to_vec();
        assert_ne!(rest, top);
        // At the top of the jump the floor under the base is clear.
        let row = |px: &[u8], y: usize| (0..96).filter(|x| px[(y * 96 + x) * 4 + 3] > 200).count();
        let floor = ((95.0 + 26.0) / 124.0 * 96.0) as usize;
        assert!(row(&rest, floor) > row(&top, floor));
    }

    #[test]
    fn same_time_same_frame() {
        let mut r = TermiRenderer::new(64);
        let p = pose(State::Work);
        let a = r.draw(&p, 2.5, Framing::Bust).to_vec();
        let b = r.draw(&p, 2.5, Framing::Bust).to_vec();
        assert_eq!(a, b);
    }

    #[test]
    fn each_crew_colour_has_its_own_light() {
        let mut seen = Vec::new();
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
        assert_eq!(kit(Tint::VIOLET, true), (LIME, Light::Ball));
    }

    #[test]
    fn moods_look_different() {
        let mut r = TermiRenderer::new(64);
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
