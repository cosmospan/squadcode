//! The panel under the menu bar glyph, for the Squad Code window.
//!
//! What needs you first (a bot waiting on your yes, with Allow and Deny right
//! there; changes waiting for review; a job that stopped with a problem), then
//! what the crew is working on, with Stop, then the last few jobs that ended.
//! A box at the top hands the crew a new job without opening the window. With
//! the window closed it reads the saved jobs, so the panel is still useful,
//! and every row opens the window on that job.
//!
//! The panel is as tall as what it shows, up to a limit, and its list scrolls
//! past that.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::store::{self, Job, Status, Store};
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// The panel's width.
pub(crate) const WIDTH: f32 = 380.;
/// The tallest it gets; past this its list scrolls.
const MAX_HEIGHT: f32 = 640.;
/// Rows per section before a "more" line.
const ROWS: usize = 5;
/// Jobs that ended, shown under Recent.
const RECENT: usize = 4;

/// The open Squad Code window's shell, so the panel can read and act on it.
pub(super) struct Here(pub WeakEntity<Shell>);

impl Global for Here {}

fn shell(cx: &App) -> Option<Entity<Shell>> {
    cx.try_global::<Here>().and_then(|h| h.0.upgrade())
}

/// What the glyph in the menu bar shows: the count beside it, whether it has
/// the dot, and its tooltip. None when the window is closed.
pub(crate) fn glyph(cx: &App) -> Option<(String, bool, String)> {
    let shell = shell(cx)?;
    let s = shell.read(cx);
    let board = Board::of(&s.crew.store, true, |j| s.crew.asks_for(j));
    let count = match (board.needs_n, board.running) {
        (0, 0) => String::new(),
        (0, n) | (n, _) => n.to_string(),
    };
    Some((count, board.needs_n > 0, board.summary()))
}

/// One job, as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    id: u64,
    bot: Member,
    title: String,
    folder: String,
    status: Status,
    /// What it is doing now, or how it ended, in a few words.
    note: String,
    /// How long it has run, or how long ago it ended.
    time: String,
    /// Steps done out of steps planned, while it runs.
    progress: Option<(usize, usize)>,
    /// The tool use it waits on: the request's id, "Builder wants to run", and
    /// the command, if any.
    ask: Option<(String, String, Option<String>)>,
    /// The seatbelt holds it, or it clashes with another job: Approve waits.
    blocked: bool,
}

/// Everything the panel lists, sorted into its sections.
#[derive(Default, Debug)]
struct Board {
    needs: Vec<Line>,
    working: Vec<Line>,
    recent: Vec<Line>,
    needs_n: usize,
    running: usize,
    review: usize,
    /// When the last job started, for the line under the title.
    last_at: Option<u64>,
}

impl Board {
    /// `live`: read from the open window. Otherwise it is the saved file, and
    /// nothing in it is running any more, whatever it last said.
    fn of(
        store: &Store,
        live: bool,
        asks: impl Fn(&Job) -> Vec<crate::approvals::Request>,
    ) -> Board {
        let mut b = Board::default();
        for j in store.listed() {
            if !live && j.status == Status::Running {
                if b.recent.len() < RECENT {
                    let mut l = line(j);
                    l.status = Status::Interrupted;
                    l.note = "Was running when the window closed".into();
                    l.time = store::ago(j.started);
                    l.progress = None;
                    b.recent.push(l);
                }
                continue;
            }
            b.last_at = b.last_at.max(Some(j.started));
            if j.status == Status::Review {
                b.review += 1;
            }
            let waiting = asks(j);
            if j.status == Status::Running {
                b.running += 1;
            }
            if let Some(req) = waiting.first() {
                b.needs_n += 1;
                let (verb, code) =
                    crate::menubar::split_code(&crate::approvals::describe(&req.tool, &req.input));
                let mut l = line(j);
                l.note = format!("Waiting on you · {}", l.note);
                l.ask = Some((
                    req.id.clone(),
                    format!("{} wants to {verb}", j.bot.name()),
                    code,
                ));
                b.needs.push(l);
            } else if j.status == Status::Running {
                b.working.push(line(j));
            } else if j.needs_you() {
                b.needs_n += 1;
                b.needs.push(line(j));
            } else if j.status != Status::Discarded && b.recent.len() < RECENT {
                b.recent.push(line(j));
            }
        }
        // The longest running first: that's the one you are wondering about.
        b.working.sort_by_key(|l| l.id);
        // The Inbox's order: a bot waiting on a yes, then reviews, then stops.
        b.needs.sort_by_key(|l| {
            (
                l.ask.is_none(),
                match l.status {
                    Status::Review => 0,
                    Status::Failed | Status::Stopped | Status::Interrupted => 1,
                    _ => 2,
                },
            )
        });
        b
    }

