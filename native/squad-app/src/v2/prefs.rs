//! Settings: a list of panes on the left and the open pane on the right, in the
//! same dark cards, pills and hairlines as the rest of v2.
//!
//! - **General**: how the app behaves while the crew works, and the limits.
//! - **Appearance**: the theme, solid or frosted glass, with a preview of each.
//! - **Mascot**: the crew's look. Peek's outline, and each bot's colour and
//!   props, with a live preview.
//! - **Engines**: which coding agent and model each bot runs on.
//!
//! Every choice is saved the moment it's made.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::fin::{Eyes, FinPose, Ink, Line, Outfit, State, Tool, Weight, ROLES};
use squad_holo::{Figure, Framing, Mood, Tint};

use crate::settings::AppSettings;

use super::data::Member;
use super::engines::{choice, section};
use super::mascot;
use super::theme::{self, Theme};
use super::ui::{self, c, ca, space};
use super::{Place, Shell};

/// A pane of Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    /// How work runs: how often the crew checks with you.
    Work,
    General,
    Appearance,
    Mascot,
    Engines,
    /// What the crew spent and did.
    Usage,
    /// Your own server, for jobs that carry on with this computer closed.
    Server,
}

impl Pane {
    pub fn name(self) -> &'static str {
        match self {
            Pane::Work => "How work runs",
            Pane::General => "General",
            Pane::Appearance => "Appearance",
            Pane::Mascot => "Mascot",
            Pane::Engines => "Engines",
            Pane::Usage => "Usage",
            Pane::Server => "Servers",
        }
    }

    fn line(self) -> &'static str {
        match self {
            Pane::Work => "When the crew checks with you",
            Pane::General => "Alerts, motion and limits",
            Pane::Appearance => "Themes, glass, background and the mascot",
            Pane::Mascot => "Peek, colours and props",
            Pane::Engines => "Agents and models",
            Pane::Usage => "Spend, jobs and time",
            Pane::Server => "Jobs that run with the lid closed",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Pane::Work => IconName::Compass,
            Pane::General => IconName::Settings,
            Pane::Appearance => IconName::Palette,
            Pane::Mascot => IconName::Bot,
            Pane::Engines => IconName::Cpu,
            Pane::Usage => IconName::ChartColumn,
            Pane::Server => IconName::Server,
        }
    }
}

fn save(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    cx.update_global::<AppSettings, _>(|st, _| {
        f(st);
        st.save();
    });
}

/// A small on/off switch: lime with a dark knob when on, a quiet track
/// with a white knob when off.
pub(super) fn switch(on: bool) -> Div {
    div()
        .w(px(30.))
        .h(px(18.))
        .flex_none()
        .rounded_full()
        .bg(c(if on { ui::pick_fill() } else { ui::off() }))
        .border_1()
        .border_color(c(if on {
            super::theme::mix(ui::pick_fill(), ui::ink(), 0.25)
        } else {
            ui::line_hi()
        }))
        .relative()
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 14. } else { 2. }))
                .size(px(12.))
                .rounded_full()
                .bg(c(if on { ui::pick_on() } else { 0xFFFFFF })),
        )
}

/// One setting: what it is and what it does on the left, its control on the
/// right, under a hairline that stops short of the card's edges.
fn row(title: &'static str, line: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .mx(px(14.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(space::M))
        .border_t_1()
        .border_color(c(ui::line()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::body()))
                        .child(line.into()),
                ),
        )
        .child(control)
}

/// A card with a title row, and an optional note on the right.
fn card_head(title: &'static str, note: Option<SharedString>) -> Div {
    ui::card().overflow_hidden().flex().flex_col().child(
        div()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(title),
            )
            .children(note.map(|n| {
                div()
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(n)
            })),
    )
}

/// A settings row led by the bot it's about.
fn face_row(
    face: AnyElement,
    title: &'static str,
    line: &'static str,
    control: impl IntoElement,
) -> Div {
    div()
        .px(px(18.))
        .py(px(13.))
        .flex()
        .items_center()
        .gap(px(14.))
        .border_t_1()
        .border_color(c(ui::line2()))
        .child(div().flex_none().child(face))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(line),
                ),
        )
        .child(control)
}

/// A card of settings rows under a mono heading.
fn group(title: &'static str, rows: Vec<Div>) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .rounded(px(6.))
        .bg(c(ui::raised()))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden();
    for (i, r) in rows.into_iter().enumerate() {
        card = card.child(if i == 0 { r.border_t(px(0.)) } else { r });
    }
    div()
        .flex()
        .flex_col()
        .gap(px(7.))
        .child(
            div()
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c(ui::faint()))
                .child(title.to_uppercase()),
        )
        .child(card)
}

/// A row of options with one picked, on a sunken track.
fn segmented(items: Vec<Stateful<Div>>) -> Div {
    let mut d = div()
        .flex()
        .flex_none()
        .gap(px(3.))
        .p(px(3.))
        .rounded(px(9.))
        .bg(c(ui::bg()))
        .border_1()
        .border_color(c(ui::line()));
    for i in items {
        d = d.child(i);
    }
    d
}

fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .text_color(c(ui::ink()))
            } else {
                d.text_color(c(ui::muted()))
                    .hover(|s| s.text_color(c(ui::ink())))
            }
        })
        .child(label.into())
}

/// The label in front of a row of choices in the Mascot customizer.
fn slot(label: &'static str, items: Div) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(space::SM))
        .child(
            div()
                .w(px(74.))
                .flex_none()
                .pt(px(6.))
                .font_family(ui::MONO)
                .text_size(px(10.5))
                .text_color(c(ui::faint()))
                .child(label),
        )
        .child(items.flex_1().flex().flex_wrap().gap(px(6.)))
}

/// A small picture of a theme: a sidebar and a card on its page, with text lines, a
/// button and the status colours. Frosted themes sit over a colourful backdrop, to
/// show the desktop coming through.
fn swatch(t: &'static Theme) -> Div {
    let line = |w: f32, color: u32| div().h(px(4.)).w(px(w)).rounded_full().bg(c(color));
    let dot = |color: u32| div().size(px(6.)).rounded_full().bg(c(color));
    let page = div()
        .size_full()
        .flex()
        .bg(c(t.bg))
        .child(
            div()
                .w(px(48.))
                .h_full()
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(8.))
                .border_r_1()
                .border_color(c(t.line_c))
                .child(line(28., t.ink))
                .child(div().h(px(10.)).w_full().rounded(px(3.)).bg(c(t.hover)))
                .child(line(22., t.muted))
                .child(line(26., t.faint)),
        )
        .child(
            div().flex_1().p(px(8.)).child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(6.))
                    .bg(c(t.panel))
                    .border_1()
                    .border_color(c(t.line_c))
                    .child(line(64., t.ink))
                    .child(line(84., t.body))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(dot(t.t_blue))
                            .child(dot(t.t_amber))
                            .child(dot(t.t_green))
                            .child(dot(t.t_red)),
                    )
                    .child(
                        div()
                            .mt_auto()
                            .flex()
                            .justify_end()
                            .child(div().h(px(12.)).w(px(34.)).rounded(px(3.)).bg(c(t.fill))),
                    ),
            ),
        );
    div()
        .h(px(112.))
        .w_full()
        .rounded(px(8.))
        .overflow_hidden()
        .relative()
        .bg(c(if t.light { 0xDCE3F2 } else { 0x0B0B10 }))
        .when(t.frosted, |d| {
            d.bg(linear_gradient(
                135.,
                linear_color_stop(c(0xF76B8A), 0.),
                linear_color_stop(c(0x5B7CFA), 1.),
            ))
            .child(
                div()
                    .absolute()
                    .top(px(-18.))
                    .left(px(70.))
                    .size(px(80.))
                    .rounded_full()
                    .bg(c(0xFFC24B)),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(-24.))
                    .left(px(-10.))
                    .size(px(90.))
                    .rounded_full()
                    .bg(c(0x2EC5A1)),
            )
        })
        .child(div().absolute().inset_0().child(page))
}

