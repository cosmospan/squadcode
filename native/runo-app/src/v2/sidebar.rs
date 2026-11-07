//! The sidebar, Crew Chat style: the workspace, search, what needs you, your
//! projects as channels, and the crew as people you can message.
//!
//! The crew lives here, one row per bot: its face, its name and a word on what
//! it's doing right now (working, needs you), so the bots are always in view.
//! The look: a soft grey column beside the white page, rows without boxes, and
//! the place you're on lifted out as a white row with a hairline edge.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::prefs::Pane;
use super::store::Status;
use super::ui::{self, c};
use super::{Place, Shell};

/// The icon rail's width, the labelled panel's beside it, and the two together.
pub(super) const RAIL: f32 = 60.;
pub(super) const PANEL: f32 = 236.;
pub(super) const WIDTH: f32 = RAIL + PANEL;

/// The sidebar's ground, a shade off the page so the page reads as the thing
/// in front.
fn ground() -> u32 {
    ui::ground()
}

/// One row: an icon or face, a label, and something on the right. Lit when
/// you're on it.
fn row(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(8.))
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_color(c(ui::ink()))
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.05),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            } else {
                d.border_1()
                    .border_color(transparent_black())
                    .text_color(c(ui::muted()))
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            }
        })
}

/// A small count on the right of a row.
fn count(n: usize, tone: Option<ui::Tone>) -> Div {
    let d = div()
        .ml_auto()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD);
    match tone {
        Some((text, fill, _)) => d.bg(c(fill)).text_color(c(text)),
        None => d.text_color(c(ui::faint())),
    }
    .child(n.min(99).to_string())
}

/// A group's label, with an optional action on its right.
fn label(text: &'static str) -> Div {
    div()
        .h(px(28.))
        .mt(px(14.))
        .pl(px(28.))
        .pr(px(6.))
        .flex()
        .items_center()
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(ui::muted()))
        .child(text)
}

/// A square button on the rail, lit like a row when you're on its place.
fn rail_btn(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(38.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .relative()
        .rounded(px(10.))
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_color(c(ui::ink()))
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.06),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
            } else {
                d.border_1()
                    .border_color(transparent_black())
                    .text_color(c(ui::muted()))
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            }
        })
}

/// A small icon button, like Search and Hide in the panel's head.
fn small_btn(id: impl Into<ElementId>, icon: IconName) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(26.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_color(c(ui::faint()))
        .cursor_pointer()
        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
        .child(Icon::new(icon).size(px(14.)))
}

/// A dot in a rail button's corner: something needs you there.
fn badge(color: u32) -> Div {
    div()
        .absolute()
        .top(px(2.))
        .right(px(2.))
        .size(px(9.))
        .rounded_full()
        .bg(c(color))
        .border_2()
        .border_color(c(ui::ground()))
}

/// A tooltip saying `text`.
fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

/// How many jobs are running, as a blue dot and a number.
fn running(n: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(5.))
        .text_size(px(11.5))
        .text_color(c(ui::t_blue()))
        .child(div().size(px(6.)).rounded_full().bg(c(ui::t_blue())))
        .child(n.to_string())
}

/// The colours projects and threads take in turn, so each is easy to spot.
const TINTS: [u32; 6] = [
    ui::VIOLET,
    ui::GREEN,
    ui::ORANGE,
    ui::BLUE,
    ui::AMBER,
    ui::RED,
];

fn project_tint(i: usize) -> u32 {
    TINTS[i % TINTS.len()]
}

/// #general is grey; every other thread takes a colour of its own.
fn thread_tint(id: u64) -> u32 {
    if id == 0 {
        0x8B8B97
    } else {
        TINTS[(id as usize + 1) % TINTS.len()]
    }
}

/// A project's first letter on a coloured tile.
fn tile(name: &str, tint: u32, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.28))
        .bg(c(tint))
        .text_color(c(0xFFFFFF))
        .text_size(px(size * 0.56))
        .font_weight(FontWeight::BOLD)
        .child(
            name.chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
        )
}

/// What a section's + does.
type AddFn = fn(&mut Shell, &mut Window, &mut Context<Shell>);

/// The + in front of an "Add a …" row, on a quiet tile.
fn add_mark() -> Div {
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(c(ui::hover()))
        .child(Icon::new(IconName::Plus).size(px(12.)))
}

