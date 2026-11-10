//! Crew: every bot, what it is for, what it can do and what it has done. Make a
//! bot and each bot's Setup open the editor in `builder`.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::Framing;

use super::data::{Member, Mode};
use super::store::Status;
use super::ui::{self, c};
use super::Pane;
use super::{Place, Shell};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;

impl Shell {
    pub(super) fn crew(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.maker_open {
            let page = div()
                .w_full()
                .max_w(px(1160.))
                .px(px(28.))
                .pt(px(24.))
                .pb(px(40. + super::DOCK_ROOM))
                .child(self.maker_page(cx));
            return div()
                .id("crew-maker")
                .size_full()
                .overflow_y_scroll()
                .child(page)
                .into_any_element();
        }
        let all = Member::all();
        let pick = match self.crew_pick {
            Some(w) if all.contains(&w) => w,
            _ => all.first().copied().unwrap_or(Member::Fin),
        };
        let n = all.len();
        let busy = self.crew.store.running().count();

        // The list: every bot on the crew, its state on the right.
        let mut list = div()
            .id("crew-list")
            .w(px(LIST_W))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .px(px(10.))
            .pt(px(12.))
            .pb(px(24. + super::DOCK_ROOM))
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_r_1()
            .border_color(c(ui::line()))
            .child(
                div()
                    .px(px(8.))
                    .pt(px(8.))
                    .pb(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Crew"),
                    )
                    .child(
                        ui::solid_i("crew-make", IconName::Plus, "Make a bot")
                            .h(px(30.))
                            .text_size(px(12.5))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_maker(None, None, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(8.))
                    .pb(px(6.))
                    .flex()
                    .justify_between()
                    .font_family(ui::MONO)
                    .text_size(px(10.5))
                    .text_color(c(ui::faint()))
                    .child("ON THE CREW")
                    .child(if busy > 0 {
                        format!("{n} \u{b7} {busy} working")
                    } else {
                        n.to_string()
                    }),
            );
        for who in all {
            let on = who == pick;
            let face = self.avatar(who, 26.);
            let (status, color) = self.status_of(who);
            let quiet = color == ui::faint();
            let word = if !quiet {
                status
            } else if who == Member::Fin {
                "lead".to_string()
            } else if !super::data::project_of(who).is_empty() {
                "this project".to_string()
            } else {
                "idle".to_string()
            };
            list = list.child(
                div()
                    .id(("crew-row", who.key() as usize))
                    .relative()
                    .h(px(50.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(7.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::raised())).border_color(c(ui::line())).child(
                                div()
                                    .absolute()
                                    .left(px(-1.))
                                    .top(px(-1.))
                                    .bottom(px(-1.))
                                    .w(px(4.))
                                    .rounded_l(px(7.))
                                    .bg(c(ui::pick_fill())),
                            )
                        } else {
                            d.border_color(transparent_black())
                                .hover(|s| s.bg(c(ui::hover())))
                        }
                    })
                    .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                        this.hover_bot(who, *on, cx);
                    }))
                    .child(face)
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
                                    .child(who.name()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(c(ui::muted()))
                                    .child(who.role()),
                            ),
                    )
                    .child(div().flex_none().max_w(px(120.)).overflow_hidden().child(
                        ui::mono_status(if quiet { ui::faint() } else { color }, &word),
                    ))
                    .on_mouse_down(
                        MouseButton::Right,
                        self.on_menu(super::menu::Ctx::Bot(who), cx),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew_pick = Some(who);
                        cx.notify();
                    })),
            );
        }
        list = list.children(self.bench(cx));

        let detail = self.bot_profile(pick, cx);
        div()
            .id("crew")
            .size_full()
            .flex()
            .child(list)
            .child(
                div()
                    .id("crew-detail")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(detail),
            )
            .into_any_element()
    }

    /// One bot: its face, name and role, Message and Give a job, then its
    /// Profile, Brain, Skills and tools, Memory and Activity.
    fn bot_profile(&mut self, who: Member, cx: &mut Context<Self>) -> Div {
        let key = who.key() as usize;
        let mood = self.mood(who);
        let face = self.bot(who, mood, 64., Framing::Bust);
        let mode = who.mode();
        let here = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let lessons_here = self
            .crew
            .store
            .lessons
            .iter()
            .filter(|l| l.bot == who && l.folder == here)
            .count();
        let tab = self.crew_tab;
        let st = cx.global::<crate::settings::AppSettings>();
        let engine = super::engine::label(st, who);
        let asks = match (st.v2_plan_first, st.v2_auto_merge) {
            (true, _) => "asks before anything",
            (false, true) => "merges when checks pass",
            (false, false) => "asks before merging",
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(div().flex_none().child(face))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child(who.name()),
                    )
                    .child(
                        div()
                            .text_size(px(13.5))
                            .text_color(c(ui::body()))
                            .child(who.role()),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::faint()))
                            .child(format!("{engine} \u{b7} {asks}")),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::ghost_i(("crew-edit", key), IconName::Pencil, "Edit").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open_setup(who, window, cx)
                            }),
                        ),
                    )
                    .child(
                        ui::ghost_i(("crew-chat", key), IconName::MessageCircle, "Message")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.go(Place::Chat(who), cx)),
                            ),
                    )
                    .when(mode != Mode::Ask, |d| {
                        d.child(
                            ui::solid_i(("crew-job", key), IconName::Plus, "Give a job").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.target = Some(who);
                                    this.set_mode(mode, window, cx);
                                    this.go(Place::Today, cx);
                                    this.say(format!(
                                        "Type the job for {} and press Enter.",
                                        who.name()
                                    ));
                                }),
                            ),
                        )
                    }),
            );
        let mut tabs = div()
            .h(px(40.))
            .flex()
            .items_end()
            .gap(px(4.))
            .border_b_1()
            .border_color(c(ui::line()));
        for t in CrewTab::ALL {
            let on = t == tab;
            let count = (t == CrewTab::Memory && lessons_here > 0).then_some(lessons_here);
            tabs = tabs.child(
                div()
                    .id(("crew-tab", t as usize))
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .border_b_2()
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::pick_fill()))
                                .text_color(c(ui::ink()))
                                .font_weight(FontWeight::SEMIBOLD)
                        } else {
                            d.border_color(transparent_black())
                                .text_color(c(ui::muted()))
                                .hover(|s| s.text_color(c(ui::ink())))
                        }
                    })
                    .child(t.name())
                    .when_some(count, |d, n| {
                        d.child(
                            div()
                                .h(px(17.))
                                .px(px(6.))
                                .flex()
                                .items_center()
                                .rounded(px(3.))
                                .bg(c(ui::hover()))
                                .font_weight(FontWeight::NORMAL)
                                .text_size(px(10.5))
                                .text_color(c(ui::muted()))
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew_tab = t;
                        cx.notify();
                    })),
            );
        }
        let body = match tab {
            CrewTab::Profile => self.bot_tab_profile(who, cx),
            CrewTab::Brain => self.bot_tab_brain(who, cx),
            CrewTab::Skills => self.bot_tab_skills(who),
            CrewTab::Memory => self.bot_tab_memory(who, &here, cx),
            CrewTab::Activity => self.bot_tab_activity(who, cx),
        };
        div()
            .px(px(32.))
            .pt(px(28.))
            .pb(px(40. + super::DOCK_ROOM))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(head)
            .child(tabs)
            .child(body)
    }

    /// Profile: what it's doing, where it works and what it has done.
    fn bot_tab_profile(&mut self, who: Member, cx: &mut Context<Self>) -> Div {
        let (status, color) = self.status_of(who);
        let mine: Vec<&super::store::Job> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.bot == who)
            .collect();
        let shipped = mine
            .iter()
            .filter(|j| matches!(j.status, Status::Shipped | Status::Shipping))
            .count();
        let review = mine.iter().filter(|j| j.status == Status::Review).count();
        let only = super::data::project_of(who);
        let setup = super::setup::of(who);
        let stat = |n: usize, word: &str| {
            ui::card()
                .flex_1()
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(n.to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(word.to_string()),
                )
        };
        let engine = super::engine::label(cx.global::<crate::settings::AppSettings>(), who);
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(fact(
                        "Now",
                        if color == ui::faint() {
                            ui::status(ui::faint(), "Idle")
                        } else {
                            ui::status(color, status)
                        },
                    ))
                    .child(fact("Engine", quiet_text(engine)))
                    .child(fact(
                        "Works on",
                        quiet_text(if only.is_empty() {
                            "Every project's crew".to_string()
                        } else {
                            format!("Only {}", super::sidebar::short_path(&only))
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(stat(mine.len(), "jobs"))
                    .child(stat(review, "waiting for review"))
                    .child(stat(shipped, "shipped")),
            )
            .when(!setup.instructions.trim().is_empty(), |d| {
                d.child(section(
                    "How it works",
                    div()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(c(ui::body()))
                        .child(setup.instructions.trim().to_string()),
                ))
            })
    }

    /// Brain: the engine it thinks with, and what it may do on its own.
    fn bot_tab_brain(&mut self, who: Member, cx: &mut Context<Self>) -> Div {
        let engine = super::engine::label(cx.global::<crate::settings::AppSettings>(), who);
        let perms = super::setup::of(who).perms;
        let yes_no = |on: bool, yes: &str, no: &str| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(13.))
                .text_color(c(ui::body()))
                .child(
                    Icon::new(if on { IconName::Check } else { IconName::X })
                        .size(px(14.))
                        .text_color(c(if on { ui::t_green() } else { ui::faint() })),
                )
                .child(if on { yes.to_string() } else { no.to_string() })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(section(
                "Thinks with",
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .font_family(ui::MONO)
                            .text_size(px(13.))
                            .text_color(c(ui::ink()))
                            .child(engine),
                    )
                    .child(
                        ui::ghost_i(("crew-engine", who.key() as usize), IconName::Cpu, "Change")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.go(Place::Settings(Pane::Engines), cx)
                            })),
                    ),
            ))
            .child(section(
                "May do on its own",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(yes_no(
                        perms.edit,
                        "Change files in its own copy",
                        "Only reads and reports",
                    ))
                    .child(yes_no(
                        perms.install,
                        "Add and install packages",
                        "Never installs packages",
                    ))
                    .child(yes_no(perms.web, "Use the web", "Stays off the web"))
                    .child(yes_no(
                        perms.push,
                        "Push or deploy, asking every time",
                        "Never pushes or deploys",
                    )),
            ))
    }

    /// Skills and tools: what it knows best, its skill files, its connected
    /// apps and the rules it follows.
    fn bot_tab_skills(&mut self, who: Member) -> Div {
        let setup = super::setup::of(who);
        let made = match who {
            Member::Made(id) => self.crew.store.made.iter().find(|m| m.id == id).cloned(),
            _ => None,
        };
        let words = made.as_ref().map(|m| m.skills.clone()).unwrap_or_default();
        let rules = made.as_ref().map(|m| m.rules.clone()).unwrap_or_default();
        let mut apps: Vec<String> = setup
            .mcp
            .iter()
            .map(|k| k.rsplit('/').next().unwrap_or(k).to_string())
            .collect();
        apps.extend(setup.servers.iter().map(|s| s.name.clone()));
        let chips = |items: Vec<String>| {
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(items.into_iter().map(|w| {
                    div()
                        .h(px(24.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .bg(c(ui::well()))
                        .border_1()
                        .border_color(c(ui::line2()))
                        .text_size(px(12.))
                        .text_color(c(ui::body()))
                        .child(w)
                }))
        };
        let mut out = div().flex().flex_col().gap(px(18.));
        let mut any = false;
        if !words.is_empty() {
            any = true;
            out = out.child(section("Jobs that mention these go to it", chips(words)));
        }
        if !setup.skill_files.is_empty() {
            any = true;
            let mut l = div().flex().flex_col().gap(px(8.));
            for s in &setup.skill_files {
                l = l.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(12.5))
                                .text_color(c(ui::ink()))
                                .child(s.name.clone()),
                        )
                        .when(!s.description.trim().is_empty(), |d| {
                            d.child(quiet_text(s.description.trim().to_string()))
                        }),
                );
            }
            out = out.child(section("Skills", l));
        }
        if !apps.is_empty() {
            any = true;
            out = out.child(section("Connected apps", chips(apps)));
        }
        if !rules.is_empty() {
            any = true;
            out = out.child(section("Rules on every job", bullets(rules)));
        }
        if !any {
            out = out.child(quiet_text(format!(
                "{} works with its built-in know-how. Edit adds skills, connected apps and rules.",
                who.name()
            )));
        }
        out
    }

    /// Memory: what the bot has learned, a card per project, this one first,
    /// with a box to teach it something here.
    fn bot_tab_memory(&mut self, who: Member, here: &str, cx: &mut Context<Self>) -> Div {
        let mut places: Vec<String> = vec![here.to_string()];
        for l in self.crew.store.lessons.iter().filter(|l| l.bot == who) {
            if !places.contains(&l.folder) {
                places.push(l.folder.clone());
            }
        }
        places.retain(|p| !p.is_empty());
        let names: Vec<(String, String)> = self
            .crew
            .store
            .folders
            .iter()
            .map(|f| (f.path.clone(), f.name.clone()))
            .collect();
        let name_of = |path: &str| {
            names
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, n)| n.clone())
                .or_else(|| {
                    std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| path.to_string())
        };
        let name = who.name();
        let mut out = div().flex().flex_col().gap(px(12.)).child(
            div()
                .text_size(px(13.))
                .text_color(c(ui::body()))
                .child(format!(
                    "What {name} has learned, per project. {name} reads these before every job. Forget anything that's wrong."
                )),
        );
        for (pi, folder) in places.into_iter().enumerate() {
            let mut lessons: Vec<(usize, super::learn::Lesson)> = self
                .crew
                .store
                .lessons
                .iter()
                .enumerate()
                .filter(|(_, l)| l.bot == who && l.folder == folder)
                .map(|(i, l)| (i, l.clone()))
                .collect();
            lessons.sort_by_key(|(_, l)| std::cmp::Reverse(l.at));
            let n = lessons.len();
            let mut card = div()
                .px(px(15.))
                .pb(px(12.))
                .flex()
                .flex_col()
                .rounded(px(6.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .child(
                    div()
                        .py(px(11.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .font_family(ui::MONO)
                        .text_size(px(11.))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c(ui::body()))
                                .child(name_of(&folder).to_uppercase()),
                        )
                        .child(
                            div()
                                .text_color(c(ui::faint()))
                                .child(format!("{n} lesson{}", if n == 1 { "" } else { "s" })),
                        ),
                );
            if lessons.is_empty() {
                card = card.child(
                    div()
                        .py(px(10.))
                        .border_t_1()
                        .border_color(c(ui::line()))
                        .border_dashed()
                        .text_size(px(12.5))
                        .text_color(c(ui::faint()))
                        .child(format!(
                            "Nothing yet. When you send {name}'s work back or comment on its changes, the lasting lessons land here."
                        )),
                );
            }
            for (i, l) in lessons {
                let tag = if l.job == 0 {
                    "you said".to_string()
                } else {
                    format!("learned from #{}", l.job)
                };
                let text = l.text.clone();
                card = card.child(
                    div()
                        .py(px(10.))
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .border_t_1()
                        .border_color(c(ui::line()))
                        .border_dashed()
                        .child(
                            div()
                                .mt(px(7.))
                                .size(px(6.))
                                .flex_none()
                                .bg(c(ui::accent_bar())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .text_color(c(ui::ink()))
                                        .child(l.text.clone()),
                                )
                                .child(
                                    div().flex().gap(px(6.)).child(
                                        div()
                                            .px(px(5.))
                                            .py(px(1.))
                                            .rounded(px(3.))
                                            .bg(c(ui::hover()))
                                            .font_family(ui::MONO)
                                            .text_size(px(10.5))
                                            .text_color(c(ui::body()))
                                            .child(tag),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .id(("mem-forget", i))
                                .px(px(10.))
                                .py(px(3.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(ui::body()))
                                .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                                .child("Forget")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let store = &mut this.crew.store;
                                    if let Some(at) = store.lessons.iter().position(|x| {
                                        x.bot == who && x.text == text && x.at == l.at
                                    }) {
                                        store.lessons.remove(at);
                                        super::learn::set_all(&store.lessons);
                                        store.save();
                                    }
                                    cx.notify();
                                })),
                        ),
                );
            }
            // Teaching it something goes in this project's card.
            if pi == 0 {
                let folder = folder.clone();
                card =
                    card.child(
                        div()
                            .pt(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(34.))
                                    .px(px(4.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(5.))
                                    .bg(c(ui::raised()))
                                    .border_1()
                                    .border_color(c(ui::line_hi()))
                                    .text_size(px(13.))
                                    .child(
                                        Input::new(&self.memory_input)
                                            .appearance(false)
                                            .bordered(false),
                                    ),
                            )
                            .child(ui::dbtn("mem-add", "Add", ui::Btn::Primary).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    let text =
                                        this.memory_input.read(cx).value().trim().to_string();
                                    if text.is_empty() {
                                        return;
                                    }
                                    let added = super::learn::add(
                                        &mut this.crew.store.lessons,
                                        super::learn::Lesson {
                                            bot: who,
                                            folder: folder.clone(),
                                            text,
                                            job: 0,
                                            at: super::store::now(),
                                            crew: false,
                                        },
                                    );
                                    if added {
                                        super::learn::set_all(&this.crew.store.lessons);
                                        this.crew.store.save();
                                    }
                                    this.memory_input
                                        .update(cx, |i, cx| i.set_value("", window, cx));
                                    cx.notify();
                                }),
                            )),
                    );
            }
            out = out.child(card);
        }
        out
    }

    /// Activity: its latest jobs, newest first.
    fn bot_tab_activity(&mut self, who: Member, cx: &mut Context<Self>) -> Div {
        let mut mine: Vec<super::store::Job> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.bot == who)
            .map(|j| {
                let mut j = j.clone();
                j.feed.clear();
                j
            })
            .collect();
        mine.sort_by_key(|j| std::cmp::Reverse(j.ended.unwrap_or(j.started)));
        mine.truncate(20);
        let mut list = div().flex().flex_col();
        if mine.is_empty() {
            return list.child(quiet_text(format!("{} hasn't had a job yet.", who.name())));
        }
        for j in mine {
            let id = j.id;
            let (word, tone) = super::home::status_of(&j);
            list = list.child(
                div()
                    .id(("crew-act", id as usize))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(38.))
                    .px(px(8.))
                    .mx(px(-8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(ui::hover())))
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(c(ui::ink()))
                            .child(super::channel::card_title(&j)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::faint()))
                            .child(format!("{} #{id}", j.folder.name)),
                    )
                    .child(super::home::pill(&word, tone))
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child(super::store::ago(j.ended.unwrap_or(j.started))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.open_job(id, cx))),
            );
        }
        list
    }

    /// Built-in bots not on the crew, each one click from joining.
    fn bench(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let off: Vec<Member> = Member::CREW.into_iter().filter(|m| !m.on_crew()).collect();
        if off.is_empty() {
            return None;
        }
        let mut col = div()
            .mt(px(14.))
            .pt(px(12.))
            .border_t_1()
            .border_color(c(ui::line()))
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                div()
                    .px(px(10.))
                    .pb(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(10.5))
                    .text_color(c(ui::faint()))
                    .child("NOT ON THE CREW"),
            );
        for who in off {
            let face = self.avatar(who, 24.);
            col = col.child(
                div()
                    .id(("bench", who.key() as usize))
                    .h(px(36.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .text_color(c(ui::muted()))
                    .child(div().opacity(0.6).child(face))
                    .child(div().flex_1().min_w_0().truncate().child(who.name()))
                    .child(
                        ui::ghost(("bench-add", who.key() as usize), "Add")
                            .h(px(26.))
                            .text_size(px(12.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.set_on_crew(who, true, cx)),
                            ),
                    ),
            );
        }
        Some(col)
    }
}

