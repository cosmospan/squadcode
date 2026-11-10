//! A job's browser, live inside Squad: the page its bot opens and drives (with
//! Squad's browser tools), streamed in as it changes, with your clicks, scrolling and
//! keys going to it. You can watch the bot test, step in, or open a page yourself.
//!
//! A job on this computer has its browser here; a job on your server has it on
//! the server, reached through an SSH tunnel, so nothing needs a desktop there.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::browse::host;
use crate::browse::view::{self, State, Viewer};
use crate::browser_engine::{key_spec, modifier_bits, KeySpec};

use super::data::{Member, Mode};
use super::remote;
use super::runtime;
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// The fastest the picture is rebuilt: 60 times a second.
const FRAME: Duration = Duration::from_millis(16);

/// A browser reached and ready to watch.
struct Opened {
    port: u16,
    path: String,
    tunnel: Option<remote::Forward>,
}

/// A browser being reached, and the page to open once it is.
type Opening = (Receiver<Result<Opened, String>>, Option<String>);

/// The Browser page's state.
pub(super) struct Web {
    /// The job whose browser this is.
    pub job: u64,
    view: Option<Viewer>,
    opening: Option<Opening>,
    tunnel: Option<remote::Forward>,
    img: Option<Arc<RenderImage>>,
    seen: u64,
    built: Instant,
    /// When the picture last changed, so it's redrawn every frame while it moves.
    moved: Instant,
    retired: Vec<Arc<RenderImage>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    address: Entity<InputState>,
    /// The address shown, so it's only replaced when the page goes somewhere new.
    shown: String,
    err: String,
    clicks: (Instant, usize),
    touched: Instant,
    /// Point at it: the next click on the page picks what's there instead of
    /// clicking it.
    pointing: bool,
    picking: Option<Receiver<Result<view::Picked, String>>>,
    /// What was picked, for the bar asking what should change about it.
    picked: Option<view::Picked>,
}

impl Web {
    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> (Web, Subscription) {
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("localhost:3000, or any address"));
        let sub = cx.subscribe_in(
            &address,
            window,
            |this: &mut Shell, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().trim().to_string();
                    if let Some(url) = crate::browse::bot::url_of(&text) {
                        let job = this.web.job;
                        this.open_job_browser(job, Some(url), cx);
                        window.focus(&this.web.focus, cx);
                    }
                }
            },
        );
        (
            Web {
                job: 0,
                view: None,
                opening: None,
                tunnel: None,
                img: None,
                seen: u64::MAX,
                built: Instant::now(),
                moved: Instant::now(),
                retired: Vec::new(),
                bounds: Rc::default(),
                focus: cx.focus_handle(),
                address,
                shown: String::new(),
                err: String::new(),
                clicks: (Instant::now(), 0),
                touched: Instant::now(),
                pointing: false,
                picking: None,
                picked: None,
            },
            sub,
        )
    }

    fn stop(&mut self) {
        self.pointing = false;
        self.picking = None;
        self.picked = None;
        self.view = None;
        self.tunnel = None;
        self.opening = None;
        self.seen = u64::MAX;
        if let Some(old) = self.img.take() {
            self.retired.push(old);
        }
    }

    fn live(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|v| !matches!(v.state(), State::Closed(_)))
    }
}

/// The `port=` and `path=` lines `squad v2 browser port` prints.
pub fn parse_endpoint(out: &str) -> Option<(u16, String)> {
    let mut port = None;
    let mut path = None;
    for line in out.lines() {
        if let Some(p) = line.trim().strip_prefix("port=") {
            port = p.parse().ok();
        } else if let Some(p) = line.trim().strip_prefix("path=") {
            path = Some(p.to_string());
        }
    }
    Some((port?, path?))
}