    /// "2 working · 1 needs you", or "All quiet".
    fn summary(&self) -> String {
        let mut out = Vec::new();
        if self.running > 0 {
            out.push(format!("{} working", self.running));
        }
        if self.needs_n > 0 {
            out.push(format!(
                "{} need{} you",
                self.needs_n,
                if self.needs_n == 1 { "s" } else { "" }
            ));
        }
        if out.is_empty() {
            "All quiet".into()
        } else {
            out.join(" · ")
        }
    }
}

fn line(j: &Job) -> Line {
    let running = j.status == Status::Running;
    let note = match j.status {
        Status::Running => j.doing(),
        Status::Review if held(j) => "held by Seatbelt".into(),
        Status::Review if !j.conflict.is_empty() => "clash ahead".into(),
        Status::Review => {
            let (add, del) = j.lines();
            let files = j.changes.len().max(j.files.len());
            if files == 0 {
                "ready for review".into()
            } else {
                format!(
                    "{files} file{} · +{add} −{del}",
                    if files == 1 { "" } else { "s" }
                )
            }
        }
        Status::Done => j
            .last_words()
            .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(str::to_string)
            .unwrap_or_else(|| "Done".into()),
        Status::Shipping if !j.pr_line.is_empty() => j.pr_line.clone(),
        s => s.label().to_string(),
    };
    Line {
        id: j.id,
        bot: j.bot,
        title: j.label().to_string(),
        folder: j.folder.name.clone(),
        status: j.status,
        note,
        time: if running {
            store::span(j.seconds())
        } else {
            store::ago(j.ended.unwrap_or(j.started))
        },
        progress: if running { j.progress() } else { None },
        ask: None,
        blocked: j.status == Status::Review && (held(j) || !j.conflict.is_empty()),
    }
}

/// The seatbelt is holding the job's changes.
fn held(j: &Job) -> bool {
    !j.held.is_empty() && !j.belt_ok
}

/// A bot's face from the open window, or its letter tile when it is closed.
fn face(bot: Member, size: f32, cx: &mut App) -> AnyElement {
    match shell(cx) {
        Some(s) => {
            let image = s.update(cx, |s, _| s.avatar_image(bot, size));
            img(image).size(px(size)).flex_none().into_any_element()
        }
        None => bot_tile(bot).into_any_element(),
    }
}

/// Brings the window forward, opening it if it was closed, and goes to `place`.
fn show(place: Place, cx: &mut App) {
    crate::menubar::close_panel(cx);
    super::to_front(cx);
    if let Some(shell) = shell(cx) {
        shell.update(cx, |s, cx| s.go(place, cx));
    }
}

/// Opens the window on Today with the job box ready to type in.
fn new_job(cx: &mut App) {
    show(Place::Today, cx);
    let (Some(shell), Some(window)) =
        (shell(cx), cx.try_global::<super::OpenWindow>().map(|w| w.0))
    else {
        return;
    };
    let _ = window.update(cx, |_, window, cx| {
        shell.update(cx, |s, cx| {
            s.composer.update(cx, |i, cx| i.focus(window, cx))
        });
    });
}

