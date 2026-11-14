//! Make a bot, and any bot's Setup: the person says what a bot is for, their
//! engine reads the project and drafts the rest, and they review it here.
//!
//! The same editor opens for a bot already on the crew, built-in or made, so
//! its brain, instructions, skills, connected apps, permissions and memory
//! can be changed any time.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::fin::{Fin, Line, Outfit};
use squad_holo::{Framing, Mood, Tint};

use crate::agent;
use crate::agent_pick::AgentPick;
use crate::settings::{AppSettings, BotEngine};

use super::brain;
use super::data::Member;
use super::engine;
use super::mascot;
use super::setup::{self, BotServer, Perms, Setup, SkillRef};
use super::ui::{self, c, ca};
use super::Shell;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;

/// Where the maker is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// Saying what the bot is for.
    #[default]
    Describe,
    /// The engine is drafting the bot.
    Drafting,
    /// Looking over the draft, or a bot's setup.
    Review,
}

/// The bot being set up, beyond what's in the text boxes.
#[derive(Clone, Debug)]
pub struct Editing {
    /// The bot on the crew, or None for a new one.
    pub who: Option<Member>,
    /// The made id a new bot gets (its skill files are written under it).
    pub id: u16,
    pub like: Member,
    pub tint: Tint,
    pub engine: BotEngine,
    /// Skill files, connected apps, permissions; the words are in the boxes.
    pub setup: Setup,
    /// The project folder it's tied to, or empty for every project.
    pub project: String,
    /// Something to fix before it can be saved, or a skill that didn't come back.
    pub error: Option<String>,
}

impl Editing {
    /// The bot this edits: itself, or the made bot it will be.
    fn member(&self) -> Member {
        self.who.unwrap_or(Member::Made(self.id))
    }

    /// A built-in bot: its name, job and kind of work are fixed.
    fn built_in(&self) -> bool {
        self.who.is_some_and(|w| !w.is_made())
    }
}

/// Starting points for a new bot: a title, one line, and the sentence it puts
/// in the box.
const STARTERS: [(&str, &str, &str); 7] = [
    (
        "Docs keeper",
        "Keeps README, docs and changelog in step with the code",
        "Keep the README, docs and changelog in step with the code. After a change, find what it made stale and fix the docs. Never change code.",
    ),
    (
        "Integration owner",
        "Owns one service (Stripe, Auth, S3) end to end",
        "Own our Stripe integration: fix webhook and billing bugs, keep the payment tests green, and never touch live keys.",
    ),
    (
        "Security reviewer",
        "Reads every change for auth, secrets and data risks",
        "Review changes for security risks: auth, secrets, injection and personal data. Change nothing; report the riskiest thing first with file and line.",
    ),
    (
        "Test writer",
        "Finds untested code and covers it",
        "Find code with no tests and write good ones for it, matching how the project already tests. Never skip or loosen a test to make it pass.",
    ),
    (
        "Release manager",
        "Changelog, version bumps, release notes",
        "Prepare releases: update the changelog from merged work, bump versions, and write release notes in plain words. Ask before tagging or publishing.",
    ),
    (
        "Dependency janitor",
        "Upgrades packages in small, tested steps",
        "Keep dependencies current: upgrade one package at a time, run the tests after each, and explain anything that breaks.",
    ),
    (
        "Performance hunter",
        "Finds what's slow and proves the fix",
        "Find what makes the app slow, measure it before and after, and fix it without changing behaviour.",
    ),
];

/// A sentence for looking at the maker with something in it.
pub const SAMPLE: &str = STARTERS[1].2;

