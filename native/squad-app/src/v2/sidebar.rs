//! The sidebar, Crew Chat style: the workspace, search, what needs you, your
//! projects as channels, and the crew as people you can message.
//!
//! The crew lives here, one row per bot: its face, its name and a word on what
//! it's doing right now (working, needs you), so the bots are always in view.
//! The look: a soft grey column beside the white page, rows without boxes, and
//! the place you're on lifted out as a white row with a hairline edge.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::data::Member;
use super::store::Status;
use super::ui::{self, c};
use super::{Place, Shell};

/// The icon rail's width, the labelled panel's beside it, and the two together.
/// The Mac window buttons sit in the title bar across the top, not over the
/// rail, so the rail only needs room for its icons.
pub(super) const RAIL: f32 = 52.;
pub(super) const PANEL: f32 = 236.;
pub(super) const WIDTH: f32 = RAIL + PANEL;

/// The sidebar's ground, a shade off the page so the page reads as the thing
/// in front.
fn ground() -> u32 {
    ui::ground()
}

/// One row: an icon or face, a label, and something on the right. Lit when
/// you're on it.
fn row(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(9.))
        .flex()
        .items_center()
        .gap(px(9.))
        .rounded(px(4.))
        .text_size(px(13.5))
        .cursor_pointer()
        .map(|d| {
            if on {
                // Picked: white, a hairline round it and the accent bar
                // down its left edge.
                d.relative()
                    .overflow_hidden()
                    .bg(c(ui::raised()))
                    .border_1()
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
                            .bg(c(ui::pick_fill())),
                    )
            } else {
                d.border_1()
                    .border_color(transparent_black())
                    .text_color(c(ui::muted()))
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            }
        })
}

/// The quiet mono words on a row's right, with a square in `col` before
/// them when there is one.
fn row_note(text: impl Into<SharedString>, col: Option<u32>) -> Div {
    div()
        .ml_auto()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.))
        .font_family(ui::MONO)
        .text_size(px(11.))
        .font_weight(FontWeight::NORMAL)
        .text_color(c(ui::faint()))
        .when_some(col, |d, col| {
            d.child(div().size(px(6.)).flex_none().rounded(px(1.)).bg(c(col)))
        })
        .child(text.into())
}

/// A count on the right of a row: mono on the accent, like the design's
/// badge.
fn count(n: usize) -> Div {
    div()
        .ml_auto()
        .flex_none()
        .h(px(17.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(3.))
        .bg(c(ui::pick_fill()))
        .font_family(ui::MONO)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::pick_on()))
        .child(n.min(99).to_string())
}

/// A square button on the rail, lit like a row when you're on its place.
fn rail_btn(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .relative()
        .rounded(px(6.))
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(c(ui::hover())).text_color(c(ui::ink()))
            } else {
                d.text_color(c(ui::muted()))
                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
            }
        })
}

/// A small icon button, like Search and Hide in the panel's head.
fn small_btn(id: impl Into<ElementId>, icon: IconName) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(26.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_color(c(ui::faint()))
        .cursor_pointer()
        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
        .child(Icon::new(icon).size(px(14.)))
}

/// A tooltip saying `text`.
fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

/// How many jobs are running: a blue square and "2 working".
fn running(n: usize) -> Div {
    row_note(format!("{n} working"), Some(ui::t_blue()))
}

/// The colours projects and threads take in turn, so each is easy to spot.
const TINTS: [u32; 6] = [
    ui::VIOLET,
    ui::GREEN,
    ui::ORANGE,
    ui::BLUE,
    ui::AMBER,
    ui::RED,
];

/// A project's colour, the same wherever it sits in the list.
fn path_tint(path: &str) -> u32 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    TINTS[(h.finish() % TINTS.len() as u64) as usize]
}

/// Up to two letters for a project: the first of its first two words
/// ("squad-code" is SC, "myApp" is MA), else its first two ("shop" is Sh).
fn initials(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if !ch.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        prev_lower = ch.is_lowercase();
        word.push(ch);
    }
    if !word.is_empty() {
        words.push(word);
    }
    match words.as_slice() {
        [] => "?".into(),
        [one] => {
            let mut cs = one.chars();
            let a: String = cs
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            let b: String = cs
                .next()
                .map(|c| c.to_lowercase().collect())
                .unwrap_or_default();
            format!("{a}{b}")
        }
        [a, b, ..] => [a, b]
            .iter()
            .filter_map(|w| w.chars().next())
            .flat_map(|c| c.to_uppercase())
            .collect(),
    }
}

