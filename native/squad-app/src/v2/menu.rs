//! Right-click menus: on a project, a thread, a bot, a job and a
//! message. Each offers what that thing's own buttons do, plus the things
//! that otherwise take a trip (copy a path, show it in Finder, a terminal).
//!
//! It also holds the confirm for taking a project away: off the list only
//! (the files stay), or the folder itself to the Trash.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::channel::Reply;
use super::data::Member;
use super::store::Status;
use super::ui::{self, c, ca};
use super::{Place, Shell};

/// What was right-clicked.
#[derive(Clone)]
pub(super) enum Ctx {
    /// A project, from the switcher, by its folder's path.
    Project(String),
    /// The switcher's list of projects, dropped from the head of the panel.
    Projects,
    /// The shapes and colours for a project's emblem, by its folder's path.
    Emblem(String),
    /// A # thread in the sidebar: the project's path and the thread.
    Thread(String, u64),
    /// A bot in Direct messages.
    Bot(Member),
    /// A job's row or card.
    Job(u64),
    /// A message in a thread or a DM: its text, who a reply goes to, your own
    /// words to send again, and the job it's about.
    Message {
        copy: String,
        reply: Option<Reply>,
        edit: Option<String>,
        job: Option<u64>,
    },
    /// The file open in the viewer, from its Open with button.
    File(PathBuf),
}

#[derive(Clone)]
enum Act {
    OpenProject(String),
    AddProject,
    /// Opens the emblem picker for a project, where its menu was.
    PickEmblem(String),
    /// A project's emblem takes the shape at this place in `emblem::SHAPES`.
    Shape(String, usize),
    /// A project's emblem takes the colour at this place in `emblem::TINTS`.
    Tint(String, usize),
    /// Squad picks the project's emblem again.
    AutoEmblem(String),
    NewThread(String),
    ProjectTerminal(String),
    RemoveProject(String),
    DeleteProject(String),
    OpenThread(String, u64),
    MarkRead(String, u64),
    RenameThread(String, u64),
    ArchiveThread(String, u64),
    DeleteThread(String, u64),
    Chat(Member),
    CrewPage,
    EditBot(Member),
    RemoveBot(Member),
    OpenJob(u64),
    StopJob(u64),
    RunAgain(u64),
    JobTerminal(u64),
    Reply(Reply),
    Edit(String),
    Quote(String),
    Copy(String, &'static str),
    Reveal(PathBuf),
    OpenLink(String),
    /// A file, in the app at the second path, or its default app.
    OpenWith(PathBuf, Option<PathBuf>),
}

struct Item {
    label: SharedString,
    icon: IconName,
    act: Act,
    danger: bool,
    /// A project's emblem drawn in place of the icon.
    lead: Option<super::emblem::Emblem>,
    /// The quiet words on the right, and whether they're a status to stand out.
    note: Option<(String, Note)>,
    /// The one you're on: in bold, like a picked row.
    on: bool,
}

/// How a note on a menu row is drawn.
#[derive(Clone, Copy, PartialEq)]
enum Note {
    /// Faint mono, like a shortcut.
    Key,
    /// Something waits on you: ink on the accent.
    Ask,
    /// The crew works there: blue.
    Busy,
}

/// `None` draws a hairline between groups.
type Entry = Option<Item>;

fn item(label: impl Into<SharedString>, icon: IconName, act: Act) -> Entry {
    Some(Item {
        label: label.into(),
        icon,
        act,
        danger: false,
        lead: None,
        note: None,
        on: false,
    })
}

fn danger(label: impl Into<SharedString>, icon: IconName, act: Act) -> Entry {
    Some(Item {
        label: label.into(),
        icon,
        act,
        danger: true,
        lead: None,
        note: None,
        on: false,
    })
}

/// The keys that open the project at `i` in the list, the first nine:
/// ⌥⌘ and its number on a Mac, Ctrl Alt and its number elsewhere.
pub(super) fn project_key(i: usize) -> Option<String> {
    (i < 9).then(|| {
        if cfg!(target_os = "macos") {
            format!("\u{2325}\u{2318}{}", i + 1)
        } else {
            format!("Ctrl Alt {}", i + 1)
        }
    })
}

/// "Show in Finder" on a Mac, the plain words elsewhere.
fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Show in file manager"
    }
}

/// No hairline first, last, or twice in a row, where a group came out empty.
fn tidy(mut v: Vec<Entry>) -> Vec<Entry> {
    let mut prev_sep = true;
    v.retain(|e| {
        let keep = !(e.is_none() && prev_sep);
        prev_sep = e.is_none();
        keep
    });
    while matches!(v.last(), Some(None)) {
        v.pop();
    }
    v
}

