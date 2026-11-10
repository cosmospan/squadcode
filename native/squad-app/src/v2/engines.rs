//! Engines: which coding agent and model each bot runs on, and the models Squad Code
//! itself thinks with. Choices are saved to Squad Code's settings straight away.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::agent;
use crate::agent_pick::AgentPick;
use crate::settings::{AppSettings, BotEngine};

use super::data::Member;
use super::engine::{self, vendor};
use super::ui::{self, c, ca, space};
use super::Shell;

fn save(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    cx.update_global::<AppSettings, _>(|st, _| {
        f(st);
        st.save();
    });
}

/// A small chip for one choice; `ok` false greys it as not installed.
pub(super) fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    ok: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(11.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_full()
        .text_size(px(12.5))
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(c(ui::fill()))
                    .text_color(c(ui::on_fill()))
                    .font_weight(FontWeight::MEDIUM)
            } else {
                d.border_1()
                    .border_color(c(ui::line()))
                    .bg(c(ui::panel()))
                    .text_color(c(if ok { ui::ink() } else { ui::faint() }))
                    .hover(|s| s.bg(c(ui::hover())))
            }
        })
        .child(label.into())
}

/// The command that installs agent `id`'s command-line tool here.
pub(super) fn install_cmd(id: &str) -> Option<String> {
    crate::registry::spec(id).and_then(|s| {
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
    })
}

/// The command that brings agent `id`'s tool to its newest version: npm's
/// latest, or the vendor's script again (it installs over the old one).
fn update_cmd(id: &str) -> Option<String> {
    let s = crate::registry::spec(id)?;
    match s.install.npm.as_ref().or(s.package.as_ref()) {
        Some(n) => Some(format!("npm install -g {n}@latest")),
        None => install_cmd(id),
    }
}

