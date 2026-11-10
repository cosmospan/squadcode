//! The first-run walkthrough, five short steps shown once: pick a look, check
//! the coding agents (Claude Code, Codex, OpenCode, Antigravity, Gemini and
//! the rest), pick a project folder, pick the crew, and say how much the bots
//! do on their own. Every step can be skipped. Peek leads the crew and is
//! always on it; what to do first is in #general, which opens next.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{Framing, Mood};

use super::data::{self, Member};
use super::ui::{self, c, ca};
use super::{Place, Shell};
use crate::settings::AppSettings;

const STEPS: [&str; 5] = ["Look", "Engines", "Project", "Crew", "How work runs"];

/// The engines the walkthrough always lists, installed or not.
const COMMON: [&str; 5] = ["claude", "codex", "opencode", "antigravity", "gemini"];

fn save(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    cx.update_global::<AppSettings, _>(|st, _| {
        f(st);
        st.save();
    });
}

impl Shell {
    /// Ends the walkthrough with the crew picked so far.
    pub(super) fn finish_setup(&mut self, _cx: &mut Context<Self>) {
        self.crew.store.removed = Member::CREW
            .into_iter()
            .filter(|m| !self.setup_crew.contains(m))
            .collect();
        data::set_removed(&self.crew.store.removed);
        self.crew.store.set_up = true;
        self.crew.store.save();
        self.trail.clear();
    }

    fn setup_step(&mut self, step: usize, cx: &mut Context<Self>) {
        self.setup_step = step.min(STEPS.len() - 1);
        self.entered = std::time::Instant::now();
        cx.notify();
    }