/// Text boxes split into a list: by commas, or one a line.
fn by_comma(text: &str) -> Vec<String> {
    text.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn by_line(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['-', '*', '•'])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

fn save_settings(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    cx.update_global::<AppSettings, _>(|st, _| {
        f(st);
        st.save();
    });
}

impl Shell {
    /// Opens the maker for a new bot, with `about` in the box when there's a
    /// job it's for.
    pub(super) fn open_maker(
        &mut self,
        about: Option<&str>,
        like: Option<Member>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.crew.cancel_draft();
        self.maker_like = like;
        self.maker_here = false;
        self.maker_edit = None;
        self.maker_stage = Stage::Describe;
        self.clear_maker(window, cx);
        if let Some(about) = about {
            self.maker_about
                .update(cx, |i, cx| ui::put_text(i, about, window, cx));
        }
        self.find_on_computer();
        self.maker_open = true;
        cx.notify();
    }

    /// Opens `who`'s setup: everything it can do, ready to change.
    pub(super) fn open_setup(&mut self, who: Member, window: &mut Window, cx: &mut Context<Self>) {
        self.crew.cancel_draft();
        self.clear_maker(window, cx);
        self.find_on_computer();
        let st = cx.global::<AppSettings>();
        let engine = st
            .bot_engines
            .get(&engine::bot_key(who))
            .cloned()
            .unwrap_or_default();
        let made = match who {
            Member::Made(id) => self.crew.store.made.iter().find(|m| m.id == id).cloned(),
            _ => None,
        };
        let tint = made
            .as_ref()
            .map_or(Tint::VIOLET, |m| mascot::colour_at(m.look[0]));
        let setup = setup::of(who);
        self.put_setup_text(&setup, window, cx);
        let name = who.name().to_string();
        let role = who.role().to_string();
        self.maker_name
            .update(cx, |i, cx| i.set_value(name, window, cx));
        self.maker_role
            .update(cx, |i, cx| i.set_value(role, window, cx));
        if let Some(m) = &made {
            let skills = m.skills.join(", ");
            let rules = m.rules.join("\n");
            self.maker_skills
                .update(cx, |i, cx| i.set_value(skills, window, cx));
            self.maker_rules
                .update(cx, |i, cx| ui::put_text(i, rules, window, cx));
        }
        self.maker_edit = Some(Editing {
            who: Some(who),
            id: made.map_or(0, |m| m.id),
            like: who.like(),
            tint,
            engine,
            setup,
            project: super::data::project_of(who),
            error: None,
        });
        self.maker_here = self
            .maker_edit
            .as_ref()
            .is_some_and(|e| !e.project.is_empty());
        self.find_on_computer();
        self.maker_stage = Stage::Review;
        self.maker_open = true;
        cx.notify();
    }

    fn close_maker(&mut self, cx: &mut Context<Self>) {
        self.crew.cancel_draft();
        self.maker_open = false;
        self.maker_edit = None;
        self.maker_stage = Stage::Describe;
        cx.notify();
    }

    /// Skills and apps on this computer, plus the project's own when the
    /// bot is tied to it.
    fn find_on_computer(&mut self) {
        let dir = self.maker_folder().map(|f| f.path);
        self.maker_found = crate::settings::discover(dir.as_deref());
    }

    /// The project the bot being made is tied to, or None for every project.
    fn maker_folder(&self) -> Option<crate::v2::store::Folder> {
        if !self.maker_here {
            return None;
        }
        let tied = self
            .maker_edit
            .as_ref()
            .map(|e| e.project.as_str())
            .unwrap_or("");
        if tied.is_empty() {
            return self.crew.store.folder().cloned();
        }
        let known = self
            .crew
            .store
            .folders
            .iter()
            .find(|f| f.path == tied)
            .cloned();
        Some(known.unwrap_or_else(|| crate::v2::store::Folder {
            name: tied.rsplit(['/', '\\']).next().unwrap_or(tied).to_string(),
            path: tied.to_string(),
            ship: String::new(),
            ..Default::default()
        }))
    }

    /// Ties the bot to the open project, or frees it for every project.
    fn set_here(&mut self, here: bool, cx: &mut Context<Self>) {
        self.maker_here = here && self.crew.store.folder().is_some();
        let path = if self.maker_here {
            self.maker_edit
                .as_ref()
                .map(|e| e.project.clone())
                .filter(|p| !p.is_empty())
                .or_else(|| self.crew.store.folder().map(|f| f.path.clone()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        if let Some(e) = self.maker_edit.as_mut() {
            e.project = path;
        }
        self.find_on_computer();
        cx.notify();
    }

    fn clear_maker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for i in [
            &self.maker_name,
            &self.maker_role,
            &self.maker_skills,
            &self.maker_reads,
            &self.maker_server,
            &self.maker_skill,
        ] {
            i.update(cx, |i, cx| i.set_value("", window, cx));
        }
        for i in [
            &self.maker_about,
            &self.maker_rules,
            &self.maker_instr,
            &self.maker_memory,
        ] {
            i.update(cx, |i, cx| ui::put_text(i, "", window, cx));
        }
    }

    fn put_setup_text(&mut self, setup: &Setup, window: &mut Window, cx: &mut Context<Self>) {
        let (instr, memory, reads) = (
            setup.instructions.clone(),
            setup.memory.join("\n"),
            setup.reads.join(", "),
        );
        self.maker_instr
            .update(cx, |i, cx| ui::put_text(i, instr, window, cx));
        self.maker_memory
            .update(cx, |i, cx| ui::put_text(i, memory, window, cx));
        self.maker_reads
            .update(cx, |i, cx| i.set_value(reads, window, cx));
    }

    /// Asks the engine to draft the bot from what's in the box.
    fn start_draft(&mut self, cx: &mut Context<Self>) {
        let mut about = self.maker_about.read(cx).value().trim().to_string();
        if about.is_empty() {
            // A redraft of a bot already set up starts from its job.
            about = self.maker_role.read(cx).value().trim().to_string();
        }
        if about.is_empty() {
            self.say("Say what the bot is for first, in a sentence.");
            cx.notify();
            return;
        }
        let name = self.maker_name.read(cx).value().trim().to_string();
        let st = cx.global::<AppSettings>().clone();
        let folder = self.maker_folder();
        self.crew.draft_bot(&st, &about, &name, folder.as_ref());
        self.maker_stage = Stage::Drafting;
        cx.notify();
    }

    /// A blank bot, from the box's words alone.
    pub(super) fn start_blank(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let about = self.maker_about.read(cx).value().trim().to_string();
        let name = self.maker_name.read(cx).value().trim().to_string();
        let mut full = brain::maker::quick(&about, &name);
        if about.is_empty() {
            full.instructions.clear();
            full.rules.clear();
        }
        self.apply_draft(full, window, cx);
        // Nothing was drafted: it's the person's own words.
        if let Some(e) = self.maker_edit.as_mut() {
            e.setup.drafted = false;
        }
    }

    /// Puts the engine's draft into the maker for review.
    fn apply_draft(
        &mut self,
        full: brain::maker::Full,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was = self.maker_edit.take();
        let id = was
            .as_ref()
            .map(|e| e.id)
            .filter(|id| *id != 0)
            .unwrap_or_else(|| self.crew.next_made_id());
        let who = was.as_ref().and_then(|e| e.who);
        let member = who.unwrap_or(Member::Made(id));
        let mut setup = brain::maker::setup_from(member, &full, &self.maker_found);
        // A redraft keeps the apps the person added by hand.
        if let Some(e) = &was {
            setup.servers = e.setup.servers.clone();
        }
        self.put_setup_text(&setup, window, cx);
        let built_in = who.is_some_and(|w| !w.is_made());
        if !built_in {
            let (name, role, skills, rules) = (
                full.name.clone(),
                full.role.clone(),
                full.skills.join(", "),
                full.rules.join("\n"),
            );
            self.maker_name
                .update(cx, |i, cx| i.set_value(name, window, cx));
            self.maker_role
                .update(cx, |i, cx| i.set_value(role, window, cx));
            self.maker_skills
                .update(cx, |i, cx| i.set_value(skills, window, cx));
            self.maker_rules
                .update(cx, |i, cx| ui::put_text(i, rules, window, cx));
        }
        let like = if built_in {
            member.like()
        } else {
            self.maker_like.or(full.like).unwrap_or(Member::Explainer)
        };
        self.maker_edit = Some(Editing {
            who,
            id,
            like,
            tint: was.as_ref().map_or(self.maker.0, |e| e.tint),
            engine: was.as_ref().map(|e| e.engine.clone()).unwrap_or_default(),
            setup,
            project: self.maker_folder().map(|f| f.path).unwrap_or_default(),
            error: None,
        });
        self.maker_stage = Stage::Review;
        cx.notify();
    }

    /// The engine's draft and skills, once they're back.
    pub(super) fn poll_maker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.maker_stage == Stage::Drafting && !self.crew.is_drafting() {
            match self.crew.drafted.take() {
                Some(full) => self.apply_draft(full, window, cx),
                None => self.maker_stage = Stage::Describe,
            }
        }
        if let Some(back) = self.crew.skilled.take() {
            let Some(e) = self.maker_edit.as_mut() else {
                return;
            };
            match back {
                Ok((name, description, body)) => {
                    match setup::write_skill(e.member(), &name, &description, &body) {
                        Ok(s) => {
                            e.setup.skill_files.retain(|k| k.name != s.name);
                            e.setup.skill_files.push(s);
                            e.error = None;
                            self.maker_skill
                                .update(cx, |i, cx| i.set_value("", window, cx));
                        }
                        Err(err) => e.error = Some(format!("Couldn't save the skill: {err}")),
                    }
                }
                Err(why) => e.error = Some(why),
            }
            cx.notify();
        }
    }

    /// Writes a skill from what's in the box, or brings in a SKILL.md.
    fn add_skill(&mut self, cx: &mut Context<Self>) {
        let text = self.maker_skill.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(shellexpand(&text));
        let Some(e) = self.maker_edit.as_mut() else {
            return;
        };
        if path.exists() {
            let md = brain::maker::skill_md(&path);
            match std::fs::read_to_string(&md) {
                Ok(body) => {
                    let name = front(&body, "name").unwrap_or_else(|| {
                        md.parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "skill".into())
                    });
                    let description = front(&body, "description").unwrap_or_default();
                    e.setup.skill_files.retain(|k| k.name != name);
                    e.setup.skill_files.push(SkillRef {
                        name,
                        description,
                        path: md,
                    });
                    e.error = None;
                }
                Err(_) => e.error = Some(format!("There's no SKILL.md at {}.", md.display())),
            }
            cx.notify();
            return;
        }
        let bot = self.maker_name.read(cx).value().trim().to_string();
        if self.maker_edit.is_none() {
            return;
        }
        let st = cx.global::<AppSettings>().clone();
        let folder = self.maker_folder();
        let error = self
            .crew
            .draft_skill(&st, &text, &bot, folder.as_ref())
            .err();
        let Some(e) = self.maker_edit.as_mut() else {
            return;
        };
        e.error = error;
        cx.notify();
    }

    fn add_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.maker_server.read(cx).value().trim().to_string();
        let Some(e) = self.maker_edit.as_mut() else {
            return;
        };
        match BotServer::parse("", &text) {
            Ok(s) => {
                e.setup.servers.retain(|x| x.name != s.name);
                e.setup.servers.push(s);
                e.error = None;
                self.maker_server
                    .update(cx, |i, cx| i.set_value("", window, cx));
            }
            Err(why) => e.error = Some(why),
        }
        cx.notify();
    }

    /// Saves the bot: a new one joins the crew; one on the crew is updated.
    fn save_bot(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.maker_edit.clone() else {
            return;
        };
        let name = self.maker_name.read(cx).value().trim().to_string();
        let mut role = self.maker_role.read(cx).value().trim().to_string();
        let skills: Vec<String> = by_comma(&self.maker_skills.read(cx).value())
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        let rules = by_line(&self.maker_rules.read(cx).value());
        let mut setup = e.setup.clone();
        setup.instructions = self.maker_instr.read(cx).value().trim().to_string();
        setup.memory = by_line(&self.maker_memory.read(cx).value());
        setup.reads = by_comma(&self.maker_reads.read(cx).value());
        let who = e.member();
        if !e.built_in() {
            if name.is_empty() {
                self.set_maker_error("Give your bot a name.", cx);
                return;
            }
            if Member::all()
                .iter()
                .any(|m| *m != who && m.name().eq_ignore_ascii_case(&name))
            {
                self.set_maker_error(&format!("There's already a bot called {name}."), cx);
                return;
            }
            if role.is_empty() {
                role = e.like.role().to_string();
            }
        }
        let look = [mascot::colour_index(e.tint), 0, 0, 0];
        match e.who {
            None => {
                self.crew.add_reviewed(
                    e.id,
                    &name,
                    &role,
                    e.like,
                    (skills, rules),
                    look,
                    setup,
                    e.project.clone(),
                );
                self.say(format!("{name} joined the crew."));
            }
            Some(Member::Made(id)) => {
                if let Some(m) = self.crew.store.made.iter_mut().find(|m| m.id == id) {
                    m.name = name.clone();
                    m.role = role;
                    m.like = e.like;
                    m.skills = skills;
                    m.rules = rules;
                    m.look[0] = look[0];
                    m.project = e.project.clone();
                }
                super::data::set_made(&self.crew.store.made);
                self.crew.set_setup(who, setup);
                self.say(format!("Saved {name}'s setup."));
            }
            Some(w) => {
                self.crew.set_setup(w, setup);
                self.say(format!("Saved {}'s setup.", w.name()));
            }
        }
        let engine = e.engine.clone();
        save_settings(cx, |st| engine::set(st, who, engine));
        self.entered = std::time::Instant::now();
        self.close_maker(cx);
    }

    fn set_maker_error(&mut self, why: &str, cx: &mut Context<Self>) {
        if let Some(e) = self.maker_edit.as_mut() {
            e.error = Some(why.to_string());
        }
        cx.notify();
    }

    /// Changes the bot being set up and redraws.
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Editing)) {
        if let Some(e) = self.maker_edit.as_mut() {
            f(e);
        }
        cx.notify();
    }

    // ---------- drawing ----------

    /// The maker, in place of the crew while it's open.
    pub(super) fn maker_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.maker_stage {
            Stage::Describe => self.describe_view(cx).into_any_element(),
            Stage::Drafting => self.drafting_view(cx).into_any_element(),
            Stage::Review => self.review_view(cx).into_any_element(),
        }
    }

    /// Every project, or only the open one.
    fn scope_pills(&self, cx: &mut Context<Self>) -> Div {
        let tied = self.maker_folder().map(|f| f.name);
        let open = self.crew.store.folder().map(|f| f.name.clone());
        let mut row = div().flex().flex_wrap().gap(px(6.)).child(
            ui::pill("mk-all", "Every project", tied.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.set_here(false, cx))),
        );
        if let Some(name) = tied.or(open) {
            row = row.child(
                ui::pill("mk-here", format!("Only in {name}"), self.maker_here)
                    .on_click(cx.listener(|this, _, _, cx| this.set_here(true, cx))),
            );
        }
        row
    }

    fn drafter_label(&self, cx: &Context<Self>) -> String {
        let st = cx.global::<AppSettings>();
        let (i, model) = brain::maker::drafter(st);
        engine::label_of(i, &model)
    }

    fn describe_view(&mut self, cx: &mut Context<Self>) -> Div {
        let tied = self.maker_folder().map(|f| f.name);
        let scope = self.scope_pills(cx);
        let mut starters = div().grid().grid_cols(4).gap(px(10.));
        for (i, (title, line, about)) in STARTERS.into_iter().enumerate() {
            starters = starters.child(
                div()
                    .id(("mk-start", i))
                    .p(px(12.))
                    .rounded(px(10.))
                    .bg(c(ui::well()))
                    .border_1()
                    .border_color(c(ui::line2()))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(ui::line_hi())))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(line),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.maker_about
                            .update(cx, |i, cx| ui::put_text(i, about, window, cx));
                        cx.notify();
                    })),
            );
        }
        starters = starters.child(
            div()
                .id("mk-blank")
                .p(px(12.))
                .rounded(px(10.))
                .border_1()
                .border_dashed()
                .border_color(c(ui::line_hi()))
                .cursor_pointer()
                .hover(|s| s.bg(c(ui::hover())))
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Blank"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::muted()))
                        .child("Skip the draft and set everything yourself"),
                )
                .on_click(cx.listener(|this, _, window, cx| this.start_blank(window, cx))),
        );
        let drafter = self.drafter_label(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(page_head(
                "Make a bot",
                "Say what it's for. Your engine drafts the rest: its instructions, skills, connected apps, permissions and memory. You review it before it joins.",
                ui::ghost("mk-cancel", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.close_maker(cx)))
                    .into_any_element(),
            ))
            .child(
                ui::card()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(ui::caption("WHAT SHOULD THIS BOT DO?"))
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(12.))
                            .bg(c(ui::well()))
                            .border_1()
                            .border_color(c(ui::line_hi()))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(
                                div().text_size(px(15.)).child(
                                    Textarea::new(&self.maker_about)
                                        .appearance(false)
                                        .bordered(false),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(field("NAME", &self.maker_name, 220.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(ui::caption("WORKS IN"))
                                            .child(scope),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(c(ui::faint()))
                                            .child(format!("Drafts with {drafter}")),
                                    )
                                    .child(
                                        ui::solid_i("mk-draft", IconName::Sparkles, "Draft my bot")
                                            .on_click(cx.listener(|this, _, _, cx| this.start_draft(cx))),
                                    ),
                            ),
                    )
                    .child(div().mt(px(6.)).child(ui::caption("OR START FROM")))
                    .child(starters),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child(match tied {
                        None => "A bot works in every project. It's drafted from your words and the skills and connected apps already on this computer, without reading any project. Pick Only in a project to fit it to that project's code.".to_string(),
                        Some(project) => format!("It only joins {project}. The draft reads the build files, CI, tests and docs there, and the skills and connected apps already on this computer. It changes no files."),
                    }),
            )
    }

    fn drafting_view(&mut self, cx: &mut Context<Self>) -> Div {
        let tied = self.maker_folder().map(|f| f.name);
        let drafter = self.drafter_label(cx);
        let about = self.maker_about.read(cx).value().to_string();
        let dots = ".".repeat(1 + (self.now() * 2.) as usize % 3);
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(page_head(
                "Make a bot",
                if tied.is_some() {
                    "Your engine is reading the project and drafting the bot."
                } else {
                    "Your engine is drafting the bot."
                },
                ui::ghost("mk-stop", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.crew.cancel_draft();
                        this.maker_stage = Stage::Describe;
                        cx.notify();
                    }))
                    .into_any_element(),
            ))
            .child(
                ui::card()
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Icon::new(IconName::Sparkles)
                            .size(px(22.))
                            .text_color(c(ui::VIOLET)),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(match &tied {
                                Some(project) => format!("Reading {project} with {drafter}{dots}"),
                                None => format!("Drafting with {drafter}{dots}"),
                            }),
                    )
                    .child(
                        div()
                            .max_w(px(620.))
                            .text_size(px(13.))
                            .text_color(c(ui::muted()))
                            .child(format!("\u{201c}{}\u{201d}", about.trim())),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("Writing its instructions, picking skills and connected apps, setting permissions and what it should know. Usually under a minute."),
                    ),
            )
    }

    fn review_view(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(e) = self.maker_edit.clone() else {
            return div();
        };
        let st = cx.global::<AppSettings>().clone();
        let who = e.member();
        let built_in = e.built_in();
        let face = if e.who.is_some() && !who.is_made() {
            self.bot(who, Mood::Idle, 60., Framing::Bust)
        } else {
            let line = Line::from_code(&st.fin_line);
            self.pose_img(
                squad_holo::Figure::Fin(
                    Fin {
                        tint: e.tint,
                        line,
                        ears: false,
                        outfit: Outfit::PLAIN,
                    }
                    .pose(Mood::Signature),
                ),
                60.,
                Framing::Bust,
            )
        };
        let save_label = if e.who.is_none() {
            "Add to crew"
        } else {
            "Save"
        };

        // Head: who it is.
        let mut kinds = div().flex().flex_wrap().gap(px(6.));
        if !built_in {
            for (i, m) in Member::KINDS.into_iter().enumerate() {
                kinds = kinds
                    .child(ui::pill(("mk-kind", i), m.name(), e.like == m).on_click(
                        cx.listener(move |this, _, _, cx| this.edit(cx, |e| e.like = m)),
                    ));
            }
        }
        let scope = self.scope_pills(cx);
        let head = ui::card()
            .p(px(18.))
            .flex()
            .gap(px(16.))
            .items_start()
            .child(face)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .when(built_in, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(who.name()),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(c(ui::muted()))
                                        .child(who.role()),
                                ),
                        )
                    })
                    .when(!built_in, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(field("NAME", &self.maker_name, 220.))
                                .child(field("ITS JOB", &self.maker_role, 0.)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(div().w(px(84.)).flex_none().child(ui::caption("WORKS LIKE")))
                                .child(kinds),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(div().w(px(84.)).flex_none().child(ui::caption("WORKS IN")))
                                .child(scope),
                        )
                    })
                    .when(e.setup.drafted, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(c(ui::muted()))
                                .child(Icon::new(IconName::Sparkles).size(px(12.)).text_color(c(ui::VIOLET)))
                                .child(if e.project.is_empty() {
                                    "Drafted by your engine. Change anything below."
                                } else {
                                    "Drafted by your engine from the project. Change anything below."
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    // Taking a bot off the crew lives here, not on its card.
                    .when_some(e.who.filter(|w| *w != Member::Fin), |d, gone| {
                        d.child(
                            ui::ghost_i("mk-remove", IconName::UserMinus, "Remove from crew")
                                .text_color(c(ui::t_red()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.close_maker(cx);
                                    this.ask_remove(gone, cx);
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        ui::ghost_i("mk-redraft", IconName::Sparkles, "Redraft")
                            .on_click(cx.listener(|this, _, _, cx| this.start_draft(cx))),
                    )
                    .child(ui::ghost("mk-close", "Cancel").on_click(cx.listener(|this, _, _, cx| this.close_maker(cx))))
                    .child(ui::solid("mk-save", save_label).on_click(cx.listener(|this, _, _, cx| this.save_bot(cx)))),
            );

        let error = e.error.clone().map(|why| {
            div()
                .px(px(14.))
                .py(px(10.))
                .rounded(px(10.))
                .bg(ca(ui::RED, 0.12))
                .text_size(px(12.5))
                .text_color(c(ui::ink()))
                .child(why)
        });

        // Brain: engine and model.
        let (cur, cur_model) = engine::engine(&st, who);
        let cur = if e.engine.agent.is_empty() {
            cur
        } else {
            engine::index_of(&st, &e.engine.agent)
        };
        let model_now = if e.engine.agent.is_empty() && e.engine.model.is_empty() {
            cur_model
        } else {
            e.engine.model.clone()
        };
        let mut engines = div().flex().flex_wrap().gap(px(6.));
        let mut offer: Vec<usize> = (0..agent::PROVIDERS.len())
            .filter(|&i| engine::ready(&st, i))
            .collect();
        if !offer.contains(&cur) {
            offer.insert(0, cur);
        }
        for i in offer {
            let id = agent::PROVIDERS[i].id;
            engines = engines.child(
                super::engines::choice(
                    ("mk-engine", i),
                    agent::PROVIDERS[i].name,
                    i == cur,
                    engine::ready(&st, i),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |e| {
                        e.engine = BotEngine {
                            agent: id.to_string(),
                            model: String::new(),
                        }
                    })
                })),
            );
        }
        let mut models = div().flex().flex_wrap().gap(px(6.));
        for (k, m) in AgentPick::models(cur).into_iter().take(7).enumerate() {
            let value = m.value.clone();
            let id = agent::PROVIDERS[cur].id;
            models = models.child(
                ui::pill(("mk-model", k), m.label.clone(), model_now == m.value).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let value = value.clone();
                        this.edit(cx, |e| {
                            e.engine = BotEngine {
                                agent: id.to_string(),
                                model: value,
                            }
                        })
                    }),
                ),
            );
        }
        let brain_card = section("Brain", "The engine and model that do its work", None)
            .child(labelled("ENGINE", engines))
            .child(labelled("MODEL", models));

        let instructions = section(
            "Instructions",
            "Who it is and how it works, told to its engine on every job",
            None,
        )
        .child(
            div()
                .p(px(12.))
                .rounded(px(10.))
                .bg(c(ui::well()))
                .border_1()
                .border_color(c(ui::line2()))
                .font_family(ui::MONO)
                .text_size(px(12.))
                .child(
                    Textarea::new(&self.maker_instr)
                        .appearance(false)
                        .bordered(false),
                ),
        );

        // Skills: the words jobs are routed by, and skill files.
        let mut files = div().flex().flex_col().gap(px(6.));
        for (k, s) in e.setup.skill_files.iter().enumerate() {
            let name = s.name.clone();
            let mine = s.path.starts_with(setup::folder(who));
            files = files.child(
                skill_row(
                    ("mk-skill-on", k),
                    true,
                    &s.name,
                    &format!(
                        "{}{}",
                        if mine { "Written for this bot · " } else { "" },
                        if s.description.is_empty() {
                            s.path.display().to_string()
                        } else {
                            s.description.clone()
                        }
                    ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let name = name.clone();
                    this.edit(cx, |e| e.setup.skill_files.retain(|s| s.name != name))
                })),
            );
        }
        let have: Vec<String> = e.setup.skill_files.iter().map(|s| s.name.clone()).collect();
        let mut more = 0;
        for (k, s) in self
            .maker_found
            .skills
            .iter()
            .filter(|s| !have.contains(&s.name))
            .enumerate()
        {
            if k >= 8 {
                more += 1;
                continue;
            }
            let pick = SkillRef {
                name: s.name.clone(),
                description: s.description.clone(),
                path: s.path.clone(),
            };
            files = files.child(
                skill_row(
                    ("mk-skill-off", k),
                    false,
                    &s.name,
                    &format!("From {} · {}", source_name(s.source), s.description),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let pick = pick.clone();
                    this.edit(cx, |e| e.setup.skill_files.push(pick))
                })),
            );
        }
        if more > 0 {
            files = files.child(div().text_size(px(11.5)).text_color(c(ui::faint())).child(
                format!("And {more} more on this computer: paste a path below to add one."),
            ));
        }
        let skilling = self.crew.is_skilling();
        let skills_card = section(
            "Skills",
            "What it's good at, and the skill files it follows",
            None,
        )
        .when(!built_in, |d| {
            d.child(ui::caption("JOBS ABOUT THESE WORDS COME TO IT"))
                .child(div().flex().child(field("WORDS", &self.maker_skills, 0.)))
        })
        .child(ui::caption("SKILL FILES"))
        .child(files)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(field("NEW", &self.maker_skill, 0.))
                .child(
                    ui::ghost_i(
                        "mk-skill-add",
                        IconName::Sparkles,
                        if skilling { "Writing…" } else { "Add" },
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.add_skill(cx))),
                ),
        );

        // Tools: connected apps, and what it may do.
        let mut apps = div().flex().flex_wrap().gap(px(6.));
        let mut seen = Vec::new();
        for (k, m) in self
            .maker_found
            .mcp
            .iter()
            .filter(|m| m.enabled)
            .enumerate()
        {
            let key = crate::mcp::key(m);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            let on = e.setup.mcp.contains(&key);
            apps = apps.child(
                ui::pill(
                    ("mk-app", k),
                    format!(
                        "{}{} · {}",
                        if on { "\u{2713} " } else { "" },
                        m.name,
                        m.provider
                    ),
                    on,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let key = key.clone();
                    this.edit(cx, |e| {
                        if let Some(i) = e.setup.mcp.iter().position(|x| *x == key) {
                            e.setup.mcp.remove(i);
                        } else {
                            e.setup.mcp.push(key);
                        }
                    })
                })),
            );
        }
        for (k, s) in e.setup.servers.iter().enumerate() {
            let name = s.name.clone();
            apps = apps.child(
                ui::pill(
                    ("mk-own", k),
                    format!("\u{2713} {} · added here  \u{00d7}", s.name),
                    true,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let name = name.clone();
                    this.edit(cx, |e| e.setup.servers.retain(|s| s.name != name))
                })),
            );
        }
        if seen.is_empty() && e.setup.servers.is_empty() {
            apps = apps.child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child("No connected apps found in Claude Code, Codex, Cursor or the others. Add one below."),
            );
        }
        let p = e.setup.perms;
        let perm = |key: &'static str,
                    label: &'static str,
                    yes: &'static str,
                    no: &'static str,
                    on: bool,
                    set: fn(&mut Perms, bool),
                    cx: &mut Context<Shell>| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().text_size(px(12.5)).child(label))
                .child(ui::pill((key, 0usize), yes, on).on_click(cx.listener(
                    move |this, _, _, cx| this.edit(cx, |e| set(&mut e.setup.perms, true)),
                )))
                .child(ui::pill((key, 1usize), no, !on).on_click(cx.listener(
                    move |this, _, _, cx| this.edit(cx, |e| set(&mut e.setup.perms, false)),
                )))
        };
        let tools_card = section(
            "Tools & permissions",
            "Connected apps it can use, on any engine, and what it may do on its own",
            None,
        )
        .child(ui::caption("CONNECTED APPS (MCP)"))
        .child(apps)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(field("ADD", &self.maker_server, 0.))
                .child(
                    ui::ghost_i("mk-app-add", IconName::Plus, "Add")
                        .on_click(cx.listener(|this, _, window, cx| this.add_server(window, cx))),
                ),
        )
        .child(div().mt(px(4.)).child(ui::caption("PERMISSIONS")))
        .child(perm(
            "mk-p-edit",
            "Change files in its own copy",
            "Allowed",
            "Read only",
            p.edit,
            |p, v| p.edit = v,
            cx,
        ))
        .child(perm(
            "mk-p-install",
            "Add or install packages",
            "Allowed",
            "Never",
            p.install,
            |p, v| p.install = v,
            cx,
        ))
        .child(perm(
            "mk-p-web",
            "Fetch web pages and search",
            "Allowed",
            "Never",
            p.web,
            |p, v| p.web = v,
            cx,
        ))
        .child(perm(
            "mk-p-push",
            "Push, deploy, publish",
            "Asks every time",
            "Never",
            p.push,
            |p, v| p.push = v,
            cx,
        ))
        .child(
            div()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child("Your keys and logins are never touched, whatever is set here."),
        );

        let knowledge = section(
            "Knowledge & memory",
            "What it reads first, and what it knows from day one",
            None,
        )
        .child(div().flex().child(field("READS", &self.maker_reads, 0.)))
        .child(ui::caption("STARTS OUT KNOWING, ONE A LINE"))
        .child(area_box(&self.maker_memory));

        let rules = (!built_in).then(|| {
            section("Rules", "How it works on every job, one a line", None)
                .child(area_box(&self.maker_rules))
        });

        let mut swatches = div().flex().items_center().gap(px(10.));
        for (i, (_, _, t)) in mascot::COLOURS.into_iter().enumerate() {
            let on = e.tint == t;
            let [l, s, _] = t.0;
            swatches = swatches.child(
                div()
                    .id(("mk-swatch", i))
                    .size(px(22.))
                    .rounded_full()
                    .border_2()
                    .border_color(c(if on { ui::ink() } else { ui::panel() }))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(c(l.0), 0.0),
                        linear_color_stop(c(s.0), 1.0),
                    ))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.edit(cx, |e| e.tint = t))),
            );
        }
        let look = (!built_in).then(|| {
            div()
                .px(px(16.))
                .py(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(c(ui::line()))
                .flex()
                .items_center()
                .gap(px(14.))
                .child(ui::caption("LOOK"))
                .child(swatches)
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(c(ui::faint()))
                        .child("Colour only. The mascot's outfit lives in Settings."),
                )
        });

        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(head)
            .children(error)
            .child(brain_card)
            .child(instructions)
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(px(14.))
                    .child(skills_card)
                    .child(tools_card)
                    .child(knowledge)
                    .children(rules),
            )
            .children(look)
    }
}