impl Shell {
    /// A right-button handler that opens the menu for `target` at the pointer.
    pub(super) fn on_menu(
        &self,
        target: Ctx,
        cx: &mut Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        let this = cx.entity().downgrade();
        move |ev: &MouseDownEvent, _: &mut Window, cx: &mut App| {
            let (at, target) = (ev.position, target.clone());
            let _ = this.update(cx, |this, cx| {
                this.menu = Some((at, target));
                this.menu_hot = None;
                cx.notify();
            });
            cx.stop_propagation();
        }
    }

    /// `button` opening the menu for `target` just below it, lined up with its
    /// left edge, the way a dropdown does, rather than over it at the pointer.
    pub(super) fn drop_menu(
        &self,
        target: Ctx,
        button: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Div {
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let seen = bounds.clone();
        let this = cx.entity().downgrade();
        div()
            .relative()
            .flex_none()
            .child(button.on_mouse_down(
                MouseButton::Left,
                move |ev: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                    let b = seen.get();
                    let at = if b.size.width > px(0.) {
                        point(b.left(), b.bottom() + px(4.))
                    } else {
                        ev.position
                    };
                    let target = target.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.menu = Some((at, target));
                        this.menu_hot = None;
                        cx.notify();
                    });
                    cx.stop_propagation();
                },
            ))
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.menu_hot = None;
        cx.notify();
    }

    /// Makes the project at `path` the one you're in. False when it's gone.
    fn pick_project(&mut self, path: &str) -> bool {
        let Some(i) = self.crew.store.folders.iter().position(|f| f.path == path) else {
            return false;
        };
        if self.crew.store.current != i {
            self.crew.store.current = i;
            self.crew.store.save();
        }
        true
    }

    fn menu_items(&mut self, target: &Ctx) -> Vec<Entry> {
        let store = &self.crew.store;
        match target {
            Ctx::Project(path) => {
                let Some(f) = store.folders.iter().find(|f| &f.path == path) else {
                    return Vec::new();
                };
                let p = f.path.clone();
                vec![
                    item("Open", IconName::ArrowRight, Act::OpenProject(p.clone())),
                    item("New thread", IconName::Hash, Act::NewThread(p.clone())),
                    item(
                        "Change emblem\u{2026}",
                        IconName::Palette,
                        Act::PickEmblem(p.clone()),
                    ),
                    item(
                        "Open terminal here",
                        IconName::SquareTerminal,
                        Act::ProjectTerminal(p.clone()),
                    ),
                    None,
                    item(
                        "Copy path",
                        IconName::Copy,
                        Act::Copy(p.clone(), "project path"),
                    ),
                    item(
                        reveal_label(),
                        IconName::FolderOpen,
                        Act::Reveal(PathBuf::from(&p)),
                    ),
                    None,
                    item(
                        "Remove from Squad Code…",
                        IconName::FolderMinus,
                        Act::RemoveProject(p.clone()),
                    ),
                    danger(
                        "Delete folder from disk…",
                        IconName::Trash,
                        Act::DeleteProject(p),
                    ),
                ]
            }
            Ctx::Projects => {
                let emblems = super::emblem::all(&store.folders);
                let current = store.current;
                let folders: Vec<(String, String)> = store
                    .folders
                    .iter()
                    .map(|f| (f.name.clone(), f.path.clone()))
                    .collect();
                let mut v: Vec<Entry> = Vec::new();
                for (i, (name, path)) in folders.into_iter().enumerate() {
                    let rows = self.thread_rows_in(&path);
                    let busy: usize = rows.iter().map(|r| r.2).sum();
                    let asks: usize = rows.iter().map(|r| r.3).sum();
                    let note = if asks > 0 {
                        Some((format!("{asks} waiting"), Note::Ask))
                    } else if busy > 0 {
                        Some((format!("{busy} working"), Note::Busy))
                    } else {
                        project_key(i).map(|k| (k, Note::Key))
                    };
                    v.push(Some(Item {
                        label: name.into(),
                        icon: IconName::Folder,
                        act: Act::OpenProject(path),
                        danger: false,
                        lead: emblems.get(i).copied(),
                        note,
                        on: i == current,
                    }));
                }
                v.push(None);
                v.push(item(
                    "Add a project\u{2026}",
                    IconName::Plus,
                    Act::AddProject,
                ));
                v
            }
            Ctx::Emblem(path) => {
                let Some(i) = store.folders.iter().position(|f| &f.path == path) else {
                    return Vec::new();
                };
                let now = super::emblem::all(&store.folders)[i];
                let mut v: Vec<Entry> = Vec::new();
                for (k, (name, _)) in super::emblem::SHAPES.iter().enumerate() {
                    v.push(Some(Item {
                        label: (*name).into(),
                        icon: IconName::Circle,
                        act: Act::Shape(path.clone(), k),
                        danger: false,
                        lead: Some(super::emblem::Emblem {
                            shape: k,
                            tint: now.tint,
                        }),
                        note: None,
                        on: k == now.shape,
                    }));
                }
                v.push(None);
                for k in 0..super::emblem::TINTS.len() {
                    v.push(Some(Item {
                        label: "".into(),
                        icon: IconName::Circle,
                        act: Act::Tint(path.clone(), k),
                        danger: false,
                        lead: Some(super::emblem::Emblem {
                            shape: now.shape,
                            tint: k,
                        }),
                        note: None,
                        on: k == now.tint,
                    }));
                }
                v.push(None);
                v.push(item(
                    "Let Squad pick",
                    IconName::Sparkles,
                    Act::AutoEmblem(path.clone()),
                ));
                v
            }
            Ctx::Thread(path, id) => {
                let (path, id) = (path.clone(), *id);
                let name = store.thread_label(id);
                let mut v = vec![item(
                    "Open",
                    IconName::ArrowRight,
                    Act::OpenThread(path.clone(), id),
                )];
                if store.unread(&path, id) {
                    v.push(item(
                        "Mark as read",
                        IconName::MailOpen,
                        Act::MarkRead(path.clone(), id),
                    ));
                }
                v.push(item(
                    "Copy name",
                    IconName::Copy,
                    Act::Copy(format!("#{name}"), "thread name"),
                ));
                // #general always stays, and keeps its name.
                if id != 0 {
                    v.push(item(
                        "Rename…",
                        IconName::Pencil,
                        Act::RenameThread(path.clone(), id),
                    ));
                    v.push(None);
                    v.push(item(
                        "Archive",
                        IconName::Archive,
                        Act::ArchiveThread(path.clone(), id),
                    ));
                    v.push(danger(
                        "Delete thread…",
                        IconName::Trash,
                        Act::DeleteThread(path, id),
                    ));
                }
                v
            }
            Ctx::Bot(who) => {
                let who = *who;
                let mut v = vec![
                    item(
                        format!("Message {}", who.name()),
                        IconName::MessageSquareText,
                        Act::Chat(who),
                    ),
                    item(
                        if who.is_made() {
                            "Rename or edit…"
                        } else {
                            "Edit setup…"
                        },
                        IconName::Pencil,
                        Act::EditBot(who),
                    ),
                    item("Show on the Crew page", IconName::Users, Act::CrewPage),
                ];
                // Peek leads the crew and always stays.
                if who != Member::Fin {
                    v.push(None);
                    v.push(danger(
                        "Remove from crew…",
                        IconName::UserMinus,
                        Act::RemoveBot(who),
                    ));
                }
                v
            }
            Ctx::Job(id) => self.job_items(*id),
            Ctx::File(path) => {
                let apps = self.viewer_apps();
                let mut v: Vec<Entry> = Vec::new();
                if apps.is_empty() {
                    v.push(item(
                        "Open in its default app",
                        IconName::SquareArrowOutUpRight,
                        Act::OpenWith(path.clone(), None),
                    ));
                }
                for (i, (name, app)) in apps.into_iter().enumerate() {
                    v.push(item(
                        if i == 0 {
                            format!("{name} (default)")
                        } else {
                            name
                        },
                        IconName::AppWindow,
                        Act::OpenWith(path.clone(), Some(app)),
                    ));
                    if i == 0 {
                        v.push(None);
                    }
                }
                v.push(None);
                v.push(item(
                    "Copy path",
                    IconName::Copy,
                    Act::Copy(path.display().to_string(), "path"),
                ));
                v.push(item(
                    reveal_label(),
                    IconName::FolderOpen,
                    Act::Reveal(path.clone()),
                ));
                v
            }
            Ctx::Message {
                copy,
                reply,
                edit,
                job,
            } => {
                let mut v = vec![item(
                    "Copy text",
                    IconName::Copy,
                    Act::Copy(copy.clone(), "message"),
                )];
                if let Some(r) = reply {
                    v.push(item("Reply", IconName::MessageSquareText, Act::Reply(*r)));
                }
                v.push(item(
                    "Quote in reply",
                    IconName::Quote,
                    Act::Quote(copy.clone()),
                ));
                if let Some(text) = edit {
                    v.push(item(
                        "Edit and send again",
                        IconName::Pencil,
                        Act::Edit(text.clone()),
                    ));
                }
                if let Some(id) = job {
                    v.push(None);
                    v.extend(self.job_items(*id));
                }
                v
            }
        }
    }

    fn job_items(&self, id: u64) -> Vec<Entry> {
        let Some(j) = self.crew.store.job(id) else {
            return Vec::new();
        };
        let mut v = vec![item("Open job", IconName::ArrowRight, Act::OpenJob(id))];
        if j.status == Status::Running {
            v.push(danger("Stop", IconName::CircleStop, Act::StopJob(id)));
        }
        if matches!(
            j.status,
            Status::Failed | Status::Stopped | Status::Interrupted
        ) {
            v.push(item("Run again", IconName::RotateCcw, Act::RunAgain(id)));
        }
        v.push(None);
        v.push(item(
            "Open terminal in its folder",
            IconName::SquareTerminal,
            Act::JobTerminal(id),
        ));
        v.push(item(
            reveal_label(),
            IconName::FolderOpen,
            Act::Reveal(PathBuf::from(j.dir())),
        ));
        v.push(None);
        if !j.pr.is_empty() {
            v.push(item(
                "Open pull request",
                IconName::GitPullRequest,
                Act::OpenLink(j.pr.clone()),
            ));
        }
        if !j.branch.is_empty() {
            v.push(item(
                "Copy branch name",
                IconName::GitBranch,
                Act::Copy(j.branch.clone(), "branch name"),
            ));
        }
        v.push(item(
            "Copy title",
            IconName::Copy,
            Act::Copy(j.label().to_string(), "job title"),
        ));
        v
    }

    fn run_menu(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let was_at = self.menu.as_ref().map(|m| m.0);
        self.menu = None;
        self.menu_hot = None;
        match act {
            Act::AddProject => self.pick_folder(cx),
            Act::PickEmblem(p) => {
                if let Some(at) = was_at {
                    self.menu = Some((at, Ctx::Emblem(p)));
                }
            }
            Act::Shape(p, k) => {
                let tint = super::emblem::of(&self.crew.store.folders, &p).tint;
                if let Some(f) = self.crew.store.folders.iter_mut().find(|f| f.path == p) {
                    f.emblem = super::emblem::SHAPES[k].0.to_string();
                    // Keep the colour it shows now, so changing one doesn't move the other.
                    f.tint = Some(tint as u8);
                }
                self.crew.store.save();
                // Stay open, to try another.
                self.menu = was_at.map(|at| (at, Ctx::Emblem(p)));
            }
            Act::Tint(p, k) => {
                let shape = super::emblem::of(&self.crew.store.folders, &p).shape;
                if let Some(f) = self.crew.store.folders.iter_mut().find(|f| f.path == p) {
                    f.tint = Some(k as u8);
                    f.emblem = super::emblem::SHAPES[shape].0.to_string();
                }
                self.crew.store.save();
                self.menu = was_at.map(|at| (at, Ctx::Emblem(p)));
            }
            Act::AutoEmblem(p) => {
                if let Some(f) = self.crew.store.folders.iter_mut().find(|f| f.path == p) {
                    f.emblem.clear();
                    f.tint = None;
                }
                self.crew.store.save();
            }
            Act::OpenProject(p) => {
                if self.pick_project(&p) {
                    self.go(Place::Today, cx);
                }
            }
            Act::NewThread(p) => {
                if self.pick_project(&p) {
                    self.new_thread(window, cx);
                }
            }
            Act::ProjectTerminal(p) => {
                if self.pick_project(&p) {
                    // Opens it, or leaves the one that's open in place.
                    let id = super::term::project_id(self.crew.store.current);
                    if self.term_open != Some(id) {
                        self.toggle_project_term(window, cx);
                    }
                }
            }
            Act::RemoveProject(p) => self.ask_gone(&p, false, cx),
            Act::DeleteProject(p) => self.ask_gone(&p, true, cx),
            Act::OpenThread(p, id) => {
                if self.pick_project(&p) {
                    self.crew.store.thread_now = id;
                    self.crew.store.save();
                    self.go(Place::Today, cx);
                }
            }
            Act::MarkRead(p, id) => {
                self.crew.store.mark_seen(&p, id);
                self.crew.store.save();
            }
            Act::RenameThread(p, id) => {
                if self.pick_project(&p) {
                    // In the thread, with /rename and its name in the box to change.
                    self.crew.store.thread_now = id;
                    self.crew.store.save();
                    self.go(Place::Today, cx);
                    let name = self.crew.store.thread_label(id);
                    self.composer.update(cx, |i, cx| {
                        ui::put_text(i, format!("/rename {name}"), window, cx);
                        i.focus(window, cx);
                    });
                }
            }
            Act::ArchiveThread(p, id) => {
                if self.pick_project(&p) {
                    self.archive_thread(id, cx);
                }
            }
            Act::DeleteThread(p, id) => {
                if self.pick_project(&p) {
                    self.deleting = Some(id);
                }
            }
            Act::Chat(who) => self.go(Place::Chat(who), cx),
            Act::CrewPage => self.go(Place::Crew, cx),
            Act::EditBot(who) => {
                self.go(Place::Crew, cx);
                self.open_setup(who, window, cx);
            }
            Act::RemoveBot(who) => self.ask_remove(who, cx),
            Act::OpenJob(id) => self.open_job(id, cx),
            Act::StopJob(id) => self.stop_job(id, cx),
            Act::RunAgain(id) => self.retry_job(id, cx),
            Act::JobTerminal(id) => {
                if self.place != Place::Live(id) {
                    self.go(Place::Live(id), cx);
                }
                if self.term_open != Some(id) {
                    self.toggle_term(id, window, cx);
                }
            }
            Act::Reply(Reply::Job(id)) => self.reply_open = Some(id),
            Act::Reply(Reply::Bot(who)) if self.dm() == Some(who) => {
                self.put_in_box(String::new(), window, cx)
            }
            Act::Reply(Reply::Bot(who)) => self.put_in_box(format!("@{} ", who.name()), window, cx),
            Act::Edit(text) => self.put_in_box(text, window, cx),
            Act::Quote(text) => {
                let quoted: String = text
                    .trim()
                    .lines()
                    .take(12)
                    .map(|l| format!("> {l}\n"))
                    .collect();
                self.put_in_box(format!("{quoted}\n"), window, cx)
            }
            Act::Copy(text, what) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.say(format!("Copied the {what}."));
            }
            Act::Reveal(path) => cx.reveal_path(&path),
            Act::OpenLink(url) => cx.open_url(&url),
            Act::OpenWith(path, app) => super::viewer::open_in(&path, app.as_deref(), cx),
        }
        cx.notify();
    }

    /// Arrow keys, Enter and Escape while a menu is open. Whether the key was used.
    pub(super) fn menu_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((_, target)) = self.menu.clone() else {
            return false;
        };
        let items = tidy(self.menu_items(&target));
        let entries: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_some())
            .map(|(i, _)| i)
            .collect();
        let at = self
            .menu_hot
            .and_then(|i| entries.iter().position(|e| *e == i));
        match key {
            "escape" => self.close_menu(cx),
            "up" | "down" if !entries.is_empty() => {
                let n = entries.len() as isize;
                let next = match (at, key) {
                    (None, "up") => n - 1,
                    (None, _) => 0,
                    (Some(i), "up") => (i as isize - 1).rem_euclid(n),
                    (Some(i), _) => (i as isize + 1).rem_euclid(n),
                };
                self.menu_hot = Some(entries[next as usize]);
                cx.notify();
            }
            "enter" | "space" => {
                let act = self
                    .menu_hot
                    .and_then(|i| items.into_iter().nth(i).flatten())
                    .map(|e| e.act);
                match act {
                    Some(act) => self.run_menu(act, window, cx),
                    None => self.close_menu(cx),
                }
            }
            _ => return false,
        }
        true
    }

    /// The open menu, at the pointer and kept inside the window.
    pub(super) fn menu_view(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, target) = self.menu.clone()?;
        let items = tidy(self.menu_items(&target));
        if items.is_empty() {
            return None;
        }
        // The emblem picker lays its shapes and colours out as a grid.
        let picker = matches!(target, Ctx::Emblem(_));
        let mut list = div().flex().flex_col().p(px(5.));
        let mut grid: Option<Div> = None;
        let mut swatch: Option<Div> = None;
        let mut section = 0;
        for (i, entry) in items.into_iter().enumerate() {
            let Some(entry) = entry else {
                if picker {
                    section += 1;
                    continue;
                }
                list = list.child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(c(ui::line())));
                continue;
            };
            let hot = self.menu_hot == Some(i);
            let act = entry.act;
            if picker && section < 2 {
                let lead = entry
                    .lead
                    .unwrap_or(super::emblem::Emblem { shape: 0, tint: 0 });
                let click =
                    cx.listener(move |this, _, window, cx| this.run_menu(act.clone(), window, cx));
                if section == 0 {
                    let cell = div()
                        .id(("menu-shape", i))
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .map(|d| {
                            if entry.on {
                                d.child(super::sidebar::emblem_tile(lead, 32.))
                            } else {
                                d.text_color(c(ui::muted()))
                                    .when(hot, |d| d.bg(c(ui::hover())))
                                    .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                                    .child(Icon::new(lead.icon()).size(px(16.)))
                            }
                        })
                        .on_click(click);
                    grid = Some(
                        grid.unwrap_or_else(|| {
                            div()
                                .w(px(7. * 34.))
                                .flex()
                                .flex_wrap()
                                .gap(px(2.))
                                .p(px(4.))
                        })
                        .child(cell),
                    );
                } else {
                    let col = lead.color();
                    let dot = div()
                        .id(("menu-tint", i))
                        .size(px(22.))
                        .rounded_full()
                        .cursor_pointer()
                        .bg(c(col))
                        .border_2()
                        .border_color(c(ui::float()))
                        .when(entry.on || hot, |d| {
                            d.shadow(vec![BoxShadow {
                                color: c(if entry.on { ui::ink() } else { ui::line_hi() }),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(1.5),
                                inset: false,
                            }])
                        })
                        .on_click(click);
                    swatch = Some(
                        swatch
                            .unwrap_or_else(|| {
                                div().flex().gap(px(6.)).px(px(8.)).pt(px(6.)).pb(px(8.))
                            })
                            .child(dot),
                    );
                }
                continue;
            }
            if let Some(g) = grid.take() {
                list = list.child(g);
            }
            if let Some(sw) = swatch.take() {
                list = list
                    .child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(c(ui::line())))
                    .child(sw)
                    .child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(c(ui::line())));
            }
            let (ink, glyph, wash) = if entry.danger {
                (ui::t_red(), ui::t_red(), ca(ui::RED, 0.10))
            } else {
                (ui::ink(), ui::muted(), c(ui::hover()))
            };
            let tall = entry.lead.is_some();
            list = list.child(
                div()
                    .id(("menu-item", i))
                    .h(px(if tall { 34. } else { 30. }))
                    .px(px(if tall { 6. } else { 9. }))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(c(ink))
                    .when(entry.on, |d| d.font_weight(FontWeight::SEMIBOLD).bg(wash))
                    .when(hot, |d| d.bg(wash))
                    .hover(move |s| s.bg(wash))
                    .map(|d| match entry.lead {
                        Some(e) => d.child(super::sidebar::emblem_tile(e, 22.)),
                        None => d.child(Icon::new(entry.icon).size(px(14.)).text_color(c(glyph))),
                    })
                    .child(div().flex_1().min_w_0().truncate().child(entry.label))
                    .when_some(entry.note, |d, (text, kind)| {
                        let n = div()
                            .flex_none()
                            .ml(px(10.))
                            .font_family(ui::MONO)
                            .text_size(px(10.5))
                            .font_weight(FontWeight::NORMAL);
                        d.child(match kind {
                            Note::Key => n.text_color(c(ui::faint())).child(text),
                            Note::Ask => n
                                .px(px(5.))
                                .rounded(px(3.))
                                .bg(c(ui::pick_fill()))
                                .text_color(c(ui::pick_on()))
                                .font_weight(FontWeight::MEDIUM)
                                .child(text),
                            Note::Busy => n
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_color(c(ui::faint()))
                                .child(div().size(px(6.)).rounded(px(1.)).bg(c(ui::t_blue())))
                                .child(text),
                        })
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_menu(act.clone(), window, cx)
                    })),
            );
        }
        if picker {
            list = list.child(
                div()
                    .px(px(9.))
                    .pt(px(2.))
                    .pb(px(4.))
                    .text_size(px(11.5))
                    .text_color(c(ui::faint()))
                    .child("Shape and colour for this project"),
            );
        }
        let menu = div()
            .id("menu")
            .min_w(px(if matches!(target, Ctx::Projects) {
                236.
            } else {
                220.
            }))
            .rounded(px(11.))
            .bg(c(ui::float()))
            .border_1()
            .border_color(c(ui::line()))
            .shadow(vec![BoxShadow {
                color: ca(0x000000, 0.18 * ui::float_shadow()),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-4.),
                inset: false,
            }])
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(list);
        // It fades in, settling a few pixels down into place.
        let menu = super::motion::pop(menu, super::motion::key("context-menu"), -4.);
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }
}