/// A project's tile: its letters in mono on a soft wash of its colour. The
/// project you're in is ringed in the accent.
fn monogram(name: &str, tint: u32, on: bool) -> Div {
    div()
        .size(px(32.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .font_family(ui::MONO)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .bg(c(super::theme::mix(ui::raised(), tint, 0.2)))
        .text_color(c(super::theme::mix(tint, ui::ink(), 0.55)))
        .when(on, |d| {
            d.shadow(vec![
                BoxShadow {
                    color: c(ground()),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(2.),
                    inset: false,
                },
                BoxShadow {
                    color: c(ui::accent_bar()),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(3.5),
                    inset: false,
                },
            ])
        })
        .child(initials(name))
}

/// How many things wait on you, on the Inbox's corner: mono on the accent.
fn count_badge(n: usize) -> Div {
    div()
        .absolute()
        .top(px(-4.))
        .right(px(-5.))
        .min_w(px(16.))
        .h(px(16.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(c(ui::pick_fill()))
        .border_2()
        .border_color(c(ground()))
        .font_family(ui::MONO)
        .text_size(px(9.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(ui::pick_on()))
        .child(if n > 9 {
            "9+".to_string()
        } else {
            n.to_string()
        })
}

/// A small square in the accent on a tile's corner: something in the
/// project is new or needs you.
fn ask_dot() -> Div {
    div()
        .absolute()
        .top(px(3.))
        .right(px(3.))
        .size(px(7.))
        .rounded(px(2.))
        .bg(c(ui::accent_bar()))
}

/// A blue square on a tile's foot while the crew works in the project.
fn busy_dot() -> Div {
    div()
        .absolute()
        .bottom(px(3.))
        .right(px(3.))
        .size(px(7.))
        .rounded(px(2.))
        .bg(c(ui::t_blue()))
}

/// What a section's + does.
type AddFn = fn(&mut Shell, &mut Window, &mut Context<Shell>);

/// A thread's #, plain and in mono like the design: faint, or ink on the
/// thread you're in.
fn hash(on: bool) -> Div {
    div()
        .w(px(14.))
        .flex_none()
        .font_family(ui::MONO)
        .text_size(px(13.))
        .text_color(c(if on { ui::accent_text() } else { ui::faint() }))
        .child("#")
}

impl Shell {
    /// The sidebar's width now: the rail alone when the panel is put away.
    pub(super) fn side_w(&self) -> f32 {
        if self.panel_off() {
            RAIL
        } else {
            WIDTH
        }
    }

    /// The labelled panel is put away, or Settings, the Inbox, the Crew or
    /// Machines has the window: each brings its own column, so only the rail stays beside it.
    fn panel_off(&self) -> bool {
        self.side_slim
            || matches!(
                self.place,
                Place::Settings(_) | Place::Inbox | Place::Crew | Place::Server
            )
    }

    pub(super) fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let inbox = self.inbox_count();
        self.inbox_seen = inbox;
        let rail = self.rail(cx);
        let panel = (!self.panel_off()).then(|| self.panel(inbox, cx));
        div()
            .id("sidebar")
            .w(px(self.side_w()))
            .h_full()
            .flex_none()
            .flex()
            .bg(c(ground()))
            .border_r_1()
            .border_color(c(ui::line()))
            .flex_col()
            .relative()
            .children(ui::pattern(crate::pattern::Area::Panels))
            .child(div().flex_1().min_h_0().flex().child(rail).children(panel))
    }

    /// The icon rail: Peek on top, the places, your projects as tiles, and
    /// Settings at the foot.
    fn rail(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let place = self.place;

        // The Inbox: everything waiting on you, in every project.
        let waiting = self.inbox_count();
        let inbox = rail_btn("rail-inbox", place == Place::Inbox)
            .child(Icon::new(IconName::Inbox).size(px(18.)))
            .when(waiting > 0, |d| d.child(count_badge(waiting)))
            .tooltip(tip(if waiting > 0 {
                format!("Inbox\n{waiting} waiting on you")
            } else {
                "Inbox\nNothing waits on you".to_string()
            }))
            .on_click(cx.listener(|this, _, _, cx| this.go(Place::Inbox, cx)));

        // Your projects as tiles, the one you're in ringed: a dot when
        // something in it needs you, blue while the crew works there.
        let current = self.crew.store.current;
        let folders: Vec<(usize, String, String)> = self
            .crew
            .store
            .folders
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.name.clone(), f.path.clone()))
            .collect();
        let mut list = div()
            .id("rail-projects")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .pt(px(2.));
        for (i, name, path) in folders {
            let rows = self.thread_rows_in(&path);
            let busy: usize = rows.iter().map(|r| r.2).sum();
            let asks: usize = rows.iter().map(|r| r.3).sum();
            let on = i == current;
            let unread = !on && rows.iter().any(|r| self.crew.store.unread(&path, r.0));
            let mut say = format!("{name}\n{}", short_path(&path));
            if asks > 0 {
                say.push_str(&format!("\n{asks} waiting on you"));
            }
            if busy > 0 {
                say.push_str(&format!("\n{busy} running"));
            }
            if unread {
                say.push_str("\nNew messages");
            }
            let group: SharedString = format!("rail-p-{i}").into();
            let tint = path_tint(&path);
            list = list.child(
                div()
                    .id(("rail-project", i))
                    .group(group.clone())
                    .w_full()
                    .h(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .child(monogram(&name, tint, on))
                            // A dot, not a number: the count lives on the Inbox.
                            .when(asks > 0 || unread, |d| d.child(ask_dot()))
                            .when(busy > 0, |d| d.child(busy_dot())),
                    )
                    // No name card over its own right-click menu.
                    .when(self.menu.is_none(), |d| d.tooltip(tip(say)))
                    .on_mouse_down(
                        MouseButton::Right,
                        self.on_menu(super::menu::Ctx::Project(path.clone()), cx),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.crew.store.current != i {
                            this.crew.store.current = i;
                            this.crew.store.save();
                        }
                        this.go(Place::Today, cx);
                    })),
            );
        }
        list = list.child(
            div()
                .w_full()
                .h(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("rail-add-project")
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(c(ui::line_hi()))
                        .text_color(c(ui::muted()))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                        .child(Icon::new(IconName::Plus).size(px(14.)))
                        .tooltip(tip("Add a project folder"))
                        .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
                ),
        );

        let slim = self.side_slim && !matches!(place, Place::Settings(_));
        let foot = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pt(px(8.))
            .when(slim, |d| {
                d.child(
                    rail_btn("rail-open", false)
                        .child(Icon::new(IconName::PanelLeftOpen).size(px(17.)))
                        .tooltip(tip("Show the sidebar"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.side_slim = false;
                            cx.notify();
                        })),
                )
            })
            // Machines: where the crew runs, this Mac and your server, with a
            // dot for whether the server answers once there is one.
            .child({
                let up = self.crew.link.as_ref().map(|l| l.up());
                rail_btn(
                    "rail-machines",
                    matches!(place, Place::Server | Place::Screen),
                )
                .child(Icon::new(IconName::Server).size(px(18.)))
                .when_some(up, |d, up| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(6.))
                            .right(px(6.))
                            .size(px(7.))
                            .rounded_full()
                            .border_1()
                            .border_color(c(ground()))
                            .bg(c(if up { ui::t_green() } else { ui::faint() })),
                    )
                })
                .tooltip(tip("Machines\nThis Mac and your server"))
                .on_click(cx.listener(|this, _, _, cx| this.go(Place::Server, cx)))
            })
            .child(
                rail_btn("rail-settings", matches!(place, Place::Settings(_)))
                    .child(Icon::new(IconName::Settings).size(px(18.)))
                    .tooltip(tip("Settings"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let pane = this.settings_pane;
                        this.go(Place::Settings(pane), cx)
                    })),
            );

        div()
            .id("rail")
            .w(px(RAIL))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pt(px(10.))
            .pb(px(12.))
            .when(!slim, |d| d.border_r_1().border_color(c(ui::line())))
            .child(inbox)
            .child(div().w(px(20.)).h(px(1.)).my(px(2.)).bg(c(ui::line_hi())))
            .child(list)
            .child(foot)
    }

    /// The labelled panel for the project you're in, laid out like a chat
    /// app: the places with their counts, the # threads as channels, the
    /// bots as direct messages, and a few things to try.
    fn panel(&mut self, inbox: usize, cx: &mut Context<Self>) -> Div {
        let place = self.place;

        let _ = inbox;
        // The project's own places. The Inbox and Machines sit on the rail,
        // since they span every project.
        let working = self.crew.store.running().count();
        let nav = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                row("side-overview", place == Place::Overview)
                    .child(Icon::new(IconName::House).size(px(16.)))
                    .child("Overview")
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::Overview, cx))),
            )
            .child(
                row("side-history", place == Place::History)
                    .child(Icon::new(IconName::LayoutDashboard).size(px(16.)))
                    .child("Jobs")
                    .when(working > 0, |d| d.child(running(working)))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Place::History, cx))),
            );

        let threads = self.thread_list(cx);
        let crew = self.crew_list(cx);

        // The project you're in, at the head of its panel: its name, and
        // where it lives on disk under it.
        let folder = self.crew.store.folder().cloned();
        let project = folder
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Squad Code".into());
        // Where it lives, its branch and how clean it is, as the design's
        // mono line.
        let where_ = match folder.as_ref() {
            Some(f) => {
                let mut bits = vec![short_path(&f.path)];
                if let Some(r) = self.crew.repos.get(&f.path).filter(|r| r.is_repo) {
                    if !r.state.branch.is_empty() {
                        bits.push(r.state.branch.clone());
                    }
                    bits.push(match r.changed {
                        0 => "clean".into(),
                        n => format!("{n} changed"),
                    });
                }
                bits.join(" \u{b7} ")
            }
            None => "No project yet".into(),
        };
        let head = div()
            .px(px(4.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(ui::ink()))
                    .child(div().truncate().child(project))
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(px(13.))
                            .text_color(c(ui::faint())),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child(where_),
            );

        // What runs in this project now: the app, and any other command.
        let here = folder.as_ref().map(|f| f.path.clone()).unwrap_or_default();
        let runs: Vec<(u64, String, Option<String>, bool)> = self
            .runs
            .iter()
            .filter(|r| r.spec.folder == here && !r.spec.setup && r.live())
            .map(|r| {
                (
                    r.id,
                    r.spec.cmd.clone(),
                    r.url().map(String::from),
                    r.state() == super::apps::State::Up,
                )
            })
            .collect();
        // Run the app, or, while it runs, its address and a way to open it.
        let app = runs.iter().find(|r| r.2.is_some()).cloned();
        let run_box = folder.is_some().then(|| {
            let base = div()
                .id("side-run")
                .mx(px(2.))
                .mb(px(10.))
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(5.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .text_size(px(12.5))
                .cursor_pointer()
                .hover(|s| s.border_color(c(ui::line_hi())));
            match app {
                Some((_, _, Some(url), up)) => {
                    let open = url.clone();
                    base.child(div().size(px(7.)).rounded_full().flex_none().bg(c(if up {
                        ui::t_green()
                    } else {
                        ui::t_amber()
                    })))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(12.))
                            .text_color(c(ui::ink()))
                            .child(url.trim_start_matches("http://").to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(c(ui::accent_text()))
                            .child("Open \u{2197}"),
                    )
                    .tooltip(tip("Open the running app in your browser"))
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&open)))
                }
                _ => base
                    .text_color(c(ui::muted()))
                    .child(Icon::new(IconName::Play).size(px(13.)))
                    .child("Run app")
                    .tooltip(tip("Start this project's app here in Squad"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.run_app(None, None, cx);
                    })),
            }
        });
        // Everything running, docked at the foot of the panel.
        let dock = (!runs.is_empty()).then(|| {
            let mut d = div()
                .flex_none()
                .pt(px(8.))
                .mt(px(4.))
                .border_t_1()
                .border_color(c(ui::line()))
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .px(px(10.))
                        .pb(px(4.))
                        .flex()
                        .justify_between()
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child("RUNNING NOW")
                        .child(runs.len().to_string()),
                );
            for (id, cmd, url, up) in runs {
                let port = url
                    .as_deref()
                    .and_then(|u| u.rsplit(':').next())
                    .map(|p| format!(":{}", p.trim_end_matches('/')));
                d = d.child(
                    row(("side-running", id as usize), false)
                        .h(px(27.))
                        .text_size(px(12.5))
                        .child(div().size(px(7.)).rounded_full().flex_none().bg(c(if up {
                            ui::t_green()
                        } else {
                            ui::t_blue()
                        })))
                        .child(div().flex_1().min_w_0().truncate().child(cmd))
                        .when_some(port, |d, p| {
                            d.child(
                                div()
                                    .flex_none()
                                    .font_family(ui::MONO)
                                    .text_size(px(11.))
                                    .text_color(c(ui::faint()))
                                    .child(p),
                            )
                        })
                        .when_some(url, |d, u| {
                            d.on_click(cx.listener(move |_, _, _, cx| cx.open_url(&u)))
                        }),
                );
            }
            d
        });

        div()
            .w(px(PANEL))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .pt(px(12.))
            .pb(px(12.))
            .px(px(10.))
            .child(head)
            .children(run_box)
            .child(
                div()
                    .id("side-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(nav)
                    .child(threads)
                    .child(crew),
            )
            .children(dock)
    }

    /// A section's heading, in small mono capitals: click it to fold the
    /// section open or shut. On the right, a + that adds to it, or a word
    /// that does something with it (Manage).
    fn fold_label(
        &mut self,
        key: &'static str,
        text: &'static str,
        add: Option<(&'static str, Option<&'static str>, AddFn)>,
        cx: &mut Context<Self>,
    ) -> (bool, Stateful<Div>) {
        let open = self.side_folds.get(key).copied().unwrap_or(true);
        let head = div()
            .id(key)
            .h(px(26.))
            .mt(px(16.))
            .mb(px(2.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .font_family(ui::MONO)
            .text_size(px(10.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c(ui::faint()))
            .cursor_pointer()
            .hover(|s| s.text_color(c(ui::muted())))
            .child(div().flex_1().child(text.to_uppercase()))
            .when(!open, |d| {
                d.child(Icon::new(IconName::ChevronRight).size(px(11.)))
            })
            .when_some(add, |d, (say, word, act)| {
                let btn = match word {
                    Some(w) => div()
                        .id(("side-add", key.len()))
                        .px(px(4.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                        .child(w.to_uppercase()),
                    None => small_btn(("side-add", key.len()), IconName::Plus).size(px(20.)),
                };
                d.child(
                    btn.tooltip(tip(say))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            act(this, window, cx);
                        })),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.side_folds.insert(key.to_string(), !open);
                cx.notify();
            }));
        (open, head)
    }

    /// The project's # threads: group chats with a few bots each. #general
    /// is always there; the one you're in is lit while you're on it.
    fn thread_list(&mut self, cx: &mut Context<Self>) -> Div {
        let (open, head) = self.fold_label(
            "threads",
            "Threads",
            Some(("New thread", None, |this, window, cx| {
                this.new_thread(window, cx)
            })),
            cx,
        );
        let mut list = div().flex().flex_col().gap(px(1.)).child(head);
        if !open {
            return list;
        }
        let Some(path) = self.crew.store.folder().map(|f| f.path.clone()) else {
            return list.child(
                row("side-pick-folder", false)
                    .child(Icon::new(IconName::FolderOpen).size(px(16.)))
                    .child("Pick a project folder")
                    .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
            );
        };
        let in_thread = matches!(self.place, Place::Today);
        let here = self.crew.store.thread_here();
        for (id, name, busy, asks) in self.thread_rows_in(&path) {
            let on = in_thread && id == here;
            // Something new since you last looked, in a thread you're not in.
            let unread = !on && self.crew.store.unread(&path, id);
            list = list.child(
                row(("side-thread", id as usize), on)
                    .child(hash(on))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(unread, |d| {
                                d.font_weight(FontWeight::SEMIBOLD).text_color(c(ui::ink()))
                            })
                            .child(name),
                    )
                    .map(|d| {
                        if asks > 0 {
                            d.child(count(asks))
                        } else if busy > 0 {
                            d.child(running(busy))
                        } else if unread {
                            d.child(row_note("new", None))
                        } else {
                            d
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Right,
                        self.on_menu(super::menu::Ctx::Thread(path.clone(), id), cx),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.crew.store.thread_now = id;
                        this.crew.store.save();
                        this.go(Place::Today, cx);
                    })),
            );
        }
        list
    }

    /// The crew as direct messages: each bot's face with a dot for what it's
    /// doing (blue working, amber asking you), its name, and a word on it.
    fn crew_list(&mut self, cx: &mut Context<Self>) -> Div {
        let (open, head) = self.fold_label(
            "crew",
            "Bots",
            Some((
                "Manage the crew: add, change or retire bots",
                Some("Manage"),
                |this, _, cx| this.go(Place::Crew, cx),
            )),
            cx,
        );
        let mut list = div().flex().flex_col().gap(px(1.)).child(head);
        if !open {
            return list;
        }
        let place = self.place;
        let members: Vec<Member> = Member::everyone()
            .into_iter()
            .filter(|m| m.on_crew())
            .collect();
        // A big crew folds its quiet bots away: the ones with nothing going on,
        // no conversation yet and not open. Peek always shows.
        let fold_quiet =
            members.len() > 6 && !self.side_folds.get("crew-all").copied().unwrap_or(false);
        let mut hidden = 0usize;
        for who in members {
            if fold_quiet
                && who != Member::Fin
                && place != Place::Chat(who)
                && self.crew.store.busy(who).is_none()
                && !self.thinking(who)
                && self.crew.store.chats.get(&who).is_none_or(|c| c.is_empty())
                && !self
                    .crew
                    .store
                    .jobs
                    .iter()
                    .any(|j| j.bot == who && (j.needs_you() || j.status == Status::Review))
            {
                hidden += 1;
                continue;
            }
            let (word, _) = self.status_of(who);
            let busy = self.crew.store.busy(who).is_some() || self.thinking(who);
            // What the bot's jobs want from you, most pressing first: a
            // question or permission it's waiting on, changes to review, a
            // job that stopped with a problem.
            let mine = || self.crew.store.jobs.iter().filter(move |j| j.bot == who);
            let asking =
                mine().any(|j| j.status == Status::Running && !self.crew.asks_for(j).is_empty());
            let review = mine().any(|j| j.status == Status::Review);
            let stopped = mine()
                .any(|j| j.needs_you() && matches!(j.status, Status::Failed | Status::Interrupted));
            let mood = self.side_mood(who);
            let face = self.bot(who, mood, 18., squad_holo::Framing::Bust);
            let on = place == Place::Chat(who);
            let (word, tint) = if asking {
                ("asks you".to_string(), ui::t_amber())
            } else if busy {
                (word, ui::t_blue())
            } else if review {
                ("to review".to_string(), ui::t_amber())
            } else if stopped {
                ("stopped".to_string(), ui::t_red())
            } else if who == Member::Fin {
                ("lead".to_string(), ui::faint())
            } else {
                (String::new(), ui::faint())
            };
            list = list.child(
                row(("side-bot", who.key() as usize), on)
                    .child(div().flex_none().child(face))
                    .child(div().flex_1().min_w_0().truncate().child(who.name()))
                    .when(!word.is_empty(), |d| {
                        let lead = tint == ui::faint();
                        d.child(row_note(word, (!lead).then_some(tint)))
                    })
                    .on_hover(
                        cx.listener(move |this, on: &bool, _, cx| this.hover_bot(who, *on, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        self.on_menu(super::menu::Ctx::Bot(who), cx),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.go(Place::Chat(who), cx))),
            );
        }
        let all_open = self.side_folds.get("crew-all").copied().unwrap_or(false);
        if hidden > 0 || all_open {
            list = list.child(
                row("side-more-bots", false)
                    .text_color(c(ui::faint()))
                    .child(
                        div()
                            .size(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(if all_open {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                })
                                .size(px(12.)),
                            ),
                    )
                    .child(if all_open {
                        "Show fewer".to_string()
                    } else if hidden == 1 {
                        "1 more bot".to_string()
                    } else {
                        format!("{hidden} more bots")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.side_folds.insert("crew-all".into(), !all_open);
                        cx.notify();
                    })),
            );
        }
        list
    }

    /// Each live thread in the current project, #general first: its id, name,
    /// jobs running and jobs waiting on you.
    #[allow(dead_code)]
    pub(super) fn thread_rows(&self) -> Vec<(u64, String, usize, usize)> {
        match self.crew.store.folder() {
            Some(f) => self.thread_rows_in(&f.path.clone()),
            None => Vec::new(),
        }
    }

    /// Each live thread in the project at `path`, #general first: its id,
    /// name, jobs running and jobs waiting on you.
    /// Alt+Up and Alt+Down move to the thread above or below; with Shift too,
    /// to the next or previous one with something unread. True when it moved.
    pub(super) fn step_thread(
        &mut self,
        down: bool,
        unread_only: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(path) = self.crew.store.folder().map(|f| f.path.clone()) else {
            return false;
        };
        let ids: Vec<u64> = self
            .thread_rows_in(&path)
            .into_iter()
            .map(|r| r.0)
            .collect();
        if ids.is_empty() {
            return false;
        }
        let here = self.crew.store.thread_here();
        let at = if self.place == Place::Today {
            ids.iter().position(|i| *i == here)
        } else {
            None
        };
        let n = ids.len();
        let order: Vec<usize> = match (at, down) {
            (Some(a), true) => (1..n).map(|k| (a + k) % n).collect(),
            (Some(a), false) => (1..n).map(|k| (a + n - k) % n).collect(),
            (None, true) => (0..n).collect(),
            (None, false) => (0..n).rev().collect(),
        };
        let next = order
            .into_iter()
            .map(|k| ids[k])
            .find(|id| !unread_only || self.crew.store.unread(&path, *id));
        let Some(id) = next else {
            return false;
        };
        self.crew.store.thread_now = id;
        self.crew.store.save();
        self.go(Place::Today, cx);
        true
    }

    fn thread_rows_in(&self, path: &str) -> Vec<(u64, String, usize, usize)> {
        let store = &self.crew.store;
        let mut ids: Vec<(u64, String)> = vec![(0, "general".into())];
        ids.extend(
            store
                .threads_in(path)
                .into_iter()
                .map(|t| (t.id, t.name.clone())),
        );
        // One row per race, as everywhere else, so the counts add up.
        let listed = store.listed();
        ids.into_iter()
            .map(|(id, name)| {
                let jobs: Vec<_> = listed
                    .iter()
                    .filter(|j| j.folder.path == path && store.thread_for(path, j.thread) == id)
                    .collect();
                let busy = jobs
                    .iter()
                    .filter(|j| matches!(j.status, Status::Running | Status::Shipping))
                    .count();
                let asks = jobs.iter().filter(|j| j.needs_you()).count();
                (id, name, busy, asks)
            })
            .collect()
    }

    /// Starts a new thread: the composer gets `/thread ` to type its name after.
    pub(super) fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Place::Today, cx);
        self.composer.update(cx, |i, cx| {
            ui::put_text(i, "/thread ", window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    /// The top bar over every page    /// The top bar over every page: where you are, the engine, and a new job.
    /// "#name" for a thread of the current project.
    fn thread_crumb(&self, thread: u64) -> String {
        let store = &self.crew.store;
        let here = store.folder().map(|f| f.path.clone()).unwrap_or_default();
        format!("#{}", store.thread_label(store.thread_for(&here, thread)))
    }

    pub(super) fn topbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = self
            .crew
            .store
            .folder()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Squad Code".into());
        let here: String = match self.place {
            Place::Today => format!(
                "#{}",
                self.crew.store.thread_label(self.crew.store.thread_here())
            ),
            Place::Inbox => "Inbox".into(),
            Place::History => "Jobs".into(),
            Place::Overview => "Overview".into(),
            Place::Chat(m) => m.name().to_string(),
            Place::Crew => "Crew".into(),
            // A job sits in its thread, so the path goes through it.
            Place::Live(id) => match self.crew.store.job(id) {
                Some(j) => format!("{} / job #{id}", self.thread_crumb(j.thread)),
                None => format!("Jobs / #{id}"),
            },
            Place::Race(id) => match self.crew.store.job(id) {
                Some(j) => format!("{} / Race / {}", self.thread_crumb(j.thread), j.label()),
                None => "Jobs / Race".into(),
            },
            Place::Goal(id) => match self.crew.store.goals.iter().find(|g| g.id == id) {
                Some(g) => format!("{} / Plan / {}", self.thread_crumb(g.thread), g.text),
                None => "Jobs / Plan".into(),
            },
            Place::Settings(p) => format!("Settings / {}", p.name()),
            Place::Server => "Machines".into(),
            Place::Screen => "Machines / Screen".into(),
            Place::Browser(id) => match self.crew.store.job(id) {
                Some(j) => format!("{} / {} / Browser", self.thread_crumb(j.thread), j.label()),
                None => "Jobs / Browser".into(),
            },
            Place::Welcome => "Welcome".into(),
        };
        // Settings apply to every project, so they get a plain title, not a path.
        let global = matches!(self.place, Place::Settings(_));
        // The thread a job or plan sits in: its crumb takes you back there.
        let back_to = match self.place {
            Place::Live(id) | Place::Race(id) | Place::Browser(id) => {
                self.crew.store.job(id).map(|j| j.thread)
            }
            Place::Goal(id) => self
                .crew
                .store
                .goals
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.thread),
            _ => None,
        }
        .map(|t| {
            let here = self
                .crew
                .store
                .folder()
                .map(|f| f.path.clone())
                .unwrap_or_default();
            self.crew.store.thread_for(&here, t)
        });
        let running = self.crew.store.running().count();
        // The right sidebar shows on a job's page, and beside a thread when
        // the Jobs panel is open.
        let right_open = match self.place {
            Place::Today => self.jobs_open,
            _ => false,
        };
        // Each step of the path after a chevron; the last, where you are, in full ink.
        let parts: Vec<&str> = here.split(" / ").collect();
        let last = parts.len() - 1;
        let crumbs: Vec<AnyElement> = parts
            .iter()
            .enumerate()
            .flat_map(|(i, part)| {
                let chev = div()
                    .flex_none()
                    .text_color(c(ui::faint()))
                    .child("/")
                    .into_any_element();
                let text = div()
                    .truncate()
                    .map(|d| {
                        if i == last {
                            d.min_w_0()
                                .text_color(c(ui::ink()))
                                .font_weight(FontWeight::MEDIUM)
                        } else {
                            d.flex_none().text_color(c(ui::faint()))
                        }
                    })
                    .child(part.to_string());
                let text = match back_to {
                    Some(t) if i == 0 => text
                        .id("crumb-thread")
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(ui::ink())))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.crew.store.thread_now = t;
                            this.crew.store.save();
                            this.go(Place::Today, cx);
                        }))
                        .into_any_element(),
                    _ => text.into_any_element(),
                };
                // Settings have no project in front, so their path starts bare.
                if global && i == 0 {
                    vec![text]
                } else {
                    vec![chev, text]
                }
            })
            .collect();
        // The title bar's left end lines up with the sidebar under it, and is
        // never narrower than the Mac window buttons it holds.
        let lights_w = if super::has_lights() { 84. } else { 0. };
        let left_w = self.side_w().max(lights_w);
        let slim = self.side_slim;
        let can_hide = !global;
        let can_back = !self.trail.is_empty();
        div()
            .h(px(super::TOP_H))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .bg(c(ui::ground()))
            .pr(px(if right_open { 0. } else { 10. }))
            .border_b_1()
            .border_color(c(ui::line()))
            .text_size(px(12.5))
            // The Mac window buttons' corner, as wide as the sidebar below.
            .child(
                div()
                    .w(px(left_w))
                    .h_full()
                    .flex_none()
                    .relative()
                    // Stand-ins for the Mac window buttons, for screenshots off a Mac.
                    .when(!cfg!(target_os = "macos") && super::has_lights(), |d| {
                        d.children(
                            [0xff5f57u32, 0xfebc2e, 0x28c840]
                                .into_iter()
                                .enumerate()
                                .map(|(i, col)| {
                                    div()
                                        .absolute()
                                        .left(px(super::LIGHTS_X + 1. + i as f32 * 20.))
                                        .top(px(super::LIGHTS_Y + 2.))
                                        .size(px(12.))
                                        .rounded_full()
                                        .bg(c(col))
                                }),
                        )
                    }),
            )
            // Show or hide the sidebar, and go back.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .pl(px(6.))
                    .pr(px(6.))
                    .when(can_hide, |d| {
                        d.child(
                            small_btn(
                                "top-side",
                                if slim {
                                    IconName::PanelLeftOpen
                                } else {
                                    IconName::PanelLeftClose
                                },
                            )
                            .tooltip(tip(if slim {
                                "Show the sidebar"
                            } else {
                                "Hide the sidebar"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.side_slim = !this.side_slim;
                                cx.notify();
                            })),
                        )
                    })
                    .child(
                        small_btn("top-back", IconName::ChevronLeft)
                            .when(!can_back, |d| d.opacity(0.4))
                            .tooltip(tip("Back"))
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    ),
            )
            // Where you are: the project, then the place.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .font_family(ui::MONO)
                    .text_size(px(12.))
                    .when(!global, |d| {
                        d.child(div().flex_none().text_color(c(ui::faint())).child(folder))
                    })
                    .children(crumbs),
            )
            .child(div().flex_1())
            // The Jobs toggle and search. With the right sidebar open they sit
            // over it as its own section, edge to edge with the column below.
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.))
                    .when(right_open, |d| {
                        d.w(px(super::home::LIST_W))
                            .pl(px(16.))
                            // The toggle keeps its place whether the sidebar is open or not.
                            .pr(px(20.))
                            .bg(c(ui::ground()))
                            .border_l_1()
                            .border_color(c(ui::line()))
                    })
                    // Search, as a quiet field: the palette finds places, bots and jobs.
                    .child(
                        div()
                            .id("top-search")
                            .map(|d| {
                                if right_open {
                                    d.flex_1().min_w_0()
                                } else {
                                    d.w(px(240.))
                                }
                            })
                            .h(px(26.))
                            .px(px(9.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(5.))
                            .bg(c(ui::raised()))
                            .border_1()
                            .border_color(c(ui::line()))
                            .text_size(px(12.))
                            .text_color(c(ui::faint()))
                            .cursor_pointer()
                            .hover(|s| s.border_color(c(ui::line_hi())).text_color(c(ui::muted())))
                            .child(Icon::new(IconName::Search).size(px(13.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child("Search or jump to"),
                            )
                            .child(ui::kbd(if cfg!(target_os = "macos") {
                                "⌘K"
                            } else {
                                "Ctrl K"
                            }))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_palette(window, cx)),
                            ),
                    )
                    // The right sidebar's toggle, at the window's right edge like
                    // the left one's at its left: its icon shows the panel it opens.
                    // A dot says something waits on you, or that the crew is working.
                    // A job's workspace has its own tabs instead of the right sidebar.
                    .when(
                        !matches!(self.place, Place::Live(_) | Place::Browser(_)),
                        |d| {
                            d.child({
                                let asks = self.inbox_count();
                                let on_job = matches!(self.place, Place::Live(_));
                                let dot = if asks > 0 {
                                    Some(ui::t_amber())
                                } else if running > 0 {
                                    Some(ui::t_blue())
                                } else {
                                    None
                                };
                                let mut say = if right_open {
                                    "Hide the right sidebar".to_string()
                                } else {
                                    "Show the right sidebar: jobs, git and files".to_string()
                                };
                                if asks > 0 {
                                    say.push_str(&format!("\n{asks} waiting on you"));
                                }
                                if running > 0 {
                                    say.push_str(&format!("\n{running} running"));
                                }
                                div()
                                    .id("top-right-panel")
                                    .relative()
                                    .size(px(28.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .text_color(c(if right_open { ui::ink() } else { ui::muted() }))
                                    .when(right_open, |d| d.bg(c(ui::press())))
                                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                                    .child(
                                        Icon::new(if right_open {
                                            IconName::PanelRightClose
                                        } else {
                                            IconName::PanelRightOpen
                                        })
                                        .size(px(17.)),
                                    )
                                    .when_some(dot, |d, col| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .top(px(4.))
                                                .right(px(4.))
                                                .size(px(7.))
                                                .rounded_full()
                                                .border_1()
                                                .border_color(c(if right_open {
                                                    ui::ground()
                                                } else {
                                                    ui::bg()
                                                }))
                                                .bg(c(col)),
                                        )
                                    })
                                    .tooltip(tip(say))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if on_job {
                                            this.right_closed = !this.right_closed;
                                            cx.notify();
                                            return;
                                        }
                                        this.jobs_open = !this.jobs_open;
                                        // The panel sits beside a thread or a job: open it there.
                                        if this.jobs_open
                                            && !matches!(this.place, Place::Today | Place::Live(_))
                                        {
                                            this.go(Place::Today, cx);
                                        }
                                        cx.notify();
                                    }))
                            })
                        },
                    ),
            )
    }
}

