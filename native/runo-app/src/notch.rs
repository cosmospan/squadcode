//! Squad in the MacBook notch, apart from the menu bar glyph.
//!
//! A black shape sits in the notch. At rest it is the notch's own size, so it
//! can't be seen, and while bots work it grows two small ears: a dot on the
//! left, the count of what needs you on the right. Resting the pointer on it
//! widens it into a one-line glance, and keeping it there drops it open with
//! the jobs that need you and the ones running. It folds back shortly after the
//! pointer leaves.
//!
//! On a screen without a notch it is a small pill at the top centre.
//!
//! This is the first cut, to prove the window behaves (above the menu bar, on
//! every Space and beside full screen apps, hover without taking focus). It is
//! off unless Settings has `notch` on or SQUAD_NOTCH is set.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::settings::AppSettings;

/// The ears either side of the notch, each this wide.
const EAR: f32 = 36.;
/// The glance's width.
const GLANCE_W: f32 = 420.;
/// The open panel's size.
const OPEN_W: f32 = 520.;
const OPEN_H: f32 = 300.;
/// Jobs listed when open.
const ROWS: usize = 4;
/// The pill on a screen without a notch.
const PILL_W: f32 = 150.;
/// The line of words under the notch, in the glance and atop the open panel.
const LINE_H: f32 = 26.;
/// The window: one size, big enough for the open panel.
const WIN_W: f32 = OPEN_W;
const WIN_H: f32 = OPEN_H;

const BLACK: u32 = 0x000000;
const INK: u32 = 0xF3F4F7;
const DIM: u32 = 0x9AA1B2;
const AMBER: u32 = 0xF5B54A;
const BLUE: u32 = 0x6AA8FF;

/// A solid colour from `0xRRGGBB`.
fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// A colour from `0xRRGGBB` at opacity `a`.
fn ca(hex: u32, a: f32) -> Hsla {
    let mut h = c(hex);
    h.a = a;
    h
}

thread_local! {
    /// The notch's window and its view, once open. gpui_kit wraps the view in
    /// its own root, so the window can't be read as a `Notch` directly.
    static OPEN: RefCell<Option<(AnyWindowHandle, Entity<Notch>)>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shape {
    Rest,
    Glance,
    Open,
}

/// The notch as the screen has it: its width (0 for a screen without one) and
/// the height of the strip along the top it sits in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Geom {
    pub notch_w: f32,
    pub top_h: f32,
}

impl Geom {
    fn notched(self) -> bool {
        self.notch_w > 0.
    }
}

fn enabled(cx: &App) -> bool {
    platform::SUPPORTED
        && (cx.global::<AppSettings>().notch || std::env::var_os("SQUAD_NOTCH").is_some())
}

/// Start the notch, once, as the app starts. It comes and goes with the
/// setting, looked at every couple of seconds.
pub(crate) fn install(cx: &mut App) {
    if !platform::SUPPORTED {
        return;
    }
    tick(cx);
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        cx.update(tick);
    })
    .detach();
}

fn tick(cx: &mut App) {
    let open = OPEN.with_borrow(|o| o.clone());
    match (enabled(cx), open) {
        (true, None) => {
            if let Some(handle) = open_notch(cx) {
                OPEN.set(Some(handle));
            }
        }
        (false, Some((handle, _))) => {
            OPEN.set(None);
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        (true, Some((handle, view))) => {
            // A display may have come or gone, or the notch moved screens.
            let gone = handle
                .update(cx, |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.fit(window, cx);
                        cx.notify();
                    })
                })
                .is_err();
            if gone {
                OPEN.set(None);
            }
        }
        (false, None) => {}
    }
}

fn open_notch(cx: &mut App) -> Option<(AnyWindowHandle, Entity<Notch>)> {
    let geom = platform::geometry()?;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(WIN_W), px(WIN_H)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        focus: false,
        show: true,
        window_background: WindowBackgroundAppearance::Transparent,
        // Opened on the notch's screen, so it starts at that screen's scale.
        display_id: platform::display().map(|id| DisplayId::new(id as u64)),
        // gpui caps a window without focus to about 30 frames a second; the
        // notch never has focus, and that cap made its animation stutter. It
        // only draws while it moves or changes, so there is nothing to save.
        inactive_frame_interval: None,
        ..Default::default()
    };
    let (handle, view) =
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Notch::new(geom))).ok()?;
    let _ = handle.update(cx, |_, window, cx| {
        gpui_kit::base::Root::update(window, cx, |root, _, cx| {
            root.style().background = Some(gpui_kit::transparent_black().into());
            cx.notify();
        });
        platform::setup(window);
        view.update(cx, |this, cx| {
            this.fit(window, cx);
            this.watch(window, cx);
        });
    });
    Some((handle, view))
}