impl Shell {
    /// Shows job `id`'s browser, starting it when it isn't running, and opens
    /// `url` there when given.
    pub(super) fn open_job_browser(
        &mut self,
        id: u64,
        url: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = self.crew.store.job(id).cloned() else {
            return;
        };
        if self.web.job != id {
            self.web.stop();
            self.web.job = id;
            self.web.shown.clear();
        }
        self.web.err.clear();
        if self.web.live() {
            if let (Some(v), Some(url)) = (&self.web.view, &url) {
                v.navigate(url);
            }
            self.go(Place::Browser(id), cx);
            return;
        }
        self.web.stop();
        let (tx, rx) = channel();
        if job.remote_on.is_empty() {
            let dir = job.dir().to_string();
            std::thread::spawn(move || {
                let _ = tx.send(host::ensure(&dir).map(|e| Opened {
                    port: e.port,
                    path: e.path,
                    tunnel: None,
                }));
            });
        } else {
            let Some(cfg) = self.crew.link.as_ref().map(|l| l.cfg.clone()) else {
                self.web.err = "Your server isn't connected, so its browser can't be shown.".into();
                self.go(Place::Browser(id), cx);
                return;
            };
            let dir = if job.remote_dir.is_empty() {
                job.folder.path.clone()
            } else {
                job.remote_dir.clone()
            };
            std::thread::spawn(move || {
                let bin = if cfg.bin.is_empty() {
                    "$HOME/.local/bin/SquadCode".to_string()
                } else {
                    remote::quoted(&cfg.bin)
                };
                let got = remote::run_script(
                    &cfg,
                    &format!("{bin} v2 browser port --dir {}", remote::quoted(&dir)),
                )
                .and_then(|out| {
                    let (port, path) = parse_endpoint(&out).ok_or_else(|| {
                        let why = out.trim();
                        if why.is_empty() {
                            "the server didn't start its browser".to_string()
                        } else {
                            why.to_string()
                        }
                    })?;
                    let tunnel = remote::forward(&cfg, port)
                        .map_err(|why| format!("couldn't reach its browser: {why}"))?;
                    Ok(Opened {
                        port: tunnel.port(),
                        path,
                        tunnel: Some(tunnel),
                    })
                });
                let _ = tx.send(got);
            });
        }
        self.web.opening = Some((rx, url));
        self.go(Place::Browser(id), cx);
    }

