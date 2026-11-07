//! The server's screen, live inside Squad: what its desktop shows (and so
//! whatever a bot opened there), with your mouse and keys going to it.
//!
//! The screen comes from [`super::machine::screen`] (a desktop on the server
//! and a tunnel to it) and is read by the VNC client in [`super::rfb`].

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::rfb::{self, State};
use super::ui::{self, c};
use super::{Place, Shell};

/// The fastest the picture is rebuilt: 30 times a second.
const FRAME: Duration = Duration::from_millis(33);

/// Where a point in the viewer lands on the server's screen, when it's on it.
/// The picture is fitted into `area` (w, h) keeping its shape, and centred.
pub fn to_screen(local: (f32, f32), area: (f32, f32), screen: (u32, u32)) -> Option<(u16, u16)> {
    let (sw, sh) = (screen.0 as f32, screen.1 as f32);
    if sw <= 0. || sh <= 0. || area.0 <= 0. || area.1 <= 0. {
        return None;
    }
    let k = (area.0 / sw).min(area.1 / sh);
    let (ox, oy) = ((area.0 - sw * k) / 2., (area.1 - sh * k) / 2.);
    let (x, y) = ((local.0 - ox) / k, (local.1 - oy) / k);
    (x >= 0. && y >= 0. && x < sw && y < sh).then_some((x as u16, y as u16))
}

impl Shell {
    /// Starts watching the screen that's been shared, and goes to it.
    pub(super) fn watch_screen(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.server_screen else {
            return;
        };
        let alive = self
            .screen_view
            .as_ref()
            .is_some_and(|v| !matches!(v.state(), State::Closed(_)));
        if !alive {
            self.screen_view = Some(rfb::Viewer::connect(s.tunnel.port(), s.pass.clone()));
            self.screen_seen = u64::MAX;
        }
        self.go(Place::Screen, cx);
    }