fn debug(what: impl FnOnce() -> String) {
    if std::env::var_os("SQUAD_NOTCH_DEBUG").is_some() {
        eprintln!("notch: {}", what());
    }
}

/// The black shape's width, height and corner radius.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Frame {
    w: f32,
    h: f32,
    r: f32,
}

impl Frame {
    fn lerp(a: Frame, b: Frame, t: f32) -> Frame {
        let m = |x: f32, y: f32| x + (y - x) * t;
        Frame {
            w: m(a.w, b.w),
            h: m(a.h, b.h),
            r: m(a.r, b.r).max(0.),
        }
    }

    fn area(self) -> f32 {
        self.w * self.h
    }
}

/// A move of the shape from one frame to another.
#[derive(Clone, Copy, Debug)]
struct Anim {
    from: Frame,
    to: Frame,
    start: Instant,
    /// Growing: it springs a touch past and settles, and the content fades
    /// in once there is room. Shrinking: a quicker, plain ease.
    growing: bool,
    /// Frames drawn during it, for the debug log.
    frames: u32,
}

impl Anim {
    fn length(&self) -> Duration {
        Duration::from_millis(if self.growing { 420 } else { 300 })
    }

    /// How far along, 0 to 1, by time.
    fn t(&self, now: Instant) -> f32 {
        (now - self.start).as_secs_f32() / self.length().as_secs_f32()
    }

    /// The eased progress: a soft spring when growing, ease-out when not.
    fn eased(&self, t: f32) -> f32 {
        let t = t.clamp(0., 1.);
        if t >= 1. {
            1.
        } else if self.growing {
            // A damped spring (damping 0.75): quick at first, about 1.5% past
            // the end, settled well before the time is up.
            let (zeta, omega) = (0.82_f32, 11_f32);
            let wd = omega * (1. - zeta * zeta).sqrt();
            1. - (-zeta * omega * t).exp() * ((wd * t).cos() + zeta * omega / wd * (wd * t).sin())
        } else {
            1. - (1. - t).powi(3)
        }
    }
}

struct Notch {
    shape: Shape,
    geom: Geom,
    /// Bots are working or something needs you, so the ears show at rest.
    busy: bool,
    /// The geometry the window was last placed for.
    placed: Option<Geom>,
    /// When the pointer came onto the shape, and when it left it.
    over_since: Option<Instant>,
    left_at: Option<Instant>,
    /// Frames drawn, for the debug log.
    frames: u64,
    /// The shape as drawn last frame, and the move under way to the next.
    drawn: Frame,
    anim: Option<Anim>,
    /// What the crew is doing, read once a second rather than while
    /// drawing: reading it in render tied this window's redraws to every
    /// change in the main window.
    glance: Option<crate::v2::tray::Glance>,
    glance_at: Option<Instant>,
}

impl Notch {
    fn new(geom: Geom) -> Self {
        Self {
            shape: Shape::Rest,
            geom,
            busy: false,
            placed: None,
            over_since: None,
            left_at: None,
            frames: 0,
            drawn: Frame::default(),
            anim: None,
            glance: None,
            glance_at: None,
        }
    }

    /// The black shape's size for its state and what it shows.
    fn size(&self) -> (f32, f32) {
        let g = self.geom;
        let base = if g.notched() { g.notch_w } else { PILL_W };
        match self.shape {
            Shape::Rest if self.busy => (base + 2. * EAR, g.top_h),
            Shape::Rest => (base, g.top_h),
            Shape::Glance => (GLANCE_W.max(base + 2. * EAR), g.top_h + LINE_H),
            Shape::Open => (OPEN_W, OPEN_H),
        }
    }

    /// Place the window over the notch, if the screen changed since.
    ///
    /// The window keeps one size, big enough for the open panel, and the
    /// shape grows inside it: gpui missed resizes made from AppKit. It lets
    /// clicks through to the menu bar except while open.
    fn fit(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(g) = platform::geometry() {
            self.geom = g;
        }
        let open = self.shape == Shape::Open;
        let geom = self.geom;
        let native = platform::native(window);
        let replace = self.placed != Some(geom);
        self.placed = Some(geom);
        cx.defer(move |_| platform::place(native, WIN_W, WIN_H, replace, !open));
    }