/// The crew list's width.
const LIST_W: f32 = 300.;

/// A bot profile's tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum CrewTab {
    #[default]
    Profile,
    Brain,
    Skills,
    Memory,
    Activity,
}

impl CrewTab {
    pub(super) const ALL: [CrewTab; 5] = [
        CrewTab::Profile,
        CrewTab::Brain,
        CrewTab::Skills,
        CrewTab::Memory,
        CrewTab::Activity,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            CrewTab::Profile => "Profile",
            CrewTab::Brain => "Brain",
            CrewTab::Skills => "Skills",
            CrewTab::Memory => "Memory",
            CrewTab::Activity => "Activity",
        }
    }
}

/// A block on a profile tab: a small heading over its body.
fn section(title: &str, inner: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .text_color(c(ui::faint()))
                .child(title.to_uppercase()),
        )
        .child(inner)
}

/// A key and its value on the Profile tab.
fn fact(key: &str, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .py(px(9.))
        .border_b_1()
        .border_color(c(ui::line2()))
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_size(px(12.5))
                .text_color(c(ui::muted()))
                .child(key.to_string()),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// Body text in the quiet colour.
fn quiet_text(text: String) -> Div {
    div()
        .text_size(px(13.))
        .line_height(px(19.))
        .text_color(c(ui::muted()))
        .child(text)
}

/// A short list with a dot before each line.
fn bullets(items: Vec<String>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(items.into_iter().map(|t| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .text_size(px(13.))
                .line_height(px(19.))
                .text_color(c(ui::body()))
                .child(
                    div()
                        .mt(px(7.))
                        .size(px(5.))
                        .flex_none()
                        .rounded_full()
                        .bg(c(ui::faint())),
                )
                .child(div().flex_1().min_w_0().child(t))
        }))
}

