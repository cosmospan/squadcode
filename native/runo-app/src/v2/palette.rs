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

pub struct Entry {
    pub label: String,
    /// What kind of thing it is, shown on the right ("Place", "Job", "⌘1").
    pub hint: String,
    pub icon: Option<IconName>,
    pub bot: Option<Member>,
    pub act: Act,
}

/// Every word typed appears somewhere in the label or hint.
pub fn matches(query: &str, label: &str, hint: &str) -> bool {
    let hay = format!("{label} {hint}").to_lowercase();
    query
        .split_whitespace()
        .all(|w| hay.contains(&w.to_lowercase()))
}

impl Shell {
    pub(super) fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut all = vec![];
        // What you can do to the job in front of you.
        if let Place::Live(id) | Place::Race(id) = self.place {
            if let Some(j) = self.crew.store.job(id) {
                if j.status == Status::Review {
                    all.push(Entry {
                        label: format!("Approve \u{201c}{}\u{201d}", j.label()),
                        hint: "This job".into(),
                        icon: Some(IconName::Check),
                        bot: None,
                        act: Act::Approve(id),
                    });
                }
                if j.mode.writes()
                    && j.race == 0
                    && matches!(j.status, Status::Running | Status::Review)
                {
                    all.push(Entry {
                        label: "Race it on other engines".into(),
                        hint: "This job".into(),
                        icon: Some(IconName::Zap),
                        bot: None,
                        act: Act::Race(id),
                    });
                }
                all.push(Entry {
                    label: "Open the terminal in its branch".into(),
                    hint: "This job · ⌃`".into(),
                    icon: Some(IconName::SquareTerminal),
                    bot: None,
                    act: Act::Terminal(id),
                });
            }
        }
        let places = [
            (Place::Today, IconName::LayoutDashboard, "Jobs", "⌘1"),
            (Place::Inbox, IconName::Inbox, "Needs you", "⌘2"),
            (Place::History, IconName::Clock, "All jobs", "⌘3"),
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
                "Settings: My server",
                "",
            ),
            (Place::Server, IconName::Server, "Your server", ""),
            (Place::Usage, IconName::ChartColumn, "Usage", "⌘7"),
        ];
        for (p, icon, label, key) in places {
            all.push(Entry {
                label: format!("Go to {label}"),
                hint: key.into(),
                icon: Some(icon),
                bot: None,
                act: Act::Go(p),
            });
        }
        for who in Member::all() {
            all.push(Entry {
                label: format!("Chat with {}", who.name()),
                hint: who.role().into(),
                icon: None,
                bot: Some(who),
                act: Act::Go(Place::Chat(who)),
            });
        }
        if self.crew.store.folder().is_some() {
            all.push(Entry {
                label: "Open the terminal in this folder".into(),
                hint: super::term::KEY.into(),
                icon: Some(IconName::SquareTerminal),
                bot: None,
                act: Act::ProjectTerminal,
            });
        }
        all.push(Entry {
            label: "Get started: show the checklist".into(),
            hint: "Help".into(),
            icon: Some(IconName::ListChecks),
            bot: None,
            act: Act::Guide,
        });
        all.push(Entry {
            label: "New job, raced on several engines".into(),
            hint: "New job".into(),
            icon: Some(IconName::Zap),
            bot: None,
            act: Act::NewRace,
        });
        all.push(Entry {
            label: "New goal, split into jobs".into(),
            hint: "New job".into(),
            icon: Some(IconName::ListTodo),
            bot: None,
            act: Act::NewGoal,
        });
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
            all.push(Entry {
                label,
                hint: "New job".into(),
                icon: None,
                bot: Some(who),
                act: Act::Job(mode),
            });
        }
        for j in self.crew.store.jobs.iter().rev().take(12) {
            let state = match j.status {
                Status::Running => "Working",
                _ if j.needs_you() => "New",
                _ => j.status.label(),
            };
            all.push(Entry {
                label: j.label().to_string(),
                hint: format!("{} · {state}", j.bot.name()),
                icon: None,
                bot: Some(j.bot),
                act: Act::Go(Place::Live(j.id)),
            });
        }
        for (i, f) in self.crew.store.folders.iter().enumerate() {
            let on = i == self.crew.store.current;
            all.push(Entry {
                label: format!("Work in {}", f.name),
                hint: if on {
                    "Current folder".into()
                } else {
                    "Folder".into()
                },
                icon: Some(IconName::Folder),
                bot: None,
                act: Act::Folder(i),
            });
        }
        all.push(Entry {
            label: "Add a folder…".into(),
            hint: "Folder".into(),
            icon: Some(IconName::FolderOpen),
            bot: None,
            act: Act::AddFolder,
        });
        all.retain(|e| matches(query, &e.label, &e.hint));
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
        let k = runo_holo::anim::spring(self.palette_at.elapsed().as_secs_f64(), 0.22) as f32;
        let mut list = div()
            .id("palette-list")
            .max_h(px(420.))
            .overflow_y_scroll()
            .p(px(6.))
            .flex()
            .flex_col();
        if entries.is_empty() {
            list = list.child(
                div()
                    .px(px(14.))
                    .py(px(18.))
                    .text_size(px(13.))
                    .text_color(c(ui::faint()))
                    .child("Nothing matches. Try a bot, a job or a place."),
            );
        }
        for (i, e) in entries.into_iter().enumerate() {
            let on = i == sel;
            // Rows ripple in, a few milliseconds apart.
            let rk = runo_holo::anim::spring(
                (self.palette_at.elapsed().as_secs_f64() - 0.02 * i.min(10) as f64).max(0.0),
                0.24,
            ) as f32;
            let lead = match (e.bot, e.icon) {
                (Some(who), _) => self.avatar(who, 26.).into_any_element(),
                (None, Some(icon)) => div()
                    .size(px(26.))
                    .rounded(px(8.))
                    .bg(c(if on { ui::hover() } else { ui::raised() }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon).size(px(14.)).text_color(c(ui::muted())))
                    .into_any_element(),
                _ => div().size(px(26.)).into_any_element(),
            };
            let act = e.act;
            list = list.child(
                div()
                    .id(("pal", i))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(10.))
                    .h(px(42.))
                    .rounded(px(12.))
                    .opacity(rk)
                    .relative()
                    .left(px(6. * (1. - rk)))
                    .cursor_pointer()
                    .when(on, |d| d.bg(c(ui::raised())))
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                        if this.palette_sel != i {
                            this.palette_sel = i;
                            cx.notify();
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.run_palette(act, window, cx)),
                    )
                    .child(lead)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(if on {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(e.label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(e.hint),
                    )
                    .when(on, |d| {
                        d.child(
                            Icon::new(IconName::CornerDownLeft)
                                .size(px(13.))
                                .text_color(c(ui::muted())),
                        )
                    }),
            );
        }
        let scrim = div()
            .id("palette-scrim")
            .absolute()
            .inset_0()
            // No dimming: GPUI orders some bot frames above a full-window wash, so
            // the card stands out by its shadow instead.
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| this.close_palette(window, cx)));
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
                    .mt(px(110. + 14. * (1. - k)))
                    .w(px(620.))
                    .h_auto()
                    .max_h(px(520.))
                    .opacity(k)
                    .rounded(px(20.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .shadow(vec![BoxShadow {
                        color: ca(0x000000, 0.55 * ui::float_shadow()),
                        offset: point(px(0.), px(30.)),
                        blur_radius: px(80.),
                        spread_radius: px(-6.),
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
                            .px(px(16.))
                            .h(px(56.))
                            .border_b_1()
                            .border_color(c(ui::line()))
                            .child(
                                Icon::new(IconName::Search)
                                    .size(px(17.))
                                    .text_color(c(ui::faint())),
                            )
                            .child(
                                div().flex_1().text_size(px(15.)).child(
                                    Input::new(&self.palette_input)
                                        .appearance(false)
                                        .bordered(false),
                                ),
                            )
                            .child(
                                div()
                                    .px(px(7.))
                                    .py(px(2.))
                                    .rounded(px(6.))
                                    .bg(c(ui::raised()))
                                    .font_family(ui::MONO)
                                    .text_size(px(10.))
                                    .text_color(c(ui::muted()))
                                    .child("esc"),
                            ),
                    )
                    .child(list)
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .px(px(16.))
                            .h(px(36.))
                            .items_center()
                            .border_t_1()
                            .border_color(c(ui::line()))
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child("↑↓ to move")
                            .child("↵ to open")
                            .child("⌘[ to go back"),
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
}