    fn set_shape(&mut self, shape: Shape, window: &mut Window, cx: &mut Context<Self>) {
        if self.shape != shape {
            debug(|| format!("{:?} -> {shape:?}", self.shape));
            self.shape = shape;
            self.fit(window, cx);
            cx.notify();
        }
    }

    /// Watch the pointer, about twenty times a second. A short rest on the
    /// shape gives the glance and a longer one opens it; leaving folds it
    /// back after a moment, so brushing past on the way to the menu bar does
    /// nothing. Read from AppKit rather than gpui's hover, so it works while
    /// another app has focus and the window lets clicks through.
    fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(30))
                .await;
            let alive = this
                .update_in(cx, |this, window, cx| this.look(window, cx))
                .is_ok();
            if !alive {
                return;
            }
        })
        .detach();
    }

    fn look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .glance_at
            .is_none_or(|at| now - at >= Duration::from_secs(1))
        {
            self.glance_at = Some(now);
            let fresh = crate::v2::tray::glance(cx, ROWS);
            if fresh != self.glance {
                self.glance = fresh;
                cx.notify();
            }
        }
        let (w, h) = self.size();
        let over = platform::pointer_over(w, h);
        let now = Instant::now();
        if over {
            self.left_at = None;
            let since = *self.over_since.get_or_insert(now);
            let rested = now - since;
            if rested >= Duration::from_millis(600) {
                self.set_shape(Shape::Open, window, cx);
            } else if rested >= Duration::from_millis(150) && self.shape == Shape::Rest {
                self.set_shape(Shape::Glance, window, cx);
            }
        } else {
            self.over_since = None;
            if self.shape != Shape::Rest {
                let left = *self.left_at.get_or_insert(now);
                if now - left >= Duration::from_millis(400) {
                    self.set_shape(Shape::Rest, window, cx);
                }
            }
        }
    }
}

fn ear_dot(color: u32) -> Div {
    div().size(px(8.)).rounded_full().bg(c(color))
}

fn badge(n: usize) -> Div {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .rounded_full()
        .bg(c(AMBER))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(0x1A1200))
        .child(n.to_string())
}

impl Render for Notch {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let glance = self.glance.clone();
        let (needs, running) = glance.as_ref().map_or((0, 0), |g| (g.needs, g.running));
        let busy = needs + running > 0;
        self.busy = busy;
        let (w, h) = self.size();
        let g = self.geom;
        let to = Frame {
            w,
            h,
            r: match self.shape {
                Shape::Open => 22.,
                _ if g.notched() => 12.,
                _ => g.top_h / 2.,
            },
        };
        // Start a move whenever the shape's target changes; the first frame
        // just takes it.
        let now = Instant::now();
        if self.drawn == Frame::default() {
            self.drawn = to;
        } else if self.anim.map_or(self.drawn, |a| a.to) != to {
            self.anim = Some(Anim {
                from: self.drawn,
                to,
                start: now,
                growing: to.area() > self.drawn.area(),
                frames: 0,
            });
        }
        // Where it is now, and how visible the content is: it fades in once
        // the shape has mostly opened, so text never squeezes as it grows.
        let mut fade = 1.;
        if let Some(mut anim) = self.anim {
            let t = anim.t(now);
            anim.frames += 1;
            self.drawn = Frame::lerp(anim.from, anim.to, anim.eased(t));
            fade = if anim.growing {
                ((t - 0.3) / 0.5).clamp(0., 1.)
            } else {
                ((t - 0.5) / 0.5).clamp(0., 1.)
            };
            if t >= 1. {
                self.drawn = anim.to;
                self.anim = None;
                let ms = anim.length().as_millis();
                debug(|| format!("moved in {} frames over {ms} ms", anim.frames));
            } else {
                self.anim = Some(anim);
                window.request_animation_frame();
            }
        }
        let shown = self.drawn;
        self.frames += 1;
        if self.frames <= 3 || self.frames % 200 == 0 {
            let vp = window.viewport_size();
            debug(|| {
                format!(
                    "frame {} {:?} shape {w}x{h} viewport {}x{} scale {}",
                    self.frames,
                    self.shape,
                    vp.width,
                    vp.height,
                    window.scale_factor()
                )
            });
        }
        let top_h = g.top_h;
        let shape = self.shape;