/// A thread's # on a coloured tile.
fn hash(tint: u32) -> Div {
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(c(tint))
        .text_color(c(0xFFFFFF))
        .child(Icon::new(IconName::Hash).size(px(11.)))
}

impl Shell {
    /// The sidebar's width now: the rail alone when the panel is put away.
    pub(super) fn side_w(&self) -> f32 {
        if self.side_slim {
            RAIL
        } else {
            WIDTH
        }
    }

    pub(super) fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let inbox = self.inbox_count();
        self.inbox_seen = inbox;
        let rail = self.rail(inbox, cx);
        let panel = (!self.side_slim).then(|| self.panel(inbox, cx));
        div()
            .id("sidebar")
            .w(px(self.side_w()))
            .h_full()
            .flex_none()
            .flex()
            .bg(c(ground()))
            .border_r_1()
            .border_color(c(ui::line()))
            .relative()
            .children(ui::pattern(crate::pattern::Area::Panels))
            .child(rail)
            .children(panel)
    }

    /// The icon rail: Peek on top, the places, your projects as tiles, and
    /// Settings at the foot.
    fn rail(&mut self, inbox: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let place = self.place;

        // Peek, the lead, as the app's mark: a click opens Peek's chat.
        let peek_mood = self.side_mood(Member::Fin);
        let peek = self.bot(Member::Fin, peek_mood, 34., runo_holo::Framing::Bust);
        let logo = rail_btn("rail-peek", place == Place::Chat(Member::Fin))
            .size(px(40.))
            .mb(px(4.))
            .child(peek)
            .tooltip(tip("Peek, your lead"))
            .on_hover(cx.listener(|this, on: &bool, _, cx| this.hover_bot(Member::Fin, *on, cx)))
            .on_click(cx.listener(|this, _, _, cx| this.go(Place::Chat(Member::Fin), cx)));

        let places = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(
                rail_btn(
                    "rail-threads",
                    matches!(
                        place,
                        Place::Today | Place::Live(_) | Place::Race(_) | Place::Goal(_)
                    ),
                )
                .child(Icon::new(IconName::Hash).size(px(18.)))
                .tooltip(tip("Threads"))
                .on_click(cx.listener(|this, _, _, cx| this.go(Place::Today, cx))),
            )
            .child(
                rail_btn("rail-inbox", place == Place::Inbox)
                    .child(Icon::new(IconName::Inbox).size(px(18.)))
                    .when(inbox > 0, |d| d.child(badge(ui::RED)))
                    .tooltip(tip(if inbox > 0 {
                        format!("Needs you: {inbox}")
                    } else {
                        "Needs you".into()
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Inbox, cx))),
            )
            .child(
                rail_btn("rail-crew", place == Place::Crew)
                    .child(Icon::new(IconName::Users).size(px(18.)))
                    .tooltip(tip("Your crew"))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Crew, cx))),
            );

        // Your projects as tiles, the one you're in ringed: a dot when
        // something in it needs you, blue while the crew works there.
        let current = self.crew.store.current;
        let folders: Vec<(usize, String, String)> = self
            .crew
            .store
            .folders
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.name.clone(), f.path.clone()))
            .collect();
        let mut list = div()
            .id("rail-projects")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .pt(px(2.));
        for (i, name, path) in folders {
            let rows = self.thread_rows_in(&path);
            let busy: usize = rows.iter().map(|r| r.2).sum();
            let asks: usize = rows.iter().map(|r| r.3).sum();
            let on = i == current;
            let mut say = format!("{name}\n{}", short_path(&path));
            if asks > 0 {
                say.push_str(&format!("\n{asks} waiting on you"));
            } else if busy > 0 {
                say.push_str(&format!("\n{busy} running"));
            }
            list = list.child(
                div()
                    .id(("rail-project", i))
                    .size(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .relative()
                    .rounded(px(12.))
                    .border_2()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::ink()))
                        } else {
                            d.border_color(transparent_black())
                                .opacity(0.85)
                                .hover(|s| s.opacity(1.).border_color(c(ui::line_hi())))
                        }
                    })
                    .child(tile(&name, project_tint(i), 32.))
                    .when(asks > 0, |d| d.child(badge(ui::AMBER)))
                    .when(asks == 0 && busy > 0, |d| d.child(badge(ui::BLUE)))
                    .tooltip(tip(say))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.crew.store.current != i {
                            this.crew.store.current = i;
                            this.crew.store.save();
                        }
                        this.go(Place::Today, cx);
                    })),
            );
        }
        list = list.child(
            rail_btn("rail-add-project", false)
                .size(px(32.))
                .rounded(px(9.))
                .border_dashed()
                .border_color(c(ui::line_hi()))
                .child(Icon::new(IconName::Plus).size(px(15.)))
                .tooltip(tip("Add a project folder"))
                .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
        );

        let slim = self.side_slim;
        let foot = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pt(px(8.))
            .when(slim, |d| {
                d.child(
                    rail_btn("rail-open", false)
                        .child(Icon::new(IconName::PanelLeftOpen).size(px(17.)))
                        .tooltip(tip("Show the sidebar"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.side_slim = false;
                            cx.notify();
                        })),
                )
            })
            .child(
                rail_btn("rail-settings", matches!(place, Place::Settings(_)))
                    .child(Icon::new(IconName::Settings).size(px(18.)))
                    .tooltip(tip("Settings"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let pane = this.settings_pane;
                        this.go(Place::Settings(pane), cx)
                    })),
            );

        div()
            .id("rail")
            .w(px(RAIL))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            // On a Mac the window buttons sit in the top-left corner.
            .pt(px(if cfg!(target_os = "macos") { 40. } else { 12. }))
            .pb(px(12.))
            .bg(ui::ca(ui::ink(), 0.03))
            .when(!slim, |d| d.border_r_1().border_color(c(ui::line2())))
            .child(logo)
            .child(places)
            .child(div().w(px(22.)).h(px(1.)).my(px(6.)).bg(c(ui::line())))
            .child(list)
            .child(foot)
    }

    /// The labelled panel for the project you're in, laid out like a chat
    /// app: the places with their counts, the # threads as channels, the
    /// bots as direct messages, and a few things to try.
    fn panel(&mut self, inbox: usize, cx: &mut Context<Self>) -> Div {
        let place = self.place;

        let folder = self.crew.store.folder().cloned();
        let project = folder
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Squad Code".into());
        let where_ = folder
            .as_ref()
            .map(|f| short_path(&f.path))
            .unwrap_or_else(|| "No project yet".into());
        let head = div()
            .h(px(34.))
            .px(px(6.))
            .mb(px(10.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(ui::ink()))
                            .child(project),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(where_),
                    ),
            )
            .child(
                small_btn("side-search", IconName::Search)
                    .tooltip(tip(if cfg!(target_os = "macos") {
                        "Search  ⌘K"
                    } else {
                        "Search  Ctrl K"
                    }))
                    .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
            )
            .child(
                small_btn("side-compose", IconName::SquarePen)
                    .tooltip(tip("New job"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go(Place::Today, cx);
                        this.composer.update(cx, |i, cx| i.focus(window, cx));
                    })),
            )
            .child(
                small_btn("side-slim", IconName::PanelLeftClose)
                    .tooltip(tip("Hide the sidebar"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.side_slim = true;
                        cx.notify();
                    })),
            );

        let all_jobs = self.crew.store.listed().len();
        let nav = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                row("side-inbox", place == Place::Inbox)
                    .child(Icon::new(IconName::Inbox).size(px(16.)))
                    .child("Needs you")
                    .when(inbox > 0, |d| d.child(count(inbox, Some(ui::ask_tone()))))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Inbox, cx))),
            )
            .child(
                row("side-history", place == Place::History)
                    .child(Icon::new(IconName::MessageSquare).size(px(16.)))
                    .child("All jobs")
                    .when(all_jobs > 0, |d| d.child(count(all_jobs, None)))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::History, cx))),
            )
            .child(
                row("side-usage", place == Place::Usage)
                    .child(Icon::new(IconName::ChartColumn).size(px(16.)))
                    .child("Usage")
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Usage, cx))),
            );

        let threads = self.thread_list(cx);
        let crew = self.crew_list(cx);
        let tries = self.try_list(cx);

        div()
            .w(px(PANEL))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .pt(px(if cfg!(target_os = "macos") { 40. } else { 12. }))
            .pb(px(12.))
            .px(px(10.))
            .child(head)
            .child(
                div()
                    .id("side-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(nav)
                    .child(threads)
                    .child(crew)
                    .children(tries),
            )
    }

    /// A section's heading that folds it open or shut: a caret, its name,
    /// and, when it has one, a + on the right that adds to it.
    fn fold_label(
        &mut self,
        key: &'static str,
        text: &'static str,
        add: Option<(&'static str, AddFn)>,
        cx: &mut Context<Self>,
    ) -> (bool, Stateful<Div>) {
        let open = self.side_folds.get(key).copied().unwrap_or(true);
        let head = div()
            .id(key)
            .h(px(28.))
            .mt(px(14.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c(ui::muted()))
            .cursor_pointer()
            .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            .child(
                div().w(px(16.)).flex().justify_center().child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(13.)),
                ),
            )
            .child(div().flex_1().child(text))
            .when_some(add, |d, (say, act)| {
                d.child(
                    small_btn(("side-add", key.len()), IconName::Plus)
                        .size(px(20.))
                        .tooltip(tip(say))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            act(this, window, cx);
                        })),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.side_folds.insert(key.to_string(), !open);
                cx.notify();
            }));
        (open, head)
    }

    /// The project's # threads: group chats with a few bots each. #general
    /// is always there; the one you're in is lit while you're on it.
    fn thread_list(&mut self, cx: &mut Context<Self>) -> Div {
        let (open, head) = self.fold_label(
            "threads",
            "Threads",
            Some(("New thread", |this, window, cx| this.new_thread(window, cx))),
            cx,
        );
        let mut list = div().flex().flex_col().gap(px(1.)).child(head);
        if !open {
            return list;
        }
        let Some(path) = self.crew.store.folder().map(|f| f.path.clone()) else {
            return list.child(
                row("side-pick-folder", false)
                    .child(Icon::new(IconName::FolderOpen).size(px(16.)))
                    .child("Pick a project folder")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
            );
        };
        let in_thread = matches!(self.place, Place::Today);
        let here = self.crew.store.thread_here();
        for (id, name, busy, asks) in self.thread_rows_in(&path) {
            let on = in_thread && id == here;
            // Something new since you last looked, in a thread you're not in.
            let unread = !on && self.crew.store.unread(&path, id);
            list = list.child(
                row(("side-thread", id as usize), on)
                    .child(hash(thread_tint(id)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(unread, |d| {
                                d.font_weight(FontWeight::SEMIBOLD).text_color(c(ui::ink()))
                            })
                            .child(name),
                    )
                    .when(unread && asks == 0 && busy == 0, |d| {
                        d.child(div().size(px(7.)).rounded_full().bg(c(ui::VIOLET)))
                    })
                    .when(asks > 0, |d| d.child(count(asks, Some(ui::ask_tone()))))
                    .when(busy > 0, |d| d.child(running(busy)))
                    // The thread you're in can be deleted from its row.
                    .when(on && id != 0, |d| {
                        d.child(
                            div()
                                .id(("thread-trash", id as usize))
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .text_color(c(ui::faint()))
                                .hover(|s| s.text_color(c(ui::t_red())).bg(c(ui::hover())))
                                .child(Icon::new(IconName::Trash).size(px(13.)))
                                .tooltip(tip("Delete thread"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.deleting = Some(id);
                                    cx.notify();
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew.store.thread_now = id;
                        this.crew.store.save();
                        this.go(Place::Today, cx);
                    })),
            );
        }
        list.child(
            row("side-new-thread", false)
                .text_color(c(ui::faint()))
                .child(add_mark())
                .child("Add a thread")
                .on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
        )
    }

    /// The crew as direct messages: each bot's face with a dot for what it's
    /// doing (blue working, amber asking you), its name, and a word on it.
    fn crew_list(&mut self, cx: &mut Context<Self>) -> Div {
        let (open, head) = self.fold_label("crew", "Direct messages", None, cx);
        let mut list = div().flex().flex_col().gap(px(1.)).child(head);
        if !open {
            return list;
        }
        let place = self.place;
        let members: Vec<Member> = Member::everyone()
            .into_iter()
            .filter(|m| m.on_crew())
            .collect();
        for who in members {
            let (word, _) = self.status_of(who);
            let busy = self.crew.store.busy(who).is_some() || self.thinking(who);
            let asks = !busy
                && self
                    .crew
                    .store
                    .jobs
                    .iter()
                    .any(|j| j.bot == who && j.needs_you());
            let mood = self.side_mood(who);
            let face = self.bot(who, mood, 24., runo_holo::Framing::Bust);
            let on = place == Place::Chat(who);
            let (word, tint) = if asks {
                ("asks you".to_string(), ui::t_amber())
            } else if busy {
                (word, ui::t_blue())
            } else if who == Member::Fin {
                ("lead".to_string(), ui::faint())
            } else {
                (String::new(), ui::faint())
            };
            list = list.child(
                row(("side-bot", who.key() as usize), on)
                    .when(busy || asks, |d| d.text_color(c(ui::ink())))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .child(face)
                            .when(busy || asks, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .right(px(-2.))
                                        .bottom(px(-2.))
                                        .size(px(9.))
                                        .rounded_full()
                                        .bg(c(if asks { ui::AMBER } else { ui::BLUE }))
                                        .border_2()
                                        .border_color(c(ground())),
                                )
                            }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(who.name()))
                    .when(!word.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c(tint))
                                .child(word),
                        )
                    })
                    .on_hover(
                        cx.listener(move |this, on: &bool, _, cx| this.hover_bot(who, *on, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Chat(who), cx))),
            );
        }
        list.child(
            row("side-add-bot", false)
                .text_color(c(ui::faint()))
                .child(add_mark())
                .child("Add a bot")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.go(Place::Crew, cx);
                    this.open_maker(None, None, window, cx);
                })),
        )
    }

    /// A few things worth trying that the person hasn't done yet: another
    /// engine, a bot of their own, a server that keeps jobs going.
    fn try_list(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if self.crew.store.try_hidden {
            return None;
        }
        let st = cx.global::<crate::settings::AppSettings>();
        let engines = (0..crate::agent::PROVIDERS.len())
            .filter(|i| super::engine::ready(st, *i))
            .count();
        let own_bot = Member::everyone().iter().any(|m| m.is_made());
        let mut list = div().flex().flex_col().gap(px(1.)).child(
            label("Try").child(div().flex_1()).child(
                small_btn("side-try-hide", IconName::X)
                    .size(px(20.))
                    .tooltip(tip("Hide these"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.crew.store.try_hidden = true;
                        this.crew.store.save();
                        cx.notify();
                    })),
            ),
        );
        let mut any = false;
        if engines < 2 {
            any = true;
            list = list.child(
                row("side-try-engine", false)
                    .child(Icon::new(IconName::Zap).size(px(16.)))
                    .child(if engines == 0 {
                        "Connect an engine"
                    } else {
                        "Connect another engine"
                    })
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Engines), cx)),
                    ),
            );
        }
        if !own_bot {
            any = true;
            list = list.child(
                row("side-try-bot", false)
                    .child(Icon::new(IconName::Sparkles).size(px(16.)))
                    .child("Make a bot of your own")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go(Place::Crew, cx);
                        this.open_maker(None, None, window, cx);
                    })),
            );
        }
        if self.crew.link.is_none() {
            any = true;
            list = list.child(
                row("side-try-server", false)
                    .child(Icon::new(IconName::Server).size(px(16.)))
                    .child("Keep jobs going on a server")
                    .on_click(
                        cx.listener(|this, _, _, cx| this.go(Place::Settings(Pane::Server), cx)),
                    ),
            );
        }
        any.then_some(list)
    }

    /// Each live thread in the current project, #general first: its id, name,
    /// jobs running and jobs waiting on you.
    #[allow(dead_code)]
    pub(super) fn thread_rows(&self) -> Vec<(u64, String, usize, usize)> {
        match self.crew.store.folder() {
            Some(f) => self.thread_rows_in(&f.path.clone()),
            None => Vec::new(),
        }
    }

    /// Each live thread in the project at `path`, #general first: its id,
    /// name, jobs running and jobs waiting on you.
    fn thread_rows_in(&self, path: &str) -> Vec<(u64, String, usize, usize)> {
        let store = &self.crew.store;
        let mut ids: Vec<(u64, String)> = vec![(0, "general".into())];
        ids.extend(
            store
                .threads_in(path)
                .into_iter()
                .map(|t| (t.id, t.name.clone())),
        );
        // One row per race, as everywhere else, so the counts add up.
        let listed = store.listed();
        ids.into_iter()
            .map(|(id, name)| {
                let jobs: Vec<_> = listed
                    .iter()
                    .filter(|j| j.folder.path == path && store.thread_for(path, j.thread) == id)
                    .collect();
                let busy = jobs
                    .iter()
                    .filter(|j| matches!(j.status, Status::Running | Status::Shipping))
                    .count();
                let asks = jobs.iter().filter(|j| j.needs_you()).count();
                (id, name, busy, asks)
            })
            .collect()
    }

    /// Starts a new thread: the composer gets `/thread ` to type its name after.
    pub(super) fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Place::Today, cx);
        self.composer.update(cx, |i, cx| {
            ui::put_text(i, "/thread ", window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    /// The top bar over every page    /// The top bar over every page: where you are, the engine, and a new job.
    /// "#name" for a thread of the current project.
    fn thread_crumb(&self, thread: u64) -> String {
        let store = &self.crew.store;
        let here = store.folder().map(|f| f.path.clone()).unwrap_or_default();
        format!("#{}", store.thread_label(store.thread_for(&here, thread)))
    }

    pub(super) fn topbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = self
            .crew
            .store
            .folder()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Squad Code".into());
        let here: String = match self.place {
            Place::Today => format!(
                "#{}",
                self.crew.store.thread_label(self.crew.store.thread_here())
            ),
            Place::Inbox => "Needs you".into(),
            Place::History => "All jobs".into(),
            Place::Chat(m) => format!("Crew / {}", m.name()),
            Place::Crew => "Crew".into(),
            // A job sits in its thread, so the path goes through it.
            Place::Live(id) => match self.crew.store.job(id) {
                Some(j) => format!("{} / #{id} {}", self.thread_crumb(j.thread), j.label()),
                None => format!("Jobs / #{id}"),
            },
            Place::Race(id) => match self.crew.store.job(id) {
                Some(j) => format!("{} / Race / {}", self.thread_crumb(j.thread), j.label()),
                None => "Jobs / Race".into(),
            },
            Place::Goal(id) => match self.crew.store.goals.iter().find(|g| g.id == id) {
                Some(g) => format!("{} / Plan / {}", self.thread_crumb(g.thread), g.text),
                None => "Jobs / Plan".into(),
            },
            Place::Settings(_) => "Settings".into(),
            Place::Usage => "Usage".into(),
            Place::Welcome => "Welcome".into(),
        };
        // Settings apply to every project, so they get a plain title, not a path.
        let global = matches!(self.place, Place::Settings(_));
        // The thread a job or plan sits in: its crumb takes you back there.
        let back_to = match self.place {
            Place::Live(id) | Place::Race(id) => self.crew.store.job(id).map(|j| j.thread),
            Place::Goal(id) => self
                .crew
                .store
                .goals
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.thread),
            _ => None,
        }
        .map(|t| {
            let here = self
                .crew
                .store
                .folder()
                .map(|f| f.path.clone())
                .unwrap_or_default();
            self.crew.store.thread_for(&here, t)
        });
        let running = self.crew.store.running().count();
        // Each step of the path after a chevron; the last, where you are, in full ink.
        let parts: Vec<&str> = here.split(" / ").collect();
        let last = parts.len() - 1;
        let crumbs: Vec<AnyElement> = parts
            .iter()
            .enumerate()
            .flat_map(|(i, part)| {
                let chev = Icon::new(IconName::ChevronRight)
                    .size(px(14.))
                    .flex_none()
                    .text_color(c(ui::faint()))
                    .into_any_element();
                let text = div()
                    .truncate()
                    .map(|d| {
                        if i == last {
                            d.min_w_0()
                                .text_color(c(ui::ink()))
                                .font_weight(FontWeight::SEMIBOLD)
                        } else {
                            d.flex_none().text_color(c(ui::muted()))
                        }
                    })
                    .child(part.to_string());
                let text = match back_to {
                    Some(t) if i == 0 => text
                        .id("crumb-thread")
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(ui::ink())))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.store.thread_now = t;
                            this.crew.store.save();
                            this.go(Place::Today, cx);
                        }))
                        .into_any_element(),
                    _ => text.into_any_element(),
                };
                if global {
                    vec![text]
                } else {
                    vec![chev, text]
                }
            })
            .collect();
        let mono = folder
            .chars()
            .next()
            .unwrap_or('S')
            .to_uppercase()
            .to_string();
        div()
            .h(px(54.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(20.))
            .pr(px(14.))
            .border_b_1()
            .border_color(c(ui::line2()))
            .text_size(px(13.))
            // Where you are: the folder as a small tile, then the place.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w_0()
                    .when(!global, |d| {
                        d.child(
                            div()
                                .h(px(24.))
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(8.))
                                .text_color(c(ui::muted()))
                                .child(
                                    div()
                                        .size(px(20.))
                                        .rounded(px(5.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(c(ui::raised()))
                                        .border_1()
                                        .border_color(c(ui::line()))
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c(ui::ink()))
                                        .child(mono),
                                )
                                .child(folder),
                        )
                    })
                    .children(crumbs),
            )
            .child(div().flex_1())
            // The Jobs panel toggle: what's running and what needs you, at a glance.
            .child({
                let asks = self.inbox_count();
                let open = self.jobs_open;
                div()
                    .id("top-jobs")
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .rounded(px(8.))
                    .border_1()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if open {
                            d.bg(c(ui::hover()))
                                .border_color(c(ui::line_hi()))
                                .text_color(c(ui::ink()))
                        } else {
                            d.border_color(c(ui::line()))
                                .text_color(c(ui::muted()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(Icon::new(IconName::PanelRight).size(px(14.)))
                    .child("Jobs")
                    .when(running > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_color(c(ui::t_blue()))
                                .child(div().size(px(6.)).rounded_full().bg(c(ui::t_blue())))
                                .child(running.to_string()),
                        )
                    })
                    .when(asks > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_color(c(ui::t_amber()))
                                .child(div().size(px(6.)).rounded_full().bg(c(ui::t_amber())))
                                .child(asks.to_string()),
                        )
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.jobs_open = !this.jobs_open;
                        // The panel sits beside a thread or a job: open it there.
                        if this.jobs_open && !matches!(this.place, Place::Today | Place::Live(_)) {
                            this.go(Place::Today, cx);
                        }
                        cx.notify();
                    }))
            })
            // Search, as a quiet field: the palette finds places, bots and jobs.
            .child(
                div()
                    .id("top-search")
                    .w(px(220.))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .bg(c(ui::well()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.5))
                    .text_color(c(ui::faint()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())).text_color(c(ui::muted())))
                    .child(Icon::new(IconName::Search).size(px(14.)))
                    .child(div().flex_1().child("Search jobs, bots…"))
                    .child(ui::kbd(if cfg!(target_os = "macos") {
                        "⌘K"
                    } else {
                        "Ctrl K"
                    }))
                    .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
            )
            .child(
                ui::solid_i("top-new", IconName::Plus, "New job")
                    .h(px(30.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go(Place::Today, cx);
                        this.composer.update(cx, |i, cx| i.focus(window, cx));
                    })),
            )
    }
}

