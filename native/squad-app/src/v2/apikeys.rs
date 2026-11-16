//! API keys on the Engines page: the keys the person added, adding one (pasted, checked
//! with its provider straight away), and choosing how an agent or a bot signs in.
//! The secrets themselves are in the keychain; see `crate::keys`.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::keys::{self, ApiKey, Status};
use crate::settings::AppSettings;

use super::data::Member;
use super::engine;
use super::engines::choice;
use super::ui::{self, c};
use super::Shell;

fn save(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    cx.update_global::<AppSettings, _>(|st, _| {
        f(st);
        st.save();
    });
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The colour of a key's status dot.
fn dot(status: Status) -> u32 {
    match status {
        Status::Works => ui::GREEN,
        Status::Refused => ui::RED,
        Status::Offline | Status::Unchecked => ui::AMBER,
    }
}

/// A one-line field with a small label, like the bot maker's.
fn field(label: &'static str, input: &Entity<InputState>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(36.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(10.))
        .bg(c(ui::well()))
        .border_1()
        .border_color(c(ui::line2()))
        .child(
            div()
                .flex_none()
                .font_family(ui::mono())
                .text_size(px(9.5))
                .text_color(c(ui::faint()))
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .child(Input::new(input).appearance(false).bordered(false)),
        )
}

/// Who uses the key `choice`: agents set to it, then bots given it.
pub(super) fn users(st: &AppSettings, choice: &str) -> Vec<String> {
    let mut out: Vec<String> = crate::agent::PROVIDERS
        .iter()
        .filter(|p| st.agent(p.id).key == choice)
        .map(|p| p.name.to_string())
        .collect();
    out.extend(
        Member::all()
            .into_iter()
            .filter(|w| engine::own_key(st, *w) == choice)
            .map(|w| w.name().to_string()),
    );
    out
}

impl Shell {
    /// Opens the add-a-key form, on `provider` when given.
    pub(super) fn open_key_form(&mut self, provider: &'static str, cx: &mut Context<Self>) {
        self.key_form = Some(provider);
        cx.notify();
    }

    /// Saves the pasted key to the keychain and checks it with its provider.
    fn add_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.key_form else {
            return;
        };
        let secret = self.key_secret.read(cx).value().trim().to_string();
        if secret.is_empty() {
            self.say("Paste the key first.");
            cx.notify();
            return;
        }
        let label = self.key_label.read(cx).value().trim().to_string();
        let id = keys::new_id();
        let in_file = match keys::store(&id, &secret) {
            Ok(in_file) => in_file,
            Err(e) => {
                self.say(e);
                cx.notify();
                return;
            }
        };
        let key = ApiKey {
            id: id.clone(),
            provider: provider.to_string(),
            label,
            last4: keys::last4(&secret),
            added: now(),
            in_file,
            note: "Checking with the provider\u{2026}".into(),
            ..ApiKey::default()
        };
        // Adding a key changes no engine's sign-in: a login may be billed differently,
        // so the person picks where the key is used.
        let name = keys::provider(provider).map_or("", |p| p.name);
        save(cx, |st| st.api_keys.push(key));
        self.say(format!(
            "Saved your {name} key. Pick it under an engine's Signs in with, or for one bot in Bot brains."
        ));
        self.key_secret
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.key_label
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.key_form = None;
        self.check_key(id, cx);
        cx.notify();
    }

    /// Asks the key's provider whether it works, in the background.
    pub(super) fn check_key(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(key) = cx
            .global::<AppSettings>()
            .api_keys
            .iter()
            .find(|k| k.id == id)
            .cloned()
        else {
            return;
        };
        if self.key_checking.contains(&id) {
            return;
        }
        self.key_checking.push(id.clone());
        let task = cx.background_spawn(async move {
            match keys::secret(&key.id) {
                Some(secret) => keys::check(&key.provider, &secret),
                None => keys::Check {
                    status: Status::Refused,
                    note: "The keychain no longer has this key. Remove it and add it again.".into(),
                    models: Vec::new(),
                },
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.key_checking.retain(|k| *k != id);
                save(cx, |st| {
                    if let Some(k) = st.api_keys.iter_mut().find(|k| k.id == id) {
                        k.status = result.status;
                        k.note = result.note;
                        if !result.models.is_empty() {
                            k.models = result.models;
                        }
                        k.checked = now();
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Removes a key: from the keychain, and from every agent and bot that used it.
    fn remove_key(&mut self, id: String, cx: &mut Context<Self>) {
        let st = cx.global::<AppSettings>();
        let Some(key) = st.api_keys.iter().find(|k| k.id == id).cloned() else {
            return;
        };
        let choice = key.choice();
        let moved = users(st, &choice).len();
        keys::forget(&id);
        save(cx, |st| {
            st.api_keys.retain(|k| k.id != id);
            for a in st.agents.values_mut() {
                if a.key == choice {
                    a.key.clear();
                }
            }
            for e in st.bot_engines.values_mut() {
                if e.key == choice {
                    e.key.clear();
                }
            }
            st.bot_engines.retain(|_, e| *e != Default::default());
        });
        self.say(match moved {
            0 => "Removed the key.".to_string(),
            n => format!("Removed the key. {n} that used it went back to signing in as before."),
        });
        cx.notify();
    }

    /// The keys card: every key with its status and who uses it, and the add form.
    pub(super) fn keys_card(&mut self, st: &AppSettings, cx: &mut Context<Self>) -> Div {
        let mut card = div()
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()))
            .px(px(14.))
            .flex()
            .flex_col();
        let mut n = 0usize;
        for key in &st.api_keys {
            let id = key.id.clone();
            let checking = self.key_checking.contains(&key.id);
            let who = users(st, &key.choice());
            let mut meta = vec![if checking {
                "Checking with the provider\u{2026}".to_string()
            } else {
                key.note.clone()
            }];
            if !who.is_empty() {
                meta.push(format!("Used by {}", who.join(", ")));
            }
            if key.in_file {
                meta.push("Kept in a private file: no keychain was found".into());
            }
            let (recheck, remove) = (id.clone(), id.clone());
            card = card.child(
                div()
                    .id(("key-row", n))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(n > 0, |d| d.border_t_1().border_color(c(ui::line())))
                    .child(
                        div()
                            .size(px(8.))
                            .flex_none()
                            .rounded_full()
                            .bg(c(if checking { ui::AMBER } else { dot(key.status) })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(key.title()),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(12.))
                                    .text_color(c(if key.status == Status::Refused {
                                        ui::t_amber()
                                    } else {
                                        ui::faint()
                                    }))
                                    .child(meta.join(" \u{b7} ")),
                            ),
                    )
                    .child(
                        ui::dbtn(("key-recheck", n), "Recheck", ui::Btn::Plain).on_click(
                            cx.listener(move |this, _, _, cx| this.check_key(recheck.clone(), cx)),
                        ),
                    )
                    .child(
                        ui::dbtn(("key-remove", n), "Remove", ui::Btn::Ghost).on_click(
                            cx.listener(move |this, _, _, cx| this.remove_key(remove.clone(), cx)),
                        ),
                    ),
            );
            n += 1;
        }
        if n == 0 && self.key_form.is_none() {
            card = card.child(
                div()
                    .py(px(12.))
                    .text_size(px(12.5))
                    .text_color(c(ui::muted()))
                    .child("No keys yet. Engines sign in with their own login. Add a key from Anthropic, OpenAI, Google, xAI or an agent's own service to run on your account's key instead."),
            );
        }
        if let Some(provider) = self.key_form {
            card = card.child(self.key_form_view(provider, n > 0, cx));
        }
        card
    }

    fn key_form_view(&mut self, sel: &'static str, line: bool, cx: &mut Context<Self>) -> Div {
        let mut providers = div().flex().flex_wrap().gap(px(6.));
        for (k, p) in keys::PROVIDERS.iter().enumerate() {
            let id = p.id;
            providers = providers.child(
                choice(("key-provider", k), p.name, sel == id, true)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_key_form(id, cx))),
            );
        }
        let p = keys::provider(sel).unwrap_or(&keys::PROVIDERS[0]);
        let agents: Vec<&str> = p
            .agents
            .iter()
            .map(|a| crate::agent::provider(a).name)
            .collect();
        let page = p.page;
        div()
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .when(line, |d| d.border_t_1().border_color(c(ui::line())))
            .child(ui::caption("ADD A KEY FROM"))
            .child(providers)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(field("KEY", &self.key_secret))
                    .child(div().w(px(200.)).flex_none().flex().child(field("NAME", &self.key_label))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(format!(
                                "Runs {} on your {} account. Looks like {}. Kept in this computer's keychain and given only to the runs that use it.",
                                agents.join(", "),
                                p.name,
                                p.hint
                            )),
                    )
                    .child(ui::ghost("key-page", "Get a key").on_click(move |_, _, cx| {
                        cx.open_url(page);
                    }))
                    .child(ui::ghost("key-cancel", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.key_form = None;
                            cx.notify();
                        },
                    )))
                    .child(
                        ui::solid("key-save", "Save and check")
                            .on_click(cx.listener(|this, _, window, cx| this.add_key(window, cx))),
                    ),
            )
    }

    /// How agent `agent` signs in: as before, its own login, or one of the keys it takes.
    pub(super) fn sign_in_row(
        &mut self,
        st: &AppSettings,
        agent: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let cur = st.agent(agent).key;
        let set = move |value: String| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let value = value.clone();
                save(cx, |st| {
                    let mut a = st.agent(agent);
                    a.key = value;
                    st.agents.insert(agent.to_string(), a);
                });
            }
        };
        let mut row = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                choice("signin-auto", "As it signs in now", cur.is_empty(), true)
                    .on_click(set(String::new())),
            )
            .child(
                choice("signin-login", "Its own login", cur == keys::LOGIN, true)
                    .on_click(set(keys::LOGIN.into())),
            );
        for (k, key) in st
            .api_keys
            .iter()
            .filter(|k| keys::provider(&k.provider).is_some_and(|p| p.agents.contains(&agent)))
            .enumerate()
        {
            let value = key.choice();
            row = row.child(
                choice(
                    ("signin-key", k),
                    key.title(),
                    cur == value,
                    key.status != Status::Refused,
                )
                .on_click(set(value)),
            );
        }
        let first = keys::providers_for(agent).next().map(|p| p.id);
        if let Some(first) = first {
            row = row.child(
                choice("signin-add", "Add a key +", false, true)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_key_form(first, cx))),
            );
        }
        let note = if cur == keys::LOGIN {
            "Key variables from your shell are left out of its runs, so only the login is billed."
        } else if keys::uses_key(&cur) {
            "Runs use this key, billed to that provider account. Bots can still pick their own."
        } else {
            "Whatever it uses today: its login, or a key set in your shell."
        };
        div().flex().flex_col().gap(px(6.)).child(row).child(
            div()
                .text_size(px(12.))
                .text_color(c(ui::muted()))
                .child(note),
        )
    }

    /// For one bot, the keys its agent can run on: chips to give it one, or follow the
    /// agent's sign-in. None when there are no such keys.
    pub(super) fn bot_key_row(
        &mut self,
        st: &AppSettings,
        who: Member,
        agent: &'static str,
    ) -> Option<Div> {
        let fits: Vec<&ApiKey> = st
            .api_keys
            .iter()
            .filter(|k| keys::provider(&k.provider).is_some_and(|p| p.agents.contains(&agent)))
            .collect();
        if fits.is_empty() {
            return None;
        }
        let own = engine::own_key(st, who);
        let bk = engine::bot_key(who);
        let set = move |value: String| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let value = value.clone();
                save(cx, |st| {
                    let mut e = st
                        .bot_engines
                        .get(&engine::bot_key(who))
                        .cloned()
                        .unwrap_or_default();
                    e.key = value;
                    engine::set(st, who, e);
                });
            }
        };
        let small = |d: Stateful<Div>| d.h(px(22.)).px(px(8.)).text_size(px(11.5));
        let mut row = div().flex().flex_wrap().items_center().gap(px(5.)).child(
            small(choice(
                (SharedString::from(format!("bot-key-{bk}")), 0usize),
                "Agent's sign-in",
                own.is_empty(),
                true,
            ))
            .on_click(set(String::new())),
        );
        for (k, key) in fits.into_iter().enumerate() {
            let value = key.choice();
            row = row.child(
                small(choice(
                    (SharedString::from(format!("bot-key-{bk}")), k + 1),
                    key.title(),
                    own == value,
                    key.status != Status::Refused,
                ))
                .on_click(set(value)),
            );
        }
        Some(row)
    }
}
