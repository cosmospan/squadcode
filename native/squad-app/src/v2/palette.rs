//! The command palette (⌘K): one box to jump anywhere, open any job, hand a
//! bot a job or switch folders, by typing a few letters.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::{Member, Mode};
use super::store::Status;
use super::ui::{self, c, ca};
use super::Pane;
use super::{Place, Shell};

/// What picking an entry does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Go(Place),
    Job(Mode),
    Folder(usize),
    AddFolder,
    /// Open a # thread in this project.
    Thread(u64),
    /// Approve the job waiting for review.
    Approve(u64),
    /// Try the job on other engines too.
    Race(u64),
    /// Open the job's terminal.
    Terminal(u64),
    /// A new job raced on several engines, or a goal split into jobs.
    NewRace,
    NewGoal,
    /// Open the terminal in the project folder.
    ProjectTerminal,
    /// Show the getting-started card again.
    Guide,
}

/// The palette's sections, in the order they show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Jobs,
    Threads,
    GoTo,
    Do,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::Jobs => "Jobs",
            Group::Threads => "Threads",
            Group::GoTo => "Go to",
            Group::Do => "Do",
        }
    }
}

pub struct Entry {
    pub label: String,
    /// Quiet mono words beside the title: a job's #id, a thread's project.
    pub hint: String,
    /// A keyboard shortcut, shown as keys on the right, like "⌘1".
    pub keys: &'static str,
    /// A job's state on the right: its colour and word.
    pub status: Option<(u32, String)>,
    pub icon: Option<IconName>,
    pub bot: Option<Member>,
    pub group: Group,
    pub act: Act,
}

impl Entry {
    fn new(group: Group, label: impl Into<String>, act: Act) -> Self {
        Entry {
            label: label.into(),
            hint: String::new(),
            keys: "",
            status: None,
            icon: None,
            bot: None,
            group,
            act,
        }
    }
    fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
    fn keys(mut self, keys: &'static str) -> Self {
        self.keys = keys;
        self
    }
    fn bot(mut self, who: Member) -> Self {
        self.bot = Some(who);
        self
    }
}