/// A project on its way out: off the list only, or the folder to the Trash.
pub(super) struct Gone {
    pub path: String,
    pub name: String,
    /// The folder goes to the Trash too, not just off the list.
    pub disk: bool,
    /// The "I understand" box, ticked: Delete only works once it is.
    pub sure: bool,
    /// Files with changes git hasn't saved in a commit, counted when asked.
    pub unsaved: usize,
    /// Moving to the Trash has started; the buttons wait for it.
    pub busy: bool,
}

/// Why a folder can't be deleted from here, if it can't: the places whole
/// lives sit (your home, the top of the disk, Desktop and friends) never go.
pub(super) fn too_big_to_delete(path: &Path, home: Option<&Path>) -> Option<&'static str> {
    if !path.is_absolute() {
        return Some("its path isn't a full one");
    }
    // `/`, `/Users` and `/Users/you` each have three parts or fewer.
    if path.components().count() <= 3 {
        return Some("it sits too near the top of the disk");
    }
    if let Some(home) = home {
        if home.starts_with(path) {
            return Some("it holds your home folder");
        }
        const KEEP: [&str; 12] = [
            "Desktop",
            "Documents",
            "Downloads",
            "Library",
            "Applications",
            "Pictures",
            "Music",
            "Movies",
            "Public",
            "Dropbox",
            "dev",
            "src",
        ];
        if path.parent() == Some(home)
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| KEEP.contains(&n) || n.starts_with('.'))
        {
            return Some("it's one of your home folder's own folders");
        }
    }
    None
}