impl Shell {
    pub(super) fn settings(&mut self, pane: Pane, cx: &mut Context<Self>) -> AnyElement {
        let nav = self.settings_nav(pane, cx);
        let body = match pane {
            Pane::Work => self.settings_work(cx),
            Pane::General => self.settings_general(cx),
            Pane::Appearance => self.settings_appearance(cx),
            Pane::Mascot => self.settings_mascot(cx),
            Pane::Engines => self.engines(cx),
            Pane::Usage => self.usage(cx),
            Pane::Server => self.settings_server(cx),
        };
        div()
            .size_full()
            .flex()
            .child(nav)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(body)),
            )
            .into_any_element()
    }

    /// Settings' own column: its title, then the panes in small groups.
    fn settings_nav(&mut self, pane: Pane, cx: &mut Context<Self>) -> Div {
        let groups: [(&'static str, &[Pane]); 3] = [
            ("CREW", &[Pane::Work, Pane::Engines, Pane::Usage]),
            ("APP", &[Pane::General, Pane::Appearance, Pane::Mascot]),
            ("CONNECTIONS", &[Pane::Server]),
        ];
        let mut col = div()
            .w(px(236.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .px(px(8.))
            .pt(px(14.))
            .border_r_1()
            .border_color(c(ui::line2()))
            .bg(c(ui::bg()))
            .child(
                div()
                    .px(px(4.))
                    .pb(px(16.))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child("Settings"),
            );
        for (label, panes) in groups {
            col = col.child(
                div()
                    .px(px(9.))
                    .pt(px(14.))
                    .pb(px(6.))
                    .font_family(ui::MONO)
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::faint()))
                    .child(label),
            );
            for &p in panes {
                let on = p == pane;
                col = col.child(
                    div()
                        .id(("settings-pane", p as usize))
                        .h(px(31.))
                        .px(px(9.))
                        .mb(px(1.))
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .rounded(px(4.))
                        .border_1()
                        .text_size(px(13.))
                        .cursor_pointer()
                        .map(|d| {
                            if on {
                                // Picked: a raised row with the accent bar,
                                // like the sidebar's.
                                d.relative()
                                    .bg(c(ui::raised()))
                                    .border_color(c(ui::line()))
                                    .text_color(c(ui::ink()))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(-1.))
                                            .top(px(-1.))
                                            .bottom(px(-1.))
                                            .w(px(4.))
                                            .rounded_l(px(4.))
                                            .bg(c(ui::pick_fill())),
                                    )
                            } else {
                                d.border_color(transparent_black())
                                    .text_color(c(ui::body()))
                                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                            }
                        })
                        .child(Icon::new(p.icon()).size(px(15.)))
                        .child(p.name())
                        .tooltip(move |window, cx| Tooltip::new(p.line()).build(window, cx))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // Switching panes stays inside Settings: no page fade, no
                            // extra step on the back trail.
                            this.settings_pane = p;
                            this.place = Place::Settings(p);
                            cx.notify();
                        })),
                );
            }
        }
        col
    }

    pub(super) fn page(
        id: &'static str,
        title: &'static str,
        line: &'static str,
        body: Div,
    ) -> AnyElement {
        div()
            .id(id)
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(980.))
                    .px(px(36.))
                    .pt(px(26.))
                    .pb(px(space::XL + super::DOCK_ROOM))
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .text_size(px(12.5))
                                    .text_color(c(ui::body()))
                                    .child(line),
                            ),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    /// A drop-down: the picked option with a chevron, and the list under it
    /// while open. `set` saves option `i`.
    fn select(
        &mut self,
        id: &'static str,
        labels: Vec<String>,
        cur: usize,
        set: fn(&mut AppSettings, usize),
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.select_open == Some(id);
        let shown = labels.get(cur).cloned().unwrap_or_default();
        let button = div()
            .id(id)
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(4.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(if open {
                ui::accent_bar()
            } else {
                ui::line_hi()
            }))
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c(ui::ink()))
            .cursor_pointer()
            .hover(|s| s.bg(c(ui::hover())))
            .child(shown)
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(12.))
                    .text_color(c(ui::body())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_open = if this.select_open == Some(id) {
                    None
                } else {
                    Some(id)
                };
                cx.notify();
            }));
        let mut d = div().relative().flex_none().child(button);
        if open {
            let mut list = div()
                .id((id, 9999usize))
                .absolute()
                .top(px(32.))
                .right_0()
                .min_w(px(150.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line_hi()))
                .shadow(vec![BoxShadow {
                    color: ca(0x000000, 0.14),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .occlude()
                .flex()
                .flex_col();
            for (i, label) in labels.into_iter().enumerate() {
                let on = i == cur;
                list = list.child(
                    div()
                        .id((id, i))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(c(ui::ink()))
                        .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())))
                        .child(div().flex_1().child(label))
                        .when(on, |d| {
                            d.child(
                                Icon::new(IconName::Check)
                                    .size(px(12.))
                                    .text_color(c(ui::accent_text())),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            save(cx, |st| set(st, i));
                            this.select_open = None;
                            cx.notify();
                        })),
                );
            }
            d = d.child(deferred(list).with_priority(2));
        }
        d
    }

    /// How work runs: one choice for how often the crew checks with you, the
    /// switches it sets, and a preview of what an ask looks like.
    fn settings_work(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppSettings>().clone();
        let level = match (st.v2_plan_first, st.v2_auto_merge) {
            (true, false) => "a_lot",
            (false, false) => "balanced",
            (false, true) => "hands_off",
            _ => "",
        };
        let levels: [(&'static str, &'static str, &'static str, usize); 3] = [
            (
                "a_lot",
                "Ask a lot",
                "Peek's plan, the changes and the merge wait for you.",
                3,
            ),
            (
                "balanced",
                "Balanced",
                "You approve changes and press merge.",
                2,
            ),
            (
                "hands_off",
                "Hands off",
                "Approved PRs merge once checks pass.",
                1,
            ),
        ];
        let mut cards = div().flex().gap(px(10.)).px(px(18.)).pb(px(16.));
        for (i, (key, name, line, bars)) in levels.into_iter().enumerate() {
            let on = level == key;
            let mut meter = div().flex().gap(px(3.));
            for b in 0..3 {
                meter = meter.child(div().w(px(16.)).h(px(5.)).rounded(px(2.)).bg(c(
                    if b < bars {
                        ui::pick_fill()
                    } else {
                        ui::press()
                    },
                )));
            }
            cards = cards.child(
                div()
                    .id(("work-level", i))
                    .flex_1()
                    .min_w_0()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .rounded(px(12.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::pick())).bg(c(super::theme::mix(
                                ui::raised(),
                                ui::pick_fill(),
                                ui::pick_k(0.05),
                            )))
                        } else {
                            d.border_color(c(ui::line()))
                                .hover(|s| s.border_color(c(ui::line_hi())))
                        }
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(16.))
                                    .rounded_full()
                                    .border_color(c(if on { ui::pick() } else { ui::line_hi() }))
                                    .map(|d| if on { d.border(px(5.)) } else { d.border_1() }),
                            )
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(name),
                            )
                            .when(on, |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded(px(5.))
                                        .bg(c(super::theme::mix(
                                            ui::bg(),
                                            ui::pick_fill(),
                                            ui::pick_k(0.12),
                                        )))
                                        .font_family(ui::MONO)
                                        .text_size(px(10.5))
                                        .text_color(c(ui::pick_text()))
                                        .child("Picked"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(c(ui::muted()))
                            .child(line),
                    )
                    .child(meter)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            st.v2_ask = key.to_string();
                            st.v2_plan_first = key == "a_lot";
                            st.v2_auto_merge = key == "hands_off";
                        });
                        cx.notify();
                    })),
            );
        }
        let ask = card_head("When should the crew ask you?", None).child(cards);

        let peek = self.avatar(Member::Fin, 24.).into_any_element();
        let builder = self.avatar(Member::Builder, 24.).into_any_element();
        let shipper = self.avatar(Member::Shipper, 24.).into_any_element();
        let toggle = |id: &'static str,
                      face: AnyElement,
                      title: &'static str,
                      line: &'static str,
                      on: bool,
                      set: fn(&mut AppSettings, bool),
                      cx: &mut Context<Self>| {
            face_row(
                face,
                title,
                line,
                div()
                    .id(id)
                    .cursor_pointer()
                    .child(switch(on))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            set(st, !on);
                            st.v2_ask = match (st.v2_plan_first, st.v2_auto_merge) {
                                (true, false) => "a_lot",
                                (false, false) => "balanced",
                                (false, true) => "hands_off",
                                _ => "custom",
                            }
                            .into();
                        });
                        cx.notify();
                    })),
            )
        };
        let set_by = match level {
            "a_lot" => "Set by Ask a lot \u{00b7} change any",
            "balanced" => "Set by Balanced \u{00b7} change any",
            "hands_off" => "Set by Hands off \u{00b7} change any",
            _ => "Your own mix",
        };
        let checkpoints = card_head("Checkpoints", Some(set_by.into()))
            .child(toggle(
                "work-plan-first",
                peek,
                "Approve Peek's plan first",
                "Peek posts a plan card. Nothing starts until you press Start.",
                st.v2_plan_first,
                |st, on| st.v2_plan_first = on,
                cx,
            ))
            .child(face_row(
                builder,
                "Changes wait for you",
                "Every job that changes files stops in Review until you approve it.",
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded(px(5.))
                            .bg(c(ui::well()))
                            .font_family(ui::MONO)
                            .text_size(px(11.))
                            .text_color(c(ui::muted()))
                            .child("Always on"),
                    )
                    .child(div().opacity(0.45).child(switch(true))),
            ))
            .child(toggle(
                "work-auto-merge",
                shipper,
                "Merge on its own",
                "A pull request Shipper opens merges once its checks pass.",
                st.v2_auto_merge,
                |st, on| st.v2_auto_merge = on,
                cx,
            ));

        let main = Self::page(
            "settings-work",
            "How work runs",
            "Decide how much the crew checks with you. It applies to every project.",
            div()
                .flex()
                .flex_col()
                .gap(px(space::M))
                .child(ask)
                .child(checkpoints),
        );
        let rail = self.work_preview(&st, cx);
        div()
            .size_full()
            .flex()
            .child(div().flex_1().min_w_0().h_full().child(main))
            .child(rail)
            .into_any_element()
    }

    /// What asking for something looks like with these settings, step by step,
    /// with the steps that wait for you marked.
    fn work_preview(&mut self, st: &AppSettings, cx: &mut Context<Self>) -> Div {
        use super::data::Member;
        let mut steps: Vec<(Option<Member>, &str, &str, bool)> =
            vec![(None, "You ask in a thread", "", false)];
        if st.v2_plan_first {
            steps.push((
                Some(Member::Fin),
                "Peek posts a plan",
                "You press Start",
                true,
            ));
        }
        steps.push((
            Some(Member::Builder),
            "Builder writes it",
            "On its own branch",
            false,
        ));
        steps.push((
            Some(Member::Builder),
            "The changes wait",
            "You approve them",
            true,
        ));
        if st.v2_auto_merge {
            steps.push((
                Some(Member::Shipper),
                "Shipper opens the PR",
                "Merges once checks pass",
                false,
            ));
        } else {
            steps.push((
                Some(Member::Shipper),
                "Shipper opens the PR",
                "You press merge",
                true,
            ));
        }
        let asks = steps.iter().filter(|s| s.3).count();
        let steps_alone = steps.len() - 1 - asks;
        let mut flow = div().flex().flex_col().gap(px(8.));
        for (who, what, sub, yours) in steps {
            let face: AnyElement = match who {
                Some(m) => self.avatar(m, 26.).into_any_element(),
                None => div()
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(c(super::theme::mix(
                        ui::raised(),
                        ui::pick_fill(),
                        ui::pick_k(0.14),
                    )))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(ui::pick_text()))
                    .child("Y")
                    .into_any_element(),
            };
            flow = flow.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().w(px(28.)).flex().justify_center().child(face))
                    .child(
                        div()
                            .flex_1()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(px(9.))
                            .border_1()
                            .map(|d| {
                                if yours {
                                    d.border_color(c(ui::ask_tone().2)).bg(c(ui::ask_tone().1))
                                } else {
                                    d.border_color(c(ui::line())).bg(c(ui::raised()))
                                }
                            })
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(what),
                            )
                            .when(!sub.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(c(if yours {
                                            ui::t_amber()
                                        } else {
                                            ui::muted()
                                        }))
                                        .child(sub),
                                )
                            }),
                    ),
            );
        }
        let alone = steps_alone;
        let balanced = !st.v2_plan_first && !st.v2_auto_merge;
        let stat = |n: String, what: &'static str| {
            div()
                .flex_1()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(n),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(c(ui::muted()))
                        .child(what),
                )
        };
        div()
            .w(px(330.))
            .h_full()
            .flex_none()
            .px(px(18.))
            .pt(px(26.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .bg(c(ui::ground()))
            .border_l_1()
            .border_color(c(ui::line2()))
            .child(
                div()
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .text_color(c(ui::faint()))
                            .child("PREVIEW"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("What \u{201c}Add gift cards\u{201d} looks like"),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .text_color(c(ui::muted()))
                            .child("Updates as you change settings."),
                    ),
            )
            .child(flow)
            .child(
                ui::card()
                    .px(px(16.))
                    .py(px(14.))
                    .flex()
                    .gap(px(12.))
                    .child(stat(
                        asks.to_string(),
                        if asks == 1 {
                            "time you're asked"
                        } else {
                            "times you're asked"
                        },
                    ))
                    .child(stat(alone.to_string(), "steps the crew runs alone")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .child("Changes save as you make them."),
                    )
                    .when(!balanced, |d| {
                        d.child(
                            ui::ghost("work-reset", "Reset to Balanced").on_click(cx.listener(
                                |_, _, _, cx| {
                                    save(cx, |st| {
                                        st.v2_ask = "balanced".into();
                                        st.v2_plan_first = false;
                                        st.v2_auto_merge = false;
                                    });
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            )
    }

    fn settings_general(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppSettings>().clone();
        // A switch; a sound's switch plays it when turned on, so you hear it.
        let toggle = |id: &'static str,
                      title: &'static str,
                      line: &'static str,
                      on: bool,
                      set: fn(&mut AppSettings, bool),
                      cue: Option<crate::sounds::Cue>,
                      cx: &mut Context<Self>| {
            row(
                title,
                line,
                div()
                    .id(id)
                    .cursor_pointer()
                    .child(switch(on))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| set(st, !on));
                        if let (false, Some(cue)) = (on, cue) {
                            let style = cx.global::<AppSettings>().sound_style.clone();
                            crate::sounds::preview(crate::sounds::Style::parse(&style), cue);
                        }
                        cx.notify();
                    })),
            )
        };
        use crate::sounds::{Cue, Style};

        let picked = Style::parse(&st.sound_style);
        let style_at = Style::ALL.iter().position(|s| *s == picked).unwrap_or(0);
        let style_pick = self.select(
            "set-sound-style",
            Style::ALL.iter().map(|s| s.label().to_string()).collect(),
            style_at,
            |st, i| {
                let style = Style::ALL[i];
                st.sound_style = style.id().to_string();
                crate::sounds::preview(style, Cue::NeedsYou);
            },
            cx,
        );
        let sounds_on = st.sounds;
        let sounds = row(
            "Sounds",
            format!(
                "{}: {}",
                picked.label(),
                match picked {
                    Style::Bubble => "quick blips that slide in pitch",
                    Style::Marimba => "warm, woody notes",
                    Style::Pluck => "a soft plucked string",
                    Style::Glass => "airy bells with a shimmer",
                }
            ),
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(style_pick)
                .child(
                    div()
                        .id("set-sounds")
                        .cursor_pointer()
                        .child(switch(sounds_on))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            save(cx, |st| st.sounds = !sounds_on);
                            if !sounds_on {
                                let style = cx.global::<AppSettings>().sound_style.clone();
                                crate::sounds::preview(Style::parse(&style), Cue::NeedsYou);
                            }
                            cx.notify();
                        })),
                ),
        );

        // Drop-downs over a few sensible values, keeping one set by hand.
        fn options(mut v: Vec<u32>, cur: u32) -> (Vec<u32>, usize) {
            if !v.contains(&cur) {
                v.push(cur);
                v.sort();
            }
            let at = v.iter().position(|x| *x == cur).unwrap_or(0);
            (v, at)
        }
        let (stuck, stuck_at) = options(vec![0, 5, 10, 15, 20, 30, 60], st.stuck_after_mins);
        let stuck_labels = stuck
            .iter()
            .map(|m| match m {
                0 => "Never".to_string(),
                1 => "1 minute".to_string(),
                m => format!("{m} minutes"),
            })
            .collect();
        let stuck_row = row(
            "Flag a stuck bot after",
            "A bot quiet this long is marked so you can look in",
            self.select(
                "set-stuck",
                stuck_labels,
                stuck_at,
                |st, i| {
                    let (v, _) = options(vec![0, 5, 10, 15, 20, 30, 60], st.stuck_after_mins);
                    st.stuck_after_mins = v[i];
                },
                cx,
            ),
        );
        let dollars = |v: &Vec<u32>| {
            v.iter()
                .map(|d| {
                    if *d == 0 {
                        "No limit".to_string()
                    } else {
                        format!("${d}")
                    }
                })
                .collect::<Vec<_>>()
        };
        let (day, day_at) = options(vec![0, 5, 10, 20, 50, 100], st.daily_limit_usd);
        let (job, job_at) = options(vec![0, 1, 2, 5, 10, 20], st.session_limit_usd);
        let day_pick = self.select(
            "set-day-limit",
            dollars(&day),
            day_at,
            |st, i| {
                let (v, _) = options(vec![0, 5, 10, 20, 50, 100], st.daily_limit_usd);
                st.daily_limit_usd = v[i];
            },
            cx,
        );
        let job_pick = self.select(
            "set-job-limit",
            dollars(&job),
            job_at,
            |st, i| {
                let (v, _) = options(vec![0, 1, 2, 5, 10, 20], st.session_limit_usd);
                st.session_limit_usd = v[i];
            },
            cx,
        );
        let motion = self.select(
            "set-motion",
            vec!["Full".into(), "Off".into()],
            if st.animations { 0 } else { 1 },
            |st, i| st.animations = i == 0,
            cx,
        );

        let alerts = group(
            "Alerts",
            vec![
                toggle(
                    "set-notify",
                    "Desktop notifications",
                    "When a job finishes or a bot needs you",
                    st.notifications,
                    |s, v| s.notifications = v,
                    None,
                    cx,
                ),
                sounds,
                toggle(
                    "set-sound-ask",
                    "When a bot needs you",
                    "It asks for your OK or a choice",
                    st.sound_needs_you,
                    |s, v| s.sound_needs_you = v,
                    Some(Cue::NeedsYou),
                    cx,
                )
                .when(!sounds_on, |d| d.opacity(0.5)),
                toggle(
                    "set-sound-done",
                    "When a job is done",
                    "Or its changes are ready to review",
                    st.sound_done,
                    |s, v| s.sound_done = v,
                    Some(Cue::Done),
                    cx,
                )
                .when(!sounds_on, |d| d.opacity(0.5)),
                toggle(
                    "set-sound-failed",
                    "When a job fails",
                    "It stops with a problem",
                    st.sound_failed,
                    |s, v| s.sound_failed = v,
                    Some(Cue::Failed),
                    cx,
                )
                .when(!sounds_on, |d| d.opacity(0.5)),
                stuck_row,
            ],
        );
        let work = group(
            "While the crew works",
            vec![
                toggle(
                    "set-awake",
                    "Keep the computer awake",
                    "While a job runs",
                    st.keep_awake,
                    |s, v| s.keep_awake = v,
                    None,
                    cx,
                ),
                toggle(
                    "set-credit",
                    "Credit the agent in commits",
                    "Adds a line naming the agent",
                    st.attribution,
                    |s, v| s.attribution = v,
                    None,
                    cx,
                ),
                toggle(
                    "set-confirm",
                    "Ask before deleting",
                    "Before a job and its work go",
                    st.confirm_delete,
                    |s, v| s.confirm_delete = v,
                    None,
                    cx,
                ),
                toggle(
                    "set-tidy",
                    "Remove the worktree with the job",
                    "Its copy of the code goes too",
                    st.remove_worktree_on_delete,
                    |s, v| s.remove_worktree_on_delete = v,
                    None,
                    cx,
                ),
            ],
        );
        let limits = group(
            "Spending limits",
            vec![
                row(
                    "Each day",
                    "Jobs wait once the crew has spent this",
                    day_pick,
                ),
                row("Each job", "A job stops and asks you at this", job_pick),
            ],
        );
        let look = group(
            "Look and feel",
            vec![row(
                "Animations",
                "Breathing borders, feed fade and Peek",
                motion,
            )],
        );
        Self::page(
            "settings-general",
            "General",
            "Alerts, motion and limits.",
            div()
                .flex()
                .items_start()
                .gap(px(22.))
                .child(div().flex_1().min_w_0().child(alerts))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(20.))
                        .child(work)
                        .child(limits)
                        .child(look),
                ),
        )
    }

    fn settings_appearance(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = ui::theme().id;
        let card_for = |t: &'static Theme, cx: &mut Context<Self>| {
            let on = t.id == current;
            div()
                .id(SharedString::from(format!("theme-{}", t.id)))
                .w(px(206.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(10.))
                .rounded(px(12.))
                .border_1()
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.border_color(c(ui::pick())).bg(c(ui::panel()))
                    } else {
                        d.border_color(c(ui::line()))
                            .bg(c(ui::panel()))
                            .hover(|s| s.border_color(c(ui::line_hi())))
                    }
                })
                .child(swatch(t))
                .child(
                    div()
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(c(ui::ink()))
                                        .child(t.name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(c(ui::muted()))
                                        .truncate()
                                        .child(t.line),
                                ),
                        )
                        .when(on, |d| {
                            d.child(
                                div()
                                    .size(px(20.))
                                    .flex_none()
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(c(ui::pick_fill()))
                                    .text_color(c(ui::pick_on()))
                                    .child(Icon::new(IconName::Check).size(px(12.))),
                            )
                        }),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    ui::set_theme(t.id);
                    save(cx, |st| st.v2_theme = t.id.to_string());
                    cx.notify();
                }))
        };
        let grid = |frosted: bool, cx: &mut Context<Self>| {
            let mut d = div().flex().flex_wrap().gap(px(space::SM));
            // The redesign's looks have their own picker above.
            for t in ui::THEMES
                .iter()
                .filter(|t| t.frosted == frosted && theme::family_of(t.id).is_none())
            {
                d = d.child(card_for(t, cx));
            }
            d
        };
        let heading = |title: &'static str, line: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(ui::ink()))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(ui::muted()))
                        .child(line),
                )
        };
        let looks = self.looks_card(cx);
        let solid = grid(false, cx);
        let frosted = grid(true, cx);
        let window = self.transparency_card(cx);
        let picture = self.image_card(cx);
        let backdrop = self.pattern_card(cx);
        Self::page(
            "settings-appearance",
            "Appearance",
            "How Squad Code looks. A theme applies the moment you pick it.",
            div()
                .flex()
                .flex_col()
                .gap(px(space::L))
                // Themes first: the choice most people come here for.
                .child(looks)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(space::SM))
                        .child(heading(
                            "More themes",
                            if theme::see_through() {
                                "Opaque by design, see-through while the window switch is on."
                            } else {
                                "An opaque window, quickest to draw."
                            },
                        ))
                        .child(solid),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(space::SM))
                        .child(heading(
                            "Frosted",
                            "A see-through window over your desktop, blurred like frosted glass.",
                        ))
                        .child(frosted),
                )
                .child(window)
                .child(picture)
                .child(backdrop),
        )
    }

    /// The three looks, each light and dark, and whether to match the system.
    pub(super) fn looks_card(&mut self, cx: &mut Context<Self>) -> Div {
        let current = ui::theme();
        let family = theme::family_of(current.id);
        let follow = theme::following();
        let dark = !current.light;
        let by_id = |id: &str| ui::THEMES.iter().find(|t| t.id == id);
        let mut cards = div().flex().flex_wrap().gap(px(space::SM));
        for f in theme::FAMILIES.iter() {
            let on = family.is_some_and(|x| x.id == f.id);
            let side = if dark { f.dark } else { f.light };
            let Some(shown) = by_id(side) else { continue };
            cards = cards.child(
                div()
                    .id(SharedString::from(format!("look-{}", f.id)))
                    .w(px(206.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(px(12.))
                    .border_1()
                    .cursor_pointer()
                    .bg(c(ui::panel()))
                    .map(|d| {
                        if on {
                            d.border_color(c(ui::pick()))
                        } else {
                            d.border_color(c(ui::line()))
                                .hover(|s| s.border_color(c(ui::line_hi())))
                        }
                    })
                    .child(swatch(shown))
                    .child(
                        div()
                            .px(px(4.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(c(ui::ink()))
                                            .child(f.name),
                                    )
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_size(px(10.5))
                                            .font_family(ui::MONO)
                                            .text_color(c(ui::faint()))
                                            .child("LIGHT · DARK"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(c(ui::muted()))
                                    .truncate()
                                    .child(f.line),
                            ),
                    )
                    .on_click(cx.listener(move |_, _, window, cx| {
                        let pref = if follow {
                            format!("{}{}", f.id, theme::SYSTEM)
                        } else if dark {
                            f.dark.to_string()
                        } else {
                            f.light.to_string()
                        };
                        theme::set_pref(&pref, super::system_dark(window.appearance()));
                        save(cx, |st| st.v2_theme = pref);
                        cx.notify();
                    })),
            );
        }
        // Light, dark or the system's, for the look in use (or the default
        // look, while an older theme is picked).
        let fam = family.unwrap_or(&theme::FAMILIES[0]);
        let mode = |id: &'static str, label: &'static str, on: bool, pref: String| {
            segment(id, label, on).on_click(cx.listener(move |_, _, window, cx| {
                theme::set_pref(&pref, super::system_dark(window.appearance()));
                let pref = pref.clone();
                save(cx, |st| st.v2_theme = pref);
                cx.notify();
            }))
        };
        let in_family = family.is_some();
        let modes = segmented(vec![
            mode(
                "look-light",
                "Light",
                in_family && !follow && !dark,
                fam.light.to_string(),
            ),
            mode(
                "look-dark",
                "Dark",
                in_family && !follow && dark,
                fam.dark.to_string(),
            ),
            mode(
                "look-system",
                "Match the system",
                in_family && follow,
                format!("{}{}", fam.id, theme::SYSTEM),
            ),
        ]);
        let note = if follow {
            if dark {
                "Every theme has both. Right now it's dark, because your system is."
            } else {
                "Every theme has both. Right now it's light, because your system is."
            }
        } else {
            "Every theme has both. Pick one, or follow your system as it changes."
        };
        div()
            .flex()
            .flex_col()
            .gap(px(space::SM))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::ink()))
                            .child("Theme"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(ui::muted()))
                            .child("Three looks to start, each in light and dark."),
                    ),
            )
            .child(cards)
            .child(group(
                "Light or dark",
                vec![row("Light or dark", note, modes)],
            ))
    }

    /// Window transparency: a switch, and a slider for how far the desktop shows
    /// through. It works with every theme; a frosted one just starts see-through.
    fn transparency_card(&mut self, cx: &mut Context<Self>) -> Div {
        let on = theme::see_through();
        let clear = (theme::clear_amount() * 100.).round() as u8;
        let frosted = ui::theme().frosted;
        // A background image covers the desktop while it's set.
        let covered = theme::image_on();
        let switch_row = row(
            "See-through window",
            if covered {
                "Your background image covers the desktop while it's set, so this waits until it's gone."
            } else if frosted {
                "This theme is frosted glass already. Turn this on to clear it further."
            } else {
                "Let your desktop show through the window, blurred, in any theme."
            },
            div()
                .id("see-through")
                .cursor_pointer()
                .child(switch(on))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let clear = this.clear_slider.read(cx).value().end().round() as u8;
                    theme::set_see_through(!on, clear);
                    save(cx, |st| {
                        st.v2_see_through = !on;
                        st.v2_clear = clear;
                    });
                    cx.notify();
                })),
        );
        let label = |text: &'static str| {
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(text)
        };
        let slider_row = row(
            "Transparency",
            "Quieter text brightens as the window clears, so everything stays readable.",
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(label("Solid"))
                .child(
                    div().w(px(180.)).child(
                        gpui_kit::component::slider::Slider::new(&self.clear_slider)
                            .disabled(!on)
                            .bg(c(if on { ui::pick_fill() } else { ui::line_hi() }))
                            .text_color(c(ui::pick_on())),
                    ),
                )
                .child(label("Clear"))
                .child(
                    div()
                        .w(px(38.))
                        .flex_none()
                        .text_right()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .text_color(c(if on { ui::ink() } else { ui::faint() }))
                        .child(if on {
                            format!("{clear}%")
                        } else {
                            "Off".to_string()
                        }),
                ),
        )
        .when(!on || covered, |d| d.opacity(0.6));
        let switch_row = switch_row.when(covered, |d| d.opacity(0.6));
        // Popovers follow the window: only meaningful while something shows through.
        let clear_window = theme::layered();
        let pops = theme::glass_popovers();
        let pop_clear = (theme::pop_amount() * 100.).round() as u8;
        let pop_live = clear_window && pops;
        let pop_row = row(
            "See-through popovers",
            if covered {
                "Menus, popovers, the command palette and dialogs go see-through over your background image."
            } else if clear_window {
                "Menus, popovers, the command palette and dialogs go see-through with the window."
            } else {
                "Menus, popovers, the command palette and dialogs follow the window once it's see-through."
            },
            div()
                .id("glass-popovers")
                .cursor_pointer()
                .child(switch(pops))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let clear = this.pop_slider.read(cx).value().end().round() as u8;
                    theme::set_glass_popovers(!pops, clear);
                    save(cx, |st| {
                        st.v2_glass_popovers = !pops;
                        st.v2_pop_clear = clear;
                    });
                    cx.notify();
                })),
        )
        .when(!clear_window, |d| d.opacity(0.6));
        let pop_slider_row = row(
            "Popover transparency",
            "Separate from the window's. Even at its clearest a popover stays opaque enough to read.",
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(label("Solid"))
                .child(
                    div().w(px(180.)).child(
                        gpui_kit::component::slider::Slider::new(&self.pop_slider)
                            .disabled(!pop_live)
                            .bg(c(if pop_live { ui::pick_fill() } else { ui::line_hi() }))
                            .text_color(c(ui::pick_on())),
                    ),
                )
                .child(label("Clear"))
                .child(
                    div()
                        .w(px(38.))
                        .flex_none()
                        .text_right()
                        .font_family(ui::MONO)
                        .text_size(px(12.))
                        .text_color(c(if pop_live { ui::ink() } else { ui::faint() }))
                        .child(if pop_live {
                            format!("{pop_clear}%")
                        } else {
                            "Off".to_string()
                        }),
                ),
        )
        .when(!pop_live, |d| d.opacity(0.6));
        group(
            "WINDOW",
            vec![switch_row, slider_row, pop_row, pop_slider_row],
        )
    }

    /// A background image under the whole window: one of a few that ship with the
    /// app, or a picture of your own, with how it fits, how blurred and dimmed it is,
    /// and how far it shows through. The window itself is the preview: every choice
    /// shows the moment it's made, and the sliders as you drag them.
    fn image_card(&mut self, cx: &mut Context<Self>) -> Div {
        use super::wallpaper::{self as wp, Fit, Source};
        wp::make_previews(cx);
        let st = cx.global::<AppSettings>().clone();
        let current = Source::parse(&st.v2_bg_image);
        let on = current.is_some() && theme::image_on();
        let tile_box = |selected: bool| {
            div()
                .h(px(64.))
                .w_full()
                .relative()
                .overflow_hidden()
                .rounded(px(8.))
                .bg(c(ui::bg()))
                .border_1()
                .border_color(c(if selected { ui::pick() } else { ui::line() }))
                .when(selected, |d| d.border_2())
        };
        let caption = |text: SharedString, selected: bool| {
            div()
                .text_size(px(12.))
                .truncate()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(c(if selected { ui::ink() } else { ui::muted() }))
                .child(text)
        };
        // The tile's overflow_hidden clips to a plain rectangle, not to its rounded
        // corners, so the picture rounds its own corners to match the inside edge of
        // the border (the tile's 8px less the border's width), or its square corners
        // poke out past the curve.
        let shot = |path: Option<std::path::PathBuf>, selected: bool| {
            path.map(|p| {
                img(p)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(if selected { 6. } else { 7. }))
                    .object_fit(ObjectFit::Cover)
            })
        };
        let mut tiles = div().flex().flex_wrap().gap(px(10.));
        // None: the theme's own ground.
        tiles = tiles.child(
            div()
                .id("bg-image-none")
                .w(px(104.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .cursor_pointer()
                .child(
                    tile_box(current.is_none())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(c(ui::faint()))
                                .child("Plain"),
                        ),
                )
                .child(caption("None".into(), current.is_none()))
                .on_click(cx.listener(|_, _, _, cx| {
                    save(cx, |st| st.v2_bg_image.clear());
                    wp::refresh(cx);
                })),
        );
        let sources: Vec<(Source, SharedString)> = wp::BUILTINS
            .iter()
            .map(|b| (Source::Builtin(b), SharedString::from(b.label)))
            .chain(
                wp::mine()
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| (Source::Own(p), format!("Yours {}", i + 1).into())),
            )
            .collect();
        for (i, (source, label)) in sources.into_iter().enumerate() {
            let selected = current.as_ref() == Some(&source);
            let own = match &source {
                Source::Own(p) => Some(p.clone()),
                Source::Builtin(_) => None,
            };
            let group = SharedString::from(format!("bg-tile-{i}"));
            let setting = source.setting();
            let mut frame = tile_box(selected).children(shot(wp::preview(&source), selected));
            if let Some(path) = own {
                frame = frame.child(
                    div()
                        .id(("bg-image-remove", i))
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(18.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(ca(0x000000, 0.55))
                        .text_color(c(0xFFFFFF))
                        .invisible()
                        .group_hover(group.clone(), |d| d.visible())
                        .cursor_pointer()
                        .child(Icon::new(IconName::X).size(px(11.)))
                        .tooltip(|window, cx| Tooltip::new("Remove this picture").build(window, cx))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            let gone = path.to_string_lossy().into_owned();
                            if cx.global::<AppSettings>().v2_bg_image == gone {
                                save(cx, |st| st.v2_bg_image.clear());
                                wp::refresh(cx);
                            }
                            wp::remove(&path);
                            cx.notify();
                        })),
                );
            }
            tiles = tiles.child(
                div()
                    .id(("bg-image", i))
                    .group(group)
                    .w(px(104.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .cursor_pointer()
                    .child(frame)
                    .child(caption(label, selected))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let setting = setting.clone();
                        save(cx, |st| st.v2_bg_image = setting);
                        wp::refresh(cx);
                    })),
            );
        }
        // Your own: a picture from disk.
        tiles = tiles.child(
            div()
                .id("bg-image-add")
                .w(px(104.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .cursor_pointer()
                .child(
                    tile_box(false)
                        .bg(c(ui::panel()))
                        .hover(|d| d.bg(c(ui::hover())))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_color(c(ui::muted()))
                        .child(Icon::new(IconName::Plus).size(px(16.)))
                        .child(div().text_size(px(11.5)).child("Choose…")),
                )
                .child(caption("Your own".into(), false))
                .on_click(cx.listener(|this, _, _, cx| this.pick_background(cx))),
        );

        let fit = Fit::parse(&st.v2_bg_fit);
        let mut fits = div().flex_none().flex().gap(px(6.));
        for (i, f) in Fit::ALL.into_iter().enumerate() {
            fits = fits.child(
                ui::pill(("bg-fit", i), f.label(), f == fit).on_click(cx.listener(
                    move |_, _, _, cx| {
                        save(cx, |st| st.v2_bg_fit = f.id().to_string());
                        let dim = cx.global::<AppSettings>().v2_bg_dim;
                        wp::set_look(f, dim);
                        cx.notify();
                    },
                )),
            );
        }
        let fit_row = row(
            "Fit",
            match fit {
                Fit::Fill => "Covers the window, cropping the edges that don't fit.",
                Fit::Contain => "Shows all of the picture, with the theme's colour beside it.",
                Fit::Stretch => "Stretches the picture to the window's shape.",
                Fit::Centre => "Shows the picture at its own size, in the middle.",
            },
            fits,
        );
        let label = |text: &'static str| {
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(c(ui::faint()))
                .child(text)
        };
        let slider_row = |title: &'static str,
                          line: &'static str,
                          slider: &Entity<gpui_kit::component::slider::SliderState>,
                          ends: (&'static str, &'static str),
                          cx: &mut Context<Self>| {
            let value = slider.read(cx).value().end().round() as u8;
            row(
                title,
                line,
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label(ends.0))
                    .child(
                        div().w(px(180.)).child(
                            gpui_kit::component::slider::Slider::new(slider)
                                .disabled(!on)
                                .bg(c(if on { ui::pick_fill() } else { ui::line_hi() }))
                                .text_color(c(ui::pick_on())),
                        ),
                    )
                    .child(label(ends.1))
                    .child(
                        div()
                            .w(px(38.))
                            .flex_none()
                            .text_right()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(if on { ui::ink() } else { ui::faint() }))
                            .child(if on {
                                format!("{value}%")
                            } else {
                                "Off".to_string()
                            }),
                    ),
            )
        };
        let blur_row = slider_row(
            "Blur",
            "Softens a busy picture. The blurred copy is made when you let go.",
            &self.bg_blur_slider.clone(),
            ("Sharp", "Soft"),
            cx,
        );
        let dim_row = slider_row(
            "Dim",
            "Fades the picture into the theme's own colour.",
            &self.bg_dim_slider.clone(),
            ("Vivid", "Faint"),
            cx,
        );
        let show_row = slider_row(
            "Show through",
            "How far the page and the sidebar clear over the picture. Text brightens as they do, so it stays readable.",
            &self.bg_show_slider.clone(),
            ("Less", "More"),
            cx,
        );
        let rows = [fit_row, blur_row, dim_row, show_row]
            .into_iter()
            .map(|r| r.when(!on, |d| d.opacity(0.6)));
        group(
            "BACKGROUND IMAGE",
            std::iter::once(div().px(px(18.)).pb(px(14.)).child(tiles))
                .chain(rows)
                .collect(),
        )
    }

    /// Asks for a picture and makes it the background, copying it into the data
    /// folder first so it stays if the original moves.
    fn pick_background(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a background image".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.first().cloned() else {
                return;
            };
            let added = cx
                .background_executor()
                .spawn(async move { super::wallpaper::add(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match added {
                    Ok(to) => {
                        save(cx, |st| st.v2_bg_image = to.to_string_lossy().into_owned());
                        super::wallpaper::refresh(cx);
                    }
                    Err(why) => this.say(format!("That picture couldn't be used. {why}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A background pattern behind the page, and the sidebar if you like: a fine
    /// motif in the theme's ink, picked from a row of live previews.
    fn pattern_card(&mut self, cx: &mut Context<Self>) -> Div {
        use crate::pattern::{self as pat, Pattern};
        let st = cx.global::<AppSettings>().clone();
        let current = pat::current();
        let apply = |st: &AppSettings| {
            pat::set(
                &st.pattern,
                st.pattern_content,
                st.pattern_panels,
                st.pattern_frame,
            )
        };
        let mut tiles = div().flex().flex_wrap().gap(px(10.));
        for p in Pattern::ALL {
            let on = p == current;
            tiles = tiles.child(
                div()
                    .id(SharedString::from(format!("pattern-{}", p.id())))
                    .w(px(104.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .cursor_pointer()
                    .child(
                        div()
                            .h(px(64.))
                            .w_full()
                            .relative()
                            .overflow_hidden()
                            .rounded(px(8.))
                            .bg(c(ui::bg()))
                            .border_1()
                            .border_color(c(if on { ui::pick() } else { ui::line() }))
                            .when(on, |d| d.border_2())
                            .child(ui::pattern_of(p, 2.))
                            .when(p == Pattern::Off, |d| {
                                d.flex().items_center().justify_center().child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(c(ui::faint()))
                                        .child("Plain"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(if on {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(c(if on { ui::ink() } else { ui::muted() }))
                            .child(p.label()),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        save(cx, |st| {
                            st.pattern = p.id().to_string();
                            // The page always shows a pattern you pick.
                            st.pattern_content = true;
                        });
                        apply(cx.global::<AppSettings>());
                        cx.notify();
                    })),
            );
        }
        let sidebar_on = st.pattern_panels;
        let sidebar_row = row(
            "On the sidebar too",
            "Carry the pattern under the sidebar as well as the page.",
            div()
                .id("pattern-sidebar")
                .cursor_pointer()
                .child(switch(sidebar_on))
                .on_click(cx.listener(move |_, _, _, cx| {
                    save(cx, |st| st.pattern_panels = !sidebar_on);
                    apply(cx.global::<AppSettings>());
                    cx.notify();
                })),
        )
        .when(current == Pattern::Off, |d| d.opacity(0.6));
        group(
            "BACKGROUND PATTERN",
            vec![div().px(px(18.)).pb(px(14.)).child(tiles), sidebar_row],
        )
    }

    fn settings_mascot(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let st = cx.global::<AppSettings>().clone();
        let crew = Member::all();
        if !crew.contains(&self.fin_sel) {
            self.fin_sel = crew.first().copied().unwrap_or(Member::Builder);
        }
        let who = self.fin_sel;
        let fin = mascot::fin_in(&st, who);
        let (tint, outfit) = (fin.tint, fin.outfit);

        // The preview plays through every state unless one is held.
        let state = self.fin_state.unwrap_or_else(|| {
            let i = (self.now() / 3.2) as usize % State::ALL.len();
            State::ALL[i]
        });
        let figure = Figure::Fin(FinPose {
            fin,
            state,
            shine: false,
            gaze: (0, 0),
            offset: 0,
            from: None,
        });
        // Glide from one state's face to the next, as the bots do in the app.
        let figure = self.eased(figure, (who, u32::MAX, false));
        let preview = self.pose_img(figure, 240., Framing::Full);
        let accent = who.accent();
        let mut states = div().flex().flex_wrap().gap(px(6.));
        for (i, s) in State::ALL.into_iter().enumerate() {
            let on = self.fin_state == Some(s) || (self.fin_state.is_none() && s == state);
            states = states.child(
                ui::pill(("fin-state", i), s.name(), on).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.fin_state = if this.fin_state == Some(s) {
                            None
                        } else {
                            Some(s)
                        };
                        cx.notify();
                    },
                )),
            );
        }
        let role_name = match mascot::role_of(outfit) {
            Some(r) => r.name.to_string(),
            None if outfit == Outfit::PLAIN => "Plain".to_string(),
            None => "Custom outfit".to_string(),
        };
        let stage = ui::card()
            .w(px(380.))
            .flex_none()
            .p(px(space::M))
            .flex()
            .flex_col()
            .gap(px(space::SM))
            .child(
                div()
                    .h(px(330.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(c(ui::line2()))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(ca(accent, 0.14), 0.),
                        linear_color_stop(c(ui::bg()), 1.),
                    ))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(preview)
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .bottom(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(who.name()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(c(ui::muted()))
                                    .child(format!("{role_name} · {}", state.name())),
                            ),
                    ),
            )
            .child(states);

        // Whose Fin is being dressed.
        let mut pick = div().flex().flex_wrap().gap(px(8.));
        for m in crew.iter().copied() {
            let on = m == who;
            let face = self.avatar(m, 34.);
            pick = pick.child(
                div()
                    .id(("fin-who", m.key() as usize))
                    .px(px(8.))
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .border_1()
                    .map(|d| {
                        if on {
                            d.bg(c(ui::hover())).border_color(c(ui::line_hi()))
                        } else {
                            d.border_color(c(ui::line()))
                                .hover(|s| s.bg(c(ui::hover())))
                        }
                    })
                    .child(face)
                    .child(
                        div()
                            .pr(px(4.))
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(if on { ui::ink() } else { ui::body() }))
                            .child(m.name()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.fin_sel = m;
                        cx.notify();
                    })),
            );
        }

        let dress = move |tint: Tint, outfit: Outfit| {
            move |_: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                save(cx, |st| mascot::set_look(st, who, tint, outfit));
                cx.notify();
            }
        };
        let mut roles = div();
        roles = roles.child(
            choice("fin-role-plain", "Plain", outfit == Outfit::PLAIN, true)
                .on_click(cx.listener(dress(tint, Outfit::PLAIN))),
        );
        for (i, r) in ROLES.iter().enumerate() {
            roles = roles.child(
                choice(("fin-role", i), r.name, outfit == r.outfit, true)
                    .on_click(cx.listener(dress(r.tint, r.outfit))),
            );
        }
        let mut rights = div();
        for (i, t) in Tool::ALL.iter().copied().enumerate() {
            rights = rights.child(
                choice(("fin-right", i), t.name(), outfit.right == t, true)
                    .on_click(cx.listener(dress(tint, Outfit { right: t, ..outfit }))),
            );
        }
        let mut colours = div().pt(px(2.));
        for (i, (_, name, t)) in mascot::COLOURS.iter().copied().enumerate() {
            let on = t == tint;
            colours = colours.child(
                div()
                    .id(("fin-colour", i))
                    .size(px(24.))
                    .rounded_full()
                    .cursor_pointer()
                    .p(px(2.))
                    .border_2()
                    .border_color(if on { c(ui::ink()) } else { ca(0, 0.) })
                    .child(div().size_full().rounded_full().bg(linear_gradient(
                        135.,
                        linear_color_stop(c(t.0[0].0), 0.),
                        linear_color_stop(c(t.0[1].0), 1.),
                    )))
                    .tooltip(move |w, cx| Tooltip::new(name).build(w, cx))
                    .on_click(cx.listener(dress(t, outfit))),
            );
        }
        let is_default = (tint, outfit) == mascot::default_look(who);
        let (dt, dout) = mascot::default_look(who);

        // Peek is only ever an outline; these tune its lines for every bot.
        let line = Line::from_code(&st.fin_line);
        let set_line = |id: &'static str, i: usize, label: &'static str, on: bool, to: Line| {
            segment((id, i), label, on).on_click(cx.listener(move |_, _, _, cx| {
                save(cx, |st| st.fin_line = to.code());
                cx.notify();
            }))
        };
        let weights: Vec<_> = Weight::ALL
            .iter()
            .enumerate()
            .map(|(i, &weight)| {
                set_line(
                    "fin-weight",
                    i,
                    weight.name(),
                    line.weight == weight,
                    Line { weight, ..line },
                )
            })
            .collect();
        let inks: Vec<_> = Ink::ALL
            .iter()
            .enumerate()
            .map(|(i, &ink)| {
                set_line(
                    "fin-ink",
                    i,
                    ink.name(),
                    line.ink == ink,
                    Line { ink, ..line },
                )
            })
            .collect();
        let eyes: Vec<_> = Eyes::ALL
            .iter()
            .enumerate()
            .map(|(i, &eyes)| {
                set_line(
                    "fin-eyes",
                    i,
                    eyes.name(),
                    line.eyes == eyes,
                    Line { eyes, ..line },
                )
            })
            .collect();
        let looks = ui::card()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(div().px(px(18.)).pt(px(16.)).pb(px(6.)).child(section(
                "Crew look",
                "How Peek looks, for every bot, everywhere in the app.",
            )))
            .child(row(
                "Line weight",
                "How heavy every line is, from fine to bold.",
                segmented(weights),
            ))
            .child(row(
                "Line colour",
                "Each bot's own colour, a softer pastel, two-tone with lighter eyes, or a slowly turning rainbow.",
                segmented(inks),
            ))
            .child(row(
                "Eyes",
                "Open eyes as rings, like the pill, or as solid dots.",
                segmented(eyes),
            ));

        let wardrobe = ui::card()
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(space::SM))
            .child(
                div()
                    .flex()
                    .items_start()
                    .child(div().flex_1().child(section(
                        "Dress a bot",
                        "Start from a role kit, then swap any prop. Each bot keeps its own.",
                    )))
                    .when(!is_default, |d| {
                        d.child(
                            ui::ghost("fin-reset", "Reset").on_click(cx.listener(dress(dt, dout))),
                        )
                    }),
            )
            .child(pick)
            .child(div().h(px(1.)).my(px(4.)).bg(c(ui::line2())))
            .child(slot("ROLE", roles))
            .child(slot("PROP", rights))
            .child(slot("COLOUR", colours.flex().gap(px(8.))));

        let mut lineup = div().flex().flex_wrap().gap(px(space::M));
        for m in crew.iter().copied() {
            let img = self.bot(m, Mood::Idle, 76., Framing::Full);
            lineup = lineup.child(
                div()
                    .id(("fin-lineup", m.key() as usize))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(img)
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(c(if m == who { ui::ink() } else { ui::muted() }))
                            .child(m.name()),
                    )
                    .on_hover(cx.listener(move |this, on: &bool, _, cx| this.hover_bot(m, *on, cx)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.fin_sel = m;
                        cx.notify();
                    })),
            );
        }
        let team = ui::card()
            .p(px(18.))
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(section(
                "Your crew",
                "How the whole crew looks together. Click one to dress it.",
            ))
            .child(lineup);

        let right = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(space::M))
            .child(looks)
            .child(wardrobe);

        Self::page(
            "settings-mascot",
            "Mascot",
            "Meet Peek: two eyes in a pill, drawn as a clean outline. The lids show mood, the pill moves with effort, and the badge beside it shows what it's working with.",
            div()
                .flex()
                .flex_col()
                .gap(px(space::M))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(space::M))
                        .child(stage)
                        .child(right),
                )
                .child(team),
        )
    }
}