        // The strip along the top: ears at rest, a line of words in the glance,
        // a heading when open.
        // The strip along the top is the notch's own height, with the dot on
        // the left and the badge on the right, and nothing between: on a
        // notched screen the middle is behind the hardware. Words go below it.
        let strip = div()
            .h(px(top_h))
            .w_full()
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .child(div().flex().items_center().gap(px(6.)).when(busy, |d| {
                d.child(ear_dot(if needs > 0 { AMBER } else { BLUE }))
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .when(needs > 0, |d| d.child(badge(needs))),
            );
        let words = match shape {
            Shape::Rest => None,
            Shape::Glance => Some(
                glance
                    .as_ref()
                    .and_then(|g| g.rows.first())
                    .map(|(bot, title, note, _)| format!("{bot} · {title} · {note}"))
                    .or_else(|| glance.as_ref().map(|g| g.summary.clone()))
                    .unwrap_or_else(|| "Squad Code isn't open".into()),
            ),
            Shape::Open => Some(format!(
                "Squad · {}",
                glance
                    .as_ref()
                    .map(|g| g.summary.clone())
                    .unwrap_or_else(|| "Squad Code isn't open".into())
            )),
        };
        let line = words.map(|words| {
            div()
                .h(px(LINE_H))
                .w_full()
                .flex_none()
                .px(px(16.))
                .flex()
                .items_center()
                .when(shape == Shape::Glance, |d| d.justify_center())
                .text_size(px(12.5))
                .text_color(c(if shape == Shape::Open { INK } else { DIM }))
                .child(div().min_w_0().truncate().child(words))
        });

        let mut body = div()
            .flex_1()
            .min_h_0()
            .px(px(16.))
            .pb(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.));
        if shape == Shape::Open {
            let rows = glance.as_ref().map(|g| g.rows.clone()).unwrap_or_default();
            if rows.is_empty() {
                body = body.child(
                    div()
                        .text_size(px(13.))
                        .text_color(c(DIM))
                        .child("Nothing running. Hand the crew a job from Squad Code."),
                );
            }
            for (i, (bot, title, note, need)) in rows.into_iter().enumerate() {
                body = body.child(
                    div()
                        .id(("notch-row", i))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(12.))
                        .when(need, |d| {
                            d.bg(ca(AMBER, 0.09))
                                .border_1()
                                .border_color(ca(AMBER, 0.35))
                        })
                        .when(!need, |d| d.bg(ca(INK, 0.05)))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(13.))
                        .child(ear_dot(if need { AMBER } else { BLUE }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(c(INK))
                                .child(format!("{bot} · {title}")),
                        )
                        .child(
                            div()
                                .max_w(px(170.))
                                .truncate()
                                .text_color(c(DIM))
                                .child(note),
                        )
                        .cursor_pointer()
                        .on_click(|_, _, cx| crate::v2::tray::open_today(cx)),
                );
            }
            body = body.child(div().flex_1()).child(
                div()
                    .id("notch-open-app")
                    .self_end()
                    .px(px(12.))
                    .py(px(5.))
                    .rounded_full()
                    .bg(c(INK))
                    .text_color(c(BLACK))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .child("Open Squad Code")
                    .on_click(|_, _, cx| crate::v2::tray::open_today(cx)),
            );
        }

        // The content is laid out at the shape's final size and the black
        // shape reveals it as it grows, so nothing reflows mid-move.
        let content = div()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .flex()
            .flex_col()
            .child(strip)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .opacity(fade)
                    .children(line)
                    .child(body),
            );
        let r = shown.r;
        let island = div()
            .id("notch")
            .w(px(shown.w.min(WIN_W)))
            .h(px(shown.h.min(WIN_H)))
            .flex_none()
            .bg(c(BLACK))
            .text_color(c(INK))
            .flex()
            .flex_col()
            .items_center()
            .overflow_hidden()
            .rounded_bl(px(r))
            .rounded_br(px(r))
            .when(!g.notched(), |d| d.rounded_tl(px(r)).rounded_tr(px(r)))
            .child(content);

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .child(island)
    }
}