pub(crate) struct Tray {
    input: Entity<InputState>,
    focus: FocusHandle,
    /// The saved jobs, read once as the panel opens, for when the window is closed.
    saved: Option<Store>,
    hovered: Option<u64>,
    /// Quit was pressed once with jobs running: the next press quits.
    quitting: bool,
    /// What the hand-off box last did, for a moment.
    sent: Option<(SharedString, Instant)>,
    /// The height the content was last drawn at, and the height the window
    /// was last given.
    drawn: Rc<Cell<f32>>,
    fitted: f32,
    _subs: Vec<Subscription>,
}

impl Tray {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Ask Peek, or give the crew a job…"));
        let focus = cx.focus_handle();
        let subs = vec![
            cx.subscribe_in(&input, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let text = input.read(cx).value().trim().to_string();
                    if !text.is_empty() {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                        this.hand_over(text, cx);
                    }
                }
            }),
            // Clicking anywhere else closes it, as a menu would.
            cx.observe_window_activation(window, |_, window, _| {
                if !window.is_window_active() {
                    crate::menubar::panel_dismissed(window);
                }
            }),
        ];
        // Running times tick over while it is open.
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        })
        .detach();
        let live = shell(cx);
        if live
            .as_ref()
            .is_some_and(|s| s.read(cx).crew.store.folder().is_some())
        {
            input.update(cx, |i, cx| i.focus(window, cx));
        } else {
            focus.focus(window, cx);
        }
        Tray {
            input,
            focus,
            saved: live.is_none().then(Store::load_view),
            hovered: None,
            quitting: false,
            sent: None,
            drawn: Rc::new(Cell::new(0.)),
            fitted: 0.,
            _subs: subs,
        }
    }

    /// A height to open at before the first frame says how tall it is.
    pub(crate) fn first_height(cx: &App) -> f32 {
        match shell(cx) {
            Some(s) => {
                let s = s.read(cx);
                let b = Board::of(&s.crew.store, true, |j| s.crew.asks_for(j));
                let rows = b.needs.len().min(ROWS) + b.working.len().min(ROWS) + b.recent.len();
                (190. + rows as f32 * 56.).min(MAX_HEIGHT)
            }
            None => 360.,
        }
    }

    fn hand_over(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(shell) = shell(cx) else {
            return;
        };
        let before = shell.read(cx).crew.store.jobs.len();
        shell.update(cx, |s, cx| {
            s.go(Place::Today, cx);
            s.hand_over(text, cx);
        });
        let started = shell.read(cx).crew.store.jobs.len() > before;
        self.sent = Some((
            if started {
                "Handed to the crew. It's under Working.".into()
            } else {
                "Sent to Peek. Open Today to see the reply.".into()
            },
            Instant::now(),
        ));
        cx.notify();
    }

    fn header(&self, b: &Board, live: bool, projects: usize, cx: &mut Context<Self>) -> Div {
        let status = match (live, b.needs_n) {
            (true, _) => format!("{projects} project{}", if projects == 1 { "" } else { "s" }),
            (false, 0) => "window closed".to_string(),
            (false, n) => format!("window closed · {n} waiting"),
        };
        let quitting = self.quitting;
        let running = b.running;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .py(px(12.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(face(Member::Fin, 22., cx))
            .child(
                div()
                    .flex_none()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Squad Code"),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(status),
            )
            .child(div().flex_1())
            .child(
                ui::dbtn("tray-open", "Open app", ui::Btn::Plain)
                    .h(px(24.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .on_click(|_, _, cx| show(Place::Today, cx)),
            )
            .child(
                tip(
                    ui::icon_btn("tray-quit", IconName::Power),
                    if quitting {
                        "Click again to quit and stop the running jobs"
                    } else {
                        "Quit Squad Code"
                    },
                )
                .when(quitting, |d| d.text_color(c(ui::t_red())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if running > 0 && !this.quitting {
                        this.quitting = true;
                        cx.notify();
                    } else {
                        crate::menubar::quit(cx);
                    }
                })),
            )
    }

    /// The box that hands the crew a job, in the current project.
    fn hand_box(&self) -> Div {
        let sent = self
            .sent
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(4))
            .map(|(t, _)| t.clone());
        div()
            .mx(px(14.))
            .my(px(10.))
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .child(Input::new(&self.input).appearance(false)),
                    )
                    .child(ui::kbd("↵")),
            )
            .when_some(sent, |d, sent| {
                d.child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::t_green()))
                        .truncate()
                        .child(sent),
                )
            })
    }

    /// The banner when the window is closed.
    fn closed_banner(&self, has_jobs: bool) -> Div {
        div()
            .mx(px(12.))
            .mb(px(10.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(12.))
            .bg(ca(ui::VIOLET, 0.08))
            .border_1()
            .border_color(ca(ui::VIOLET, 0.3))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("The window is closed"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(c(ui::body()))
                            .child(if has_jobs {
                                "Your jobs are saved. Open one below to pick up where you left off."
                            } else {
                                "Open it to hand the crew its first job."
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::solid_i("tray-reopen", IconName::ExternalLink, "Open Squad Code")
                            .h(px(30.))
                            .on_click(|_, _, cx| show(Place::Today, cx)),
                    )
                    .child(
                        ui::ghost_i("tray-new-closed", IconName::Plus, "New job")
                            .h(px(30.))
                            .on_click(|_, _, cx| new_job(cx)),
                    ),
            )
    }

    /// A job waiting on your yes: what it wants to do, with Allow and Deny.
    fn ask_card(&self, l: &Line, cx: &mut Context<Self>) -> Stateful<Div> {
        let (req, wants, code) = l.ask.clone().unwrap_or_default();
        let id = l.id;
        let yes = req.clone();
        div()
            .id(("tray-ask", id))
            .mx(px(8.))
            .mb(px(6.))
            .px(px(10.))
            .py(px(10.))
            .rounded(px(11.))
            .bg(ca(ui::AMBER, 0.08))
            .border_1()
            .border_color(ca(ui::AMBER, 0.4))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(head(l, Some(ui::t_amber())))
            .child(
                div()
                    .pl(px(36.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::body()))
                            .child(wants),
                    )
                    .when_some(code, |d, code| {
                        d.child(
                            div()
                                .px(px(8.))
                                .py(px(6.))
                                .rounded(px(7.))
                                .bg(c(ui::raised()))
                                .font_family(ui::MONO)
                                .text_size(px(11.5))
                                .text_color(c(ui::ink()))
                                .line_clamp(3)
                                .child(code),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                ui::solid(("tray-allow", id), "Allow").h(px(28.)).on_click(
                                    cx.listener(move |_, _, _, cx| answer(&yes, true, cx)),
                                ),
                            )
                            .child(
                                ui::ghost(("tray-deny", id), "Deny").h(px(28.)).on_click(
                                    cx.listener(move |_, _, _, cx| answer(&req, false, cx)),
                                ),
                            )
                            .child(div().flex_1())
                            .child(open_link(id)),
                    ),
            )
    }

    /// A job waiting for you: its bot, title and a mono line under it. Changes
    /// to review get Open and Approve; Approve waits while the seatbelt holds
    /// it. The row under the pointer is raised with a lime ring.
    fn row(&self, l: &Line, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = l.id;
        let hovered = self.hovered == Some(id);
        let review = l.status == Status::Review;
        let blocked = l.blocked;
        let mut meta = vec![l.bot.name().to_string(), format!("#{id}")];
        if !l.folder.is_empty() {
            meta.push(l.folder.clone());
        }
        if !l.note.is_empty() {
            meta.push(l.note.clone());
        }
        let tone = match l.status {
            Status::Failed | Status::Interrupted => ui::t_red(),
            _ => ui::faint(),
        };
        let bot = face(l.bot, 20., cx);
        div()
            .id(("tray-row", id))
            .flex()
            .items_start()
            .gap(px(10.))
            .py(px(9.))
            .cursor_pointer()
            .map(|d| {
                if hovered {
                    d.mx(px(8.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .bg(c(ui::raised()))
                        .border(px(1.5))
                        .border_color(c(ui::accent_bar()))
                        .shadow(vec![BoxShadow {
                            color: ca(ui::pick_fill(), 0.35),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(3.),
                            inset: false,
                        }])
                } else {
                    d.px(px(14.))
                        .border(px(1.5))
                        .border_color(gpui_kit::transparent_black())
                }
            })
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if *over {
                    this.hovered = Some(id);
                } else if this.hovered == Some(id) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(move |_, _, cx| show(Place::Live(id), cx))
            .child(bot)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(l.title.clone()),
                    )
                    .child(
                        div()
                            .mt(px(1.))
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .text_color(c(tone))
                            .child(meta.join(" · ")),
                    )
                    .when(review, |d| {
                        d.child(
                            div()
                                .mt(px(7.))
                                .flex()
                                .gap(px(6.))
                                .child(
                                    ui::dbtn(("tray-open-job", id), "Open", ui::Btn::Plain)
                                        .h(px(24.))
                                        .px(px(9.))
                                        .text_size(px(12.))
                                        .on_click(move |_, _, cx| show(Place::Live(id), cx)),
                                )
                                .child(
                                    ui::dbtn(("tray-approve", id), "Approve", ui::Btn::Primary)
                                        .h(px(24.))
                                        .px(px(9.))
                                        .text_size(px(12.))
                                        .when(blocked, |d| d.opacity(0.4).cursor_default())
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            if blocked {
                                                show(Place::Live(id), cx);
                                            } else if let Some(shell) = shell(cx) {
                                                shell.update(cx, |s, cx| s.approve(id, cx));
                                            }
                                        }),
                                ),
                        )
                    }),
            )
            .when(!review, |d| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(l.time.clone()),
                )
            })
    }

    /// A running job: its bot, title, its step in mono blue and a dashed bar.
    /// Stop shows under the pointer.
    fn working_row(&self, l: &Line, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = l.id;
        let hovered = self.hovered == Some(id);
        let step = match l.progress {
            Some((d, n)) if n > 0 => format!("{} of {n}", (d + 1).min(n)),
            _ => l.time.clone(),
        };
        let frac = match l.progress {
            Some((d, n)) if n > 0 => (d + 1).min(n) as f32 / n as f32,
            _ => 0.3,
        };
        let bot = face(l.bot, 18., cx);
        div()
            .id(("tray-row", id))
            .flex()
            .items_start()
            .gap(px(10.))
            .px(px(14.))
            .pt(px(9.))
            .pb(px(6.))
            .cursor_pointer()
            .when(hovered, |d| d.bg(c(ui::hover())))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if *over {
                    this.hovered = Some(id);
                } else if this.hovered == Some(id) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(move |_, _, cx| show(Place::Live(id), cx))
            .child(bot)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::ink()))
                                    .child(l.title.clone()),
                            )
                            .child(if hovered {
                                ui::dbtn(("tray-stop", id), "Stop", ui::Btn::Plain)
                                    .h(px(22.))
                                    .px(px(8.))
                                    .text_size(px(11.5))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        if let Some(shell) = shell(cx) {
                                            shell.update(cx, |s, cx| s.stop_job(id, cx));
                                        }
                                    }))
                                    .into_any_element()
                            } else {
                                ui::mono_status(ui::t_blue(), &step)
                                    .flex_none()
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .mt(px(6.))
                            .child(super::channel::dashed_bar(frac, ui::t_blue())),
                    ),
            )
    }

    fn section(&self, label: &str, lines: &[Line], more: Place, cx: &mut Context<Self>) -> Div {
        let mut out = div().flex().flex_col().child(
            div()
                .flex()
                .justify_between()
                .px(px(14.))
                .pt(px(12.))
                .pb(px(4.))
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::faint()))
                .child(label.to_uppercase())
                .child(lines.len().to_string()),
        );
        for l in lines.iter().take(ROWS) {
            out = out.child(if l.ask.is_some() {
                self.ask_card(l, cx)
            } else if l.status == Status::Running {
                self.working_row(l, cx)
            } else {
                self.row(l, cx)
            });
        }
        if lines.len() > ROWS {
            let n = lines.len() - ROWS;
            out = out.child(
                div()
                    .id(SharedString::from(format!("tray-more-{label}")))
                    .px(px(14.))
                    .py(px(5.))
                    .cursor_pointer()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .hover(|d| d.text_color(c(ui::ink())))
                    .child(format!("{n} more"))
                    .on_click(move |_, _, cx| show(more, cx)),
            );
        }
        out
    }

    fn empty(&self) -> Div {
        div()
            .mx(px(12.))
            .mb(px(8.))
            .px(px(14.))
            .py(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(c(ui::line()))
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Nothing running"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::muted()))
                    .child("Type a job above and Peek hands it to the right bot."),
            )
    }

    /// Where the crew runs, the plan closest to its limit, and today's spend.
    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let (plan, spend) = match shell(cx) {
            Some(s) => {
                let st = cx.global::<crate::settings::AppSettings>();
                super::statusbar::plan_and_spend(st, &s.read(cx).crew.store)
            }
            None => (None, 0.),
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .border_t_1()
            .border_color(c(ui::line()))
            .font_family(ui::MONO)
            .text_size(px(11.))
            .text_color(c(ui::body()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(6.)).rounded(px(1.)).bg(c(ui::t_green())))
                    .child(super::machines::here_name()),
            )
            .when_some(plan, |d, used| d.child(format!("Plan {used:.0}%")))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_color(c(ui::ink()))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("${spend:.2}")),
                    )
                    .child("today"),
            )
    }
}