    /// Picks up a browser that's ready, stops streaming one nobody looks at, and
    /// closes the ones left idle.
    pub(super) fn poll_web(&mut self, cx: &mut Context<Self>) {
        if self.place != Place::Browser(self.web.job)
            && (self.web.view.is_some() || self.web.opening.is_some())
        {
            self.web.stop();
        }
        if let Some((rx, url)) = &self.web.opening {
            match rx.try_recv() {
                Ok(Ok(o)) => {
                    let url = url.clone();
                    self.web.opening = None;
                    self.web.view = Some(Viewer::connect(o.port, o.path, url));
                    self.web.tunnel = o.tunnel;
                    cx.notify();
                }
                Ok(Err(why)) => {
                    self.web.opening = None;
                    self.web.err = why;
                    cx.notify();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.web.opening = None,
                Err(_) => {}
            }
        }
        if let Some(rx) = &self.web.picking {
            match rx.try_recv() {
                Ok(Ok(p)) => {
                    self.web.picking = None;
                    self.web.picked = Some(p);
                    cx.notify();
                }
                Ok(Err(why)) => {
                    self.web.picking = None;
                    self.say(format!("Couldn't pick that: {why}."));
                    cx.notify();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.web.picking = None,
                Err(_) => {}
            }
        }
        // While you watch a job's browser here it isn't idle.
        if self.web.touched.elapsed() > Duration::from_secs(60) {
            self.web.touched = Instant::now();
            let watching = (self.web.view.is_some() && self.web.tunnel.is_none())
                .then(|| {
                    self.crew
                        .store
                        .job(self.web.job)
                        .map(|j| j.dir().to_string())
                })
                .flatten();
            if let Some(dir) = watching {
                std::thread::spawn(move || host::touch(&dir));
            }
        }
        host::reap_now_and_then();
    }

    fn web_point(&self, pos: Point<Pixels>) -> Option<(f64, f64)> {
        let b = self.web.bounds.get();
        let css = self.web.view.as_ref()?.css();
        view::to_page(
            (f32::from(pos.x - b.origin.x), f32::from(pos.y - b.origin.y)),
            (f32::from(b.size.width), f32::from(b.size.height)),
            css,
        )
    }

    fn web_mouse(&mut self, kind: &str, pos: Point<Pixels>, button: &str, mods: Modifiers) {
        let Some(at) = self.web_point(pos) else {
            return;
        };
        let clicks = if kind == "mousePressed" {
            // Two presses close together are a double click.
            let (when, n) = self.web.clicks;
            let n = if when.elapsed() < Duration::from_millis(400) {
                n + 1
            } else {
                1
            };
            self.web.clicks = (Instant::now(), n);
            n
        } else if kind == "mouseReleased" {
            self.web.clicks.1
        } else {
            0
        };
        if let Some(v) = &self.web.view {
            let m = modifier_bits(mods.alt, mods.control, mods.platform, mods.shift);
            v.mouse(kind, at, button, clicks, m);
        }
    }

    /// Keys typed with the page focused: text as text, other keys as key events,
    /// and the usual shortcuts do what they do in a browser.
    fn web_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = &self.web.view else {
            return;
        };
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let mods = modifier_bits(m.alt, m.control, m.platform, m.shift);
        let command = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        if command {
            let edit = |command: &'static str, k: &'static str, code: &'static str, vk: u32| {
                let spec = KeySpec {
                    key: k,
                    code,
                    vk,
                    text: "",
                    command,
                };
                v.key(&spec, mods, &[command]);
            };
            match ks.key.as_str() {
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        v.insert_text(&text);
                    }
                }
                "a" => edit("selectAll", "a", "KeyA", 65),
                "z" if m.shift => edit("redo", "z", "KeyZ", 90),
                "z" => edit("undo", "z", "KeyZ", 90),
                "r" => v.reload(),
                "[" => v.history(true),
                "]" => v.history(false),
                "l" => window.focus(&self.web.address.read(cx).focus_handle(cx), cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if let Some(spec) = key_spec(&ks.key) {
            v.key(&spec, mods, &[]);
        } else if let Some(text) = ks.key_char.as_deref().filter(|_| !m.control) {
            v.insert_text(text);
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// Turns the newest frame into the picture when it changed.
    fn web_picture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for old in self.web.retired.drain(..) {
            cx.drop_image(old, Some(window));
        }
        let Some(v) = &self.web.view else {
            return;
        };
        let version = v.version.load(Ordering::Relaxed);
        if version != self.web.seen && self.web.built.elapsed() >= FRAME {
            if let Some((pic, _)) = v.frame() {
                if self
                    .web
                    .img
                    .as_ref()
                    .is_none_or(|old| !Arc::ptr_eq(old, &pic))
                {
                    if let Some(old) = self.web.img.replace(pic) {
                        self.web.retired.push(old);
                    }
                    self.web.moved = Instant::now();
                }
            }
            self.web.seen = version;
            self.web.built = Instant::now();
        }
        // The page is moving (scrolling, typing, an animation): keep up with it.
        if self.web.moved.elapsed() < Duration::from_secs(2) || version != self.web.seen {
            window.request_animation_frame();
        }
    }

    pub(super) fn web_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.web_picture(window, cx);
        let job = self.crew.store.job(self.web.job).cloned();
        let remote = job.as_ref().is_some_and(|j| !j.remote_on.is_empty());
        let state = match (&self.web.view, &self.web.opening) {
            (Some(v), _) => v.state(),
            (None, Some(_)) => State::Connecting,
            (None, None) if !self.web.err.is_empty() => State::Closed(self.web.err.clone()),
            (None, None) => State::Closed("Not open.".into()),
        };
        let url = self.web.view.as_ref().map(|v| v.url()).unwrap_or_default();
        let loading = self.web.view.as_ref().is_some_and(|v| v.loading());
        // The address follows the page, unless you're typing a new one.
        if url != self.web.shown
            && !self
                .web
                .address
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            self.web.shown = url.clone();
            let shown = if url == "about:blank" {
                String::new()
            } else {
                url.clone()
            };
            self.web
                .address
                .update(cx, |i, cx| i.set_value(shown, window, cx));
        }
        let (dot, line) = match &state {
            State::Connecting if remote => (
                ui::t_amber(),
                "Reaching its browser on your server…".to_string(),
            ),
            State::Connecting => (ui::t_amber(), "Starting its browser…".to_string()),
            State::Live if loading => (ui::t_amber(), "Loading…".to_string()),
            State::Live => (ui::t_green(), String::new()),
            State::Closed(why) => (ui::t_red(), why.clone()),
        };
        let closed = matches!(state, State::Closed(_));
        let title = job
            .as_ref()
            .map(|j| format!("{}'s browser", j.bot.name()))
            .unwrap_or_else(|| "Browser".into());
        let id = self.web.job;
        let nav = |key: &'static str, icon: IconName| {
            ui::icon_btn(key, icon).when(closed, |d| d.opacity(0.4))
        };
        let bar = div()
            .flex_none()
            .h(px(48.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                ui::ghost_i("web-job", IconName::ArrowLeft, "Job")
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Live(id), cx))),
            )
            .child(div().size(px(8.)).flex_none().rounded_full().bg(c(dot)))
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(title),
            )
            .child(
                nav("web-back", IconName::ChevronLeft).on_click(cx.listener(|this, _, _, _| {
                    if let Some(v) = &this.web.view {
                        v.history(true);
                    }
                })),
            )
            .child(
                nav("web-forward", IconName::ChevronRight).on_click(cx.listener(
                    |this, _, _, _| {
                        if let Some(v) = &this.web.view {
                            v.history(false);
                        }
                    },
                )),
            )
            .child(
                nav("web-reload", IconName::RotateCw).on_click(cx.listener(|this, _, _, _| {
                    if let Some(v) = &this.web.view {
                        v.reload();
                    }
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .bg(c(ui::bg()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.5))
                    .child(
                        Icon::new(IconName::Globe)
                            .size(px(13.))
                            .text_color(c(ui::faint())),
                    )
                    .child(
                        Input::new(&self.web.address)
                            .appearance(false)
                            .bordered(false),
                    ),
            )
            .when(!closed, |d| {
                let on = self.web.pointing;
                d.child(
                    ui::ghost_i(
                        "web-point",
                        IconName::SquareMousePointer,
                        if on {
                            "Click the page…"
                        } else {
                            "Point at it"
                        },
                    )
                    .when(on, |b| {
                        b.bg(ca(ui::AMBER, 0.18)).text_color(c(ui::t_amber()))
                    })
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Click something on the page, then say what should change about it",
                        )
                        .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.web.pointing = !this.web.pointing;
                        if this.web.pointing {
                            this.web.picked = None;
                        }
                        cx.notify();
                    })),
                )
            })
            .when(closed, |d| {
                d.child(
                    ui::ghost_i("web-again", IconName::RotateCcw, "Open again").on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.web.stop();
                            this.open_job_browser(id, None, cx);
                        }),
                    ),
                )
            })
            // A page here can open in your own browser too; one on the server
            // is only reachable through Squad.
            .when(!remote && !url.is_empty() && url != "about:blank", |d| {
                let u = url.clone();
                d.child(
                    ui::ghost_i("web-outside", IconName::ExternalLink, "Open outside")
                        .on_click(move |_, _, cx| cx.open_url(&u)),
                )
            });
        let bounds = self.web.bounds.clone();
        let down = |button: &'static str| {
            cx.listener(move |this: &mut Self, ev: &MouseDownEvent, window, cx| {
                // Pointing: this click picks what's under it, for a bot to change.
                if this.web.pointing && button == "left" {
                    if let (Some(at), Some(v)) = (this.web_point(ev.position), &this.web.view) {
                        this.web.picking = Some(v.pick_at(at));
                        this.pick_input
                            .update(cx, |i, cx| i.set_value("", window, cx));
                        this.web.pointing = false;
                        this.web.picked = None;
                        this.focus_moved = Instant::now();
                        let input = this.pick_input.clone();
                        window.on_next_frame(move |window, cx| {
                            input.update(cx, |i, cx| i.focus(window, cx))
                        });
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                window.focus(&this.web.focus, cx);
                this.web_mouse("mousePressed", ev.position, button, ev.modifiers);
                cx.stop_propagation();
            })
        };
        let up = |button: &'static str| {
            cx.listener(move |this: &mut Self, ev: &MouseUpEvent, _, _| {
                if this.web.picking.is_some() || this.web.picked.is_some() && button == "left" {
                    return;
                }
                this.web_mouse("mouseReleased", ev.position, button, ev.modifiers);
            })
        };
        let mut area = div()
            .id("web-area")
            .track_focus(&self.web.focus)
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(c(ui::bg()))
            .cursor_default()
            .when(self.web.pointing, |d| d.cursor_crosshair())
            .on_mouse_down(MouseButton::Left, down("left"))
            .on_mouse_down(MouseButton::Middle, down("middle"))
            .on_mouse_down(MouseButton::Right, down("right"))
            .on_mouse_up(MouseButton::Left, up("left"))
            .on_mouse_up(MouseButton::Middle, up("middle"))
            .on_mouse_up(MouseButton::Right, up("right"))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, _| {
                let button = if ev.pressed_button == Some(MouseButton::Left) {
                    "left"
                } else {
                    "none"
                };
                this.web_mouse("mouseMoved", ev.position, button, ev.modifiers);
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(16.));
                if let (Some(at), Some(v)) = (this.web_point(ev.position), &this.web.view) {
                    // The wheel's pixels are the page's: scrolling down moves it up.
                    v.wheel(at, -f64::from(f32::from(d.x)), -f64::from(f32::from(d.y)));
                }
                cx.stop_propagation();
            }))
            .on_key_down(
                cx.listener(|this, ev: &KeyDownEvent, window, cx| this.web_key(ev, window, cx)),
            )
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            );
        let empty = |text: String| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .px(px(40.))
                .text_size(px(13.))
                .text_color(c(ui::muted()))
                .child(text)
        };
        let blank = url.is_empty() || url == "about:blank";
        area = match (&self.web.img, &state) {
            (_, State::Live) if blank => area.child(empty(
                "No page yet. When the bot tries the app, it shows up here. You can open one yourself above, like localhost:3000.".into(),
            )),
            (Some(pic), _) => area.child(
                img(ImageSource::Render(pic.clone()))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            ),
            (None, State::Closed(_)) => area.child(empty(line.clone())),
            (None, _) => area.child(empty("Waiting for the first picture…".into())),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .when(!line.is_empty() && !closed, |d| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(16.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .child(line.clone()),
                )
            })
            .child(area)
            .children(self.pick_bar(cx))
            .into_any_element()
    }

    /// The bar under the page while pointing or once something is picked.
    fn pick_bar(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        // A pick starts a new job: by this job's bot when it changes files,
        // else by whoever builds.
        let bot = self
            .crew
            .store
            .job(self.web.job)
            .and_then(|j| {
                if j.mode.writes() {
                    Some(j.bot)
                } else {
                    Member::doer_for(Mode::Build)
                }
            })
            .map(|m| m.name())
            .unwrap_or("the bot");
        let bar = || {
            div()
                .flex_none()
                .min_h(px(48.))
                .px(px(16.))
                .py(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .border_t_1()
                .border_color(c(ui::line()))
                .bg(c(ui::panel()))
                .text_size(px(13.))
        };
        // While pointing, nothing is added under the page, so it doesn't move
        // under the pointer; the bar comes once something is picked.
        if self.web.picking.is_some() {
            return Some(
                bar()
                    .child(
                        Icon::new(IconName::SquareMousePointer)
                            .size(px(15.))
                            .text_color(c(ui::t_amber())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(c(ui::body()))
                            .child("Looking at what you picked…".to_string()),
                    )
                    .child(
                        ui::ghost("web-point-cancel", "Cancel").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.web.pointing = false;
                                this.web.picking = None;
                                cx.notify();
                            },
                        )),
                    ),
            );
        }
        let picked = self.web.picked.clone()?;
        Some(
            bar()
                .child(
                    div()
                        .flex_none()
                        .max_w(px(300.))
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::ink()))
                        .child(format!("Change {}", picked.short())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(c(ui::bg()))
                        .border_1()
                        .border_color(ca(ui::AMBER, 0.5))
                        .child(
                            Input::new(&self.pick_input)
                                .appearance(false)
                                .bordered(false),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .child(format!("new job for {bot}")),
                )
                .child(ui::ghost("web-pick-cancel", "Cancel").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.web.picked = None;
                        this.pick_input
                            .update(cx, |i, cx| i.set_value("", window, cx));
                        cx.notify();
                    },
                ))),
        )
    }

    /// Starts a new job to change the picked element. It's new work, so it
    /// isn't sent as another turn of the job whose browser this is.
    pub(super) fn send_pick(&mut self, words: String, cx: &mut Context<Self>) {
        let Some(picked) = self.web.picked.take() else {
            return;
        };
        if self.sample_guard(cx) {
            self.web.picked = Some(picked);
            return;
        }
        let st = Self::app_settings(cx);
        let Some(opened) = self
            .crew
            .start_from_page(&st, self.web.job, picked.ask(&words))
        else {
            return;
        };
        if let runtime::Opened::Job(id) = opened {
            if let Some(job) = self.crew.store.job(id) {
                self.say(format!(
                    "New job for {}: change {}.",
                    job.bot.name(),
                    picked.short()
                ));
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::parse_endpoint;

    #[test]
    fn reads_where_the_servers_browser_is() {
        assert_eq!(
            parse_endpoint("port=41235\npath=/devtools/browser/abc\n"),
            Some((41235, "/devtools/browser/abc".into()))
        );
        assert_eq!(parse_endpoint("error: no browser\n"), None);
    }
}