impl Shell {
    /// Smart Peek: off until the person turns it on, which downloads Squad's
    /// own model once. Squad then runs it only when the rules aren't sure.
    fn smart_peek(&mut self, st: &AppSettings, cx: &mut Context<Self>) -> Div {
        use super::brain::helper::{self, Download};
        let have = helper::model_path().is_file();
        let ours = helper::is_ours(&st.brain_model) && st.brain_url.trim().is_empty();
        let other = !st.brain_model.trim().is_empty() && !ours;
        let state = helper::download_state();
        let downloading = matches!(state, Download::Running { .. });
        let mut chips = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                choice("peek-off", "Off", st.brain_model.trim().is_empty(), true).on_click(
                    cx.listener(|_, _, _, cx| {
                        helper::stop();
                        save(cx, |st| st.brain_model.clear());
                        cx.notify();
                    }),
                ),
            )
            .child(
                choice("peek-on", "On \u{b7} Squad's own model", ours, true).on_click(cx.listener(
                    |_, _, _, cx| {
                        let url = cx.global::<AppSettings>().brain_model_url.clone();
                        helper::download(&url, false);
                        save(cx, |st| {
                            st.brain_model = helper::MODEL.to_string();
                            st.brain_url.clear();
                        });
                        cx.notify();
                    },
                )),
            );
        if other {
            chips = chips.child(choice(
                "peek-other",
                format!("On \u{b7} {}", st.brain_model.trim()),
                true,
                true,
            ));
        }
        if have && ours && !downloading {
            chips = chips.child(
                choice("peek-update", "Check for a newer model", false, true).on_click(
                    cx.listener(|_, _, _, cx| {
                        let url = cx.global::<AppSettings>().brain_model_url.clone();
                        helper::download(&url, true);
                        cx.notify();
                    }),
                ),
            );
        }
        if have && !ours && !downloading {
            chips = chips.child(
                choice("peek-delete", "Delete the model (505 MB)", false, true).on_click(
                    cx.listener(|_, _, _, cx| {
                        helper::remove();
                        cx.notify();
                    }),
                ),
            );
        }
        let mb = |b: u64| b / 1_000_000;
        let (line, warn) = match state {
            Download::Running { got, total } if ours => {
                (format!("Downloading Squad's model: {} of {} MB.", mb(got), mb(total)), false)
            }
            Download::Failed(e) if ours && !have => (format!("The download didn't finish: {e}. Turn it on again to retry."), true),
            Download::Failed(e) if ours => (format!("Couldn't update: {e}. The model you have keeps working."), true),
            _ if other => (format!("Asking {} at {}. You run that server yourself.", st.brain_model.trim(), if st.brain_url.trim().is_empty() { super::brain::model::DEFAULT_URL } else { st.brain_url.trim() }), false),
            _ if ours && have && helper::server_bin().is_none() => (
                "Needs llama-server from llama.cpp (on a Mac: brew install llama.cpp). Until then the rules decide.".to_string(),
                true,
            ),
            _ if ours && have => (format!("On{}. A small model on this computer picks the bot when the rules aren't sure, in about half a second. It stops after {} idle minutes.", helper::installed().map(|r| format!(", Peek v{}", r.version)).unwrap_or_default(), helper::IDLE_MINUTES), false),
            _ => ("Off: the rules pick the bot. Turning it on downloads Squad's own model once (505 MB); it runs on this computer, only when the rules aren't sure.".to_string(), false),
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(chips)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(if warn { ui::AMBER } else { ui::muted() }))
                    .child(line),
            )
    }

    pub(super) fn engines(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppSettings>().clone();
        let ready: Vec<usize> = (0..agent::PROVIDERS.len())
            .filter(|&i| engine::ready(&st, i))
            .collect();
        let default_i = engine::index_of(&st, "");

        let brains = self.bot_brains(&st, &ready, default_i, cx);

        // Squad Code's own thinking: shaping jobs, and a second opinion.
        let brain_i = crate::settings::assistant_provider(&st);
        let mut brain_agents = div().flex().flex_wrap().gap(px(6.));
        for &j in &ready {
            let id = agent::PROVIDERS[j].id;
            brain_agents = brain_agents.child(
                choice(("brain", j), agent::PROVIDERS[j].name, j == brain_i, true).on_click(
                    cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            if st.assistant_agent != id {
                                st.assistant_model.clear();
                            }
                            st.assistant_agent = id.to_string();
                        });
                        cx.notify();
                    }),
                ),
            );
        }
        let mut brain_models = div().flex().flex_wrap().gap(px(6.));
        for (k, m) in AgentPick::models(brain_i).into_iter().take(7).enumerate() {
            let value = m.value.clone();
            let label = if m.value.is_empty() && agent::PROVIDERS[brain_i].id == "claude" {
                "Fast default · Haiku".to_string()
            } else {
                m.label.clone()
            };
            brain_models = brain_models.child(
                choice(
                    ("brain-model", k),
                    label,
                    st.assistant_model == m.value,
                    true,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    let value = value.clone();
                    save(cx, |st| st.assistant_model = value);
                    cx.notify();
                })),
            );
        }
        let mut challengers = div().flex().flex_wrap().gap(px(6.));
        challengers = challengers.child(
            choice("challenge-off", "Off", st.challenge_agent.is_empty(), true).on_click(
                cx.listener(|_, _, _, cx| {
                    save(cx, |st| st.challenge_agent.clear());
                    cx.notify();
                }),
            ),
        );
        for &j in &ready {
            let id = agent::PROVIDERS[j].id;
            challengers = challengers.child(
                choice(
                    ("challenge", j),
                    agent::PROVIDERS[j].name,
                    st.challenge_agent == id,
                    true,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    save(cx, |st| st.challenge_agent = id.to_string());
                    cx.notify();
                })),
            );
        }
        let smart = self.smart_peek(&st, cx);
        let work_vendor = vendor(agent::PROVIDERS[default_i].id);
        let same_vendor =
            !st.challenge_agent.is_empty() && vendor(&st.challenge_agent) == work_vendor;
        let brain = ui::card()
            .p(px(space::L))
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(
                div()
                    .flex()
                    .gap(px(space::M))
                    .child(div().w(px(200.)).flex_none().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child("Shapes jobs, writes the one question"))
                    .child(div().flex_1().flex().flex_col().gap(px(8.)).child(brain_agents).child(div().flex().items_center().gap(px(8.)).child(ui::caption("MODEL")).child(brain_models))),
            )
            .child(div().h(px(1.)).bg(c(ui::line())))
            .child(
                div()
                    .flex()
                    .gap(px(space::M))
                    .child(div().w(px(200.)).flex_none().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child("Double-checks decisions"))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(challengers)
                            .child(div().text_size(px(12.)).text_color(c(if same_vendor { ui::AMBER } else { ui::muted() })).child(if same_vendor {
                                format!("Same company as the work ({work_vendor}). One from another company catches more.")
                            } else {
                                "One vendor writes, another checks. Pick an agent from a different company than the default.".to_string()
                            })),
                    ),
            )
            .child(div().h(px(1.)).bg(c(ui::line())))
            .child(
                div()
                    .flex()
                    .gap(px(space::M))
                    .child(div().w(px(200.)).flex_none().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child("Smart Peek"))
                    .child(smart),
            );

        let none_ready = ready.is_empty();
        let setup = self.setup_runs(cx);
        let engines = self.engine_rows(&st, &ready, default_i, cx);
        let all = self.engines_all;
        let label = |text: &'static str, right: Option<Stateful<Div>>| {
            div()
                .flex()
                .items_center()
                .mb(px(-8.))
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(ui::faint()))
                        .child(text),
                )
                .child(div().flex_1())
                .children(right)
        };
        let link = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .text_color(c(ui::body()))
                .cursor_pointer()
                .hover(|s| s.text_color(c(ui::ink())))
                .child(text)
        };
        div()
            .id("engines")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(820.))
                    .px(px(36.))
                    .pt(px(26.))
                    .pb(px(space::XL + super::DOCK_ROOM))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Engines"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(13.))
                                    .text_color(c(ui::body()))
                                    .child("Every bot works through an agent on this computer. Add and update engines, or pick each bot's brain."),
                            ),
                    )
                    .child(label(
                        "ENGINES ON THIS COMPUTER",
                        Some(
                            link(
                                "eng-add",
                                if all { "Show fewer" } else { "Add an engine +" },
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.engines_all = !all;
                                cx.notify();
                            })),
                        ),
                    ))
                    .children(setup)
                    .when(none_ready, |d| {
                        d.child(
                            div()
                                .text_size(px(13.))
                                .text_color(c(ui::t_amber()))
                                .child("None of the agents' command-line tools were found. Install one (for example Claude Code, Codex or OpenCode) and it shows up here."),
                        )
                    })
                    .child(engines)
                    .child(label(
                        "BOT BRAINS",
                        Some(link("eng-brains-note", "applies to the next job")),
                    ))
                    .child(brains)
                    .child(label("SQUAD CODE'S OWN THINKING", None))
                    .child(brain),
            )
            .into_any_element()
    }

    /// The engines card: each installed engine with its default mark, what it
    /// is and Update; the common ones not installed (Claude Code, Codex,
    /// OpenCode, Antigravity, Gemini), greyed with Install; every other one
    /// after "Add an engine". A click on an installed one opens its detail.
    fn engine_rows(
        &mut self,
        st: &AppSettings,
        ready: &[usize],
        default_i: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        const COMMON: [&str; 5] = ["claude", "codex", "opencode", "antigravity", "gemini"];
        let mut order: Vec<usize> = ready.to_vec();
        for (i, p) in agent::PROVIDERS.iter().enumerate() {
            if !order.contains(&i) && (self.engines_all || COMMON.contains(&p.id)) {
                order.push(i);
            }
        }
        let open = self.engine_sel;
        let mut card = div()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .px(px(14.))
            .flex()
            .flex_col();
        for (n, i) in order.into_iter().enumerate() {
            let p = &agent::PROVIDERS[i];
            let id = p.id;
            let ok = ready.contains(&i);
            if ok && open == Some(i) {
                let detail = self.engine_detail(st, ready, default_i, i, cx);
                card = card.child(
                    div()
                        .py(px(10.))
                        .when(n > 0, |d| d.border_t_1().border_color(c(ui::line())))
                        .child(detail),
                );
                continue;
            }
            let models: Vec<String> = AgentPick::models(i)
                .into_iter()
                .filter(|m| !m.value.is_empty())
                .map(|m| m.label)
                .take(3)
                .collect();
            let meta = if ok {
                let mut bits = vec![vendor(id).to_string()];
                if !models.is_empty() {
                    bits.push(models.join(", "));
                }
                bits.join(" \u{b7} ")
            } else {
                "Not on this computer".to_string()
            };
            let mut row = div()
                .id(("eng-row", i))
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .when(n > 0, |d| d.border_t_1().border_color(c(ui::line())))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c(if ok { ui::ink() } else { ui::body() }))
                                        .child(p.name),
                                )
                                .when(ok && i == default_i, |d| {
                                    d.child(
                                        div()
                                            .px(px(6.))
                                            .rounded(px(3.))
                                            .bg(c(ui::pick_fill()))
                                            .text_color(c(ui::pick_on()))
                                            .font_family(ui::MONO)
                                            .text_size(px(10.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .line_height(px(17.))
                                            .child("DEFAULT"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .truncate()
                                .text_size(px(12.))
                                .text_color(c(ui::faint()))
                                .child(meta),
                        ),
                );
            if ok {
                row = row
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.engine_sel = Some(i);
                        cx.notify();
                    }));
                if i != default_i {
                    row = row.child(
                        ui::dbtn(("eng-default", i), "Make default", ui::Btn::Ghost).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                save(cx, |st| st.default_provider = id.to_string());
                                this.say(format!(
                                    "New jobs now run on {} unless a bot has its own engine.",
                                    agent::provider(id).name
                                ));
                                cx.notify();
                            }),
                        ),
                    );
                }
                if let Some(cmd) = update_cmd(id) {
                    row = row.child(
                        ui::dbtn(("eng-update", i), "Update", ui::Btn::Plain).on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.run_setup(&cmd, cx)
                            }),
                        ),
                    );
                }
            } else if let Some(cmd) = install_cmd(id) {
                row = row.child(
                    ui::dbtn(("eng-install", i), "Install", ui::Btn::Plain).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.engine_sel = Some(i);
                            this.run_setup(&cmd, cx)
                        },
                    )),
                );
            }
            card = card.child(row);
        }
        card
    }

    /// Bot brains: every bot on one row with the engine and model it thinks
    /// with, each switched in one click, and one row to set them all at once.
    fn bot_brains(
        &mut self,
        st: &AppSettings,
        ready: &[usize],
        default_i: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        // The engines worth offering: the installed ones, plus any a bot is on.
        let mut offer: Vec<usize> = ready.to_vec();
        if !offer.contains(&default_i) {
            offer.insert(0, default_i);
        }
        let crew = Member::all();
        for who in &crew {
            let (i, _) = engine::engine(st, *who);
            if !offer.contains(&i) {
                offer.push(i);
            }
        }
        let everyone_on = |i: usize| {
            crew.iter().all(|w| {
                let (k, m) = engine::engine(st, *w);
                k == i && m.is_empty()
            })
        };

        // Set every bot to one engine: it becomes the default, and every bot follows it.
        let mut all = div().flex().flex_wrap().items_center().gap(px(6.));
        for &i in &offer {
            let p = &agent::PROVIDERS[i];
            let id = p.id;
            all = all.child(
                choice(
                    ("brains-all", i),
                    p.name,
                    everyone_on(i),
                    engine::ready(st, i),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    save(cx, |st| {
                        st.default_provider = id.to_string();
                        st.bot_engines.clear();
                    });
                    cx.notify();
                })),
            );
        }

        let mut rows = div().flex().flex_col();
        for who in crew {
            let (cur, model) = engine::engine(st, who);
            let own = st.bot_engines.contains_key(&engine::bot_key(who));
            let face = self.avatar(who, 26.);
            let mut engines_row = div().flex().flex_wrap().gap(px(5.));
            for &i in &offer {
                let id = agent::PROVIDERS[i].id;
                engines_row = engines_row.child(
                    choice(
                        (
                            SharedString::from(format!("brain-{}", engine::bot_key(who))),
                            i,
                        ),
                        agent::PROVIDERS[i].name,
                        i == cur,
                        engine::ready(st, i),
                    )
                    .h(px(24.))
                    .px(px(9.))
                    .text_size(px(12.))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            engine::set(
                                st,
                                who,
                                BotEngine {
                                    agent: id.to_string(),
                                    model: String::new(),
                                },
                            )
                        });
                        cx.notify();
                    })),
                );
            }
            let mut models_row = div().flex().flex_wrap().gap(px(5.));
            let cur_id = agent::PROVIDERS[cur].id;
            for (k, m) in AgentPick::models(cur).into_iter().take(5).enumerate() {
                let value = m.value.clone();
                let on = model == m.value;
                models_row = models_row.child(
                    div()
                        .id((
                            SharedString::from(format!("brain-model-{}", engine::bot_key(who))),
                            k,
                        ))
                        .h(px(22.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .map(|d| {
                            if on {
                                d.border_color(c(ui::pick()))
                                    .bg(c(super::theme::mix(
                                        ui::raised(),
                                        ui::pick_fill(),
                                        ui::pick_k(0.1),
                                    )))
                                    .text_color(c(ui::t_violet()))
                                    .font_weight(FontWeight::MEDIUM)
                            } else {
                                d.border_color(c(ui::line()))
                                    .text_color(c(ui::muted()))
                                    .hover(|s| s.text_color(c(ui::ink())).bg(c(ui::hover())))
                            }
                        })
                        .child(m.label.clone())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let value = value.clone();
                            save(cx, |st| {
                                engine::set(
                                    st,
                                    who,
                                    BotEngine {
                                        agent: cur_id.to_string(),
                                        model: value,
                                    },
                                )
                            });
                            cx.notify();
                        })),
                );
            }
            rows = rows.child(
                div()
                    .px(px(18.))
                    .py(px(11.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .border_t_1()
                    .border_color(c(ui::line2()))
                    .child(div().pt(px(2.)).flex_none().child(face))
                    .child(
                        div()
                            .w(px(130.))
                            .flex_none()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(who.name()),
                            )
                            .child(div().text_size(px(11.5)).text_color(c(ui::faint())).child(
                                if offer.len() == 1 {
                                    agent::PROVIDERS[cur].name
                                } else if own {
                                    "its own pick"
                                } else {
                                    "follows the default"
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .when(offer.len() > 1, |d| d.child(engines_row))
                            .child(models_row),
                    )
                    .when(own, |d| {
                        d.child(
                            div()
                                .id((
                                    SharedString::from(format!(
                                        "brain-reset-{}",
                                        engine::bot_key(who)
                                    )),
                                    0usize,
                                ))
                                .pt(px(4.))
                                .text_size(px(12.))
                                .text_color(c(ui::muted()))
                                .cursor_pointer()
                                .hover(|s| s.text_color(c(ui::ink())))
                                .child("Use default")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    save(cx, |st| engine::set(st, who, BotEngine::default()));
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }

        ui::card()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(18.))
                    .pt(px(16.))
                    .pb(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c(ui::muted()))
                                    .child("Set every bot to"),
                            )
                            .child(all),
                    ),
            )
            .child(rows)
    }

    /// One agent: whether it's here, how to get it, and the bots on it.
    fn engine_detail(
        &mut self,
        st: &AppSettings,
        ready: &[usize],
        default_i: usize,
        sel: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = &agent::PROVIDERS[sel];
        let ok = ready.contains(&sel);
        let id = p.id;
        let spent: f64 = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.agent == id && j.started / 86_400 == super::store::now() / 86_400)
            .filter_map(|j| super::home::cost(&j.summary))
            .sum::<f64>()
            + 0.0;
        let jobs = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.agent == id)
            .count();
        let mut head =
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .rounded(px(12.))
                        .bg(ca(p.color, 0.18))
                        .text_color(c(p.color))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(18.))
                        .font_weight(FontWeight::BOLD)
                        .child(p.mark),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(17.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(p.name),
                        )
                        .child(div().text_size(px(12.5)).text_color(c(ui::muted())).child(
                            format!(
                                "{} \u{b7} {} \u{b7} {jobs} jobs in all \u{b7} ${spent:.2} today",
                                vendor(id),
                                if ok { "ready" } else { "not installed" }
                            ),
                        )),
                );
        if ok && sel != default_i {
            head = head.child(
                ui::solid("engine-default", "Make default").on_click(cx.listener(
                    move |this, _, _, cx| {
                        save(cx, |st| st.default_provider = id.to_string());
                        this.say(format!(
                            "New jobs now run on {} unless a bot has its own engine.",
                            agent::provider(id).name
                        ));
                        cx.notify();
                    },
                )),
            );
        } else if ok {
            head = head.child(ui::status(ui::GREEN, "Default for new jobs"));
        }
        if let Some(cmd) = update_cmd(id).filter(|_| ok) {
            head = head.child(
                ui::ghost("engine-update", "Update")
                    .on_click(cx.listener(move |this, _, _, cx| this.run_setup(&cmd, cx))),
            );
        }
        let head = head.child(
            ui::icon_btn("engine-close", gpui_kit::assets::IconName::X).on_click(cx.listener(
                |this, _, _, cx| {
                    this.engine_sel = None;
                    cx.notify();
                },
            )),
        );
        let mut body = div()
            .flex_1()
            .min_w_0()
            .py(px(6.))
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(head);
        if !ok {
            let install = install_cmd(id);
            body = body.child(
                div()
                    .text_size(px(13.))
                    .text_color(c(ui::body()))
                    .child(format!("{} isn't on this computer. Install its command-line tool, sign in once, and it shows as ready here.", p.name)),
            );
            if let Some(cmd) = install {
                let copy = cmd.clone();
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(px(8.))
                                .bg(c(ui::bg()))
                                .border_1()
                                .border_color(c(ui::line()))
                                .font_family(ui::MONO)
                                .text_size(px(12.))
                                .truncate()
                                .child(cmd),
                        )
                        .child({
                            let run = copy.clone();
                            ui::solid("engine-install", "Install").on_click(
                                cx.listener(move |this, _, _, cx| this.run_setup(&run, cx)),
                            )
                        })
                        .child(ui::ghost("engine-copy", "Copy").on_click(cx.listener(
                            move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                this.say("Copied.");
                                cx.notify();
                            },
                        ))),
                );
            }
            return body;
        }
        // What the engine is: where its tool lives and the models it offers.
        let bin = st.agent(id).binary.clone();
        let tool = if bin.is_empty() {
            p.bin.to_string()
        } else {
            bin
        };
        let mut models = div().flex().flex_wrap().gap(px(6.));
        let mut seen = std::collections::HashSet::new();
        for m in AgentPick::models(sel)
            .into_iter()
            .filter(|m| !m.value.is_empty())
            .filter(|m| seen.insert(m.label.clone()))
        {
            models = models.child(
                div()
                    .px(px(9.))
                    .py(px(4.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(c(ui::line()))
                    .text_size(px(12.))
                    .text_color(c(ui::body()))
                    .child(m.label.clone()),
            );
        }
        body = body
            .child(ui::caption("COMMAND"))
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(c(ui::bg()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .child(tool),
            )
            .child(ui::caption("MODELS"))
            .child(models);
        body
    }
}