#[cfg(target_os = "macos")]
use mac as platform;

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Geom;

    pub(super) const SUPPORTED: bool = false;

    pub(super) fn geometry() -> Option<Geom> {
        None
    }

    pub(super) fn setup(_: &gpui_kit::Window) {}

    pub(super) fn display() -> Option<u32> {
        None
    }

    pub(super) type Native = ();

    pub(super) fn native(_: &gpui_kit::Window) -> Native {}

    pub(super) fn place(_: Native, _: f32, _: f32, _: bool, _: bool) {}

    pub(super) fn pointer_over(_: f32, _: f32) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod mac {
    //! The window over the notch, through AppKit. All on the main thread.

    use super::Geom;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSEvent, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub(super) const SUPPORTED: bool = true;

    /// Over the menu bar (24) and its status items, under menus (101), so a
    /// menu opened beside the notch still draws on top.
    const LEVEL: isize = 25;

    /// The screen it shows on: the one with a notch, else the main one.
    fn screen() -> Option<Retained<NSScreen>> {
        let mtm = MainThreadMarker::new()?;
        let screens = NSScreen::screens(mtm);
        screens
            .iter()
            .find(|s| s.safeAreaInsets().top > 0.)
            .or_else(|| NSScreen::mainScreen(mtm))
    }

    /// The screen's number, as gpui names displays.
    pub(super) fn display() -> Option<u32> {
        screen()?
            .deviceDescription()
            .objectForKey(&NSString::from_str("NSScreenNumber"))
            .and_then(|n| n.downcast::<objc2_foundation::NSNumber>().ok())
            .map(|n| n.unsignedIntValue())
    }

    pub(super) fn geometry() -> Option<Geom> {
        let screen = screen()?;
        let frame = screen.frame();
        let inset = screen.safeAreaInsets().top;
        if inset > 0. {
            let left = screen.auxiliaryTopLeftArea();
            let right = screen.auxiliaryTopRightArea();
            let notch = frame.size.width - left.size.width - right.size.width;
            if notch > 0. {
                return Some(Geom {
                    notch_w: notch as f32,
                    top_h: inset as f32,
                });
            }
        }
        // No notch: as tall as the menu bar, or a usual one if it hides.
        let visible = screen.visibleFrame();
        let bar = frame.origin.y + frame.size.height - (visible.origin.y + visible.size.height);
        Some(Geom {
            notch_w: 0.,
            top_h: if bar >= 20. { (bar - 4.) as f32 } else { 22. },
        })
    }

    fn ns_window(window: &gpui_kit::Window) -> Option<Retained<NSWindow>> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: gpui hands out its own content view, alive while the window is.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// Once, after it opens: no title bar to keep it under the menu bar, above
    /// the menu bar itself, on every Space and beside full screen apps, out of
    /// ⌘` and Mission Control, and no shadow.
    pub(super) fn setup(window: &gpui_kit::Window) {
        let Some(w) = ns_window(window) else {
            return;
        };
        w.setStyleMask(
            NSWindowStyleMask::Borderless
                | NSWindowStyleMask::FullSizeContentView
                | NSWindowStyleMask::NonactivatingPanel,
        );
        w.setLevel(LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        w.setHasShadow(false);
    }

    /// The pointer is over a `width` by `height` shape at the notch.
    pub(super) fn pointer_over(width: f32, height: f32) -> bool {
        let Some(screen) = screen() else {
            return false;
        };
        let r = hung(&screen, width as f64, height as f64);
        let p = NSEvent::mouseLocation();
        // The top row of points counts, where the pointer stops at the edge.
        p.x >= r.origin.x
            && p.x <= r.origin.x + r.size.width
            && p.y >= r.origin.y
            && p.y <= r.origin.y + r.size.height + 1.
    }

    /// The AppKit window, to place once gpui is done with it.
    pub(super) type Native = Option<Retained<NSWindow>>;

    pub(super) fn native(window: &gpui_kit::Window) -> Native {
        ns_window(window)
    }

    /// The rect on screen, in AppKit's points from the bottom left, of a
    /// `width` by `height` shape hung from the top edge, centred on the notch.
    fn hung(screen: &NSScreen, width: f64, height: f64) -> NSRect {
        let frame = screen.frame();
        let notched = screen.safeAreaInsets().top > 0.;
        // The pill hangs a little below the top edge; the notch shape is flush.
        let top = frame.origin.y + frame.size.height - if notched { 0. } else { 2. };
        let x = frame.origin.x + (frame.size.width - width) / 2.;
        NSRect::new(NSPoint::new(x, top - height), NSSize::new(width, height))
    }

    /// Size it to `width` by `height` over the notch when `replace`, let clicks
    /// through or not, and show it without taking focus.
    pub(super) fn place(window: Native, width: f32, height: f32, replace: bool, through: bool) {
        let (Some(w), Some(screen)) = (window, screen()) else {
            return;
        };
        if replace {
            w.setFrame_display(hung(&screen, width as f64, height as f64), true);
        }
        w.setIgnoresMouseEvents(through);
        w.orderFrontRegardless();
    }
}
