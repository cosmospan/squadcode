//! Getting started: what the Jobs page shows before there are any jobs, and
//! the small checklist in the sidebar that walks through the first few things
//! worth trying. Both point at real controls, so nothing here is a tour that
//! has to be sat through.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::data::{Member, Mode};
use super::store::Step;
use super::ui::{self, c, ca};
use super::Shell;

/// First jobs worth trying: the mode, a name for the card, what it does, and
/// the words it puts in the box.
const STARTERS: [(Mode, &str, &str, &str); 3] = [
    (
        Mode::Ask,
        "Ask about this project",
        "Only reads the code, changes nothing. A safe first try.",
        "What does this project do, in plain words?",
    ),
    (
        Mode::Fix,
        "Fix something",
        "Describe a bug and the crew finds and fixes it on a branch.",
        "Find the most likely bug in this project and fix it",
    ),
    (
        Mode::Build,
        "Build something",
        "A small feature, start to finish, ready for you to review.",
        "Add a section to the README on how to run this project",
    ),
];

impl Shell {
    /// Puts a starter job in the box, in its mode, ready to send.
    fn use_starter(&mut self, mode: Mode, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.target = None;
        self.splitting = false;
        self.racing = 0;
        self.set_mode(mode, window, cx);
        let text = text.to_string();
        self.composer.update(cx, |i, cx| {
            ui::put_text(i, text, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    /// The Jobs page with nothing on it yet: a hello, three first jobs, and
    /// the tools that otherwise only show up once there's work.
    pub(super) fn start_here(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let face = self.bot(Member::Builder, Mood::Signature, 72., Framing::Full);
        let engine_ok = self.step_is_done(Step::Engine, cx);
        let folder_ok = self.crew.store.folder().is_some();
        let (title, words) = match (engine_ok, folder_ok) {
            (true, true) => (
                "Your crew is ready.",
                "Tell it what you want in plain words in the box above. Each job runs on its own branch, and nothing lands in your code until you approve it.",
            ),
            (false, true) => (
                "One thing first: a coding agent.",
                "The crew does its work through a coding agent installed on this computer. Install one below, sign in to it once, and you're ready.",
            ),
            (true, false) => (
                "One thing first: your project.",
                "Pick the folder with your code. The crew works on its own copy of it, so your files stay as they are until you approve a change.",
            ),
            (false, false) => (
                "Two things first.",
                "The crew needs a coding agent on this computer and a project folder to work in. Both take a minute.",
            ),
        };
        let hello = div().flex().items_center().gap(px(16.)).child(face).child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .line_height(px(20.))
                        .text_color(c(ui::muted()))
                        .child(words),
                ),
        );

        let mut starters = div().flex().gap(px(10.));
        // All of them, staffed or not: one whose bot isn't on the crew yet
        // makes Fin offer it when sent.
        for (i, (mode, name, what, text)) in STARTERS.into_iter().enumerate() {
            starters = starters.child(
                ui::card()
                    .id(("starter", i))
                    .flex_1()
                    .min_w_0()
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::raised())).border_color(c(ui::line_hi())))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(c(ui::muted()))
                            .child(what),
                    )
                    .child(
                        div()
                            .mt_auto()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(c(ui::raised()))
                            .font_family(ui::mono())
                            .text_size(px(11.5))
                            .text_color(c(ui::body()))
                            .child(text),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.use_starter(mode, text, window, cx)
                    })),
            );
        }

        let tool = |id: &'static str,
                    icon: IconName,
                    name: &'static str,
                    key: Option<&'static str>,
                    what: &'static str| {
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .rounded(px(10.))
                .border_1()
                .border_dashed()
                .border_color(c(ui::line_hi()))
                .cursor_pointer()
                .hover(|s| s.bg(c(ui::hover())).border_color(c(ui::muted())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(Icon::new(icon).size(px(15.)).text_color(c(ui::muted())))
                        .child(name)
                        .children(key.map(|k| {
                            div()
                                .px(px(5.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(c(ui::line()))
                                .font_family(ui::mono())
                                .text_size(px(10.5))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(c(ui::faint()))
                                .child(k)
                        })),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(c(ui::muted()))
                        .child(what),
                )
        };
        let tools = div()
            .flex()
            .gap(px(10.))
            .child(
                tool(
                    "start-term",
                    IconName::SquareTerminal,
                    "Terminal",
                    Some(super::term::KEY),
                    "A shell in this folder, under any page. Type squad help to hand out jobs from it.",
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_project_term(window, cx))),
            )
            .child(
                tool(
                    "start-split",
                    IconName::Split,
                    "Split",
                    None,
                    "For a bigger goal: the crew plans it as a few jobs that run one after another.",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.use_starter(Mode::Build, "", window, cx);
                    this.splitting = true;
                    this.say("Split is on. Describe the bigger goal, then press Start.");
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "start-race",
                    IconName::Zap,
                    "Race",
                    None,
                    "Run one job on two or three engines at once and keep the best try.",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.use_starter(Mode::Build, "", window, cx);
                    this.racing = 2;
                    this.say("Race is on. Describe the job, then press Start.");
                    cx.notify();
                })),
            )
            .child(
                tool(
                    "start-palette",
                    IconName::Search,
                    "Commands",
                    Some(if cfg!(target_os = "macos") { "\u{2318}K" } else { "Ctrl K" }),
                    "Every action in the app, searchable, from anywhere.",
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
            );

        let ready = engine_ok && folder_ok;
        let setup = (!ready).then(|| {
            div()
                .flex()
                .gap(px(10.))
                .when(!engine_ok, |d| d.child(self.install_card(cx)))
                .when(!folder_ok, |d| d.child(folder_card(cx)))
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(hello)
            .children(setup)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    // Waiting on setup, the first jobs stay in view but quiet.
                    .when(!ready, |d| d.opacity(0.45))
                    .child(label("Try a first job"))
                    .child(starters),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(label("What you can do"))
                    .child(tools),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child("Not ready to start?")
                    .child(
                        div()
                            .id("start-sample")
                            .text_color(c(ui::ink()))
                            .underline()
                            .cursor_pointer()
                            .child("Look around with sample jobs first")
                            .on_click(cx.listener(|this, _, _, cx| this.sample_jobs(true, cx))),
                    ),
            )
            .into_any_element()
    }

    /// Swaps in a made-up store so every screen has something on it, or puts
    /// it away and brings the real one back. The sample store is never saved.
    pub(super) fn sample_jobs(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.sampling {
            return;
        }
        self.sampling = on;
        self.term_open = None;
        self.home_pick = None;
        self.inbox_pick = None;
        // What the real store still has to write goes out before it's set aside.
        self.crew.store.flush(true);
        self.crew.store = if on {
            super::demo::seed()
        } else {
            let mut s = super::store::Store::load();
            s.deferred = true;
            s
        };
        self.go(super::Place::Today, cx);
        self.say(if on {
            "These are sample jobs. Nothing here is real or saved."
        } else {
            "Back to your own project."
        });
        cx.notify();
    }

    /// Nothing real runs on sample jobs: true (and a word on why) while they're
    /// showing.
    pub(super) fn sample_guard(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.sampling {
            return false;
        }
        self.say("Nothing runs while sample jobs are showing. Press Back to my project first.");
        cx.notify();
        true
    }

    /// The strip over Jobs while sample jobs are showing.
    pub(super) fn sample_banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.sampling {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(10.))
                .bg(ca(ui::AMBER, 0.10))
                .border_1()
                .border_color(ca(ui::AMBER, 0.35))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .child("You're looking at sample jobs. Click around: nothing here is real, and nothing is saved."),
                )
                .child(
                    ui::solid("sample-off", "Back to my project")
                        .on_click(cx.listener(|this, _, _, cx| this.sample_jobs(false, cx))),
                )
                .into_any_element(),
        )
    }

    /// How to install a coding agent, for when none is found.
    fn install_card(&mut self, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().gap(px(8.));
        for (n, id) in ["claude", "codex", "gemini"].into_iter().enumerate() {
            let Some(p) = crate::agent::PROVIDERS.iter().find(|p| p.id == id) else {
                continue;
            };
            let Some(cmd) = crate::registry::spec(id).and_then(|s| {
                s.install
                    .npm
                    .as_ref()
                    .map(|n| format!("npm install -g {n}"))
                    .or_else(|| {
                        if cfg!(windows) {
                            s.install.windows.clone()
                        } else {
                            s.install.unix.clone()
                        }
                    })
            }) else {
                continue;
            };
            let copy = cmd.clone();
            let run = cmd.clone();
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(p.name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(7.))
                            .bg(c(ui::bg()))
                            .border_1()
                            .border_color(c(ui::line()))
                            .font_family(ui::mono())
                            .text_size(px(11.5))
                            .truncate()
                            .child(cmd),
                    )
                    .child(ui::ghost(("install-copy", n), "Copy").on_click(cx.listener(
                        move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                            this.say("Copied.");
                            cx.notify();
                        },
                    )))
                    .child(
                        ui::ghost(("install-run", n), "Install")
                            .on_click(cx.listener(move |this, _, _, cx| this.run_setup(&run, cx))),
                    ),
            );
        }
        ui::card()
            .flex_1()
            .min_w_0()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Icon::new(IconName::Cpu).size(px(16.)).text_color(c(ui::AMBER)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Install a coding agent"),
                    )
                    .child(ui::ghost("install-recheck", "Check again").on_click(cx.listener(
                        |this, _, _, cx| {
                            crate::caps::refresh();
                            this.say("Looking for coding agents…");
                            cx.notify();
                        },
                    ))),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(18.))
                    .text_color(c(ui::muted()))
                    .child("Any one is enough. Claude Code is the one most people start with. Install runs right here; sign in once when it asks."),
            )
            .child(rows)
            .children(self.setup_runs(cx))
    }

    /// Whether a getting-started step is done, counting the engine check the
    /// store can't see.
    fn step_is_done(&self, step: Step, cx: &App) -> bool {
        match step {
            Step::Engine => {
                let st = cx.global::<crate::settings::AppSettings>();
                (0..crate::agent::PROVIDERS.len()).any(|i| super::engine::ready(st, i))
            }
            s => self.crew.store.step_done(s),
        }
    }

    /// The getting-started card at the foot of the sidebar, until every step
    /// is done or it's put away.
    pub(super) fn getting_started(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.crew.store.guide_hidden || self.crew.store.demo {
            return None;
        }
        let done: Vec<bool> = Step::ALL
            .into_iter()
            .map(|s| self.step_is_done(s, cx))
            .collect();
        let n = done.iter().filter(|d| **d).count();
        if n == Step::ALL.len() {
            return None;
        }
        let next = done.iter().position(|d| !d);
        let mut list = div().flex().flex_col().gap(px(2.));
        for (i, step) in Step::ALL.into_iter().enumerate() {
            let ok = done[i];
            let now = next == Some(i);
            list =
                list.child(
                    div()
                        .id(("guide-step", i))
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .px(px(6.))
                        .py(px(5.))
                        .rounded(px(6.))
                        .text_size(px(12.5))
                        .when(!ok, |d| d.cursor_pointer().hover(|s| s.bg(c(ui::hover()))))
                        .child(
                            div()
                                .size(px(15.))
                                .flex_none()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(9.5))
                                .map(|d| {
                                    if ok {
                                        d.bg(c(ui::GREEN)).text_color(c(0x06150E)).child("\u{2713}")
                                    } else if now {
                                        d.border_1().border_color(c(ui::RED))
                                    } else {
                                        d.border_1().border_color(c(ui::faint()))
                                    }
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(c(if ok {
                                    ui::faint()
                                } else if now {
                                    ui::ink()
                                } else {
                                    ui::body()
                                }))
                                .when(ok, |d| d.line_through())
                                .child(step.label()),
                        )
                        .when(!ok, |d| {
                            d.on_click(cx.listener(move |this, _, window, cx| {
                                this.show_step(step, window, cx)
                            }))
                        }),
                );
        }
        let frac = n as f32 / Step::ALL.len() as f32;
        Some(
            div()
                .mx(px(2.))
                .mb(px(8.))
                .p(px(10.))
                .rounded(px(10.))
                .bg(c(ui::panel()))
                .border_1()
                .border_color(c(ui::line()))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Get started"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(format!("{n} of {}", Step::ALL.len())),
                        )
                        .child(
                            div()
                                .id("guide-hide")
                                .px(px(4.))
                                .rounded(px(4.))
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .cursor_pointer()
                                .hover(|s| s.text_color(c(ui::ink())).bg(c(ui::hover())))
                                .child("Hide")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.crew.store.guide_hidden = true;
                                    this.crew.store.save();
                                    this.say(
                                        "Hidden. Bring it back from the command menu: Get started.",
                                    );
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div().h(px(4.)).rounded(px(2.)).bg(c(ui::line())).child(
                        div()
                            .h_full()
                            .w(relative(frac))
                            .rounded(px(2.))
                            .bg(c(ui::RED)),
                    ),
                )
                .child(list)
                .into_any_element(),
        )
    }

    /// Takes you to where a getting-started step is done, with a word on how.
    pub(super) fn show_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        match step {
            Step::Engine => self.go(super::Place::Settings(super::prefs::Pane::Engines), cx),
            Step::Folder => self.pick_folder(cx),
            Step::Ask => {
                self.go(super::Place::Today, cx);
                let (mode, _, _, text) = STARTERS[0];
                self.use_starter(mode, text, window, cx);
                self.say("A safe first question is in the box. Press Start to send it.");
            }
            Step::Terminal => self.toggle_project_term(window, cx),
            Step::Race => {
                self.go(super::Place::Today, cx);
                self.use_starter(Mode::Build, "", window, cx);
                self.racing = 2;
                self.say("Race is on. Describe a small job, then press Start.");
            }
        }
        cx.notify();
    }
}

/// A small heading over a group, like "Try a first job".
fn label(text: &'static str) -> Div {
    div()
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::faint()))
        .child(text)
}

/// Where to pick the project folder, for when none is picked yet.
fn folder_card(cx: &mut Context<Shell>) -> Div {
    ui::card()
        .flex_1()
        .min_w_0()
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(Icon::new(IconName::Folder).size(px(16.)).text_color(c(ui::AMBER)))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Pick your project folder"),
                ),
        )
        .child(
            div()
                .text_size(px(12.5))
                .line_height(px(18.))
                .text_color(c(ui::muted()))
                .child("The folder with your code, usually a git repository. You can also drop a folder anywhere on this window."),
        )
        .child(
            div().flex().child(
                ui::solid("start-folder", "Choose a folder…")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::STARTERS;

    #[test]
    fn starters_cover_reading_fixing_and_building() {
        let modes: Vec<_> = STARTERS.iter().map(|s| s.0).collect();
        assert_eq!(
            modes,
            vec![super::Mode::Ask, super::Mode::Fix, super::Mode::Build]
        );
        assert!(STARTERS.iter().all(|s| !s.3.is_empty()));
    }
}
