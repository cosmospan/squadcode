//! The terminal drawer: a real shell at the bottom of a job, opened in the
//! job's own worktree, so you can poke at its branch (run it, test it, look at
//! git) without touching your checkout. `squad` works in it too, for driving the
//! crew from the keyboard.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::pty;

use super::store::Job;
use super::ui::{self, c};
use super::Shell;

/// Text size and row height in the drawer.
const FONT: f32 = 12.5;
const LINE: f32 = 17.;
/// Rows the drawer shows.
pub const ROWS: u16 = 14;
/// Width of one column, near enough for a monospace face at FONT.
const CELL: f32 = FONT * 0.6;

// The drawer is a dark terminal in every theme, like the terminal tails on cards:
// a light theme's own dark well, and a dark theme's sunken one (`ui::term`).
fn term_bg() -> u32 {
    ui::term().bg
}
fn term_fg() -> u32 {
    ui::term().fg
}
fn term_dim() -> u32 {
    ui::term().dim
}
fn term_line() -> u32 {
    ui::term().line
}

/// A folder with a `squad` command in it, put first on the drawer's PATH so
/// typing `squad` drives this app's crew.
fn shim_dir() -> Option<PathBuf> {
    let dir = crate::store::data_dir().join("bin");
    std::fs::create_dir_all(&dir).ok()?;
    let exe = std::env::current_exe().ok()?;
    if cfg!(windows) {
        let file = dir.join("squad.cmd");
        let text = format!("@echo off\r\n\"{}\" v2 %*\r\n", exe.display());
        std::fs::write(file, text).ok()?;
    } else {
        let file = dir.join("squad");
        let text = format!("#!/bin/sh\nexec \"{}\" v2 \"$@\"\n", exe.display());
        std::fs::write(&file, text).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700));
        }
    }
    Some(dir)
}

/// The shell the drawer runs: the person's own, with `squad` on its PATH.
fn program() -> Option<pty::Program> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut env = vec![("RUNO_V2".to_string(), "1".to_string())];
    if let Some(dir) = shim_dir() {
        let path = std::env::var("PATH").unwrap_or_default();
        env.push(("PATH".into(), format!("{}:{path}", dir.display())));
    }
    Some(pty::Program {
        bin: shell,
        args: vec!["-l".into()],
        env,
        unset: Vec::new(),
    })
}

/// The key that opens and closes the terminal, as it's written on this platform.
pub const KEY: &str = if cfg!(target_os = "macos") {
    "\u{2303}`"
} else {
    "Ctrl `"
};

/// Terminals that belong to a project folder rather than a job sit at the top
/// of the id range, one per folder, so they never meet a job's id.
const PROJECT_BASE: u64 = u64::MAX - 4096;

/// The id of the project terminal for the folder at `index`.
pub fn project_id(index: usize) -> u64 {
    u64::MAX - index as u64
}

/// Whether a terminal id is a project's rather than a job's.
pub fn is_project(id: u64) -> bool {
    id > PROJECT_BASE
}

/// Quick commands over a project's drawer.
fn project_quick(dir: &str) -> Vec<(&'static str, String)> {
    let mut v = vec![("Status", "git status --short".to_string())];
    if let Some(check) = super::work::checks_for(dir).first() {
        v.push(("Run checks", check.clone()));
    }
    v.push(("Crew", "squad ls".to_string()));
    v.push(("Help", "squad help".to_string()));
    v
}

/// Quick commands over the drawer: what they're called and what they type.
pub fn quick(job: &Job) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("Status", "git status --short".to_string()),
        ("What changed", "git diff --stat".to_string()),
    ];
    if let Some(check) = super::work::checks_for(job.dir()).first() {
        v.push(("Run checks", check.clone()));
    }
    v.push(("Crew", "squad ls".to_string()));
    v
}