/// What taking a bot off the crew throws away for good: its chat with you, in
/// every project, and for a bot you made, what it knows and how it works.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Loss {
    pub messages: usize,
    pub skills: usize,
    pub rules: usize,
    /// A bot you made goes for good; one of the built-in crew can come back.
    pub made: bool,
    /// Its past jobs, which stay in History.
    pub jobs: usize,
}

impl Loss {
    pub(super) fn of(store: &super::store::Store, who: Member) -> Self {
        let made = match who {
            Member::Made(id) => store.made.iter().find(|m| m.id == id),
            _ => None,
        };
        Loss {
            messages: store.chats.get(&who).map_or(0, Vec::len),
            skills: made.map_or(0, |m| m.skills.len()),
            rules: made.map_or(0, |m| m.rules.len()),
            made: made.is_some(),
            jobs: store.jobs.iter().filter(|j| j.bot == who).count(),
        }
    }

    /// Whether removing it loses anything for good, so it's worth asking first.
    pub(super) fn any(&self) -> bool {
        // A bot you made can't be brought back once it's gone.
        self.messages > 0 || self.skills > 0 || self.rules > 0 || self.made
    }

    /// The lines the confirm dialog lists, one per thing that goes.
    pub(super) fn lines(&self, name: &str) -> Vec<String> {
        let plural = |n: usize, one: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {one}s")
            }
        };
        let mut out = Vec::new();
        if self.messages > 0 {
            out.push(format!(
                "Your chat with {name}: {}, in every project",
                plural(self.messages, "message")
            ));
        }
        if self.skills > 0 {
            out.push(format!(
                "What it knows best: {}",
                plural(self.skills, "skill")
            ));
        }
        if self.rules > 0 {
            out.push(format!(
                "How it works: {} it follows on every job",
                plural(self.rules, "rule")
            ));
        }
        if self.made {
            out.push("Its name, look and role".to_string());
        }
        out
    }
}