/// Allows or denies a waiting request, from the window's crew.
fn answer(req: &str, yes: bool, cx: &mut App) {
    if let Some(shell) = shell(cx) {
        shell.update(cx, |s, cx| {
            s.crew.answer_ask(req, yes, "");
            cx.notify();
        });
    }
}

/// A job's first line on a card: its bot, title and status.
fn head(l: &Line, tone: Option<u32>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(bot_tile(l.bot))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(l.title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(tone.unwrap_or(ui::muted())))
                .child(l.time.clone()),
        )
}

/// The bot's first letter on a soft tile of its colour.
fn bot_tile(bot: Member) -> Div {
    let accent = bot.accent();
    div()
        .size(px(26.))
        .flex_none()
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .bg(ca(accent, 0.16))
        .border_1()
        .border_color(ca(accent, 0.3))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(accent))
        .child(bot.name().chars().next().unwrap_or('?').to_string())
}

fn open_link(id: u64) -> Stateful<Div> {
    div()
        .id(("tray-open-job", id))
        .flex()
        .items_center()
        .gap(px(3.))
        .px(px(6.))
        .py(px(4.))
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(c(ui::muted()))
        .hover(|d| d.text_color(c(ui::ink())).bg(c(ui::hover())))
        .child("Open")
        .child(Icon::new(IconName::ArrowUpRight).size(px(12.)))
        .on_click(move |_, _, cx| show(Place::Live(id), cx))
}