    pub(super) fn welcome(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let step = self.setup_step.min(STEPS.len() - 1);
        let card = match step {
            0 => self.welcome_look(cx),
            1 => self.welcome_engines(cx),
            2 => self.welcome_folder(cx),
            3 => self.welcome_crew(cx),
            _ => self.welcome_work(cx),
        };
        // The steps as numbered chips: done ones ticked, this one filled.
        let mut steps = div().flex().items_center().justify_center().gap(px(6.));
        for (i, name) in STEPS.into_iter().enumerate() {
            let done = i < step;
            let on = i == step;
            steps = steps.child(
                div()
                    .id(("setup-dot", i))
                    .h(px(28.))
                    .px(px(11.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .map(|d| {
                        if on {
                            d.bg(c(ui::pick_fill()))
                                .text_color(c(ui::pick_on()))
                                .border_1()
                                .border_color(ca(0x000000, 0.12))
                        } else {
                            d.border_1()
                                .border_color(c(ui::line()))
                                .text_color(c(if done { ui::accent_text() } else { ui::faint() }))
                                .hover(|s| s.bg(c(ui::hover())))
                        }
                    })
                    .child(if done {
                        format!("\u{2713} {:02} {name}", i + 1)
                    } else {
                        format!("{:02} {name}", i + 1)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.setup_step(i, cx))),
            );
        }
        // The first steps greet you big; later ones keep the card in view.
        let big = step < 3;
        let mark = self.bot(
            Member::Fin,
            Mood::Signature,
            if big { 64. } else { 48. },
            Framing::Bust,
        );
        let head = div()
            .flex()
            .flex_col()
            .items_center()
            .child(mark)
            .child(
                div()
                    .mt(px(if big { 14. } else { 10. }))
                    .text_size(px(if big { 30. } else { 26. }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Welcome to Squad Code"),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(if big { 15. } else { 13.5 }))
                    .text_color(c(ui::body()))
                    .child("Your coding agents, as a crew you talk to in threads."),
            )
            .child(div().mt(px(24.)).mb(px(18.)).child(steps));
        let width = match step {
            0 => 780.,
            3 => 680.,
            4 => 640.,
            _ => 620.,
        };
        div()
            .id("welcome")
            .size_full()
            .relative()
            .children(ui::feed_grid())
            .child(
                div()
                    .id("welcome-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .min_h_full()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .items_center()
                            .px(px(40.))
                            .py(px(40.))
                            .child(head)
                            .child(ui::rise(
                                div().w_full().max_w(px(width)).child(card),
                                self.stagger(0),
                            )),
                    ),
            )
            .into_any_element()
    }

    /// The card's foot: a quiet note on the left, then Back and Continue (or
    /// the last step's button) on the right.
    fn setup_foot(
        &mut self,
        step: usize,
        note: impl Into<SharedString>,
        lead: Option<Stateful<Div>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let last = step + 1 == STEPS.len();
        let next = if last {
            let name = self
                .crew
                .store
                .folder()
                .map(|f| format!("Open {}", f.name))
                .unwrap_or_else(|| "Start using Squad Code".into());
            ui::dbtn("setup-done", name, ui::Btn::Primary)
                .h(px(30.))
                .px(px(13.))
                .text_size(px(13.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.finish_setup(cx);
                    this.go(Place::Today, cx);
                }))
        } else {
            ui::dbtn("setup-next", "Continue", ui::Btn::Primary)
                .h(px(30.))
                .px(px(13.))
                .text_size(px(13.))
                .on_click(cx.listener(move |this, _, _, cx| this.setup_step(step + 1, cx)))
        };
        div()
            .px(px(22.))
            .py(px(14.))
            .flex()
            .items_center()
            .gap(px(6.))
            .border_t_1()
            .border_color(c(ui::line()))
            .children(lead)
            .child(
                div()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(note.into()),
            )
            .child(div().flex_1())
            .when(step > 0, |d| {
                d.child(
                    ui::dbtn("setup-back", "Back", ui::Btn::Plain)
                        .h(px(30.))
                        .px(px(13.))
                        .text_size(px(13.))
                        .on_click(cx.listener(move |this, _, _, cx| this.setup_step(step - 1, cx))),
                )
            })
            .child(next)
    }

    /// 01 Look: the three looks, each light and dark, and whether to follow
    /// the system.
    fn welcome_look(&mut self, cx: &mut Context<Self>) -> Div {
        let looks = self.looks_card(cx);
        let foot = self.setup_foot(0, "More themes are on the way", None, cx);
        setup_card(
            "Pick a look",
            "The whole app changes as you pick. Switch any time in Settings.",
        )
        .child(
            div()
                .px(px(22.))
                .pt(px(14.))
                .pb(px(16.))
                .border_t_1()
                .border_color(c(ui::line()))
                .child(looks),
        )
        .child(foot)
    }

    /// 02 Engines: the coding agents the bots run on, each ready or with its
    /// one-click install. Claude Code, Codex, OpenCode, Antigravity and Gemini
    /// are always listed; the rest are behind "Another engine".
    fn welcome_engines(&mut self, cx: &mut Context<Self>) -> Div {
        let st = cx.global::<AppSettings>().clone();
        let all = &crate::agent::PROVIDERS;
        let ready: Vec<usize> = (0..all.len())
            .filter(|i| super::engine::ready(&st, *i))
            .collect();
        let mut shown: Vec<usize> = Vec::new();
        for id in COMMON {
            if let Some(i) = all.iter().position(|p| p.id == id) {
                shown.push(i);
            }
        }
        for &i in &ready {
            if !shown.contains(&i) {
                shown.push(i);
            }
        }
        let more = self.setup_more;
        if more {
            for i in 0..all.len() {
                if !shown.contains(&i) {
                    shown.push(i);
                }
            }
        }
        let mut list = div().flex().flex_col();
        for (n, i) in shown.into_iter().enumerate() {
            let p = &all[i];
            let ok = ready.contains(&i);
            let install = super::engines::install_cmd(p.id);
            let meta = if ok {
                if st.default_provider == p.id {
                    "on this computer \u{b7} the default".to_string()
                } else {
                    "on this computer".to_string()
                }
            } else if install.is_some() {
                "not installed \u{b7} one click".to_string()
            } else {
                "not installed".to_string()
            };
            let (word, col) = if ok {
                ("Ready", ui::t_green())
            } else {
                ("Not installed", ui::faint())
            };
            list = list.child(
                engine_row(p.mark, p.color, p.name, meta)
                    .child(ui::mono_status(col, word).ml_auto())
                    .when_some(install.filter(|_| !ok), |d, cmd| {
                        d.child(
                            ui::dbtn(("setup-install", n), "Install", ui::Btn::Plain)
                                .ml(px(10.))
                                .h(px(26.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.run_setup(&cmd, cx)),
                                ),
                        )
                    }),
            );
        }
        if !more {
            list = list.child(
                engine_row(
                    "+",
                    ui::faint(),
                    "Another engine",
                    "Cursor, Copilot, Amp, Qwen, Grok and more".to_string(),
                )
                .child(
                    ui::dbtn("setup-more", "Add", ui::Btn::Plain)
                        .ml_auto()
                        .h(px(26.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.setup_more = true;
                            cx.notify();
                        })),
                ),
            );
        }
        let recheck = ui::dbtn("setup-recheck", "Check again", ui::Btn::Ghost).on_click(
            cx.listener(|this, _, _, cx| {
                crate::caps::refresh();
                this.say("Looking for coding agents\u{2026}");
                cx.notify();
            }),
        );
        let words = if ready.is_empty() {
            "Bots run on the coding agents you already use. None was found on this computer yet: install one and sign in, then check again."
        } else {
            "Bots run on the coding agents you already use. We found these on this computer."
        };
        let foot = self.setup_foot(
            1,
            "You can change this later in Settings",
            Some(recheck),
            cx,
        );
        setup_card("Connect an engine", words)
            .child(list)
            .child(foot)
    }

    /// 03 Project: drop or choose a folder, or use one found already.
    fn welcome_folder(&mut self, cx: &mut Context<Self>) -> Div {
        let drop = div()
            .mx(px(22.))
            .my(px(16.))
            .p(px(22.))
            .flex()
            .items_center()
            .gap(px(14.))
            .rounded(px(6.))
            .border_1()
            .border_dashed()
            .border_color(c(ui::line_hi()))
            .child(
                div()
                    .size(px(42.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(c(ui::hover()))
                    .child(
                        Icon::new(IconName::Folder)
                            .size(px(20.))
                            .text_color(c(ui::ink())),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Drop a folder here"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::faint()))
                            .child("or choose one. A git repo works best."),
                    ),
            )
            .child(
                ui::dbtn("setup-add-folder", "Choose folder\u{2026}", ui::Btn::Plain)
                    .h(px(30.))
                    .px(px(13.))
                    .text_size(px(13.))
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
            );
        let current = self.crew.store.current;
        let folders: Vec<(usize, String, String)> = self
            .crew
            .store
            .folders
            .iter()
            .enumerate()
            .take(5)
            .map(|(i, f)| (i, f.name.clone(), f.path.clone()))
            .collect();
        let mut list = div().flex().flex_col();
        if !folders.is_empty() {
            list = list.child(
                div()
                    .px(px(22.))
                    .pb(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::faint()))
                    .child("FOUND ON THIS COMPUTER"),
            );
        }
        for (i, name, path) in folders {
            let on = i == current;
            let home = std::env::var("HOME").unwrap_or_default();
            let short = if !home.is_empty() && path.starts_with(&home) {
                format!("~{}", &path[home.len()..])
            } else {
                path.clone()
            };
            list = list.child(
                div()
                    .px(px(22.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(c(ui::line()))
                    .text_size(px(13.))
                    .child(div().size(px(7.)).rounded_full().bg(c(if on {
                        ui::t_green()
                    } else {
                        ui::line_hi()
                    })))
                    .child(div().text_color(c(ui::ink())).child(name))
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(short),
                    )
                    .child(
                        ui::dbtn(
                            ("setup-folder", i),
                            if on { "In use" } else { "Use this" },
                            if on { ui::Btn::Primary } else { ui::Btn::Plain },
                        )
                        .h(px(26.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.store.current = i;
                            this.crew.store.save();
                            cx.notify();
                        })),
                    ),
            );
        }
        let skip = ui::dbtn("setup-skip", "Skip for now", ui::Btn::Ghost)
            .h(px(30.))
            .text_size(px(13.))
            .on_click(cx.listener(|this, _, _, cx| this.setup_step(3, cx)));
        let foot = self.setup_foot(2, "", Some(skip), cx);
        setup_card(
            "Pick a project folder",
            "The crew works inside it, each bot on its own branch. Nothing leaves your computer.",
        )
        .child(drop)
        .child(list)
        .child(div().mt(px(6.)).child(foot))
    }

    /// 04 Crew: Peek, always on, then the bots to start with.
    fn welcome_crew(&mut self, cx: &mut Context<Self>) -> Div {
        let project = self
            .crew
            .store
            .folder()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let st = cx.global::<AppSettings>().clone();
        let engine = crate::agent::PROVIDERS
            .iter()
            .find(|p| p.id == st.default_provider)
            .map(|p| p.name)
            .unwrap_or("your engine");
        let picked = self.setup_crew.clone();
        let mut grid = div().px(px(22.)).pb(px(16.)).flex().flex_wrap().gap(px(8.));
        let bots: Vec<Member> = std::iter::once(Member::Fin).chain(Member::CREW).collect();
        for (n, who) in bots.into_iter().enumerate() {
            let lead = who == Member::Fin;
            let on = lead || picked.contains(&who);
            let face = self.avatar(who, 28.);
            grid = grid.child(
                div()
                    .id(("setup-bot", n))
                    .w(px(313.))
                    .px(px(12.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::ink()))
                        } else {
                            d.border_color(c(ui::line()))
                                .hover(|s| s.border_color(c(ui::line_hi())))
                        }
                    })
                    .when(lead, |d| d.bg(ca(who.accent(), 0.05)))
                    .when(!lead, |d| d.cursor_pointer())
                    .child(face)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(who.name()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::faint()))
                                    .child(if lead {
                                        "Lead \u{b7} always on the crew"
                                    } else {
                                        who.role()
                                    }),
                            ),
                    )
                    .child(check(on))
                    .when(!lead, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(k) = this.setup_crew.iter().position(|m| *m == who) {
                                this.setup_crew.remove(k);
                            } else {
                                this.setup_crew.push(who);
                            }
                            cx.notify();
                        }))
                    }),
            );
        }
        let count = self.setup_crew.len() + 1;
        let foot = self.setup_foot(
            3,
            format!(
                "{count} {} \u{b7} all on {engine}",
                if count == 1 { "bot" } else { "bots" }
            ),
            None,
            cx,
        );
        let title = if project.is_empty() {
            "Pick your crew".to_string()
        } else {
            format!("Pick your crew for {project}")
        };
        setup_card(
            title,
            "Peek leads and hands each job to the right bot. Start with a few; you can add or make more later.",
        )
        .child(grid)
        .child(foot)
    }

    /// 05 How work runs: how much the bots do on their own, and the seatbelt.
    fn welcome_work(&mut self, cx: &mut Context<Self>) -> Div {
        let st = cx.global::<AppSettings>().clone();
        let level = match (st.v2_plan_first, st.v2_auto_merge) {
            (true, _) => 0,
            (false, false) => 1,
            (false, true) => 2,
        };
        let options: [(&str, &str, bool); 3] = [
            (
                "Ask before anything",
                "Bots show a plan and wait before they change a file.",
                false,
            ),
            (
                "Ask before merging",
                "Bots work on their own branch. Nothing reaches main until you approve.",
                true,
            ),
            (
                "Merge when checks pass",
                "Bots merge their own work when every check passes.",
                false,
            ),
        ];
        let mut opts = div().px(px(22.)).flex().flex_col().gap(px(8.));
        for (i, (name, line, best)) in options.into_iter().enumerate() {
            let on = i == level;
            opts = opts.child(
                div()
                    .id(("setup-level", i))
                    .px(px(14.))
                    .py(px(11.))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.border(px(1.5)).border_color(c(ui::ink()))
                        } else {
                            d.border_1()
                                .border_color(c(ui::line()))
                                .hover(|s| s.border_color(c(ui::line_hi())))
                        }
                    })
                    .child(radio(on))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(c(ui::ink()))
                                            .child(name),
                                    )
                                    .when(best, |d| {
                                        d.child(
                                            div()
                                                .font_family(ui::MONO)
                                                .text_size(px(10.5))
                                                .text_color(c(ui::accent_text()))
                                                .child("RECOMMENDED"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(c(ui::body()))
                                    .child(line),
                            ),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            st.v2_plan_first = i == 0;
                            st.v2_auto_merge = i == 2;
                            st.v2_ask = ["a_lot", "balanced", "hands_off"][i].into();
                        });
                        cx.notify();
                    })),
            );
        }
        // The seatbelt is always on; these say what it does.
        let mut belt = div().flex().flex_col();
        for (n, line) in [
            "Stop before pushing secrets",
            "Keep bots out of .env files",
            "Run checks before review",
        ]
        .into_iter()
        .enumerate()
        {
            belt = belt.child(
                div()
                    .id(("setup-belt", n))
                    .mx(px(22.))
                    .py(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(n < 2, |d| d.border_b_1().border_color(c(ui::line())))
                    .text_size(px(13.))
                    .text_color(c(ui::ink()))
                    .child(line)
                    .child(div().ml_auto().child(super::prefs::switch(true)))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Always on, for every bot and every project",
                        )
                        .build(window, cx)
                    }),
            );
        }
        let foot = self.setup_foot(4, "Last step", None, cx);
        setup_card(
            "How much should bots do on their own?",
            "You can change this later in Settings.",
        )
        .child(opts)
        .child(
            div()
                .px(px(22.))
                .pt(px(18.))
                .pb(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::faint()))
                .child(Icon::new(IconName::ShieldCheck).size(px(12.)))
                .child("SEATBELT"),
        )
        .child(belt)
        .child(div().mt(px(10.)).child(foot))
    }
}