impl Shell {
    /// Remove, from the crew page: straight away when the bot has nothing of
    /// its own to lose, after a confirm when it has a chat or know-how.
    pub(super) fn ask_remove(&mut self, who: Member, cx: &mut Context<Self>) {
        if self.crew.store.busy(who).is_some() {
            self.say(format!("{} is working. Stop the job first.", who.name()));
            cx.notify();
            return;
        }
        if Loss::of(&self.crew.store, who).any() {
            self.removing = Some(who);
            cx.notify();
        } else {
            self.remove_now(who, cx);
        }
    }

    fn remove_now(&mut self, who: Member, cx: &mut Context<Self>) {
        self.removing = None;
        if who.is_made() {
            self.remove_made(who, cx)
        } else {
            self.set_on_crew(who, false, cx)
        }
    }

    /// The confirm for removing a bot that has a chat or know-how: what goes
    /// for good, and what stays.
    pub(super) fn remove_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let who = self.removing?;
        let loss = Loss::of(&self.crew.store, who);
        let name = who.name();
        let scrim = div()
            .id("remove-scrim")
            .absolute()
            .inset_0()
            .bg(ui::ca(0x000000, 0.28))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.removing = None;
                cx.notify();
            }));
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(10.))
            .bg(c(ui::bad_tone().1))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::bad_tone().0))
                    .child("Removed for good"),
            );
        for line in loss.lines(name) {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .text_color(c(ui::ink()))
                    .child(
                        div()
                            .mt(px(7.))
                            .size(px(5.))
                            .flex_none()
                            .rounded_full()
                            .bg(c(ui::bad_tone().0)),
                    )
                    .child(div().flex_1().child(line)),
            );
        }
        let keeps = match (loss.jobs, loss.made) {
            (0, true) => "It can't be brought back.".to_string(),
            (0, false) => format!("You can add {name} back later, but this can't be recovered."),
            (n, made) => format!(
                "{} past job{} stay{} in History.{}",
                n,
                if n == 1 { "" } else { "s" },
                if n == 1 { "s" } else { "" },
                if made {
                    " The bot itself can't be brought back."
                } else {
                    " You can add it back later, but not what's above."
                }
            ),
        };
        let card = div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_center()
            .child(
                div()
                    .id("remove-card")
                    .w(px(420.))
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .rounded(px(16.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .shadow(vec![BoxShadow {
                        color: ui::ca(0x000000, 0.35),
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
                            .child(self.avatar(who, 32.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(format!("Remove {name} from your crew?")),
                            ),
                    )
                    .child(list)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child(keeps),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(ui::ghost("remove-cancel", "Cancel").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.removing = None;
                                    cx.notify();
                                },
                            )))
                            .child(
                                ui::solid("remove-yes", format!("Remove {name}"))
                                    .bg(c(ui::bad_tone().0))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.remove_now(who, cx)),
                                    ),
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

#[cfg(test)]
mod remove_tests {
    use super::Loss;

    #[test]
    fn nothing_stored_needs_no_confirm() {
        assert!(!Loss::default().any());
        let jobs_only = Loss {
            jobs: 4,
            ..Loss::default()
        };
        assert!(!jobs_only.any());
    }

    #[test]
    fn a_chat_or_know_how_is_listed() {
        let l = Loss {
            messages: 12,
            skills: 1,
            rules: 3,
            made: true,
            jobs: 2,
        };
        assert!(l.any());
        let lines = l.lines("Stripey");
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("12 messages"));
        assert!(lines[1].contains("1 skill"));
        assert!(lines[2].contains("3 rules"));
    }
}