fn tip(el: Stateful<Div>, text: &'static str) -> Stateful<Div> {
    el.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx))
}

impl Render for Tray {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live = shell(cx);
        // The window opened again while the panel was up: read it, not the file.
        if live.is_some() {
            self.saved = None;
        }
        let (board, folder, projects) = match &live {
            Some(s) => {
                let s = s.read(cx);
                (
                    Board::of(&s.crew.store, true, |j| s.crew.asks_for(j)),
                    s.crew.store.folder().map(|f| f.name.clone()),
                    s.crew.store.folders.len(),
                )
            }
            None => {
                let store = self.saved.get_or_insert_with(Store::load_view);
                // Nothing runs with the window closed, whatever the file last said.
                (
                    Board::of(store, false, |_| Vec::new()),
                    None,
                    store.folders.len(),
                )
            }
        };
        if board.running == 0 {
            self.quitting = false;
        }

        // Fit the window to what was drawn, once it settles.
        let drawn = self.drawn.get();
        if drawn > 0. && (drawn - self.fitted).abs() > 0.5 {
            self.fitted = drawn;
            // The content and the hairline ring around it.
            let h = (drawn + 2.).min(MAX_HEIGHT);
            window.defer(cx, move |window, _| {
                crate::menubar::fit_panel(window, WIDTH, h)
            });
        }