/// "~/x" with the home folder written out.
fn shellexpand(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::Path::new(&home)
            .join(rest)
            .to_string_lossy()
            .into_owned(),
        _ => path.to_string(),
    }
}

/// A `key: value` line from a SKILL.md's front matter.
fn front(text: &str, key: &str) -> Option<String> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    body[..end].lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// Where a skill found on this computer came from, in words.
fn source_name(source: &str) -> &'static str {
    match source {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "agents" => "~/.agents",
        "project" => "this project",
        _ => "this computer",
    }
}

fn page_head(title: &'static str, sub: &'static str, right: AnyElement) -> Div {
    div()
        .flex()
        .items_end()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .text_color(c(ui::muted()))
                        .child(sub),
                ),
        )
        .child(right)
}

/// A card with a title and one line under it.
fn section(title: &'static str, sub: &'static str, right: Option<AnyElement>) -> Div {
    ui::card().p(px(16.)).flex().flex_col().gap(px(10.)).child(
        div()
            .flex()
            .items_start()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child(sub),
                    ),
            )
            .children(right),
    )
}

fn labelled(label: &'static str, body: Div) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .w(px(56.))
                .flex_none()
                .pt(px(8.))
                .child(ui::caption(label)),
        )
        .child(body.flex_1().min_w_0())
}