    fn screen_point(&self, pos: Point<Pixels>) -> Option<(u16, u16)> {
        let b = self.screen_bounds.get();
        let f = self.screen_view.as_ref()?.frame.lock().ok()?;
        to_screen(
            (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y)),
            (f32::from(b.size.width), f32::from(b.size.height)),
            (f.w, f.h),
        )
    }

    fn screen_mouse(&mut self, pos: Point<Pixels>, button: Option<(u8, bool)>) {
        let Some((x, y)) = self.screen_point(pos) else {
            return;
        };
        if let Some(v) = &mut self.screen_view {
            v.pointer(x, y, button);
        }
    }

    fn screen_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(v) = &self.screen_view else {
            return;
        };
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        // ⌘ works as Ctrl there, the way a Mac person expects copy and paste
        // to; ⌘V first puts this computer's clipboard on the server.
        let ctrl = m.control || m.platform;
        if m.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                v.clipboard(&text);
            }
        }
        let Some(sym) = rfb::keysym(&ks.key, ks.key_char.as_deref()) else {
            return;
        };
        // A typed character already says whether Shift was down.
        let printable = sym < 0xff00;
        let mods: Vec<u32> = [
            (ctrl, rfb::CONTROL),
            (m.alt, rfb::ALT),
            (m.shift && !printable, rfb::SHIFT),
        ]
        .into_iter()
        .filter_map(|(on, k)| on.then_some(k))
        .collect();
        for k in &mods {
            v.key(*k, true);
        }
        v.key(sym, true);
        v.key(sym, false);
        for k in mods.iter().rev() {
            v.key(*k, false);
        }
        cx.stop_propagation();
    }

    /// Turns the latest frame into a picture when it changed.
    fn screen_picture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for old in self.screen_retired.drain(..) {
            cx.drop_image(old, Some(window));
        }
        let Some(v) = &self.screen_view else {
            return;
        };
        let version = v.version.load(Ordering::Relaxed);
        if version == self.screen_seen || self.screen_built.elapsed() < FRAME {
            return;
        }
        let Ok(f) = v.frame.lock() else {
            return;
        };
        if f.w == 0 || f.h == 0 {
            return;
        }
        let Some(buf) = image::RgbaImage::from_raw(f.w, f.h, f.px.clone()) else {
            return;
        };
        drop(f);
        let pic = Arc::new(RenderImage::new(vec![image::Frame::new(buf)]));
        if let Some(old) = self.screen_img.replace(pic) {
            self.screen_retired.push(old);
        }
        self.screen_seen = version;
        self.screen_built = Instant::now();
    }

    pub(super) fn screen_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.screen_picture(window, cx);
        let label = self
            .crew
            .link
            .as_ref()
            .map(|l| l.cfg.label())
            .unwrap_or_else(|| "Your server".into());
        let state = self
            .screen_view
            .as_ref()
            .map(|v| v.state())
            .unwrap_or(State::Closed("Not connected.".into()));
        let (dot, line) = match &state {
            State::Connecting => (ui::t_amber(), "Connecting…".to_string()),
            State::Live => (
                ui::t_green(),
                "Live. Click into it to use the mouse and keyboard there.".to_string(),
            ),
            State::Closed(why) => (ui::t_red(), why.clone()),
        };
        let closed = matches!(state, State::Closed(_));
        let url = self.server_screen.as_ref().map(|s| s.url());
        let bar = div()
            .flex_none()
            .h(px(48.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                ui::ghost_i("screen-back", IconName::ArrowLeft, "Your server")
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Server, cx))),
            )
            .child(div().size(px(8.)).flex_none().rounded_full().bg(c(dot)))
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(format!("{label}'s screen")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(c(if closed { ui::t_red() } else { ui::muted() }))
                    .child(line),
            )
            .when(closed, |d| {
                d.child(
                    ui::ghost_i("screen-again", IconName::RotateCcw, "Connect again").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.screen_view = None;
                            this.open_server_screen(cx);
                        }),
                    ),
                )
            })
            .when_some(url.filter(|_| cfg!(target_os = "macos")), |d, url| {
                d.child(
                    ui::ghost_i(
                        "screen-sharing",
                        IconName::ExternalLink,
                        "Open in Screen Sharing",
                    )
                    .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            });
        let bounds = self.screen_bounds.clone();
        let mouse_down = |b: u8| {
            cx.listener(move |this: &mut Self, ev: &MouseDownEvent, window, cx| {
                window.focus(&this.screen_focus, cx);
                this.screen_mouse(ev.position, Some((b, true)));
                cx.stop_propagation();
            })
        };
        let mouse_up = |b: u8| {
            cx.listener(move |this: &mut Self, ev: &MouseUpEvent, _, _| {
                this.screen_mouse(ev.position, Some((b, false)));
            })
        };
        let mut area = div()
            .id("screen-area")
            .track_focus(&self.screen_focus)
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(c(0x0B0B10))
            .cursor_default()
            .on_mouse_down(MouseButton::Left, mouse_down(1))
            .on_mouse_down(MouseButton::Middle, mouse_down(2))
            .on_mouse_down(MouseButton::Right, mouse_down(3))
            .on_mouse_up(MouseButton::Left, mouse_up(1))
            .on_mouse_up(MouseButton::Middle, mouse_up(2))
            .on_mouse_up(MouseButton::Right, mouse_up(3))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, _| {
                this.screen_mouse(ev.position, None);
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = f32::from(ev.delta.pixel_delta(px(16.)).y);
                if let (Some((x, y)), true) = (this.screen_point(ev.position), d.abs() > 0.5) {
                    if let Some(v) = &mut this.screen_view {
                        v.wheel(x, y, d > 0.);
                    }
                }
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.screen_key(ev, cx)))
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            );
        area = match &self.screen_img {
            Some(pic) => area.child(
                img(ImageSource::Render(pic.clone()))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            ),
            None => area.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(c(0x9A9AA8))
                    .child(if closed {
                        "No picture: the screen isn't connected."
                    } else {
                        "Waiting for the first picture…"
                    }),
            ),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .child(area)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::to_screen;

    #[test]
    fn points_land_on_the_fitted_screen() {
        // A 1600×1000 screen in a 800×800 area: half size, 150 px bars above and below.
        assert_eq!(
            to_screen((0., 150.), (800., 800.), (1600, 1000)),
            Some((0, 0))
        );
        assert_eq!(
            to_screen((400., 400.), (800., 800.), (1600, 1000)),
            Some((800, 500))
        );
        assert_eq!(to_screen((400., 100.), (800., 800.), (1600, 1000)), None);
        assert_eq!(to_screen((10., 10.), (0., 0.), (1600, 1000)), None);
    }
}