/// A path with the home folder written as `~`.
pub(super) fn short_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_string(),
    }
}

impl Shell {
    /// "Delete #name?" over the whole window, so the choice is in front of you
    /// rather than squeezed into the sidebar.
    pub(super) fn delete_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.deleting?;
        let name = self
            .crew
            .store
            .threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.clone())?;
        let jobs = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.thread == id)
            .count();
        let scrim = div()
            .id("delete-scrim")
            .absolute()
            .inset_0()
            .bg(ui::ca(0x000000, 0.28))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.deleting = None;
                cx.notify();
            }));
        let card = div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_center()
            .child(
                div()
                    .id("delete-card")
                    .w(px(400.))
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .shadow(vec![BoxShadow {
                        color: ui::ca(0x000000, 0.35 * ui::float_shadow()),
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
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .bg(c(ui::bad_tone().1))
                                    .child(
                                        Icon::new(IconName::Trash)
                                            .size(px(16.))
                                            .text_color(c(ui::bad_tone().0)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(format!("Delete #{name}?")),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(c(ui::body()))
                            .child(if jobs == 0 {
                                "What was said in this thread goes. This can't be undone.".to_string()
                            } else {
                                format!(
                                    "What was said in this thread goes. Its {jobs} job{} move to #general, so no work is lost.",
                                    if jobs == 1 { "" } else { "s" }
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(ui::ghost("delete-cancel", "Cancel").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.deleting = None;
                                    cx.notify();
                                },
                            )))
                            .child(
                                ui::solid("delete-yes", "Delete thread")
                                    .bg(c(ui::bad_tone().0))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.delete_thread(id, cx)
                                    })),
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
mod tests {
    use super::initials;

    #[test]
    fn a_project_gets_up_to_two_letters() {
        assert_eq!(initials("squad-code"), "SC");
        assert_eq!(initials("myApp"), "MA");
        assert_eq!(initials("shop"), "Sh");
        assert_eq!(initials("x"), "X");
        assert_eq!(initials("--"), "?");
    }
}
