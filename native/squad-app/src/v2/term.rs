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
/// Rows the job workspace's Terminal tab shows.
const TAB_ROWS: u16 = 28;
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
    let mut env = vec![("SQUADCODE".to_string(), "1".to_string())];
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

/// The shell on your server, over SSH: just under the projects' ids.
pub const SERVER: u64 = PROJECT_BASE;

/// Whether a terminal belongs to no job: a project's, or the server's.
pub fn is_shared(id: u64) -> bool {
    is_project(id) || id == SERVER
}

/// Quick commands over the server's drawer.
fn server_quick() -> Vec<(&'static str, String)> {
    vec![
        (
            "Squad's log",
            "journalctl --user -u squad -n 40 --no-pager".to_string(),
        ),
        ("Disk", "df -h".to_string()),
        ("Busiest", "top -bn1 | head -20".to_string()),
        ("Home", "cd ~ && ls".to_string()),
    ]
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

/// Most shells one drawer holds as tabs.
const MAX_TABS: usize = 8;
/// Most shells shown side by side.
const MAX_SIDE: usize = 3;
/// Height of the drawer's bar of tabs and buttons.
const BAR_H: f32 = 36.;

/// One drawer's shells: a tab each, the one in front, and how they're laid
/// out. A job, a project folder and the server each have their own.
pub(super) struct Drawer {
    tabs: Vec<Tab>,
    active: usize,
    /// Shows the tabs side by side instead of one at a time.
    side: bool,
    /// Fills the page instead of sitting under it.
    max: bool,
    /// The number the next tab gets, so closing one never renames the rest.
    next: u32,
}

struct Tab {
    sh: pty::Shell,
    n: u32,
}

impl Drawer {
    fn new(sh: pty::Shell) -> Self {
        Self {
            tabs: vec![Tab { sh, n: 1 }],
            active: 0,
            side: false,
            max: false,
            next: 2,
        }
    }

    fn front(&self) -> Option<&pty::Shell> {
        self.tabs.get(self.active).map(|t| &t.sh)
    }

    /// The tabs on screen: the one in front alone, or side by side up to
    /// [`MAX_SIDE`] of them, always including the one in front.
    fn shown(&self) -> std::ops::Range<usize> {
        let len = self.tabs.len();
        if !self.side || len < 2 {
            return self.active..(self.active + 1).min(len);
        }
        let n = len.min(MAX_SIDE);
        let start = self.active.saturating_sub(n - 1).min(len - n);
        start..start + n
    }

    /// Drops shells that have ended, keeping at least the one in front
    /// so its last words stay readable.
    fn drop_ended(&mut self) {
        if self.tabs.len() < 2 {
            return;
        }
        let front = self.tabs[self.active].n;
        self.tabs.retain(|t| !t.sh.exited() || t.n == front);
        self.active = self.tabs.iter().position(|t| t.n == front).unwrap_or(0);
    }
}

/// What a tab is called: the title its program set, else the shell's name.
fn tab_label(sh: &pty::Shell, fallback: &str) -> String {
    let title = sh.with_screen(|s| s.title().trim().to_string());
    let text = if title.is_empty() {
        fallback.to_string()
    } else {
        title
    };
    let mut short: String = text.chars().take(26).collect();
    if text.chars().count() > 26 {
        short.push('\u{2026}');
    }
    short
}

/// The shell's own name, like `zsh`.
fn shell_name() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sh".into())
}

/// The keys the drawer answers itself, as written on this platform.
const KEYS_HINT: &str = if cfg!(target_os = "macos") {
    "\u{2318}T new tab \u{b7} \u{2318}D side by side \u{b7} \u{2318}\u{21e7}\u{21a9} fill the page \u{b7} \u{2318}W close tab"
} else {
    "Ctrl Shift T new tab \u{b7} Ctrl Shift D side by side \u{b7} Ctrl Shift Enter fill the page \u{b7} Ctrl Shift W close tab"
};