/// One skill file with its switch.
fn skill_row(id: (&'static str, usize), on: bool, name: &str, line: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(10.))
        .py(px(8.))
        .rounded(px(9.))
        .bg(c(ui::well()))
        .border_1()
        .border_color(c(if on { ui::line_hi() } else { ui::line2() }))
        .cursor_pointer()
        .hover(|s| s.border_color(c(ui::line_hi())))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(switch(id, on))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .text_color(c(if on { ui::ink() } else { ui::body() }))
                        .child(name.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(c(ui::faint()))
                        .truncate()
                        .child(line.to_string()),
                ),
        )
}

/// A small on/off switch; flipped, its knob glides across.
fn switch(key: impl std::hash::Hash, on: bool) -> Div {
    use super::motion::mix;
    let k = super::motion::glide(
        super::motion::key(("builder-switch", key)),
        if on { 1. } else { 0. },
        super::motion::BASE,
    )
    .clamp(0., 1.);
    div()
        .w(px(28.))
        .h(px(16.))
        .flex_none()
        .rounded_full()
        .bg(mix(c(ui::line_hi()), c(ui::pick_fill()), k))
        .relative()
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(2. + 12. * k))
                .size(px(12.))
                .rounded_full()
                .bg(mix(c(ui::faint()), c(0xFFFFFF), k)),
        )
}