/// The walkthrough's card: white, ringed in the look's accent, with its title
/// and a line under it.
fn setup_card(title: impl Into<SharedString>, words: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .rounded(px(8.))
        .bg(c(ui::raised()))
        .border(px(1.5))
        .border_color(c(ui::accent_bar()))
        .shadow(vec![BoxShadow {
            color: ca(ui::pick_fill(), 0.35),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(4.),
            inset: false,
        }])
        .flex()
        .flex_col()
        .child(
            div()
                .px(px(22.))
                .pt(px(18.))
                .pb(px(4.))
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(ui::ink()))
                .child(title.into()),
        )
        .child(
            div()
                .px(px(22.))
                .pb(px(12.))
                .text_size(px(13.))
                .text_color(c(ui::body()))
                .child(words.into()),
        )
}

/// One engine in the list: its mark, name and a mono line.
fn engine_row(mark: &'static str, color: u32, name: &'static str, meta: String) -> Div {
    div()
        .px(px(22.))
        .py(px(14.))
        .flex()
        .items_center()
        .gap(px(12.))
        .border_t_1()
        .border_color(c(ui::line()))
        .child(
            div()
                .size(px(38.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .bg(ca(color, 0.16))
                .text_color(c(color))
                .font_family(ui::MONO)
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(mark),
        )
        .child(
            div()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(name),
                )
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(11.))
                        .text_color(c(ui::faint()))
                        .child(meta),
                ),
        )
}

/// A ticked or empty box.
fn check(on: bool) -> Div {
    div()
        .size(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .map(|d| {
            if on {
                d.bg(c(ui::pick_fill()))
                    .border_1()
                    .border_color(ca(0x000000, 0.12))
                    .child(
                        Icon::new(IconName::Check)
                            .size(px(10.))
                            .text_color(c(ui::pick_on())),
                    )
            } else {
                d.border(px(1.5)).border_color(c(ui::line_hi()))
            }
        })
}

/// A round choice, filled when picked.
fn radio(on: bool) -> Div {
    div()
        .mt(px(1.))
        .size(px(16.))
        .flex_none()
        .rounded_full()
        .map(|d| {
            if on {
                d.bg(c(ui::pick_fill()))
                    .border(px(5.))
                    .border_color(c(ui::pick_on()))
            } else {
                d.bg(c(ui::raised()))
                    .border(px(1.5))
                    .border_color(c(ui::line_hi()))
            }
        })
}