/// How many files in `dir` have changes git hasn't committed (0 outside git).
fn unsaved_files(dir: &str) -> usize {
    std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0)
}

/// Moves `path` to the Trash (the Recycle Bin on Windows). Never deletes for
/// good: where there's no Trash to use, it says so and leaves the folder be.
pub(super) fn to_trash(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| e.localizedDescription().to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Add-Type -AssemblyName Microsoft.VisualBasic; \
                 [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($env:SQUAD_TRASH, 'OnlyErrorDialogs', 'SendToRecycleBin')",
            ])
            .env("SQUAD_TRASH", path)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // The desktop's own Trash: GNOME's gio, or trash-cli.
        let tries: [(&str, &[&str]); 2] = [("gio", &["trash", "--"]), ("trash-put", &["--"])];
        let mut last = String::from("no Trash tool (gio or trash-put) was found");
        for (tool, args) in tries {
            match std::process::Command::new(tool)
                .args(args)
                .arg(path)
                .stdin(std::process::Stdio::null())
                .output()
            {
                Ok(o) if o.status.success() => return Ok(()),
                Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
                Err(_) => {}
            }
        }
        Err(last)
    }
}

impl Shell {
    /// Opens the confirm for taking the project at `path` away.
    pub(super) fn ask_gone(&mut self, path: &str, disk: bool, cx: &mut Context<Self>) {
        let Some(f) = self.crew.store.folders.iter().find(|f| f.path == path) else {
            return;
        };
        let path = f.path.clone();
        self.gone = Some(Gone {
            path: path.clone(),
            name: f.name.clone(),
            disk,
            sure: false,
            unsaved: 0,
            busy: false,
        });
        cx.notify();
        // Counting unsaved files runs git, which can take a while in a big
        // repository: the confirm opens straight away and the count joins it.
        if disk {
            let dir = path.clone();
            let task = cx.background_spawn(async move { unsaved_files(&dir) });
            cx.spawn(async move |this, cx| {
                let n = task.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(g) = this.gone.as_mut().filter(|g| g.path == path) {
                        g.unsaved = n;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    /// Jobs still working in the project at `path`.
    fn running_in(&self, path: &str) -> usize {
        self.crew
            .store
            .jobs
            .iter()
            .filter(|j| {
                j.folder.path == path && matches!(j.status, Status::Running | Status::Shipping)
            })
            .count()
    }

    /// Takes the project off the list; with `disk`, its folder goes to the Trash first.
    fn confirm_gone(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.gone.as_mut() else {
            return;
        };
        if g.busy {
            return;
        }
        let (path, name, disk) = (g.path.clone(), g.name.clone(), g.disk);
        let forget = |this: &mut Self, cx: &mut Context<Self>| {
            if let Some(i) = this.crew.store.folders.iter().position(|f| f.path == path) {
                this.forget_folder(i, cx);
            }
        };
        if !disk {
            self.gone = None;
            forget(self, cx);
            self.say(format!(
                "Removed {name} from Squad Code. The folder is still on disk."
            ));
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(why) = too_big_to_delete(Path::new(&path), home.as_deref()) {
            self.say(format!("{name} can't be deleted from here: {why}."));
            cx.notify();
            return;
        }
        g.busy = true;
        cx.notify();
        let at = PathBuf::from(&path);
        let task = cx.background_spawn(async move { to_trash(&at) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                this.gone = None;
                match done {
                    Ok(()) => {
                        if let Some(i) = this.crew.store.folders.iter().position(|f| f.path == path)
                        {
                            this.forget_folder(i, cx);
                        }
                        this.say(format!(
                            "Moved {name} to the Trash. Restore it from there if you need it back."
                        ));
                    }
                    Err(e) => this.say(format!("Couldn't move {name} to the Trash: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Remove name?" or "Delete name from disk?", over the whole window.
    pub(super) fn gone_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let g = self.gone.as_ref()?;
        let (name, path, disk, sure, busy, unsaved) = (
            g.name.clone(),
            g.path.clone(),
            g.disk,
            g.sure,
            g.busy,
            g.unsaved,
        );
        let running = self.running_in(&path);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let blocked = if !disk {
            None
        } else if running > 0 {
            Some(format!(
                "{running} job{} still working here. Stop {} first.",
                if running == 1 { " is" } else { "s are" },
                if running == 1 { "it" } else { "them" }
            ))
        } else {
            too_big_to_delete(Path::new(&path), home.as_deref())
                .map(|why| format!("This folder can't be deleted from here: {why}."))
        };
        let tone = if disk { ui::bad_tone() } else { ui::ask_tone() };
        let close = |this: &mut Self, cx: &mut Context<Self>| {
            if !this.gone.as_ref().is_some_and(|g| g.busy) {
                this.gone = None;
                cx.notify();
            }
        };
        let scrim = div()
            .id("gone-scrim")
            .absolute()
            .inset_0()
            .bg(ca(0x000000, 0.28))
            .occlude()
            .on_click(cx.listener(move |this, _, _, cx| close(this, cx)));

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(13.))
            .text_color(c(ui::body()));
        if disk {
            body = body
                .child("This moves the whole folder, and every file in it, to the Trash:")
                .child(
                    div()
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(8.))
                        .bg(c(ui::raised()))
                        .border_1()
                        .border_color(c(ui::line()))
                        .font_family("Geist Mono")
                        .text_size(px(12.))
                        .text_color(c(ui::ink()))
                        .child(path.clone()),
                )
                .when(unsaved > 0, |d| {
                    d.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_color(c(ui::t_red()))
                            .child(
                                div()
                                    .pt(px(2.))
                                    .flex_none()
                                    .child(Icon::new(IconName::TriangleAlert).size(px(14.))),
                            )
                            .child(div().flex_1().min_w_0().child(format!(
                                "{unsaved} file{} changes that aren't committed. Anything not pushed lives only here.",
                                if unsaved == 1 { " has" } else { "s have" }
                            ))),
                    )
                })
                .child("Squad Code stops listing it too. Restore it from the Trash if you change your mind.");
        } else {
            body = body.child(
                "The folder stays on disk, untouched. Add it again any time and its threads and jobs come back.",
            );
        }
        if let Some(why) = blocked.clone() {
            body = body.child(
                div()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(c(ui::ask_tone().1))
                    .text_color(c(ui::ask_tone().0))
                    .child(why),
            );
        }
        // The box to tick before a folder can go: the path in front of you, and a
        // deliberate second click.
        let tick = (disk && blocked.is_none()).then(|| {
            div()
                .id("gone-sure")
                .flex()
                .items_center()
                .gap(px(9.))
                .cursor_pointer()
                .text_size(px(13.))
                .text_color(c(ui::ink()))
                .child(
                    div()
                        .size(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .border_1()
                        .map(|d| {
                            if sure {
                                d.bg(c(ui::bad_tone().0))
                                    .border_color(c(ui::bad_tone().0))
                                    .child(
                                        Icon::new(IconName::Check)
                                            .size(px(12.))
                                            .text_color(c(0xFFFFFF)),
                                    )
                            } else {
                                d.border_color(c(ui::line_hi()))
                            }
                        }),
                )
                .child(format!("Yes, move {name} and all its files to the Trash"))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(g) = this.gone.as_mut() {
                        g.sure = !g.sure;
                    }
                    cx.notify();
                }))
        });
        let can = !busy && blocked.is_none() && (!disk || sure);
        let go_label = match (disk, busy) {
            (true, true) => "Moving to Trash…",
            (true, false) => "Move to Trash",
            (false, _) => "Remove",
        };
        let switch = (!disk).then(|| {
            div()
                .id("gone-switch")
                .flex_1()
                .min_w_0()
                .text_size(px(12.5))
                .text_color(c(ui::muted()))
                .cursor_pointer()
                .hover(|s| s.text_color(c(ui::t_red())))
                .child("Delete it from disk instead…")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(path) = this.gone.as_ref().map(|g| g.path.clone()) {
                        this.ask_gone(&path, true, cx);
                    }
                }))
        });
        let card = div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_center()
            .child(
                div()
                    .id("gone-card")
                    .w(px(440.))
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .rounded(px(16.))
                    .bg(c(ui::float()))
                    .border_1()
                    .border_color(c(ui::line()))
                    .shadow(vec![BoxShadow {
                        color: ca(0x000000, 0.35 * ui::float_shadow()),
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
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .bg(c(tone.1))
                                    .child(
                                        Icon::new(if disk {
                                            IconName::Trash
                                        } else {
                                            IconName::FolderMinus
                                        })
                                        .size(px(16.))
                                        .text_color(c(tone.0)),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c(ui::ink()))
                                    .child(if disk {
                                        format!("Delete {name} from disk?")
                                    } else {
                                        format!("Remove {name} from Squad Code?")
                                    }),
                            ),
                    )
                    .child(body)
                    .children(tick)
                    .child(
                        div()
                            .mt(px(6.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .children(switch)
                            .child(
                                ui::ghost("gone-cancel", "Cancel")
                                    .flex_none()
                                    .on_click(cx.listener(move |this, _, _, cx| close(this, cx))),
                            )
                            .child(
                                ui::solid("gone-yes", go_label)
                                    .flex_none()
                                    .when(disk, |d| d.bg(c(ui::bad_tone().0)))
                                    .when(!can, |d| d.opacity(0.45).cursor_default())
                                    .when(can, |d| {
                                        d.on_click(
                                            cx.listener(|this, _, _, cx| this.confirm_gone(cx)),
                                        )
                                    }),
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
    use super::too_big_to_delete;
    use std::path::Path;

    #[test]
    fn whole_lives_never_go_to_the_trash() {
        let home = Some(Path::new("/Users/ana"));
        for p in [
            "/",
            "/Users",
            "/Users/ana",
            "/Users/ana/Desktop",
            "/Users/ana/Documents",
            "/Users/ana/dev",
            "/Users/ana/.ssh",
            "relative/path/here/x",
        ] {
            assert!(too_big_to_delete(Path::new(p), home).is_some(), "{p}");
        }
    }

    #[test]
    fn a_project_folder_can_go() {
        let home = Some(Path::new("/Users/ana"));
        for p in [
            "/Users/ana/dev/shop",
            "/Users/ana/Desktop/scratch",
            "/Users/ana/my-app",
            "/opt/work/app",
        ] {
            assert!(too_big_to_delete(Path::new(p), home).is_none(), "{p}");
        }
    }
}
