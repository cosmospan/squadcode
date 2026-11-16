//! A bot asking to do something it isn't allowed on its own: one card in its
//! job, with Allow once, Always allow (saved for this project) and Deny, and a
//! box to tell it why. Several asks from one job stack in the same card.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::approvals::{self, Request};
use crate::settings::AppSettings;

use super::store::Job;
use super::ui::{self, c};
use super::Shell;

impl Shell {
    pub(super) fn ask_card(&mut self, j: &Job, cx: &mut Context<Self>) -> Option<Div> {
        let asks = self.crew.asks_for(j);
        if asks.is_empty() {
            return None;
        }
        let tone = ui::ask_tone();
        let mut card = div()
            .mx(px(14.))
            .mb(px(10.))
            .rounded(px(12.))
            .bg(c(tone.1))
            .border_1()
            .border_color(c(tone.2))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .pt(px(10.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(
                        Icon::new(IconName::Hand)
                            .size(px(14.))
                            .text_color(c(tone.0)),
                    )
                    .child(if asks.len() == 1 {
                        format!("{} needs your OK", j.bot.name())
                    } else {
                        format!("{} needs your OK for {} things", j.bot.name(), asks.len())
                    }),
            );
        for (n, r) in asks.into_iter().enumerate() {
            card = card.child(self.ask_row(j, n, r, cx));
        }
        Some(card)
    }

    fn ask_row(&mut self, j: &Job, n: usize, r: Request, cx: &mut Context<Self>) -> Div {
        let what = approvals::describe(&r.tool, &r.input);
        let full = approvals::full_text(&r.tool, &r.input).unwrap_or_else(|| what.clone());
        let rule = approvals::always_rule(&r.tool, &r.input);
        let key = (j.id as usize) * 16 + n;
        let denying = self.denying.as_deref() == Some(r.id.as_str());
        let folder = j.folder.path.clone();
        let agent = j.agent.clone();
        let (once, always, deny, send) = (r.id.clone(), r.id.clone(), r.id.clone(), r.id.clone());
        let mut row = div()
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(n > 0, |d| d.border_t_1().border_color(c(ui::ask_tone().2)))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(c(ui::body()))
                    .child(super::store::plain_step(&format!("Ran {what}"), true)),
            )
            .child(
                div()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(c(ui::panel()))
                    .border_1()
                    .border_color(c(ui::line2()))
                    .font_family(ui::mono())
                    .text_size(px(12.))
                    .text_color(c(ui::ink()))
                    .max_h(px(120.))
                    .overflow_hidden()
                    .child(full),
            );
        if denying {
            row =
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .child(Input::new(&self.deny_input)),
                        )
                        .child(ui::solid(("ask-send-no", key), "Deny").h(px(28.)).on_click(
                            cx.listener(move |this, _, window, cx| {
                                let why = this.deny_input.read(cx).value().to_string();
                                this.deny_input
                                    .update(cx, |i, cx| i.set_value("", window, cx));
                                this.crew.answer_ask(&send, false, &why);
                                this.denying = None;
                                cx.notify();
                            }),
                        ))
                        .child(ui::ghost(("ask-back", key), "Back").h(px(28.)).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.denying = None;
                                cx.notify();
                            }),
                        )),
                );
            return row;
        }
        row.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    ui::solid(("ask-once", key), "Allow once")
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.answer_ask(&once, true, "");
                            cx.notify();
                        })),
                )
                .when_some(rule, |d, rule| {
                    let tip = format!("Saves {rule} for this project, so it won't ask again.");
                    d.child(
                        ui::ghost(("ask-always", key), "Always allow")
                            .h(px(28.))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                save_rule(cx, &agent, &folder, &rule);
                                this.crew.answer_ask(&always, true, "");
                                this.say(format!("Saved. {rule} won't ask again in this project."));
                                cx.notify();
                            })),
                    )
                })
                .child(div().flex_1())
                .child(
                    ui::ghost(("ask-deny", key), "Deny")
                        .h(px(28.))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.denying = Some(deny.clone());
                            this.deny_input.update(cx, |i, cx| i.focus(window, cx));
                            cx.notify();
                        })),
                ),
        )
    }
}

/// Keeps an "Always allow" rule for one project folder and engine.
fn save_rule(cx: &mut App, agent: &str, folder: &str, rule: &str) {
    if approvals::is_bare_bash(rule) {
        return;
    }
    cx.update_global::<AppSettings, _>(|st, _| {
        let entry = st.agents.entry(agent.to_string()).or_default();
        let saved = entry.project_tools.entry(folder.to_string()).or_default();
        let mut list: Vec<String> = saved
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if !list.iter().any(|r| r == rule) {
            list.push(rule.to_string());
        }
        *saved = list.join(",");
        st.save();
    });
}