/// A path with the home folder written as `~`.
fn short_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_string(),
    }
}

impl Shell {
    /// "Delete #name?" over the whole window, so the choice is in front of you
    /// rather than squeezed into the sidebar.
    pub(super) fn delete_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.deleting?;
        let name = self
            .crew
            .store
            .threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.clone())?;
        let jobs = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.thread == id)
            .count();
        let scrim = div()
            .id("delete-scrim")
            .absolute()
            .inset_0()
            .bg(ui::ca(0x000000, 0.28))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.deleting = None;
                cx.notify();
            }));
        let card = div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_center()
            .child(
                div()
                    .id("delete-card")
                    .w(px(400.))
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .shadow(vec![BoxShadow {
                        color: ui::ca(0x000000, 0.35 * ui::float_shadow()),
                        offset: point(px(0.), px(20.)),
                        blur_radius: px(60.),
                        spread_radius: px(-6.),
                        inset: false,
                    }])
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .bg(c(ui::bad_tone().1))
                                    .child(
                                        Icon::new(IconName::Trash)
                                            .size(px(16.))
                                            .text_color(c(ui::bad_tone().0)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(format!("Delete #{name}?")),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(c(ui::body()))
                            .child(if jobs == 0 {
                                "What was said in this thread goes. This can't be undone.".to_string()
                            } else {
                                format!(
                                    "What was said in this thread goes. Its {jobs} job{} move to #general, so no work is lost.",
                                    if jobs == 1 { "" } else { "s" }
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(ui::ghost("delete-cancel", "Cancel").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.deleting = None;
                                    cx.notify();
                                },
                            )))
                            .child(
                                ui::solid("delete-yes", "Delete thread")
                                    .bg(c(ui::bad_tone().0))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.delete_thread(id, cx)
                                    })),
                            ),
                    ),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .child(deferred(scrim).with_priority(5))
                .child(deferred(card).with_priority(6))
                .into_any_element(),
        )
    }
}
