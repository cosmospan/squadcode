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
use std::time::Duration;

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
    static OPEN: RefCell<Option<WindowHandle<Notch>>> = const { RefCell::new(None) };
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
    let open = OPEN.with_borrow(|o| *o);
    match (enabled(cx), open) {
        (true, None) => {
            if let Some(handle) = open_notch(cx) {
                OPEN.set(Some(handle));
            }
        }
        (false, Some(handle)) => {
            OPEN.set(None);
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        (true, Some(handle)) => {
            // What it shows, and where: a display may have come or gone.
            let _ = handle.update(cx, |this, window, cx| {
                this.fit(window);
                cx.notify();
            });
        }
        (false, None) => {}
    }
}

fn open_notch(cx: &mut App) -> Option<WindowHandle<Notch>> {
    let geom = platform::geometry()?;
    let (w, h) = (geom.notch_w.max(PILL_W), geom.top_h);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(w), px(h)),
        ))),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        focus: false,
        show: true,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, _) =
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Notch::new(geom))).ok()?;
    let handle = handle.downcast::<Notch>()?;
    let _ = handle.update(cx, |this, window, cx| {
        gpui_kit::base::Root::update(window, cx, |root, _, cx| {
            root.style().background = Some(gpui_kit::transparent_black().into());
            cx.notify();
        });
        platform::setup(window);
        this.fit(window);
    });
    Some(handle)
}

struct Notch {
    shape: Shape,
    geom: Geom,
    /// Bots are working or something needs you, so the ears show at rest.
    busy: bool,
    /// The size it was last given, so it is only resized on a change.
    placed: Option<(f32, f32)>,
    /// Counts up on each hover change; a timer from an older one does nothing.
    hover_gen: u64,
}

impl Notch {
    fn new(geom: Geom) -> Self {
        Self {
            shape: Shape::Rest,
            geom,
            busy: false,
            placed: None,
            hover_gen: 0,
        }
    }

    /// The size for its shape and what it shows.
    fn size(&self) -> (f32, f32) {
        let g = self.geom;
        let base = if g.notched() { g.notch_w } else { PILL_W };
        match self.shape {
            Shape::Rest if self.busy => (base + 2. * EAR, g.top_h),
            Shape::Rest => (base, g.top_h),
            Shape::Glance => (GLANCE_W.max(base + 2. * EAR), g.top_h + 6.),
            Shape::Open => (OPEN_W, OPEN_H),
        }
    }

    /// Size and place the window for what it shows now, if that changed.
    fn fit(&mut self, window: &mut Window) {
        if let Some(g) = platform::geometry() {
            if g != self.geom {
                self.geom = g;
                self.placed = None;
            }
        }
        let want = self.size();
        if self.placed != Some(want) {
            self.placed = Some(want);
            platform::place(window, want.0, want.1);
        }
    }

    fn set_shape(&mut self, shape: Shape, window: &mut Window, cx: &mut Context<Self>) {
        if self.shape != shape {
            self.shape = shape;
            self.fit(window);
            cx.notify();
        }
    }

    /// The pointer came onto it or left it. Coming on, a short rest gives the
    /// glance and a longer one opens it; leaving folds it back after a moment,
    /// so brushing past on the way to the menu bar does nothing.
    fn hover(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hover_gen += 1;
        let gen = self.hover_gen;
        let steps: Vec<(u64, Shape)> = if on {
            match self.shape {
                Shape::Open => Vec::new(),
                _ => vec![(150, Shape::Glance), (450, Shape::Open)],
            }
        } else {
            vec![(400, Shape::Rest)]
        };
        if steps.is_empty() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            for (ms, shape) in steps {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                let still = this
                    .update_in(cx, |this, window, cx| {
                        if this.hover_gen != gen {
                            return false;
                        }
                        this.set_shape(shape, window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !still {
                    return;
                }
            }
        })
        .detach();
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let glance = crate::v2::tray::glance(cx, ROWS);
        let (needs, running) = glance.as_ref().map_or((0, 0), |g| (g.needs, g.running));
        let busy = needs + running > 0;
        self.busy = busy;
        // Resized after this frame: AppKit shouldn't move the window mid-draw.
        let (w, h) = self.size();
        if self.placed != Some((w, h)) {
            cx.defer_in(window, |this, window, _| this.fit(window));
        }
        let g = self.geom;
        let radius = if g.notched() { 12. } else { g.top_h / 2. };
        let open_radius = 22.;
        let top_h = g.top_h;
        let shape = self.shape;

        // The strip along the top: ears at rest, a line of words in the glance,
        // a heading when open.
        let strip = div()
            .h(px(if shape == Shape::Glance {
                top_h + 6.
            } else {
                top_h
            }))
            .w_full()
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(12.5))
            .text_color(c(DIM))
            .child(div().flex().items_center().gap(px(6.)).when(busy, |d| {
                d.child(ear_dot(if needs > 0 { AMBER } else { BLUE }))
            }))
            .when(shape == Shape::Glance, |d| {
                let words = glance
                    .as_ref()
                    .and_then(|g| g.rows.first())
                    .map(|(bot, title, note, _)| format!("{bot} · {title} · {note}"))
                    .or_else(|| glance.as_ref().map(|g| g.summary.clone()))
                    .unwrap_or_else(|| "Squad Code isn't open".into());
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(10.))
                        .text_center()
                        .truncate()
                        .child(words),
                )
            })
            .when(shape == Shape::Open, |d| {
                let summary = glance
                    .as_ref()
                    .map(|g| g.summary.clone())
                    .unwrap_or_else(|| "Squad Code isn't open".into());
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(10.))
                        .text_color(c(INK))
                        .truncate()
                        .child(format!("Squad · {summary}")),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .when(needs > 0, |d| d.child(badge(needs))),
            );

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
                                .max_w(px(200.))
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

        let r = if shape == Shape::Open {
            open_radius
        } else {
            radius
        };
        let island = div()
            .id("notch")
            .size_full()
            .bg(c(BLACK))
            .text_color(c(INK))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_bl(px(r))
            .rounded_br(px(r))
            .when(!g.notched(), |d| d.rounded_tl(px(r)).rounded_tr(px(r)))
            .on_hover(cx.listener(|this, on: &bool, window, cx| this.hover(*on, window, cx)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.hover_gen += 1;
                this.set_shape(Shape::Open, window, cx);
            }))
            .child(strip)
            .child(body);

        div().size(px(w)).max_w_full().child(island)
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

    pub(super) fn place(_: &gpui_kit::Window, _: f32, _: f32) {}
}

#[cfg(target_os = "macos")]
mod mac {
    //! The window over the notch, through AppKit. All on the main thread.

    use super::Geom;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
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

    /// Size it to `width` by `height`, hung from the top edge of its screen,
    /// centred under the notch, and show it without taking focus.
    pub(super) fn place(window: &gpui_kit::Window, width: f32, height: f32) {
        let (Some(w), Some(screen)) = (ns_window(window), screen()) else {
            return;
        };
        let frame = screen.frame();
        let (ww, hh) = (width as f64, height as f64);
        let x = frame.origin.x + (frame.size.width - ww) / 2.;
        let top = frame.origin.y + frame.size.height;
        let notched = screen.safeAreaInsets().top > 0.;
        // The pill hangs a little below the top edge; the notch shape is flush.
        let top = if notched { top } else { top - 2. };
        w.setFrame_display(
            NSRect::new(NSPoint::new(x, top - hh), NSSize::new(ww, hh)),
            true,
        );
        w.orderFrontRegardless();
    }
}