        let mut list = div()
            .id("tray-list")
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if !board.needs.is_empty() {
            list = list.child(self.section("Inbox", &board.needs, Place::Inbox, cx));
        }
        if !board.working.is_empty() {
            list = list.child(self.section("Working", &board.working, Place::History, cx));
        }
        if board.needs.is_empty() && board.working.is_empty() && live.is_some() {
            list = list.child(self.empty());
        }
        // With the window closed, the last jobs that ended, to pick one up.
        if live.is_none() && !board.recent.is_empty() {
            list = list.child(self.section("Recent", &board.recent, Place::History, cx));
        }

        let drawn = self.drawn.clone();
        let content = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            // Never taller than the panel gets: the list gives way and scrolls.
            .max_h(px(MAX_HEIGHT - 2.))
            .child(
                self.header(&board, live.is_some(), projects, cx)
                    .flex_none(),
            )
            .when(live.is_none(), |d| {
                d.child(
                    self.closed_banner(!board.needs.is_empty() || !board.recent.is_empty())
                        .flex_none(),
                )
            })
            .child(list)
            .when(folder.is_some() && live.is_some(), |d| {
                d.child(self.hand_box().flex_none())
            })
            .child(self.footer(cx).flex_none())
            .child(
                canvas(
                    move |bounds, window, _| {
                        let h = f32::from(bounds.size.height);
                        if (h - drawn.get()).abs() > 0.5 {
                            drawn.set(h);
                            // Draw again now, to fit the window to it.
                            window.refresh();
                        }
                    },
                    |_, _, _, _| (),
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );

        div()
            .id("tray")
            .track_focus(&self.focus)
            .on_key_down(|e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    crate::menubar::close_panel(cx);
                }
            })
            .w_full()
            .p(px(1.))
            .rounded(px(12.))
            .bg(c(ui::line_hi()))
            .child(
                div()
                    .w_full()
                    .rounded(px(11.))
                    .overflow_hidden()
                    .bg(c(ui::bg() & 0xff_ffff))
                    .text_color(c(ui::ink()))
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn job(id: u64, status: Status) -> Job {
        Job {
            id,
            title: format!("Job {id}"),
            status,
            started: store::now(),
            ended: (status != Status::Running).then(store::now),
            seen: false,
            ..Default::default()
        }
    }

    #[test]
    fn jobs_sort_into_their_sections() {
        let store = Store {
            jobs: vec![
                job(1, Status::Done),
                job(2, Status::Running),
                job(3, Status::Review),
                job(4, Status::Failed),
                job(5, Status::Discarded),
            ],
            ..Default::default()
        };
        let b = Board::of(&store, true, |_| Vec::new());
        let ids = |ls: &[Line]| ls.iter().map(|l| l.id).collect::<Vec<_>>();
        // Unseen endings need you; changes to review always do. They come in
        // the Inbox's order: reviews, then stops, then the rest.
        assert_eq!(ids(&b.needs), vec![3, 4, 1]);
        assert_eq!(ids(&b.working), vec![2]);
        assert!(b.recent.is_empty());
        assert_eq!((b.running, b.needs_n, b.review), (1, 3, 1));
        assert_eq!(b.summary(), "1 working · 3 need you");
    }

    #[test]
    fn seen_endings_are_recent_and_a_quiet_crew_says_so() {
        let mut done = job(1, Status::Done);
        done.seen = true;
        assert!(!done.needs_you());
        let store = Store {
            jobs: vec![done],
            ..Default::default()
        };
        let b = Board::of(&store, true, |_| Vec::new());
        assert_eq!(b.recent.len(), 1);
        assert!(b.needs.is_empty() && b.working.is_empty());
        assert_eq!(b.summary(), "All quiet");
    }
}