impl Shell {
    /// Where drawer `id`'s shells start and what they run. None, after saying
    /// why, when there's nowhere to start one.
    fn term_place(
        &mut self,
        id: u64,
        cx: &mut Context<Self>,
    ) -> Option<(String, Option<pty::Program>)> {
        if id == SERVER {
            let Some(link) = &self.crew.link else {
                self.say("Connect a server first, in Settings, Servers.".to_string());
                cx.notify();
                return None;
            };
            let (bin, args) = super::remote::shell_program(&link.cfg, None);
            let prog = Some(pty::Program {
                bin,
                args,
                env: Vec::new(),
                unset: Vec::new(),
            });
            return Some((std::env::var("HOME").unwrap_or_else(|_| "/".into()), prog));
        }
        let dir = if is_project(id) {
            let index = (u64::MAX - id) as usize;
            match self.crew.store.folders.get(index) {
                Some(f) => f.path.clone(),
                None => {
                    self.say("Pick a folder first, then the terminal opens in it.".to_string());
                    cx.notify();
                    return None;
                }
            }
        } else {
            self.crew.store.job(id)?.dir().to_string()
        };
        Some((dir, program()))
    }

    /// Starts one more shell for drawer `id`.
    fn spawn_shell(&mut self, id: u64, cx: &mut Context<Self>) -> Option<pty::Shell> {
        let (dir, prog) = self.term_place(id, cx)?;
        match pty::Shell::spawn_program(id, &dir, ROWS, self.term_cols, prog, |_| {}) {
            Ok(sh) => Some(sh),
            Err(why) => {
                self.say(format!("Couldn't open a terminal: {why}"));
                cx.notify();
                None
            }
        }
    }