/// A labelled one-line box.
fn field(label: &'static str, input: &Entity<InputState>, w: f32) -> Div {
    div()
        .when(w > 0., |d| d.w(px(w)).flex_none())
        .when(w == 0., |d| d.flex_1().min_w_0())
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
                .font_family(ui::MONO)
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

/// A box of several lines.
fn area_box(input: &Entity<TextareaState>) -> Div {
    div()
        .p(px(10.))
        .rounded(px(10.))
        .bg(c(ui::well()))
        .border_1()
        .border_color(c(ui::line2()))
        .text_size(px(12.5))
        .child(Textarea::new(input).appearance(false).bordered(false))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's own `test` attribute would come with it.
    use super::{by_comma, by_line, front};

    #[test]
    fn lists_come_from_commas_or_lines() {
        assert_eq!(by_comma(" docs, , readme "), vec!["docs", "readme"]);
        assert_eq!(
            by_line("- one\n\n* two\n• three"),
            vec!["one", "two", "three"]
        );
    }

    #[test]
    fn a_skill_names_itself_in_its_front_matter() {
        let md = "---\nname: docs-sync\ndescription: \"Fix stale docs\"\n---\n\nSteps";
        assert_eq!(front(md, "name").as_deref(), Some("docs-sync"));
        assert_eq!(front(md, "description").as_deref(), Some("Fix stale docs"));
        assert_eq!(front("no front matter", "name"), None);
    }
}