impl Shell {
    /// Opens or closes the drawer on a job, starting its shell the first time.
    pub(super) fn toggle_term(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.term_open == Some(id) {
            self.term_open = None;
            cx.notify();
            return;
        }
        let dir = if is_project(id) {
            let index = (u64::MAX - id) as usize;
            match self.crew.store.folders.get(index) {
                Some(f) => f.path.clone(),
                None => {
                    self.say("Pick a folder first, then the terminal opens in it.".to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            match self.crew.store.job(id) {
                Some(j) => j.dir().to_string(),
                None => return,
            }
        };
        let gone = self.terms.get(&id).is_some_and(|t| t.exited());
        if gone {
            self.terms.remove(&id);
        }
        if !self.terms.contains_key(&id) {
            let cols = self.term_cols;
            match pty::Shell::spawn_program(id, id, &dir, ROWS, cols, program(), |_| {}) {
                Ok(sh) => {
                    self.terms.insert(id, sh);
                }
                Err(why) => {
                    self.say(format!("Couldn't open a terminal: {why}"));
                    cx.notify();
                    return;
                }
            }
        }
        self.term_open = Some(id);
        window.focus(&self.term_focus, cx);
        // Not handed back to the window's own focus while the drawer appears.
        self.focus_moved = std::time::Instant::now();
        self.term_grab = 6;
        // The drawer isn't drawn yet: focus it again once it is.
        let h = self.term_focus.clone();
        window.on_next_frame(move |window, cx| window.focus(&h, cx));
        cx.notify();
    }

    /// Types a command into the drawer's shell and runs it.
    pub(super) fn term_run(
        &mut self,
        id: u64,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.term_open != Some(id) {
            self.toggle_term(id, window, cx);
        }
        if let Some(t) = self.terms.get(&id) {
            t.write(format!("{command}\r").as_bytes());
        }
        window.focus(&self.term_focus, cx);
    }

    /// Opens or closes the terminal in the current project folder, from any
    /// page: no job needed.
    pub(super) fn toggle_project_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.crew.store.folder().is_none() {
            self.say("Pick a folder first, then the terminal opens in it.".to_string());
            cx.notify();
            return;
        }
        if self.crew.store.done_step(super::store::Step::Terminal) {
            self.crew.store.save();
        }
        let id = project_id(self.crew.store.current);
        self.toggle_term(id, window, cx);
    }

    /// The drawer under a job, when it's open.
    pub(super) fn term_drawer(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<Div> {
        if self.term_open != Some(job.id) {
            return None;
        }
        let place = if job.branch.is_empty() {
            job.dir().to_string()
        } else {
            format!("{} \u{b7} {}", job.branch, job.dir())
        };
        let width = self.view_w - 560.;
        self.drawer(job.id, place, quick(job), width, cx)
    }

    /// The project's drawer, under whatever page is showing, when it's open.
    pub(super) fn project_drawer(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let id = self.term_open.filter(|id| is_project(*id))?;
        let dir = self
            .crew
            .store
            .folders
            .get((u64::MAX - id) as usize)?
            .path
            .clone();
        let quick = project_quick(&dir);
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = match dir.strip_prefix(&home) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => dir,
        };
        let width = self.view_w - super::sidebar::WIDTH - 24.;
        self.drawer(id, dir, quick, width, cx)
    }

    fn drawer(
        &mut self,
        id: u64,
        place: String,
        quick: Vec<(&'static str, String)>,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        // Fit the shell to the drawer's width.
        let cols = ((width / CELL).floor() as u16).clamp(40, 400);
        if cols != self.term_cols {
            self.term_cols = cols;
        }
        let sh = self.terms.get(&id)?;
        sh.resize(ROWS, cols);
        let focused = self.term_focused;
        let exited = sh.exited();
        let color = |col: vt100::Color, default: u32| match col {
            vt100::Color::Default => default,
            vt100::Color::Idx(i) => pty::palette(i, true),
            vt100::Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        };
        let lines = sh.with_screen(|screen| {
            let mut lines = div().flex().flex_col();
            let (rows, cols) = screen.size();
            let (cur_row, cur_col) = screen.cursor_position();
            let show_cursor = !screen.hide_cursor() && screen.scrollback() == 0 && !exited;
            for row in 0..rows {
                let mut line = div().h(px(LINE)).flex().whitespace_nowrap();
                let mut run = String::new();
                let mut style: Option<(u32, Option<u32>, bool)> = None;
                let flush =
                    |line: Div, run: &mut String, style: Option<(u32, Option<u32>, bool)>| {
                        let Some((fg, bg, bold)) = style else {
                            return line;
                        };
                        if run.is_empty() {
                            return line;
                        }
                        let text = std::mem::take(run);
                        line.child(
                            div()
                                .flex_none()
                                .text_color(c(fg))
                                .when_some(bg, |d, bg| d.bg(c(bg)))
                                .when(bold, |d| d.font_weight(FontWeight::SEMIBOLD))
                                .child(text),
                        )
                    };
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else {
                        break;
                    };
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let mut fg = color(cell.fgcolor(), term_fg());
                    let mut bg = match cell.bgcolor() {
                        vt100::Color::Default => None,
                        x => Some(color(x, 0)),
                    };
                    if cell.inverse() {
                        let back = bg.unwrap_or(term_bg());
                        bg = Some(fg);
                        fg = back;
                    }
                    if show_cursor && focused && row == cur_row && col == cur_col {
                        let back = bg.unwrap_or(term_bg());
                        bg = Some(term_fg());
                        fg = back;
                    }
                    let s = (fg, bg, cell.bold());
                    if style != Some(s) {
                        line = flush(line, &mut run, style);
                        style = Some(s);
                    }
                    match cell.first_char() {
                        Some(ch) => run.push(ch),
                        None => run.push(' '),
                    }
                }
                line = flush(line, &mut run, style);
                lines = lines.child(line);
            }
            lines
        });
        let mut bar = div()
            .h(px(34.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(c(term_line()))
            .child(
                Icon::new(IconName::SquareTerminal)
                    .size(px(13.))
                    .text_color(c(term_dim())),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(term_fg()))
                    .child("Terminal"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(term_dim()))
                    .child(place),
            );
        for (k, (label, command)) in quick.into_iter().enumerate() {
            bar = bar.child(
                div()
                    .id(("term-quick", k))
                    .h(px(22.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(c(term_line()))
                    .text_size(px(11.5))
                    .text_color(c(term_dim()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(c(term_fg())).bg(c(term_line())))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.term_run(id, &command, window, cx)
                    })),
            );
        }
        bar = bar.child(
            div()
                .id("term-close")
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_color(c(term_dim()))
                .cursor_pointer()
                .hover(|s| s.text_color(c(term_fg())).bg(c(term_line())))
                .child(Icon::new(IconName::ChevronDown).size(px(14.)))
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new("Hide; the shell keeps running")
                        .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.term_open = None;
                    cx.notify();
                })),
        );
        // Rises from the bottom edge when opened.
        Some(super::motion::panel(
            div()
                .flex_none()
                .mx(px(8.))
                .rounded_t(px(10.))
                .overflow_hidden()
                .bg(c(term_bg()))
                .border_1()
                .border_b_0()
                .border_color(c(term_line()))
                .child(bar)
                .child(
                    div()
                        .id("term-screen")
                        .track_focus(&self.term_focus)
                        .px(px(12.))
                        .py(px(8.))
                        .font_family(ui::MONO)
                        .text_size(px(FONT))
                        .line_height(px(LINE))
                        .text_color(c(term_fg()))
                        .cursor_text()
                        .on_click(
                            cx.listener(|this, _, window, cx| window.focus(&this.term_focus, cx)),
                        )
                        .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| {
                            this.term_key(id, ev, cx)
                        }))
                        .child(lines)
                        .when(exited, |d| {
                            d.child(div().text_color(c(term_dim())).child(
                                "The shell ended. Close and open the terminal again for a new one.",
                            ))
                        }),
                ),
            super::motion::key(("drawer", id)),
            0.,
            24.,
        ))
    }

    fn term_key(&mut self, id: u64, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(sh) = self.terms.get(&id) else {
            return;
        };
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let paste = (m.platform && ks.key == "v") || (m.control && m.shift && ks.key == "v");
        if paste {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                sh.paste(&text);
            }
            cx.stop_propagation();
            return;
        }
        if m.platform {
            return;
        }
        let app_cursor = sh.with_screen(|s| s.application_cursor());
        if let Some(bytes) = pty::key_bytes(
            &ks.key,
            ks.key_char.as_deref(),
            m.control,
            m.alt,
            m.shift,
            app_cursor,
        ) {
            sh.write(&bytes);
            cx.stop_propagation();
            cx.notify();
        }
    }
}