    /// Hands focus to the open drawer, now and once it's drawn.
    fn grab_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.term_focus, cx);
        // Not handed back to the window's own focus while the drawer appears.
        self.focus_moved = std::time::Instant::now();
        self.term_grab = 6;
        // The drawer isn't drawn yet: focus it again once it is.
        let h = self.term_focus.clone();
        window.on_next_frame(move |window, cx| window.focus(&h, cx));
        cx.notify();
    }

    /// Opens or closes the drawer on a job, starting its shell the first time.
    pub(super) fn toggle_term(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.term_open == Some(id) {
            self.term_open = None;
            cx.notify();
            return;
        }
        if let Some(d) = self.terms.get_mut(&id) {
            d.drop_ended();
        }
        let alive = self
            .terms
            .get(&id)
            .is_some_and(|d| d.tabs.iter().any(|t| !t.sh.exited()));
        if !alive {
            let Some(sh) = self.spawn_shell(id, cx) else {
                return;
            };
            // Keeps how it was laid out, with a fresh shell in it.
            match self.terms.get_mut(&id) {
                Some(d) => {
                    let n = d.next;
                    d.next += 1;
                    d.tabs = vec![Tab { sh, n }];
                    d.active = 0;
                }
                None => {
                    self.terms.insert(id, Drawer::new(sh));
                }
            }
        }
        self.term_open = Some(id);
        self.grab_term(window, cx);
    }

    /// Opens one more shell in drawer `id`, as a tab in front.
    pub(super) fn new_term_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(len) = self.terms.get(&id).map(|d| d.tabs.len()) else {
            self.toggle_term(id, window, cx);
            return;
        };
        if len >= MAX_TABS {
            self.say(format!(
                "A terminal holds {MAX_TABS} tabs at most. Close one to open another."
            ));
            cx.notify();
            return;
        }
        let Some(sh) = self.spawn_shell(id, cx) else {
            return;
        };
        if let Some(d) = self.terms.get_mut(&id) {
            let n = d.next;
            d.next += 1;
            d.tabs.push(Tab { sh, n });
            d.active = d.tabs.len() - 1;
        }
        self.term_open = Some(id);
        self.grab_term(window, cx);
    }

    /// Closes tab `k` of drawer `id`, ending its shell. The last one closes
    /// the drawer.
    fn close_term_tab(&mut self, id: u64, k: usize, cx: &mut Context<Self>) {
        let Some(d) = self.terms.get_mut(&id) else {
            return;
        };
        if k >= d.tabs.len() {
            return;
        }
        d.tabs.remove(k);
        if d.tabs.is_empty() {
            self.terms.remove(&id);
            if self.term_open == Some(id) {
                self.term_open = None;
            }
        } else {
            if d.active > k || d.active >= d.tabs.len() {
                d.active -= 1;
            }
            // Nothing left to sit beside.
            if d.tabs.len() < 2 {
                d.side = false;
            }
        }
        cx.notify();
    }

    /// Brings tab `k` of drawer `id` to the front.
    fn pick_term_tab(&mut self, id: u64, k: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.terms.get_mut(&id) {
            if k < d.tabs.len() {
                d.active = k;
            }
        }
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// Shows drawer `id`'s tabs side by side, or one at a time again.
    fn toggle_term_side(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(len) = self.terms.get(&id).map(|d| d.tabs.len()) else {
            return;
        };
        let side = self.terms.get(&id).is_some_and(|d| d.side);
        // Side by side with one shell opens a second to sit beside it.
        if !side && len < 2 {
            self.new_term_tab(id, window, cx);
        }
        if let Some(d) = self.terms.get_mut(&id) {
            d.side = !side;
        }
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// Lets drawer `id` fill the page, or puts it back under it.
    fn toggle_term_max(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.terms.get_mut(&id) {
            d.max = !d.max;
        }
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// Whether the open drawer fills the page.
    pub(super) fn term_max(&self) -> bool {
        self.term_open
            .and_then(|id| self.terms.get(&id))
            .is_some_and(|d| d.max)
    }

    /// Whether the open drawer is a project's or the server's, filling the
    /// page: the page under it is then not drawn at all.
    pub(super) fn shared_term_max(&self) -> bool {
        self.term_open.is_some_and(is_shared) && self.term_max()
    }

    /// Runs the app the crew made, and keeps it running: job `id`'s branch
    /// when given, else the project's folder. It runs inside Squad, never in
    /// your terminal, and shows as a card: under Peek's message `talk` when
    /// given, else in the job. Returns what Peek says about it. None when
    /// Squad Code can't tell how to start it.
    pub(super) fn run_app(
        &mut self,
        job: Option<u64>,
        talk: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let folder = self.crew.store.folder()?.path.clone();
        let (dir, jid) = match job.and_then(|id| self.crew.store.job(id)) {
            Some(j) => (j.dir().to_string(), j.id),
            None => (folder.clone(), 0),
        };
        let (cmd, url) = super::work::start_command(&dir, jid)?;
        let spec = super::apps::RunSpec {
            cmd: cmd.clone(),
            dir,
            folder,
            job: (jid != 0).then_some(jid),
            talk,
            hint: url,
            keeps: true,
            remote: None,
            setup: false,
        };
        let line = match self.start_run(spec, cx) {
            Ok(_) => format!(
                "Started {cmd} for you. It runs here in Squad: the card shows its address once it's up, and its output. Say \u{201c}stop app\u{201d} or press Stop to end it."
            ),
            Err(why) => format!("I couldn't start {cmd}: {why}"),
        };
        if talk.is_none() {
            if let Some(j) = job.and_then(|id| self.crew.store.job_mut(id)) {
                j.feed.push(super::store::Item::Note(line.clone()));
            }
        }
        self.crew.store.save();
        cx.notify();
        Some(line)
    }

    /// Types a command into the drawer's shell in front and runs it.
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
        if let Some(t) = self.terms.get(&id).and_then(Drawer::front) {
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
    pub(super) fn term_drawer(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.term_open != Some(job.id) {
            return None;
        }
        let place = if job.branch.is_empty() {
            job.dir().to_string()
        } else {
            format!("{} \u{b7} {}", job.branch, job.dir())
        };
        let width = self.view_w - 560.;
        self.drawer(job.id, place, quick(job), width, false, cx)
    }

    /// The job workspace's Terminal tab: quick commands over a dark shell
    /// that fills the pane, in the job's own copy of the project.
    pub(super) fn term_tab(&mut self, job: &Job, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.term_open != Some(job.id) {
            return None;
        }
        let width = self.view_w - self.side_w() - super::peek::TALK_W - 36. - 26.;
        self.drawer(job.id, String::new(), quick(job), width, true, cx)
    }

    /// The project's drawer, under whatever page is showing, when it's open.
    pub(super) fn project_drawer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.term_open == Some(SERVER) {
            let host = self.crew.link.as_ref()?.cfg.host.clone();
            let width = self.view_w - super::sidebar::WIDTH - 24.;
            return self.drawer(
                SERVER,
                format!("ssh {host}"),
                server_quick(),
                width,
                false,
                cx,
            );
        }
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
        let width = self.view_w - self.side_w() - 24.;
        self.drawer(id, dir, quick, width, false, cx)
    }

    /// A drawer: its bar of tabs over the shells on screen, one or side by
    /// side. `tab` lays it out for the job workspace's Terminal tab.
    fn drawer(
        &mut self,
        id: u64,
        place: String,
        quick: Vec<(&'static str, String)>,
        width: f32,
        tab: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fallback = if id == SERVER {
            "ssh".to_string()
        } else {
            shell_name()
        };
        let (shown, active, side, max, tabs) = {
            let d = self.terms.get(&id)?;
            let tabs: Vec<(u32, String, bool)> = d
                .tabs
                .iter()
                .map(|t| (t.n, tab_label(&t.sh, &fallback), t.sh.exited()))
                .collect();
            (d.shown(), d.active, d.side, d.max, tabs)
        };
        if shown.is_empty() {
            return None;
        }
        let fill = tab || max;
        // Fit each shell to its share of the width, and, when the drawer
        // fills its space, to the height it was last drawn at.
        let n = shown.len();
        let pad = if tab { 14. } else { 12. };
        let pane_w = (width - (n as f32 - 1.)) / n as f32;
        let cols = (((pane_w - 2. * pad) / CELL).floor() as u16).clamp(20, 400);
        self.term_cols = cols;
        let fallback_rows = if tab { TAB_ROWS } else { ROWS };
        let h = self.term_h.get();
        let rows = if fill && h > 4. * LINE {
            (((h - 16.) / LINE).floor() as u16).clamp(4, 300)
        } else {
            fallback_rows
        };
        let focused = self.term_focused;
        let accent = ui::accent_bar();
        let both = side && n > 1;
        let mut panes = div().flex().w_full().when(fill, |d| d.h_full());
        for k in shown.clone() {
            let d = self.terms.get(&id)?;
            let sh = &d.tabs[k].sh;
            sh.resize(rows, cols);
            let front = k == active;
            let exited = sh.exited();
            let lines = screen_lines(sh, focused && front);
            if k != shown.start {
                panes = panes.child(div().flex_none().w(px(1.)).bg(c(term_line())));
            }
            panes = panes.child(
                div()
                    .id(("term-pane", k))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .px(px(pad))
                    .pt(px(if both { 6. } else { 8. }))
                    .pb(px(8.))
                    // Side by side, the one taking your keys wears the accent.
                    .when(both, |d| {
                        d.border_t_2()
                            .border_color(c(if front { accent } else { term_bg() }))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_term_tab(id, k, window, cx)
                    }))
                    .child(lines)
                    .when(exited, |d| {
                        d.child(div().text_color(c(term_dim())).child(
                            "The shell ended. Close its tab, or open the terminal again for a new one.",
                        ))
                    }),
            );
        }
        let h_cell = self.term_h.clone();
        let screen = div()
            .id("term-screen")
            .track_focus(&self.term_focus)
            .relative()
            .flex()
            .flex_col()
            .when(fill, |d| d.flex_1().min_h_0())
            .overflow_hidden()
            .font_family(ui::MONO)
            .text_size(px(FONT))
            .line_height(px(LINE))
            .text_color(c(term_fg()))
            .cursor_text()
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                this.term_key(id, ev, window, cx)
            }))
            .child(
                canvas(
                    move |b, _, _| h_cell.set(f32::from(b.size.height)),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(panes);
        let strip = self.tab_strip(id, &tabs, active, shown.clone(), cx);
        if tab {
            return Some(self.tab_terminal(id, cols, rows, quick, strip, screen, side, cx));
        }
        let mut bar = div()
            .h(px(BAR_H))
            .flex_none()
            .pl(px(10.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(c(term_line()))
            .child(
                div()
                    .id("term-keys")
                    .pr(px(8.))
                    .child(
                        Icon::new(IconName::SquareTerminal)
                            .size(px(13.))
                            .text_color(c(term_dim())),
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(KEYS_HINT).build(window, cx)
                    }),
            )
            .child(strip)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(12.))
                    .truncate()
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .text_color(c(term_dim()))
                    .child(place),
            );
        let mut pills = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pl(px(8.));
        for (k, (label, command)) in quick.into_iter().enumerate() {
            pills = pills.child(
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
        bar = bar
            .child(pills)
            .child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h(px(18.))
                    .mx(px(8.))
                    .bg(c(term_line())),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .pr(px(8.))
                    .child(self.side_button(id, side, cx))
                    .child(
                        bar_btn(
                            "term-max",
                            if max {
                                IconName::Minimize2
                            } else {
                                IconName::Maximize2
                            },
                            max,
                            if max {
                                "Back under the page"
                            } else {
                                "Fill the page"
                            },
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.toggle_term_max(id, window, cx),
                        )),
                    )
                    .child(
                        bar_btn(
                            "term-close",
                            IconName::ChevronDown,
                            false,
                            "Hide; the shells keep running",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.term_open = None;
                            cx.notify();
                        })),
                    ),
            );
        let body = div()
            .mx(px(8.))
            .rounded_t(px(10.))
            .overflow_hidden()
            .bg(c(term_bg()))
            .border_1()
            .border_b_0()
            .border_color(c(term_line()))
            .flex()
            .flex_col()
            .child(bar)
            .child(screen);
        if max {
            // Fills the page in place of what it was under.
            return Some(body.flex_1().min_h_0().mt(px(8.)).into_any_element());
        }
        // Rises out of its own slot when opened, opaque the whole way, so the
        // translucent status bar under it never shows through mid-motion.
        Some(
            super::motion::drawer(body.flex_none(), super::motion::key(("drawer", id)), 24.)
                .into_any_element(),
        )
    }

    /// The drawer's tabs, then a button for one more.
    fn tab_strip(
        &mut self,
        id: u64,
        tabs: &[(u32, String, bool)],
        active: usize,
        shown: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Div {
        let accent = ui::accent_bar();
        let lit = super::theme::mix(term_bg(), term_fg(), 0.07);
        let mut strip = div().h_full().flex().flex_none().items_center().gap(px(2.));
        for (k, (n, label, ended)) in tabs.iter().enumerate() {
            let front = k == active;
            let on_screen = shown.contains(&k);
            let group: SharedString = format!("term-tab-{k}").into();
            let dot = if *ended {
                0xE5534B
            } else if front {
                accent
            } else if on_screen {
                term_fg()
            } else {
                term_dim()
            };
            let edge = if front {
                accent
            } else if on_screen {
                term_dim()
            } else {
                term_bg()
            };
            strip = strip.child(
                div()
                    .id(("term-tab", k))
                    .group(group.clone())
                    .h_full()
                    .pl(px(11.))
                    .pr(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(c(edge))
                    .when(front, |d| d.bg(c(lit)).text_color(c(term_fg())))
                    .when(!front, |d| {
                        d.text_color(c(term_dim()))
                            .hover(|s| s.text_color(c(term_fg())))
                    })
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(c(dot)))
                    .child(
                        div()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .whitespace_nowrap()
                            .child(label.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(c(term_dim()))
                            .child(n.to_string()),
                    )
                    .child(
                        div()
                            .id(("term-tab-close", k))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_color(c(term_dim()))
                            .when(!front, |d| {
                                d.opacity(0.).group_hover(group, |s| s.opacity(1.))
                            })
                            .hover(|s| s.text_color(c(term_fg())).bg(c(term_line())))
                            .child(Icon::new(IconName::X).size(px(11.)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_term_tab(id, k, cx)
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_term_tab(id, k, window, cx)
                    })),
            );
        }
        let new_tip: SharedString = format!("New tab ({})", key_name("T")).into();
        strip.child(div().pl(px(4.)).child(
            bar_btn("term-new", IconName::Plus, false, new_tip).on_click(
                cx.listener(move |this, _, window, cx| this.new_term_tab(id, window, cx)),
            ),
        ))
    }

    /// The button that lays the tabs side by side.
    fn side_button(&mut self, id: u64, side: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let tip: SharedString = if side {
            format!("One at a time ({})", key_name("D"))
        } else {
            format!("Side by side ({})", key_name("D"))
        }
        .into();
        bar_btn("term-side", IconName::Columns2, side, tip)
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_term_side(id, window, cx)))
    }

    /// The Terminal tab's layout around the shells.
    #[allow(clippy::too_many_arguments)]
    fn tab_terminal(
        &mut self,
        id: u64,
        cols: u16,
        rows: u16,
        quick: Vec<(&'static str, String)>,
        strip: Div,
        screen: Stateful<Div>,
        side: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut bar = div().flex().items_center().gap(px(6.)).child(
            div()
                .mr(px(2.))
                .font_family(ui::MONO)
                .text_size(px(11.))
                .text_color(c(ui::faint()))
                .child("Quick"),
        );
        for (k, (label, command)) in quick.into_iter().enumerate() {
            bar = bar.child(
                div()
                    .id(("term-quick", k))
                    .h(px(24.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded(px(4.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(ui::body()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(c(ui::ink())).bg(c(ui::hover())))
                    .when(label == "Run checks", |d| {
                        d.child(Icon::new(IconName::Play).size(px(10.)))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.term_run(id, &command, window, cx)
                    })),
            );
        }
        bar = bar.child(div().flex_1()).child(
            div()
                .font_family(ui::MONO)
                .text_size(px(11.))
                .text_color(c(ui::faint()))
                .child(format!(
                    "{} \u{b7} {cols}\u{d7}{rows} {}",
                    shell_name(),
                    KEY
                )),
        );
        let top = div()
            .h(px(32.))
            .flex_none()
            .pl(px(6.))
            .pr(px(6.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(c(term_line()))
            .child(strip)
            .child(div().flex_1())
            .child(self.side_button(id, side, cx));
        div()
            .size_full()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(px(6.))
                    .bg(c(term_bg()))
                    .child(top)
                    .child(screen.pt(px(4.))),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child("These shells run in the job's own copy of the repo, so nothing here touches your checkout. squad drives the crew from here."),
            )
            .into_any_element()
    }

    fn term_key(
        &mut self,
        id: u64,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        // The drawer's own keys: Cmd on a Mac, Ctrl+Shift elsewhere.
        let chord = if cfg!(target_os = "macos") {
            m.platform && !m.control && !m.alt
        } else {
            m.control && m.shift && !m.alt
        };
        if chord {
            let done = match ks.key.as_str() {
                "t" => {
                    self.new_term_tab(id, window, cx);
                    true
                }
                "d" => {
                    self.toggle_term_side(id, window, cx);
                    true
                }
                "w" => {
                    let k = self.terms.get(&id).map_or(0, |d| d.active);
                    self.close_term_tab(id, k, cx);
                    true
                }
                "enter" if m.shift => {
                    self.toggle_term_max(id, window, cx);
                    true
                }
                _ => false,
            };
            if done {
                cx.stop_propagation();
                return;
            }
        }
        let Some(sh) = self.terms.get(&id).and_then(Drawer::front) else {
            return;
        };
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

/// A drawer key as it's written on this platform: Cmd+`k` on a Mac, else
/// Ctrl+Shift+`k`.
fn key_name(k: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("\u{2318}{k}")
    } else {
        format!("Ctrl Shift {k}")
    }
}

/// A small icon button on the drawer's dark bar, lit in the accent while `on`.
fn bar_btn(
    id: &'static str,
    icon: IconName,
    on: bool,
    tip: impl Into<SharedString>,
) -> Stateful<Div> {
    let tip: SharedString = tip.into();
    div()
        .id(id)
        .size(px(24.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(c(if on { ui::accent_bar() } else { term_dim() }))
        .when(on, |d| d.bg(c(term_line())))
        .hover(|s| s.text_color(c(term_fg())).bg(c(term_line())))
        .child(Icon::new(icon).size(px(14.)))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
}

/// A shell's screen as rows of styled runs, with the cursor drawn when it
/// takes your keys.
fn screen_lines(sh: &pty::Shell, cursor: bool) -> Div {
    let exited = sh.exited();
    let color = |col: vt100::Color, default: u32| match col {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => pty::palette(i, true),
        vt100::Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
    };
    sh.with_screen(|screen| {
        let mut lines = div().flex().flex_col();
        let (rows, cols) = screen.size();
        let (cur_row, cur_col) = screen.cursor_position();
        let show_cursor = !screen.hide_cursor() && screen.scrollback() == 0 && !exited;
        for row in 0..rows {
            let mut line = div().h(px(LINE)).flex().whitespace_nowrap();
            let mut run = String::new();
            let mut style: Option<(u32, Option<u32>, bool)> = None;
            let flush = |line: Div, run: &mut String, style: Option<(u32, Option<u32>, bool)>| {
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
                if show_cursor && cursor && row == cur_row && col == cur_col {
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
    })
}
