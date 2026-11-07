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
use super::store::Status;
use super::ui::{self, c};
use super::{Place, Shell};

/// The sidebar's width.
pub(super) const WIDTH: f32 = 248.;

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
        .px(px(10.))
        .flex()
        .items_center()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(ui::faint()))
        .child(text.to_uppercase())
}

impl Shell {
    pub(super) fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let place = self.place;
        let inbox = self.inbox_count();
        self.inbox_seen = inbox;
        let folder = self.crew.store.folder().cloned();
        let where_ = folder
            .as_ref()
            .map(|f| short_path(&f.path))
            .unwrap_or_else(|| "No project yet".into());

        // The project switcher: Peek, the project's name and its path. Click it to
        // move to another project folder, or add one.
        let peek_mood = self.side_mood(Member::Fin);
        let peek = self.bot(Member::Fin, peek_mood, 30., runo_holo::Framing::Bust);
        let project = folder
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Squad Code".into());
        let open = self.folder_menu;
        let head = div()
            .id("side-head")
            .h(px(44.))
            .px(px(6.))
            .mb(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(10.))
            .cursor_pointer()
            .when(open, |d| d.bg(c(ui::hover())))
            .hover(|s| s.bg(c(ui::hover())))
            .child(peek)
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
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(where_),
                    ),
            )
            .child(
                Icon::new(IconName::ChevronsUpDown)
                    .size(px(15.))
                    .text_color(c(ui::faint())),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if this.crew.store.folders.is_empty() {
                    this.pick_folder(cx);
                } else {
                    this.folder_menu = !this.folder_menu;
                    cx.notify();
                }
            }));
        let switcher = self.folder_menu.then(|| self.project_menu(cx));

        // Search: the palette finds places, bots and jobs.
        let search = div()
            .id("side-search")
            .h(px(34.))
            .px(px(10.))
            .mb(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(9.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .text_size(px(13.))
            .text_color(c(ui::faint()))
            .cursor_pointer()
            .hover(|s| s.border_color(c(ui::line_hi())))
            .child(Icon::new(IconName::Search).size(px(15.)))
            .child(div().flex_1().child("Search"))
            .child(ui::kbd(if cfg!(target_os = "macos") {
                "⌘K"
            } else {
                "Ctrl K"
            }))
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)));

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
                row(
                    "side-history",
                    matches!(place, Place::History | Place::Race(_) | Place::Goal(_)),
                )
                .child(Icon::new(IconName::MessageSquare).size(px(16.)))
                .child("All jobs")
                .when(all_jobs > 0, |d| d.child(count(all_jobs, None)))
                .on_click(cx.listener(|this, _, _, cx| this.go(Place::History, cx))),
            )
            .child(
                row("side-crew", place == Place::Crew)
                    .child(Icon::new(IconName::Users).size(px(16.)))
                    .child("Your crew")
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Crew, cx))),
            )
            // Your server, once there is one, with a dot for whether it answers.
            .when_some(self.crew.link.as_ref().map(|l| l.up()), |d, up| {
                d.child(
                    row(
                        "side-server",
                        matches!(place, Place::Server | Place::Screen),
                    )
                    .child(Icon::new(IconName::Server).size(px(16.)))
                    .child("Your server")
                    .child(div().ml_auto().size(px(7.)).rounded_full().bg(c(if up {
                        ui::t_green()
                    } else {
                        ui::faint()
                    })))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Server, cx))),
                )
            });

        // The project's # threads: group chats with a few bots each. #general
        // is always there; the one you're in is lit while you're on it.
        let in_thread = matches!(place, Place::Today);
        let here = self.crew.store.thread_here();
        let rows = self.thread_rows();
        let mut threads = div().flex().flex_col().gap(px(1.)).child(
            label("Threads").child(div().flex_1()).child(
                div()
                    .id("side-add-thread")
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                    .child(Icon::new(IconName::Plus).size(px(13.)))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "New thread: a group chat with a few bots",
                        )
                        .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
            ),
        );
        if folder.is_none() {
            threads = threads.child(
                row("side-pick-folder", false)
                    .child(Icon::new(IconName::FolderOpen).size(px(16.)))
                    .child("Pick a project folder")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
            );
        }
        let here_folder = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        for (id, name, busy, asks) in rows {
            let on = in_thread && id == here;
            // Something new since you last looked, in a thread you're not in.
            let unread = !on && self.crew.store.unread(&here_folder, id);
            threads = threads.child(
                row(("side-thread", id as usize), on)
                    .child(
                        div()
                            .w(px(16.))
                            .flex()
                            .justify_center()
                            .text_size(px(15.))
                            .text_color(c(if on { ui::ink() } else { ui::faint() }))
                            .child("#"),
                    )
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
                    .when(busy > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(11.5))
                                .text_color(c(ui::t_blue()))
                                .child(div().size(px(6.)).rounded_full().bg(c(ui::t_blue())))
                                .child(busy.to_string()),
                        )
                    })
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
                                .tooltip(|window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new("Delete thread")
                                        .build(window, cx)
                                })
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

        // The crew, as people you can message: face, name, and what each is up to.
        let members: Vec<Member> = Member::everyone()
            .into_iter()
            .filter(|m| m.on_crew())
            .collect();
        let mut crew = div().flex().flex_col().gap(px(1.)).child(label("Crew"));
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
            crew = crew.child(
                row(("side-bot", who.key() as usize), on)
                    .when(!busy && !asks && !on, |d| d.text_color(c(ui::muted())))
                    .when(busy || asks, |d| d.text_color(c(ui::ink())))
                    .child(face)
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

        let settings = div().flex().flex_col().gap(px(1.)).child(
            row("side-settings", matches!(place, Place::Settings(_)))
                .child(Icon::new(IconName::Settings).size(px(16.)))
                .child("Settings")
                .on_click(cx.listener(|this, _, _, cx| {
                    let pane = this.settings_pane;
                    this.go(Place::Settings(pane), cx)
                })),
        );

        div()
            .id("sidebar")
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            // On a Mac the window buttons sit in the top-left corner.
            .pt(px(if cfg!(target_os = "macos") { 38. } else { 14. }))
            .pb(px(12.))
            .px(px(12.))
            .bg(c(ground()))
            .border_r_1()
            .border_color(c(ui::line()))
            .relative()
            .children(ui::pattern(crate::pattern::Area::Panels))
            .child(head)
            .child(search)
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
                    .child(crew),
            )
            .child(settings.pt(px(8.)))
            .children(switcher)
    }

    /// Each live thread in the current project, #general first: its id, name,
    /// jobs running and jobs waiting on you.
    pub(super) fn thread_rows(&self) -> Vec<(u64, String, usize, usize)> {
        let store = &self.crew.store;
        let Some(folder) = store.folder() else {
            return Vec::new();
        };
        let mut ids: Vec<(u64, String)> = vec![(0, "general".into())];
        ids.extend(
            store
                .threads_in(&folder.path)
                .into_iter()
                .map(|t| (t.id, t.name.clone())),
        );
        ids.into_iter()
            .map(|(id, name)| {
                // One row per race, as everywhere else, so the counts add up.
                let listed = store.listed();
                let jobs: Vec<_> = listed
                    .iter()
                    .filter(|j| {
                        j.folder.path == folder.path
                            && store.thread_for(&folder.path, j.thread) == id
                    })
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

    /// The project switcher's menu: every project folder with what's moving in
    /// it, the ones that need you first, then Add a folder.
    fn project_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = &self.crew.store;
        let mut list: Vec<(usize, String, String, usize, usize)> = store
            .folders
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let mine = store.jobs.iter().filter(|j| j.folder.path == f.path);
                let busy = mine
                    .clone()
                    .filter(|j| matches!(j.status, Status::Running | Status::Shipping))
                    .count();
                let asks = mine.filter(|j| j.needs_you()).count();
                (i, f.name.clone(), short_path(&f.path), busy, asks)
            })
            .collect();
        list.sort_by_key(|(i, _, _, _, asks)| (*asks == 0, *i));
        let current = store.current;
        let mut menu = div()
            .id("project-menu")
            .occlude()
            .w(px(300.))
            .p(px(6.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(12.))
            .bg(c(ui::float()))
            .border_1()
            .border_color(c(ui::line()))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.14 * ui::float_shadow()),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-8.),
                inset: false,
            }])
            .child(
                div()
                    .px(px(10.))
                    .pt(px(6.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::faint()))
                    .child("PROJECTS"),
            );
        for (i, name, path, busy, asks) in list {
            let on = i == current;
            menu = menu.child(
                div()
                    .id(("project-row", i))
                    .px(px(10.))
                    .py(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(c(ui::well())))
                    .hover(|s| s.bg(c(ui::hover())))
                    .child(
                        div()
                            .size(px(26.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .bg(c(if on { ui::fill() } else { ui::press() }))
                            .text_color(c(if on { ui::on_fill() } else { ui::body() }))
                            .text_size(px(12.))
                            .font_weight(FontWeight::BOLD)
                            .child(
                                name.chars()
                                    .next()
                                    .unwrap_or('?')
                                    .to_uppercase()
                                    .to_string(),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::faint()))
                                    .child(path),
                            ),
                    )
                    .when(asks > 0, |d| d.child(count(asks, Some(ui::ask_tone()))))
                    .when(busy > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(11.5))
                                .text_color(c(ui::t_blue()))
                                .child(div().size(px(6.)).rounded_full().bg(c(ui::t_blue())))
                                .child(busy.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.folder_menu = false;
                        this.crew.store.current = i;
                        this.crew.store.save();
                        this.go(Place::Today, cx);
                    })),
            );
        }
        menu = menu
            .child(div().h(px(1.)).my(px(4.)).bg(c(ui::line2())))
            .child(
                div()
                    .id("project-add")
                    .px(px(10.))
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(c(ui::muted()))
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                    .child(Icon::new(IconName::FolderOpen).size(px(15.)))
                    .child("Add a project folder")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.folder_menu = false;
                        this.pick_folder(cx);
                    })),
            );
        deferred(
            div()
                .absolute()
                .top(px(if cfg!(target_os = "macos") { 84. } else { 60. }))
                .left(px(12.))
                .child(menu),
        )
        .with_priority(2)
    }

    /// The top bar over every page: where you are, the engine, and a new job.
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
            Place::Server => "Your server".into(),
            Place::Screen => "Your server / Screen".into(),
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