/// Every word typed appears somewhere in the label or hint.
pub fn matches(query: &str, label: &str, hint: &str) -> bool {
    let hay = format!("{label} {hint}").to_lowercase();
    query
        .split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

/// Where the typed words fall in a label, as byte ranges, for shading.
pub fn hits(query: &str, label: &str) -> Vec<std::ops::Range<usize>> {
    let low = label.to_lowercase();
    // Lower-casing can change byte lengths; shade nothing rather than misplace.
    if low.len() != label.len() {
        return vec![];
    }
    let mut out: Vec<std::ops::Range<usize>> = vec![];
    for w in query.split_whitespace() {
        let w = w.to_lowercase();
        if let Some(at) = low.find(&w) {
            out.push(at..at + w.len());
        }
    }
    out.sort_by_key(|r| r.start);
    out.dedup_by(|b, a| {
        if b.start <= a.end {
            a.end = a.end.max(b.end);
            true
        } else {
            false
        }
    });
    out
}

impl Shell {
    pub(super) fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut all = vec![];
        let store = &self.crew.store;
        // Jobs: the latest first, with how each stands.
        let take = if query.trim().is_empty() { 6 } else { 40 };
        for j in store.jobs.iter().rev().take(take) {
            let (word, tone) = super::home::status_of(j);
            let col = if j.status == Status::Running {
                ui::t_blue()
            } else {
                tone.0
            };
            let word = match (j.status, j.progress()) {
                (Status::Running, Some((d, n))) => format!("Step {} of {n}", (d + 1).min(n)),
                _ => word,
            };
            let mut e = Entry::new(Group::Jobs, j.label(), Act::Go(Place::Live(j.id)))
                .hint(format!("#{}", j.id))
                .bot(j.bot);
            e.status = Some((col, word));
            all.push(e);
        }
        // Threads in this project.
        if let Some(f) = store.folder() {
            for t in store.threads_in(&f.path) {
                let waiting = store
                    .jobs
                    .iter()
                    .filter(|j| j.folder.path == f.path && j.thread == t.id && j.needs_you())
                    .count();
                let hint = if waiting > 0 {
                    format!("{} \u{b7} {waiting} need you", f.name)
                } else {
                    f.name.clone()
                };
                all.push(
                    Entry::new(Group::Threads, format!("#{}", t.name), Act::Thread(t.id))
                        .hint(hint),
                );
            }
        }
        let places = [
            (Place::Today, IconName::MessageSquare, "Threads", "⌘1"),
            (Place::Inbox, IconName::Inbox, "Inbox", "⌘2"),
            (
                Place::History,
                IconName::LayoutDashboard,
                "Jobs board",
                "⌘3",
            ),
            (Place::Overview, IconName::House, "Overview", ""),
            (
                Place::Chat(self.last_chat()),
                IconName::MessageCircle,
                "Chat",
                "⌘4",
            ),
            (Place::Crew, IconName::Users, "Crew", "⌘5"),
            (
                Place::Settings(Pane::General),
                IconName::Settings,
                "Settings",
                "⌘6",
            ),
            (
                Place::Settings(Pane::Appearance),
                IconName::Palette,
                "Settings: Appearance",
                "",
            ),
            (
                Place::Settings(Pane::Mascot),
                IconName::Bot,
                "Settings: Mascot",
                "",
            ),
            (
                Place::Settings(Pane::Engines),
                IconName::Cpu,
                "Settings: Engines",
                "",
            ),
            (
                Place::Settings(Pane::Server),
                IconName::Server,
                "Settings: Servers",
                "",
            ),
            (Place::Server, IconName::Server, "Machines", ""),
            (
                Place::Settings(Pane::Usage),
                IconName::ChartColumn,
                "Usage",
                "⌘7",
            ),
        ];
        for (p, icon, label, key) in places {
            all.push(
                Entry::new(Group::GoTo, label, Act::Go(p))
                    .icon(icon)
                    .keys(key),
            );
        }
        for who in Member::all() {
            all.push(
                Entry::new(
                    Group::GoTo,
                    format!("Chat with {}", who.name()),
                    Act::Go(Place::Chat(who)),
                )
                .hint(who.role())
                .bot(who),
            );
        }
        for (i, f) in store.folders.iter().enumerate() {
            let on = i == store.current;
            all.push(
                Entry::new(Group::GoTo, format!("Work in {}", f.name), Act::Folder(i))
                    .icon(IconName::Folder)
                    .hint(if on { "current project" } else { "project" }),
            );
        }
        // What you can do to the job in front of you.
        if let Place::Live(id) | Place::Race(id) = self.place {
            if let Some(j) = store.job(id) {
                if j.status == Status::Review {
                    all.push(
                        Entry::new(
                            Group::Do,
                            format!("Approve \u{201c}{}\u{201d}", j.label()),
                            Act::Approve(id),
                        )
                        .icon(IconName::Check)
                        .hint(format!("#{id}")),
                    );
                }
                if j.mode.writes()
                    && j.race == 0
                    && matches!(j.status, Status::Running | Status::Review)
                {
                    all.push(
                        Entry::new(Group::Do, "Race it on other engines", Act::Race(id))
                            .icon(IconName::Zap)
                            .hint(format!("#{id}")),
                    );
                }
                all.push(
                    Entry::new(
                        Group::Do,
                        "Open the terminal in its branch",
                        Act::Terminal(id),
                    )
                    .icon(IconName::SquareTerminal)
                    .keys("⌃`"),
                );
            }
        }
        let here = format!("#{}", store.thread_label(store.thread_here()));
        for mode in Mode::ALL.into_iter().filter(|m| *m != Mode::Ask) {
            let (who, label) = match Member::doer_for(mode) {
                Some(who) => (who, format!("{}: give {} a job", mode.label(), who.name())),
                None => {
                    let who = Member::for_mode(mode);
                    (
                        who,
                        format!(
                            "{}: a job for {} (Peek asks to add it)",
                            mode.label(),
                            who.name()
                        ),
                    )
                }
            };
            all.push(
                Entry::new(Group::Do, label, Act::Job(mode))
                    .hint(format!("in {here}"))
                    .bot(who),
            );
        }
        all.push(
            Entry::new(Group::Do, "New job, raced on several engines", Act::NewRace)
                .icon(IconName::Zap),
        );
        all.push(
            Entry::new(Group::Do, "New goal, split into jobs", Act::NewGoal)
                .icon(IconName::ListTodo),
        );
        if store.folder().is_some() {
            all.push(
                Entry::new(
                    Group::Do,
                    "Open the terminal in this project",
                    Act::ProjectTerminal,
                )
                .icon(IconName::SquareTerminal)
                .keys(super::term::KEY),
            );
        }
        all.push(
            Entry::new(Group::Do, "Add a project folder…", Act::AddFolder)
                .icon(IconName::FolderOpen),
        );
        all.push(
            Entry::new(Group::Do, "Get started: show the checklist", Act::Guide)
                .icon(IconName::ListChecks),
        );
        all.retain(|e| matches(query, &e.label, &e.hint));
        // Stable, so each section keeps its own order.
        all.sort_by_key(|e| e.group);
        all
    }

    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = true;
        self.palette_sel = 0;
        self.palette_at = std::time::Instant::now();
        self.focus_moved = std::time::Instant::now();
        self.folder_menu = false;
        self.palette_input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = false;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn run_palette(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        match act {
            Act::Go(p) => self.go(p, cx),
            Act::Job(mode) => {
                self.set_mode(mode, window, cx);
                self.go(Place::Today, cx);
                self.composer.update(cx, |i, cx| i.focus(window, cx));
                self.focus_moved = std::time::Instant::now();
            }
            Act::Folder(i) => {
                self.crew.store.current = i;
                self.crew.store.save();
                if let Some(f) = self.crew.store.folder() {
                    let name = f.name.clone();
                    self.say(format!("The crew now works in {name}."));
                }
            }
            Act::AddFolder => self.pick_folder(cx),
            Act::Thread(t) => {
                self.crew.store.thread_now = t;
                self.crew.store.save();
                self.go(Place::Today, cx);
            }
            Act::Approve(id) => self.approve(id, cx),
            Act::Race(id) => self.race_it(id, cx),
            Act::Terminal(id) => {
                if self.place != Place::Live(id) {
                    self.go(Place::Live(id), cx);
                }
                if self.term_open != Some(id) {
                    self.toggle_term(id, window, cx);
                }
            }
            Act::ProjectTerminal => {
                if !self.term_open.is_some_and(super::term::is_project) {
                    self.toggle_project_term(window, cx);
                }
            }
            Act::Guide => {
                self.crew.store.guide_hidden = false;
                self.crew.store.save();
            }
            Act::NewRace | Act::NewGoal => {
                if act == Act::NewRace {
                    self.racing = 2;
                    self.splitting = false;
                } else {
                    self.splitting = true;
                    self.racing = 0;
                }
                if !self.mode.writes() {
                    self.set_mode(Mode::Build, window, cx);
                }
                self.go(Place::Today, cx);
                self.composer.update(cx, |i, cx| i.focus(window, cx));
                self.focus_moved = std::time::Instant::now();
            }
        }
        cx.notify();
    }

    /// Up, Down, Enter and Escape while the palette is open.
    pub(super) fn palette_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let query = self.palette_input.read(cx).value().to_string();
        let n = self.palette_entries(&query).len();
        match key {
            "escape" => self.close_palette(window, cx),
            "down" if n > 0 => self.palette_sel = (self.palette_sel + 1) % n,
            "up" if n > 0 => self.palette_sel = (self.palette_sel + n - 1) % n,
            "enter" => {
                if let Some(e) = self.palette_entries(&query).get(self.palette_sel) {
                    let act = e.act;
                    self.run_palette(act, window, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    pub(super) fn palette_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.palette_input.read(cx).value().to_string();
        let entries = self.palette_entries(&query);
        let sel = self.palette_sel.min(entries.len().saturating_sub(1));
        let k = squad_holo::anim::spring(self.palette_at.elapsed().as_secs_f64(), 0.22) as f32;
        let mut list = div()
            .id("palette-list")
            .max_h(px(440.))
            .overflow_y_scroll()
            .pb(px(6.))
            .flex()
            .flex_col();
        if entries.is_empty() {
            list = list.child(
                div()
                    .px(px(18.))
                    .py(px(18.))
                    .text_size(px(13.))
                    .text_color(c(ui::faint()))
                    .child("Nothing matches. Try a job, a thread or a place."),
            );
        }
        let mut last: Option<Group> = None;
        for (i, e) in entries.into_iter().enumerate() {
            if last != Some(e.group) {
                last = Some(e.group);
                list = list.child(
                    div()
                        .flex_none()
                        .px(px(18.))
                        .pt(px(12.))
                        .pb(px(4.))
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::faint()))
                        .child(e.group.title().to_uppercase()),
                );
            }
            let on = i == sel;
            let lead = match (e.bot, e.icon) {
                (Some(who), _) => self.avatar(who, 18.).into_any_element(),
                (None, Some(icon)) => Icon::new(icon)
                    .size(px(15.))
                    .text_color(c(ui::body()))
                    .into_any_element(),
                _ if e.group == Group::Threads => div()
                    .font_family(ui::MONO)
                    .text_size(px(13.))
                    .text_color(c(ui::accent_text()))
                    .child("#")
                    .into_any_element(),
                _ => div().into_any_element(),
            };
            // A thread's # sits in the lead, so the title drops it.
            let label = if e.group == Group::Threads {
                e.label.trim_start_matches('#').to_string()
            } else {
                e.label.clone()
            };
            let shade = hits(&query, &label)
                .into_iter()
                .map(|r| {
                    (
                        r,
                        HighlightStyle {
                            background_color: Some(ca(ui::pick_fill(), 0.55)),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>();
            let keys = (!e.keys.is_empty()).then(|| {
                let mut row = div().flex().gap(px(4.));
                let mut chars = e.keys.chars();
                if let Some(first) = chars.next() {
                    let rest: String = chars.collect();
                    if matches!(first, '⌘' | '⌃' | '⌥' | '⇧') && !rest.is_empty() {
                        row = row.child(ui::kbd(first.to_string())).child(ui::kbd(rest));
                    } else {
                        row = row.child(ui::kbd(e.keys));
                    }
                }
                row
            });
            let act = e.act;
            list = list.child(
                div()
                    .id(("pal", i))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(18.))
                    .h(px(38.))
                    .relative()
                    .cursor_pointer()
                    .when(on, |d| {
                        d.bg(c(ui::hover())).child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.))
                                .bg(c(ui::accent_bar())),
                        )
                    })
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                        if this.palette_sel != i {
                            this.palette_sel = i;
                            cx.notify();
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.run_palette(act, window, cx)),
                    )
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(lead),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink(1.)
                            .truncate()
                            .text_size(px(13.5))
                            .text_color(c(ui::ink()))
                            .child(StyledText::new(label).with_highlights(shade)),
                    )
                    .when(!e.hint.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .font_family(ui::MONO)
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(e.hint),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(e.status, |d, (col, word)| {
                        d.child(ui::mono_status(col, &word).flex_none())
                    })
                    .when_some(keys, |d, k| d.child(k))
                    .when(on, |d| d.child(ui::kbd("↵"))),
            );
        }
        let scrim = div()
            .id("palette-scrim")
            .absolute()
            .inset_0()
            .bg(ca(0x1A1A17, 0.28 * ui::float_shadow().min(1.)))
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.close_palette(window, cx)));
        let foot_key = |key: &'static str, word: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(ui::kbd(key))
                .child(word)
        };
        let card = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .items_start()
            .child(
                div()
                    .id("palette-card")
                    .mt(px(96. + 14. * (1. - k)))
                    .w(px(640.))
                    .h_auto()
                    .max_h(px(560.))
                    .opacity(k)
                    .rounded(px(10.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .shadow(vec![BoxShadow {
                        color: ca(0x000000, 0.22),
                        offset: point(px(0.), px(24.)),
                        blur_radius: px(60.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(18.))
                            .h(px(52.))
                            .border_b_1()
                            .border_color(c(ui::line()))
                            .child(
                                Icon::new(IconName::Search)
                                    .size(px(16.))
                                    .text_color(c(ui::faint())),
                            )
                            .child(
                                div().flex_1().text_size(px(16.)).child(
                                    Input::new(&self.palette_input)
                                        .appearance(false)
                                        .bordered(false),
                                ),
                            )
                            .child(ui::kbd("esc")),
                    )
                    .child(list)
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .px(px(18.))
                            .py(px(10.))
                            .items_center()
                            .border_t_1()
                            .border_color(c(ui::line()))
                            .bg(c(ui::bg()))
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(foot_key("↑↓", "move"))
                            .child(foot_key("↵", "open"))
                            .child(foot_key("⌘[", "go back")),
                    ),
            );
        div()
            .absolute()
            .inset_0()
            .child(deferred(scrim).with_priority(3))
            .child(deferred(card).with_priority(4))
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn every_word_must_match() {
        assert!(matches("", "Go to Needs you", "⌘2"));
        assert!(matches("nee", "Go to Needs you", "⌘2"));
        assert!(matches("chat fix", "Chat with Fixer", "Hunts the bug"));
        assert!(!matches("chat guard", "Chat with Fixer", "Hunts the bug"));
        assert!(matches("BUILD", "Build: give Builder a job", "New job"));
    }

    #[test]
    fn hits_mark_each_typed_word() {
        use super::hits;
        assert_eq!(hits("che", "Run the checks"), vec![8..11]);
        assert_eq!(hits("run che", "Run the checks"), vec![0..3, 8..11]);
        assert_eq!(hits("chec che", "checks"), vec![0..4]);
        assert!(hits("", "anything").is_empty());
    }
}
