//! Squad Code v2: the crew app.
//!
//! A shell around Squad Code's bots: Today (hand the crew a job and see what is
//! moving), Live (watch one job as the bot works), Chat (ask a bot in plain
//! words), Crew, Inbox and History. Every job runs for real on the bot's
//! engine in a folder the person picked, and is kept between launches. The
//! bots are Fin from `squad-holo`, drawn as their moods change.
//!
//! Squad Code v1 stays one click away on the rail while v2 grows.

mod apps;
mod asks;
mod board;
mod brain;
mod builder;
mod channel;
mod chat;
pub mod cli;
mod crew;
mod data;
mod demo;
mod engine;
mod engines;
mod filetab;
mod gitcards;
mod goal;
mod history;
mod home;
mod issues;
mod jobs;
mod learn;
mod live;
mod machine;
mod machines;
mod mascot;
mod menu;
mod motion;
mod myserver;
mod overview;
mod palette;
mod peek;
mod prefs;
mod project;
mod race;
mod remote;
mod review;
mod rewind;
mod rfb;
mod rich;
mod runtime;
mod screenview;
pub mod server;
mod serverpage;
mod setup;
mod shift;
mod ship;
pub(crate) mod shown;
mod sidebar;
mod start;
mod statusbar;
mod store;
mod term;
mod theme;
mod today;
pub(crate) mod tray;
mod trouble;
mod ui;
mod usage;
mod viewer;
mod waiting;
mod wallpaper;
mod webpage;
mod welcome;
mod work;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use squad_holo::{CrewRenderer, Figure, Framing, Mood};

use data::{Member, Mode};
pub use prefs::Pane;
use store::{Status, Store};
use ui::{c, ca};

/// Where the person is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Today,
    /// The project's Overview: run, Git, setup and activity.
    Overview,
    Inbox,
    History,
    Chat(Member),
    Crew,
    /// One job, by id.
    Live(u64),
    /// The tries of one job on several engines, side by side, by the id of
    /// its first try.
    Race(u64),
    /// A goal split into jobs.
    Goal(u64),
    /// Settings, open on one pane.
    Settings(Pane),
    /// Your server: how it's doing, and its controls.
    Server,
    /// Your server's screen, live.
    Screen,
    /// A job's own browser, live, by the job's id.
    Browser(u64),
    /// The first-run walkthrough.
    Welcome,
}

/// The crew's frames, drawn once a tick and shared by every place that shows the
/// same bot at the same size. Small bots move less, so they get fewer ticks: a 24 px
/// avatar redraws 10 times a second, a big lively one 30. New frames are drawn on
/// the painter thread while the last one stays on screen, so the window itself
/// never waits for a bot. A still bot (motion off) is drawn once and never again.
struct Holos {
    frame: u64,
    images: HashMap<(Figure, u32, bool), Frame>,
    renderers: HashMap<u16, CrewRenderer>,
    /// The fastest rate any bot drawn this frame needed, so the clock can slow
    /// down on screens with only small bots.
    asked: u32,
    /// Replaced frames, released at the start of the next window frame, once the
    /// new ones have painted.
    retired: Vec<Arc<RenderImage>>,
    /// Frames asked of the painter and not back yet, one per bot at most.
    pending: std::collections::HashSet<Slot>,
    painter: Painters,
    painted: std::sync::mpsc::Receiver<Painted>,
    /// Time spent drawing first frames on the main thread this window frame.
    /// Past [`FIRST_BUDGET`] the rest go to the painters, so a screen full of
    /// new bots never stalls a frame.
    spent: Duration,
    /// Shown for the one frame a new bot waits on the painters.
    blank: Arc<RenderImage>,
}

/// Main-thread time a window frame may spend on first frames of new bots.
const FIRST_BUDGET: Duration = Duration::from_millis(5);

struct Frame {
    tick: u64,
    used: u64,
    image: Arc<RenderImage>,
}

/// Frames a second while anything moves: the painters draw at the display's
/// rate, so motion is never stepped.
const FPS: f64 = 60.0;

/// Frames a second for a bot this many logical pixels tall.
/// How often a bot of this size needs a new frame. A calm bot drifts slowly,
/// so it needs far fewer than one that is busy, celebrating or asking.
fn fps_for(size: f32, pose: &Figure) -> u32 {
    // Anything moving quickly gets every frame, even in a small avatar, so
    // hops, wiggles and mood changes never step. Calm bots drift so slowly
    // that fewer frames look the same.
    if pose.lively() || size >= 90.0 {
        60
    } else {
        30
    }
}

impl Holos {
    fn new() -> Self {
        let (painter, painted) = painter();
        Holos {
            pending: Default::default(),
            painter,
            painted,
            frame: 0,
            images: HashMap::new(),
            renderers: HashMap::new(),
            asked: 0,
            retired: Vec::new(),
            spent: Duration::ZERO,
            blank: blank_image(),
        }
    }

    /// Starts a window frame: frames replaced or not shown last time are released
    /// from the GPU atlas.
    fn begin(&mut self, window: &mut Window, cx: &mut App) {
        self.frame += 1;
        let frame = self.frame;
        self.asked = 0;
        self.spent = Duration::ZERO;
        self.collect();
        for old in self.retired.drain(..) {
            cx.drop_image(old, Some(window));
        }
        let gone: Vec<_> = self
            .images
            .iter()
            .filter(|(_, f)| f.used + 2 < frame)
            .map(|(k, _)| *k)
            .collect();
        for k in gone {
            if let Some(f) = self.images.remove(&k) {
                cx.drop_image(f.image, Some(window));
            }
        }
    }

    fn image(
        &mut self,
        pose: Figure,
        logical: f32,
        framing: Framing,
        t: f64,
        scale: f32,
        moving: bool,
    ) -> Arc<RenderImage> {
        let device = (logical * scale).round().clamp(8.0, 1024.0) as u16;
        let bust = framing == Framing::Bust;
        let fps = fps_for(logical, &pose);
        let tick = if moving { (t * fps as f64) as u64 } else { 0 };
        if moving {
            self.asked = self.asked.max(fps);
        }
        let frame = self.frame;
        let slot = (pose, device as u32, bust);
        // A frame on screen already: keep showing it, and have the painter draw
        // the next one off the main thread, so the window never waits on a bot.
        if let Some(f) = self.images.get_mut(&slot) {
            f.used = frame;
            if f.tick != tick && self.pending.insert(slot) {
                let at = if moving { tick as f64 / fps as f64 } else { t };
                let _ = self.painter.send(Paint {
                    slot,
                    tick,
                    at,
                    pose,
                    device,
                    framing,
                });
            }
            return f.image.clone();
        }
        // A bot's first frame is drawn right away, so it never shows up blank,
        // as long as this frame has time left; otherwise a painter draws it and
        // it appears on the next frame.
        let at = if moving { tick as f64 / fps as f64 } else { t };
        if self.spent >= FIRST_BUDGET {
            if self.pending.insert(slot) {
                let _ = self.painter.send(Paint {
                    slot,
                    tick,
                    at,
                    pose,
                    device,
                    framing,
                });
            }
            return self.blank.clone();
        }
        let began = Instant::now();
        let r = self
            .renderers
            .entry(device)
            .or_insert_with(|| CrewRenderer::new(device));
        let image = to_image(r.draw(&pose, at, framing), device);
        self.spent += began.elapsed();
        self.images.insert(
            slot,
            Frame {
                tick,
                used: frame,
                image: image.clone(),
            },
        );
        image
    }

    /// Takes the frames the painter has finished, in place of the ones showing.
    fn collect(&mut self) {
        while let Ok(done) = self.painted.try_recv() {
            self.pending.remove(&done.slot);
            if let Some(f) = self.images.get_mut(&done.slot) {
                self.retired
                    .push(std::mem::replace(&mut f.image, done.image));
                f.tick = done.tick;
            } else {
                // A first frame the painters drew while the window was busy.
                self.images.insert(
                    done.slot,
                    Frame {
                        tick: done.tick,
                        used: self.frame,
                        image: done.image,
                    },
                );
            }
        }
    }
}

type Slot = (Figure, u32, bool);

/// A bot frame for the painter to draw.
struct Paint {
    slot: Slot,
    tick: u64,
    at: f64,
    pose: Figure,
    device: u16,
    framing: Framing,
}

struct Painted {
    slot: Slot,
    tick: u64,
    image: Arc<RenderImage>,
}

/// The painters: threads that draw bot frames, so the window's own frames stay
/// quick. Each bot always goes to the same painter, and when one falls behind,
/// only the newest ask for each bot is drawn.
fn painter() -> (Painters, std::sync::mpsc::Receiver<Painted>) {
    let (done, dones) = std::sync::mpsc::channel::<Painted>();
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 3);
    let mut workers = Vec::with_capacity(n);
    for i in 0..n {
        let (ask, asks) = std::sync::mpsc::channel::<Paint>();
        let done = done.clone();
        let _ = std::thread::Builder::new()
            .name(format!("squadcode-bots-{i}"))
            .spawn(move || {
                let mut renderers: HashMap<u16, CrewRenderer> = HashMap::new();
                while let Ok(first) = asks.recv() {
                    let mut todo: Vec<Paint> = vec![first];
                    todo.extend(asks.try_iter());
                    // The newest ask per bot wins.
                    let mut newest: HashMap<Slot, Paint> = HashMap::new();
                    for p in todo {
                        newest.insert(p.slot, p);
                    }
                    for (_, p) in newest {
                        let r = renderers
                            .entry(p.device)
                            .or_insert_with(|| CrewRenderer::new(p.device));
                        let image = to_image(r.draw(&p.pose, p.at, p.framing), p.device);
                        if done
                            .send(Painted {
                                slot: p.slot,
                                tick: p.tick,
                                image,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        workers.push(ask);
    }
    (Painters(workers), dones)
}

/// Where bot frames are asked for: one queue per painter thread.
struct Painters(Vec<std::sync::mpsc::Sender<Paint>>);

impl Painters {
    fn send(&self, p: Paint) -> Result<(), std::sync::mpsc::SendError<Paint>> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        p.slot.hash(&mut h);
        self.0[(h.finish() % self.0.len() as u64) as usize].send(p)
    }
}

/// A clear 1 × 1 frame, for a bot whose first frame is still being drawn.
fn blank_image() -> Arc<RenderImage> {
    let buf = image::RgbaImage::from_raw(1, 1, vec![0; 4]).expect("blank");
    Arc::new(RenderImage::new(vec![image::Frame::new(buf)]))
}

/// A drawn bot (premultiplied RGBA) as an image GPUI can paint.
fn to_image(premul: &[u8], device: u16) -> Arc<RenderImage> {
    // Straight-alpha BGRA, as GPUI's image path expects.
    let mut px = Vec::with_capacity(premul.len());
    for p in premul.as_chunks::<4>().0 {
        let a = p[3];
        if a == 255 {
            px.extend_from_slice(&[p[2], p[1], p[0], 255]);
            continue;
        }
        if a == 0 {
            px.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let un = |v: u8| {
            if a == 0 {
                0
            } else {
                ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
            }
        };
        px.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), a]);
    }
    let buf = image::RgbaImage::from_raw(device as u32, device as u32, px).expect("frame size");
    Arc::new(RenderImage::new(vec![image::Frame::new(buf)]))
}

/// A message Peek's engine is reading (see `Shell::reading`).
type Reading = (
    u64,
    String,
    Vec<(String, String)>,
    std::sync::mpsc::Receiver<crate::agent::Event>,
);

/// Hears back once a pull request is merged or closed on GitHub.
/// The v2 window's root.
pub struct Shell {
    place: Place,
    /// The look last put on the window (theme, transparency on, how see-through),
    /// so a change in Settings is applied once.
    applied_theme: Look,
    clock: Instant,
    holos: Holos,
    scale: f32,
    mode: Mode,
    /// The crew: the saved store and everything running for it.
    crew: runtime::Runtime,
    composer: Entity<TextareaState>,
    chat_input: Entity<InputState>,
    /// Why you said no to a bot's request, when you say.
    deny_input: Entity<InputState>,
    /// The request whose Deny is open for a reason.
    denying: Option<String>,
    live_input: Entity<InputState>,
    /// The job box's Ask chip: the next follow-up only asks.
    live_ask: bool,
    /// Live's feed scrolls itself to the newest line while the job runs.
    live_scroll: ScrollHandle,
    live_len: usize,
    chat_scroll: ScrollHandle,
    chat_len: usize,
    thread_scroll: ScrollHandle,
    thread_key: u64,
    /// Frames left to keep the thread at its newest after it changed.
    thread_settle: u8,
    /// The thread whose earlier pages are open, and how many more pages.
    thread_more: (String, usize),
    /// Who spoke last in the chat being drawn, and when: a run of messages
    /// from one author shows their face and name once.
    streak: Option<(String, u64)>,
    /// The Fin in the "make your own" playground on Crew: its colour and outfit.
    maker: (squad_holo::Tint, squad_holo::fin::Outfit),
    /// Which jobs History shows: 0 all, 1 running, 2 done, 3 problems.
    history_filter: usize,
    folder_menu: bool,
    /// The sidebar shows only its icon rail.
    side_slim: bool,
    /// The thread on screen and when you last looked at it before opening it,
    /// for the feed's New line.
    new_mark: (String, u64, u64),
    /// A job's own conversation, open beside its thread like a reply pane.
    reply_open: Option<u64>,
    /// The sidebar's sections folded open or shut, by name; unset, open.
    side_folds: HashMap<String, bool>,
    toast: Option<(SharedString, Instant)>,
    /// Motion is on (Settings → Animations); off, the crew holds still.
    motion: bool,
    /// When the current place was opened, for its entrance.
    entered: Instant,
    /// Whether the window is in front; behind, the crew slows to save power.
    active: Arc<std::sync::atomic::AtomicBool>,
    /// Redraws a second the screen needs: 30 with a big bot or an entrance
    /// playing, fewer with only small bots, a trickle when nothing moves.
    rate: Arc<std::sync::atomic::AtomicU32>,
    /// Nobody has touched the app for a while and nothing is running: the crew
    /// holds still and the window stops redrawing until the pointer moves.
    dozing: Arc<std::sync::atomic::AtomicBool>,
    /// The last mouse move or key press. Left alone for a while, the crew keeps
    /// moving at half the frame rate.
    touched: Instant,
    /// The bot under the pointer: it gets the light sweep.
    hovered: Option<Member>,
    /// Each bot's face per place it's drawn (bot, size, framing): the state
    /// it shows, the one before, and when it changed, so moods ease.
    moods: HashMap<(Member, u32, bool), (squad_holo::fin::State, squad_holo::fin::State, f64)>,
    /// Where the pointer is in the window, for the sidebar bots' eyes.
    mouse: Point<Pixels>,
    /// Where each sidebar bot was painted last frame, so its eyes can find the pointer.
    faces: std::rc::Rc<std::cell::RefCell<HashMap<Member, Bounds<Pixels>>>>,
    /// The bot maker on Crew, which is also any bot's Setup: its name and job
    /// boxes, and the built-in bot its work is like.
    maker_name: Entity<InputState>,
    maker_role: Entity<InputState>,
    /// What it knows best (words jobs are routed by) and its rules, one a line.
    maker_skills: Entity<InputState>,
    maker_rules: Entity<TextareaState>,
    maker_like: Option<Member>,
    /// The bot being made is tied to one project, not every project.
    maker_here: bool,
    /// What the bot is for, in the person's words: the engine drafts from it.
    maker_about: Entity<TextareaState>,
    /// Its instructions, memory (one a line) and what it reads first.
    maker_instr: Entity<TextareaState>,
    maker_memory: Entity<TextareaState>,
    maker_reads: Entity<InputState>,
    /// A connected app to add: a URL or a command.
    maker_server: Entity<InputState>,
    /// A new skill to write, or a SKILL.md to bring in.
    maker_skill: Entity<InputState>,
    /// Where the maker is, and the bot being set up.
    maker_stage: builder::Stage,
    maker_edit: Option<builder::Editing>,
    /// The skills and connected apps on this computer, read when the maker opens.
    maker_found: crate::settings::Discovered,
    /// The walkthrough's step and the crew picked in it.
    setup_step: usize,
    /// The walkthrough's engine step lists every agent, not just the common ones.
    setup_more: bool,
    /// The bot maker is open on Crew.
    maker_open: bool,
    /// The engines popover over the status bar is open.
    usage_open: bool,
    /// The agent picked on Engines.
    engine_sel: Option<usize>,
    /// Engines shows each bot's brain rather than the engines themselves.
    /// Settings › Engines lists every engine it knows, not just the common ones.
    engines_all: bool,
    /// The Settings pane last open, where the sidebar's Settings goes back to.
    settings_pane: Pane,
    /// The bot being dressed in Settings → Mascot.
    fin_sel: Member,
    /// The state the Mascot preview holds; it plays through them all when unset.
    fin_state: Option<squad_holo::fin::State>,
    /// The job Home's inspector shows; the newest active one when unset.
    home_pick: Option<u64>,
    /// The Inbox item open on the right.
    inbox_pick: Option<u64>,
    /// Which jobs the list beside a job shows: all, needing you, running or done.
    /// The Jobs panel shows the thread you're in (false) or every job (true).
    list_all: bool,
    /// The Jobs panel shows its Git side: branches, the folder's state, watch-outs.
    list_git: bool,
    /// The Jobs panel beside a thread or job, opened from the top bar.
    jobs_open: bool,
    /// The right sidebar hidden on a job's page, from its toggle in the top bar.
    right_closed: bool,
    /// On a job's page the one right sidebar shows that job's details (true)
    /// or the Jobs panel (false). Opening a job from elsewhere starts on details.
    side_job: bool,
    /// A thread waiting for you to confirm deleting it.
    deleting: Option<u64>,
    /// A bot with a chat or know-how of its own, waiting for you to confirm
    /// taking it off the crew.
    removing: Option<Member>,
    /// The open right-click menu: where it was clicked and on what.
    menu: Option<(Point<Pixels>, menu::Ctx)>,
    /// The menu entry picked with the arrow keys.
    menu_hot: Option<usize>,
    /// The file open in the viewer, over the page.
    viewer: Option<viewer::Viewer>,
    /// A project waiting for you to confirm removing it, or deleting its folder.
    gone: Option<menu::Gone>,
    /// The thread's "About this thread" panel is open.
    about_open: bool,
    /// A risky ask Peek wants you to OK first: its message in the thread, the
    /// words, and where it would go.
    confirming: Option<(u64, String, brain::route::Route)>,
    /// A message Peek's engine is reading: Peek's message showing it, the
    /// words, the conversation before them, and the decision to come.
    reading: Option<Reading>,
    /// When Peek's engine started reading it.
    reading_since: Option<std::time::Instant>,
    /// The bot the message being read named with @: it answers or does the
    /// work, whichever Peek decides.
    reading_for: Option<Member>,
    /// While Peek's decision is acted on: the bot named with @, who answers.
    answer_by: Option<Member>,
    /// Apps and commands Squad runs for you, each in a hidden shell of its
    /// own and shown as a card: never in your terminal.
    runs: Vec<apps::Run>,
    next_run: u64,
    /// Typing into a run's output, for a prompt it shows.
    run_focus: FocusHandle,
    run_typing: Option<u64>,
    /// Where a command clicked in a message was said: Peek's message, the
    /// job. Its card shows there.
    act_from: (Option<u64>, Option<u64>),
    /// When Peek's engine was last made sure of being warm.
    warmed_at: Option<std::time::Instant>,
    /// "run the app", read by Peek: started on the next draw, which has the
    /// window the terminal needs. The words, and Peek's reading message.
    /// and the finished job it was said to, if any.
    run_wanted: Option<(String, Option<u64>, Option<u64>)>,
    /// A message to a finished job that Peek's engine is reading: the job,
    /// the words, and the decision to come.
    follow_reading: Option<(u64, String, std::sync::mpsc::Receiver<crate::agent::Event>)>,
    /// Peek's message saying its engine couldn't read a message, and the
    /// words, for "Try again".
    retry: Option<(u64, String)>,
    list_filter: u8,
    setup_crew: Vec<Member>,
    /// A job Fin held back because nobody on the crew does its kind of work,
    /// waiting for the person to add the bot Fin suggests.
    offer: Option<Offer>,
    /// Sample jobs are showing, picked from the Start here page; the real
    /// store comes back when they're put away.
    sampling: bool,
    /// A made bot chosen for Today's next job (its mode must match).
    target: Option<Member>,
    hover_at: Instant,
    /// The command palette (⌘K), its box, the picked row and when it opened.
    palette: bool,
    /// The window's own focus, so shortcuts work before any box is clicked.
    focus: FocusHandle,
    /// When focus was last moved to a box on purpose.
    focus_moved: Instant,
    palette_input: Entity<InputState>,
    /// History's search box.
    history_input: Entity<InputState>,
    /// The Overview's form for squad.toml.
    proj_form: overview::ProjectForm,
    /// The bot open on the Crew page, and its tab.
    crew_pick: Option<Member>,
    /// The machine open on the Machines page: 0 this computer, 1 your server.
    machine_pick: u8,
    crew_tab: crew::CrewTab,
    /// Memory's line for adding something a bot should always know.
    memory_input: Entity<InputState>,
    /// The filter over the right sidebar's Files tab.
    files_input: Entity<InputState>,
    /// The right sidebar shows its Files tab (beats `list_git`).
    list_files: bool,
    /// Folders open in the Files tab, by path from the folder's top.
    files_open: std::collections::HashSet<String>,
    /// The files last listed for the Files tab: the folder, when, and its files.
    files_seen: Option<(String, Instant, std::sync::Arc<Vec<String>>)>,
    /// The file shown in a job's Files tab, and whether its tree lists only
    /// the files the job changed.
    file_pick: Option<String>,
    files_changed_only: bool,
    /// The bot the DM box's placeholder names.
    chat_hint: Option<Member>,
    /// What the Memory tab's box says it teaches.
    memory_hint: Option<String>,
    /// How many days Settings > Usage looks back: 7, 14 or 30.
    usage_days: u64,
    /// The Settings drop-down that is open, by its id.
    select_open: Option<&'static str>,
    /// Job folders whose changed files' folders were opened once already.
    files_seeded: std::collections::HashSet<String>,
    /// The picked file as read: its path, lines, and the lines the job
    /// changed, so it isn't read again every frame.
    job_view: Option<filetab::FileView>,
    /// Settings, My server: the `you@host` box, the connecting in progress and
    /// what it has said so far.
    server_input: Entity<InputState>,
    server_setup: Option<std::sync::mpsc::Receiver<remote::Step>>,
    server_log: Vec<String>,
    server_failed: bool,
    /// The server showed a different host key from the one Squad trusted.
    server_key_changed: bool,
    /// The server wants a new password before anyone logs in, and the one
    /// the person picks for it.
    server_wants_new_pass: bool,
    server_new_pass: Entity<InputState>,
    /// The server's password (first connection only), Claude's sign-in code,
    /// an API key and which company it's for.
    server_pass: Entity<InputState>,
    server_code: Entity<InputState>,
    server_key: Entity<InputState>,
    server_key_pick: usize,
    /// Giving the server an agent: a key being saved, or a Claude sign-in
    /// and the link it opened.
    server_brain: Option<std::sync::mpsc::Receiver<remote::Brain>>,
    server_sign_in: Option<remote::SignIn>,
    server_brain_log: Vec<String>,
    server_brain_failed: bool,
    /// The Your server page: the server read every few seconds while it's
    /// open, what it looked like last, and why it couldn't be read.
    server_watch: Option<machine::Watch>,
    server_look: Option<machine::Look>,
    server_look_err: String,
    /// Things being done there, what they said (what, output, failed), the
    /// button waiting for a second click, and processes asked to stop.
    server_acts: Vec<(
        machine::Act,
        std::sync::mpsc::Receiver<Result<String, String>>,
    )>,
    server_out: Vec<(String, String, bool)>,
    server_arm: Option<(String, Instant)>,
    server_stopped: std::collections::HashSet<u32>,
    server_cmd: Entity<InputState>,
    /// The server's screen: being set up, and once it's shared, the tunnel
    /// that keeps it reachable.
    server_screen_rx: Option<std::sync::mpsc::Receiver<Result<machine::Screen, String>>>,
    server_screen: Option<machine::Screen>,
    /// What the server has for showing its screen, read once it answers:
    /// for which server, the reading or install under way, whether that's an
    /// install, why it failed, and whether the person asked for the screen
    /// (so it opens once the desktop is in).
    server_desktop: Option<machine::Desktop>,
    server_desktop_host: String,
    server_desktop_rx: Option<std::sync::mpsc::Receiver<Result<machine::Desktop, String>>>,
    server_desktop_installing: bool,
    server_desktop_err: String,
    server_desktop_want: bool,
    /// While a server job's words are laid out: its folder on the server, so
    /// what they offer (a local port, a command) is done there.
    rich_there: Option<String>,
    /// Tunnels to ports on the server that a job's links opened, by port there.
    forwards: HashMap<u16, remote::Forward>,
    /// Watching that screen in Squad: the connection, the picture and the
    /// frame it was made from, pictures to let go of, where it's drawn and
    /// the focus that sends keys to it.
    screen_view: Option<rfb::Viewer>,
    screen_img: Option<Arc<RenderImage>>,
    screen_seen: u64,
    screen_built: Instant,
    screen_retired: Vec<Arc<RenderImage>>,
    screen_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    screen_focus: FocusHandle,
    /// A job's browser, watched in Squad.
    web: webpage::Web,
    /// The job workspace's tab.
    ws_tab: peek::Tab,
    /// Design boards by job: one for each running job that designs, and the one
    /// on screen (see [`board`]).
    boards: HashMap<u64, Entity<crate::designdraw::view::DesignCanvas>>,
    /// The Inbox's project filter, by folder path: None shows every project.
    inbox_project: Option<String>,
    /// Jobs shows as a list instead of the board.
    jobs_list: bool,
    server_url: Option<String>,
    palette_sel: usize,
    palette_at: Instant,
    /// Where the person has been, newest last, for Back (⌘[).
    trail: Vec<Place>,
    /// The rail's highlight glides between items: from, to, and since when.
    nav_from: f32,
    nav_to: f32,
    nav_at: Instant,
    /// Progress bars' shown values, by job, so they glide.
    bars: HashMap<u64, f32>,
    /// The job box's kind of work was set on purpose (Give a job, a hand-off),
    /// so it isn't guessed from the text until that job is sent.
    mode_pinned: bool,
    /// The Inbox count last frame, and when it last went up, for the badge's pop.
    inbox_seen: usize,
    inbox_at: Instant,
    /// What needed the person at the last poll, so only something new chimes.
    /// None until the first poll, which takes what is already there as heard.
    heard: Option<std::collections::HashSet<String>>,
    /// When the newest chat line arrived, so it can slide in.
    chat_at: Instant,
    /// The changed file whose diff is open on a job's review, if any.
    diff_open: Option<(u64, String)>,
    diff_text: String,
    /// The diff line a comment is being written on: job, file, and the
    /// line's place in the shown diff.
    line_at: Option<(u64, String, usize)>,
    line_input: Entity<InputState>,
    /// The side panel's box for more words when sending a job back.
    peek_note: Entity<InputState>,
    /// Comments on diff lines, waiting to go back with Send back.
    line_notes: Vec<review::LineNote>,
    /// The point-and-change bar on a job's browser.
    pick_input: Entity<InputState>,
    /// How many engines a job from Today's box races on: 0 or 1 is no race.
    racing: usize,
    /// A job from Today's box is a goal to split into jobs.
    splitting: bool,
    /// The Run it later menu by the send button is open.
    later_menu: bool,
    /// A DM's box starts a job (Do) instead of asking (Ask).
    dm_do: bool,
    /// When the next job sent from the thread's box starts, as /later words
    /// ("tonight", "tomorrow 9am"); None sends it now.
    later_pick: Option<String>,
    /// The project's open issues on its Overview, as last read.
    issue_list: Option<IssueList>,
    /// A rewind waiting for its OK: the job and the point.
    rewind_ask: Option<(u64, usize)>,
    /// Terminal drawers' shells, by job, and the job whose drawer is open.
    terms: HashMap<u64, crate::pty::Shell>,
    term_open: Option<u64>,
    term_focus: FocusHandle,
    term_focused: bool,
    /// Frames left to keep handing focus to a drawer that just opened.
    term_grab: u8,
    term_cols: u16,
    /// The window's width, for fitting the terminal to it.
    view_w: f32,
    /// How see-through the window is, under Settings > Appearance.
    clear_slider: Entity<SliderState>,
    /// How see-through popovers, menus and dialogs go.
    pop_slider: Entity<SliderState>,
    /// The background image's blur, dim, and how far it shows through.
    bg_blur_slider: Entity<SliderState>,
    bg_dim_slider: Entity<SliderState>,
    bg_show_slider: Entity<SliderState>,
    _subs: Vec<Subscription>,
    _pump: Task<()>,
}

/// The folders behind dropped paths: a folder itself, or the folder a file is in,
/// each once.
/// A job held back from Today's box until the bot for it is on the crew.
struct Offer {
    /// The bot Fin suggests adding.
    who: Member,
    mode: Mode,
    text: String,
    /// It was going to be split into a goal.
    goal: bool,
    /// How many engines it was going to race on.
    race: usize,
    /// Peek's line on why this bot is taking it, for the job's card.
    handoff: String,
    /// Where Peek said it runs ("server", "here"), or empty for the usual.
    site: String,
}

/// "A", "A and B", "A, B and C".
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn folders_from(paths: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for p in paths {
        let dir = if p.is_dir() {
            p.clone()
        } else {
            match p.parent() {
                Some(d) if d.is_dir() => d.to_path_buf(),
                _ => continue,
            }
        };
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// How long the app waits, untouched, before the crew dozes (five minutes;
/// `SQUADCODE_DOZE_SECS` shortens it for testing).
fn doze_after() -> Duration {
    let secs = std::env::var("SQUADCODE_DOZE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

/// Extra room at the bottom of a page (the crew is in the sidebar now).
pub(super) const DOCK_ROOM: f32 = 16.;
/// How tall the title bar across the whole window is.
pub(super) const TOP_H: f32 = 38.;
/// Where the Mac window buttons sit: their left edge, and the top of their
/// frames (16pt tall), so they centre on the title bar's row, at its left end.
pub(super) const LIGHTS_X: f32 = 13.;
pub(super) const LIGHTS_Y: f32 = (TOP_H - 16.) / 2.;

/// Whether the window has Mac window buttons in its top-left corner. Off a
/// Mac, `SQUADCODE_FAKE_LIGHTS` draws stand-ins, for screenshots taken on Linux.
pub(super) fn has_lights() -> bool {
    cfg!(target_os = "macos") || std::env::var_os("SQUADCODE_FAKE_LIGHTS").is_some()
}

/// The rail's place for each part of the app, as a y offset of its highlight.
fn nav_slot(place: Place) -> Option<f32> {
    let i = match place {
        Place::Today => 0,
        Place::Inbox => 1,
        Place::Overview
        | Place::History
        | Place::Live(_)
        | Place::Race(_)
        | Place::Goal(_)
        | Place::Browser(_) => 2,
        Place::Chat(_) => 3,
        Place::Crew => 4,
        Place::Settings(_) | Place::Server | Place::Screen | Place::Welcome => return None,
    };
    Some(i as f32 * 46.)
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A long job wraps and the box grows with it; Shift+Enter starts a new line.
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(Mode::ANY_PLACEHOLDER)
                .submit_on_enter(true)
                .auto_grow(1, 6)
        });
        let chat_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Ask anything about the code…"));
        let live_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add to the job…"));
        let line_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("What should change here? Enter adds it")
        });
        let peek_note = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Or write here to add more words\u{2026}")
        });
        let pick_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("What should change about it? Enter sends it")
        });
        let deny_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Tell it why (optional)"));
        let palette_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Jump to a place, a bot, a job or a folder…")
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let maker_name = cx.new(|cx| InputState::new(window, cx).placeholder("Scout"));
        let maker_role =
            cx.new(|cx| InputState::new(window, cx).placeholder("Checks the docs match the code"));
        let maker_skills =
            cx.new(|cx| InputState::new(window, cx).placeholder("stripe, webhooks, billing"));
        let area = |hint: &'static str,
                    rows: (usize, usize),
                    window: &mut Window,
                    cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(hint)
                    .auto_grow(rows.0, rows.1)
            })
        };
        let maker_rules = area(
            "One rule a line: Run pnpm test before you stop",
            (3, 10),
            window,
            cx,
        );
        let maker_about = area(
            "Own our Stripe integration: fix webhook and billing bugs, keep the payment tests green, never touch live keys.",
            (3, 8),
            window,
            cx,
        );
        let maker_instr = area(
            "# Who it is and how it works, in Markdown",
            (6, 18),
            window,
            cx,
        );
        let maker_memory = area(
            "One fact a line: Payment tests run with pnpm vitest src/billing",
            (3, 10),
            window,
            cx,
        );
        let maker_reads = cx
            .new(|cx| InputState::new(window, cx).placeholder("docs/, CONTRIBUTING.md, https://…"));
        let maker_server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://mcp.linear.app/sse  or  npx -y @stripe/mcp")
        });
        let maker_skill = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Describe a skill to write, or paste a path to a SKILL.md")
        });
        let server_input = cx.new(|cx| InputState::new(window, cx).placeholder("root@203.0.113.7"));
        let server_pass = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password (first time only)")
                .masked(true)
        });
        let server_new_pass = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("New password for the server")
                .masked(true)
        });
        let server_cmd = cx.new(|cx| {
            InputState::new(window, cx).placeholder("df -h, docker ps, systemctl status nginx…")
        });
        let server_code =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste the code from Claude"));
        let server_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste the API key")
                .masked(true)
        });
        let history_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter jobs by title, bot or folder…")
        });
        let proj_form = overview::ProjectForm::new(window, cx);
        let memory_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Add something this bot should always know here\u{2026}")
        });
        let files_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find a file…"));
        let clear = cx
            .global::<crate::settings::AppSettings>()
            .v2_clear
            .min(100);
        let clear_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(clear as f32)
        });
        let pop_clear = cx
            .global::<crate::settings::AppSettings>()
            .v2_pop_clear
            .min(100);
        let pop_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(pop_clear as f32)
        });
        let bg_slider = |value: u8, cx: &mut Context<Self>| {
            cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(100.)
                    .step(1.)
                    .default_value(value.min(100) as f32)
            })
        };
        let (blur, dim, show) = {
            let st = cx.global::<crate::settings::AppSettings>();
            (st.v2_bg_blur, st.v2_bg_dim, st.v2_bg_show)
        };
        let bg_blur_slider = bg_slider(blur, cx);
        let bg_dim_slider = bg_slider(dim, cx);
        let bg_show_slider = bg_slider(show, cx);
        wallpaper::refresh(cx);
        let slid = |event: &SliderEvent| match event {
            SliderEvent::Change(v) => (v.end().round().clamp(0., 100.) as u8, false),
            SliderEvent::Release(v) => (v.end().round().clamp(0., 100.) as u8, true),
        };
        let (web, web_sub) = webpage::Web::new(window, cx);
        let mut subs = vec![
            // A theme that follows the system redraws when the system switches.
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            // The background image's dim and show-through preview as you drag; its
            // blur needs a new copy made, so that waits until you let go.
            cx.subscribe_in(&bg_dim_slider, window, move |_, _, event, _, cx| {
                let (dim, keep) = slid(event);
                wallpaper::set_look(wallpaper::fit(), dim);
                if keep {
                    cx.update_global::<crate::settings::AppSettings, _>(|st, _| {
                        st.v2_bg_dim = dim;
                        st.save();
                    });
                }
                cx.notify();
            }),
            cx.subscribe_in(&bg_show_slider, window, move |_, _, event, _, cx| {
                let (show, keep) = slid(event);
                theme::set_image(theme::image_on(), show);
                if keep {
                    cx.update_global::<crate::settings::AppSettings, _>(|st, _| {
                        st.v2_bg_show = show;
                        st.save();
                    });
                }
                cx.notify();
            }),
            cx.subscribe_in(&bg_blur_slider, window, move |_, _, event, _, cx| {
                let (blur, keep) = slid(event);
                if keep {
                    cx.update_global::<crate::settings::AppSettings, _>(|st, _| {
                        st.v2_bg_blur = blur;
                        st.save();
                    });
                    wallpaper::refresh(cx);
                }
                cx.notify();
            }),
            // Popovers too: dragging previews, letting go keeps it.
            cx.subscribe_in(&pop_slider, window, |_, _, event, _, cx| {
                let (value, keep) = match event {
                    SliderEvent::Change(v) => (v.end(), false),
                    SliderEvent::Release(v) => (v.end(), true),
                };
                let clear = value.round().clamp(0., 100.) as u8;
                theme::set_glass_popovers(theme::glass_popovers(), clear);
                if keep {
                    cx.update_global::<crate::settings::AppSettings, _>(|st, _| {
                        st.v2_pop_clear = clear;
                        st.save();
                    });
                }
                cx.notify();
            }),
            // Dragging previews the window as it goes; letting go keeps it.
            cx.subscribe_in(&clear_slider, window, |_, _, event, _, cx| {
                let (value, keep) = match event {
                    SliderEvent::Change(v) => (v.end(), false),
                    SliderEvent::Release(v) => (v.end(), true),
                };
                let clear = value.round().clamp(0., 100.) as u8;
                theme::set_see_through(theme::see_through(), clear);
                if keep {
                    cx.update_global::<crate::settings::AppSettings, _>(|st, _| {
                        st.v2_clear = clear;
                        st.save();
                    });
                }
                cx.notify();
            }),
            cx.subscribe_in(&history_input, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&files_input, window, |_, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&palette_input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.palette_sel = 0;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&composer, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    // Enter on "@bu" picks Builder rather than sending.
                    let raw = input.read(cx).value().trim_end_matches('\n').to_string();
                    if let Some((head, found)) = this.at_matches(&raw) {
                        let text = format!("{head}@{} ", found[0].name());
                        input.update(cx, |i, cx| ui::put_text(i, text, window, cx));
                        cx.notify();
                        return;
                    }
                    let text = input.read(cx).value().trim().to_string();
                    input.update(cx, |i, cx| i.set_value("", window, cx));
                    this.hand_over(text, cx);
                    this.unpin_mode(window, cx);
                }
            }),
            cx.subscribe_in(&chat_input, window, |this, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().trim().to_string();
                    if let (false, Place::Chat(who)) = (text.is_empty(), this.place) {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                        if this.dm_do {
                            this.job_from_dm(who, text, cx);
                        } else {
                            this.ask(who, text, cx);
                        }
                    }
                }
            }),
            cx.subscribe_in(&server_cmd, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    if this.place == Place::Server {
                        this.run_server_command(window, cx);
                    }
                }
            }),
            cx.subscribe_in(&line_input, window, |this, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().trim().to_string();
                    if !text.is_empty() {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                        this.add_line_note(text, cx);
                    }
                }
            }),
            cx.subscribe_in(&pick_input, window, |this, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().trim().to_string();
                    if !text.is_empty() {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                        this.send_pick(text, cx);
                    }
                }
            }),
            cx.subscribe_in(&live_input, window, |this, _input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.send_live(window, cx);
                }
            }),
        ];
        // The crew's clock: as many redraws as the screen needs while the window is
        // in front (see `rate`), a few a second behind other windows.
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let rate = Arc::new(std::sync::atomic::AtomicU32::new(FPS as u32));
        let (seen, wanted) = (active.clone(), rate.clone());
        let dozing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let asleep = dozing.clone();
        let mut slept = 0u32;
        // Saves still waiting for a tick are written before Squad Code quits.
        cx.on_app_quit(|this, _| {
            this.crew.store.flush(true);
            async {}
        })
        .detach();
        let pump = cx.spawn(async move |this, cx| loop {
            // Dozing (nobody around, nothing running): no redraws at all, apart from
            // one every half minute so times like "4m ago" stay true.
            if asleep.load(std::sync::atomic::Ordering::Relaxed) {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                slept += 1;
                if slept < 60 {
                    // No redraw, but a save asked for still gets written.
                    if this
                        .update(cx, |this, _| this.crew.store.flush(false))
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
                slept = 0;
                if this
                    .update(cx, |this, cx| {
                        this.poll(cx);
                        cx.notify()
                    })
                    .is_err()
                {
                    break;
                }
                continue;
            }
            slept = 0;
            let front = seen.load(std::sync::atomic::Ordering::Relaxed);
            let want = wanted
                .load(std::sync::atomic::Ordering::Relaxed)
                .clamp(2, FPS as u32);
            let wait = if front {
                1000.0 / want as f64
            } else {
                1000.0 / want.min(4) as f64
            };
            cx.background_executor()
                .timer(Duration::from_millis(wait as u64))
                .await;
            // Move the crew along here too, not only when a frame is drawn:
            // macOS draws nothing for a window that's hidden or covered, and
            // jobs, the server page and notices should keep up all the same.
            if this
                .update(cx, |this, cx| {
                    this.poll(cx);
                    cx.notify()
                })
                .is_err()
            {
                break;
            }
        });
        // Learn which agents are installed and the models they offer.
        crate::caps::refresh();
        let store = if std::env::var_os("SQUADCODE_DEMO").is_some() {
            let mut s = demo::seed();
            demo::usage(&mut s);
            s
        } else {
            // The window writes saves on its ticks, not as each change asks.
            let mut s = Store::load();
            s.deferred = true;
            s
        };
        let start = match std::env::var("SQUADCODE_PLACE").as_deref() {
            Ok("crew") => Place::Crew,
            Ok(p) if p.starts_with("crew:") => Place::Crew,
            Ok("chat") => Place::Chat(Member::Fin),
            // "chat:explainer" opens a direct message with that bot.
            Ok(p) if p.starts_with("chat:") => Member::all()
                .into_iter()
                .find(|m| m.name().eq_ignore_ascii_case(&p[5..]))
                .map(Place::Chat)
                .unwrap_or(Place::Today),
            Ok("inbox") => Place::Inbox,
            Ok("history") => Place::History,
            Ok("overview") => Place::Overview,
            Ok("engines") => Place::Settings(Pane::Engines),
            Ok("server") => Place::Settings(Pane::Server),
            Ok("work") => Place::Settings(Pane::Work),
            Ok("settings") => Place::Settings(Pane::Work),
            Ok("general") => Place::Settings(Pane::General),
            Ok("mascot") => Place::Settings(Pane::Mascot),
            Ok("appearance") => Place::Settings(Pane::Appearance),
            Ok("usage") => Place::Settings(Pane::Usage),
            Ok("machine") => Place::Server,
            Ok("needs") => Place::Inbox,
            Ok("goal") => store
                .goals
                .first()
                .map(|g| Place::Goal(g.id))
                .unwrap_or(Place::Today),
            Ok("jobs" | "files") => Place::Today,
            Ok(p) if p.starts_with("thread:") => Place::Today,
            Ok("race") => store
                .jobs
                .iter()
                .find(|j| j.race != 0)
                .map(|j| Place::Race(j.race))
                .unwrap_or(Place::Today),
            // "job:9" opens job #9's workspace.
            Ok(p) if p.starts_with("job:") => p[4..]
                .split(':')
                .next()
                .unwrap_or_default()
                .parse()
                .ok()
                .filter(|id| store.job(*id).is_some())
                .map(Place::Live)
                .unwrap_or(Place::Today),
            Ok("review" | "review-jobs" | "review-files") => store
                .jobs
                .iter()
                .find(|j| j.status == Status::Review && j.race == 0)
                .map(|j| Place::Live(j.id))
                .unwrap_or(Place::Today),
            Ok("pr") => store
                .jobs
                .iter()
                .find(|j| j.status == Status::Shipping)
                .map(|j| Place::Live(j.id))
                .unwrap_or(Place::Today),
            Ok("live") => store
                .jobs
                .last()
                .map(|j| Place::Live(j.id))
                .unwrap_or(Place::Today),
            Ok(p) if p == "welcome" || p.starts_with("welcome:") => Place::Welcome,
            Ok("maker" | "maker-review" | "setup") => Place::Crew,
            _ if !store.set_up => Place::Welcome,
            _ => Place::Today,
        };
        let mut shell = Shell {
            place: start,
            clock: Instant::now(),
            holos: Holos::new(),
            scale: 1.0,
            mode: Mode::Build,
            crew: {
                let demo = store.demo;
                let mut rt = runtime::Runtime::new(store);
                rt.link = remote::load().map(remote::Link::start);
                if demo {
                    rt.repos = demo::repos();
                }
                rt
            },
            composer,
            chat_input,
            deny_input,
            denying: None,
            live_input,
            live_ask: false,
            line_input: line_input.clone(),
            peek_note,
            line_notes: Vec::new(),
            line_at: None,
            pick_input: pick_input.clone(),
            live_scroll: ScrollHandle::new(),
            live_len: 0,
            chat_scroll: ScrollHandle::new(),
            chat_len: 0,
            thread_scroll: ScrollHandle::new(),
            thread_key: 0,
            thread_settle: 0,
            thread_more: (String::new(), 0),
            streak: None,
            maker: (squad_holo::Tint::VIOLET, squad_holo::fin::ROLES[6].outfit),
            history_filter: 0,
            folder_menu: false,
            side_slim: false,
            new_mark: (String::new(), 0, 0),
            reply_open: None,
            side_folds: HashMap::new(),
            // A saved store that couldn't be read in full: say so once.
            toast: store::take_load_note().map(|n| (n.into(), Instant::now())),
            entered: Instant::now(),
            motion: true,
            active,
            rate,
            dozing,
            touched: Instant::now(),
            hovered: None,
            moods: HashMap::new(),
            target: None,
            setup_step: 0,
            setup_more: false,
            maker_open: false,
            usage_open: false,
            engine_sel: None,
            engines_all: false,
            settings_pane: Pane::Work,
            applied_theme: look(),
            fin_sel: Member::Guard,
            fin_state: None,
            home_pick: None,
            inbox_pick: None,
            list_filter: 0,
            list_all: false,
            list_git: false,
            jobs_open: false,
            right_closed: false,
            side_job: true,
            deleting: None,
            removing: None,
            menu: None,
            menu_hot: None,
            viewer: None,
            gone: None,
            about_open: false,
            confirming: None,
            reading: None,
            reading_since: None,
            reading_for: None,
            answer_by: None,
            runs: Vec::new(),
            next_run: 0,
            run_focus: cx.focus_handle(),
            run_typing: None,
            act_from: (None, None),
            warmed_at: None,
            run_wanted: None,
            follow_reading: None,
            retry: None,
            setup_crew: data::STARTER_CREW.to_vec(),
            offer: None,
            sampling: false,
            maker_name,
            maker_role,
            maker_skills,
            maker_rules,
            maker_like: None,
            maker_here: false,
            maker_about,
            maker_instr,
            maker_memory,
            maker_reads,
            maker_server,
            maker_skill,
            maker_stage: builder::Stage::Describe,
            maker_edit: None,
            maker_found: Default::default(),
            hover_at: Instant::now(),
            mouse: Point::default(),
            faces: Default::default(),
            palette: false,
            focus,
            focus_moved: Instant::now(),
            palette_input,
            history_input,
            proj_form,
            crew_pick: None,
            machine_pick: 0,
            crew_tab: crew::CrewTab::default(),
            memory_input,
            files_input,
            list_files: false,
            files_open: Default::default(),
            files_seen: None,
            file_pick: None,
            files_changed_only: false,
            usage_days: 7,
            chat_hint: None,
            memory_hint: None,
            select_open: None,
            files_seeded: Default::default(),
            job_view: None,
            server_input,
            server_setup: None,
            server_log: Vec::new(),
            server_failed: false,
            server_key_changed: false,
            server_wants_new_pass: false,
            server_new_pass,
            server_pass,
            server_code,
            server_key,
            server_key_pick: 0,
            server_brain: None,
            server_sign_in: None,
            server_brain_log: Vec::new(),
            server_brain_failed: false,
            server_watch: None,
            server_look: None,
            server_look_err: String::new(),
            server_acts: Vec::new(),
            server_out: Vec::new(),
            server_arm: None,
            server_stopped: Default::default(),
            server_cmd,
            server_screen_rx: None,
            server_screen: None,
            server_desktop: None,
            server_desktop_host: String::new(),
            server_desktop_rx: None,
            server_desktop_installing: false,
            server_desktop_err: String::new(),
            server_desktop_want: false,
            rich_there: None,
            forwards: HashMap::new(),
            screen_view: None,
            screen_img: None,
            screen_seen: u64::MAX,
            screen_built: Instant::now(),
            screen_retired: Vec::new(),
            screen_bounds: Default::default(),
            screen_focus: cx.focus_handle(),
            web,
            ws_tab: peek::Tab::default(),
            boards: HashMap::new(),
            inbox_project: None,
            jobs_list: false,
            server_url: None,
            palette_sel: 0,
            palette_at: Instant::now(),
            trail: Vec::new(),
            nav_from: nav_slot(start).unwrap_or(0.),
            nav_to: nav_slot(start).unwrap_or(0.),
            nav_at: Instant::now(),
            bars: HashMap::new(),
            mode_pinned: false,
            inbox_seen: usize::MAX,
            inbox_at: Instant::now(),
            heard: None,
            chat_at: Instant::now(),
            diff_open: None,
            diff_text: String::new(),
            racing: 0,
            splitting: false,
            later_menu: false,
            dm_do: false,
            later_pick: None,
            issue_list: None,
            rewind_ask: None,
            terms: HashMap::new(),
            term_open: None,
            term_focus: cx.focus_handle(),
            term_focused: false,
            term_grab: 0,
            term_cols: 100,
            view_w: 1280.,
            clear_slider,
            pop_slider,
            bg_blur_slider,
            bg_dim_slider,
            bg_show_slider,
            _subs: {
                subs.push(web_sub);
                subs
            },
            _pump: pump,
        };
        // The maker, for looking at it: empty, with a draft, or a bot's setup.
        match std::env::var("SQUADCODE_PLACE").as_deref() {
            Ok("maker") => shell.open_maker(None, None, window, cx),
            Ok("maker-review") => {
                shell.open_maker(Some(builder::SAMPLE), None, window, cx);
                shell.start_blank(window, cx);
            }
            // The Jobs panel open on its Git side.
            Ok("git") => {
                shell.jobs_open = true;
                shell.list_git = true;
            }
            // The status bar's usage popover, open.
            Ok("usage-open") => shell.usage_open = true,
            Ok("palette") => shell.open_palette(window, cx),
            // "job:9:files" opens job #9's workspace on its Files tab.
            Ok(p) if p.starts_with("job:") && p.matches(':').count() == 2 => {
                let tab = p.rsplit(':').next().unwrap_or_default();
                if let Some(t) = peek::Tab::ALL
                    .into_iter()
                    .find(|t| t.name().eq_ignore_ascii_case(tab))
                {
                    shell.ws_tab = t;
                    if t == peek::Tab::Terminal {
                        if let Ok(id) = p[4..].split(':').next().unwrap_or_default().parse() {
                            shell.toggle_term(id, window, cx);
                        }
                    }
                }
            }
            // "thread:1" opens thread #1.
            Ok(p) if p.starts_with("thread:") => {
                if let Ok(id) = p[7..].parse() {
                    shell.crew.store.thread_now = id;
                }
            }
            // "crew:fixer:memory" opens Fixer's profile on its Memory tab.
            Ok(p) if p.starts_with("crew:") => {
                let mut parts = p[5..].split(':');
                let who = parts.next().unwrap_or_default();
                shell.crew_pick = Member::all()
                    .into_iter()
                    .find(|m| m.name().eq_ignore_ascii_case(who));
                if let Some(tab) = parts.next() {
                    if let Some(t) = crew::CrewTab::ALL
                        .into_iter()
                        .find(|t| format!("{t:?}").eq_ignore_ascii_case(tab))
                    {
                        shell.crew_tab = t;
                    }
                }
            }
            // The menu bar panel, open on its own at the screen's corner.
            Ok("tray") => cx.defer(crate::menubar::open_panel),
            // "welcome:3" opens the walkthrough on its fourth step.
            Ok(p) if p.starts_with("welcome:") => {
                shell.setup_step = p[8..].parse().unwrap_or(0);
            }
            // A job's peek open beside its thread.
            Ok("peek") => {
                let pick = shell
                    .crew
                    .store
                    .jobs
                    .iter()
                    .find(|j| j.status == Status::Review && j.race == 0)
                    .map(|j| (j.id, j.thread));
                if let Some((id, thread)) = pick {
                    shell.crew.store.thread_now = thread;
                    shell.reply_open = Some(id);
                }
            }
            // "peek:2" opens job #2's peek beside its thread.
            Ok(p) if p.starts_with("peek:") => {
                let pick = p[5..]
                    .parse::<u64>()
                    .ok()
                    .and_then(|id| shell.crew.store.job(id))
                    .map(|j| (j.id, j.thread));
                if let Some((id, thread)) = pick {
                    shell.crew.store.thread_now = thread;
                    shell.reply_open = Some(id);
                }
            }
            // The right sidebar open on its Files tab.
            Ok("files") => {
                shell.jobs_open = true;
                shell.list_files = true;
                shell.files_open = ["src", "src/checkout"].map(String::from).into();
            }
            // A job open with the sidebar on its Files tab.
            Ok("review-files") => {
                shell.jobs_open = true;
                shell.side_job = false;
                shell.list_files = true;
                shell.files_open = ["src", "src/checkout"].map(String::from).into();
            }
            // A job open with the sidebar on its Jobs list.
            Ok("review-jobs") => {
                shell.jobs_open = true;
                shell.side_job = false;
            }
            // A job's browser, on SQUADCODE_URL when it's set.
            Ok("browser") => {
                if let Some(id) = shell.crew.store.jobs.last().map(|j| j.id) {
                    let url = std::env::var("SQUADCODE_URL").ok();
                    shell.open_job_browser(id, url, cx);
                }
            }
            Ok("setup") => {
                let who = Member::all()
                    .into_iter()
                    .find(|m| m.is_made() && !setup::of(*m).is_empty())
                    .unwrap_or(Member::Builder);
                shell.open_setup(who, window, cx);
            }
            _ => {}
        }
        // A file open in the viewer, for looking at it.
        if let Some(file) = std::env::var_os("SQUADCODE_VIEW") {
            let job = match shell.place {
                Place::Live(id) => Some(id),
                _ => None,
            };
            shell.open_file(file.into(), job, window, cx);
        }
        shell
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    fn go(&mut self, place: Place, cx: &mut Context<Self>) {
        self.viewer = None;
        if self.place != place {
            self.trail.push(self.place);
            if self.trail.len() > 40 {
                self.trail.remove(0);
            }
            self.arrive(place);
        }
        self.visit(place, cx);
    }

    /// Back to the place before this one.
    fn back(&mut self, cx: &mut Context<Self>) {
        // An open file closes first, back to the page under it.
        if self.viewer.take().is_some() {
            cx.notify();
            return;
        }
        while let Some(p) = self.trail.pop() {
            // Skip jobs that are gone and repeats of where we are.
            let ok = match p {
                Place::Live(id) | Place::Race(id) => self.crew.store.job(id).is_some(),
                Place::Goal(id) => self.crew.store.goals.iter().any(|g| g.id == id),
                _ => true,
            };
            if ok && p != self.place {
                self.arrive(p);
                self.visit(p, cx);
                return;
            }
        }
    }

    /// Switches place and starts its entrance and the rail's glide.
    fn arrive(&mut self, place: Place) {
        {
            if let Some(to) = nav_slot(place) {
                let now = self.nav_y();
                self.nav_from = if nav_slot(self.place).is_some() {
                    now
                } else {
                    to
                };
                self.nav_to = to;
                self.nav_at = Instant::now();
            }
            // Coming to a job from elsewhere: the sidebar starts on its details.
            // Moving between jobs keeps whichever side you were looking at.
            if matches!(place, Place::Live(_)) && !matches!(self.place, Place::Live(_)) {
                self.side_job = true;
            }
            self.place = place;
            if let Place::Settings(pane) = place {
                self.settings_pane = pane;
            }
            self.entered = Instant::now();
            self.folder_menu = false;
        }
    }

    /// How far item `i` of the current place has eased in, 0 → 1; each item
    /// starts a beat after the one before.
    fn stagger(&self, i: usize) -> f32 {
        if !self.motion {
            return 1.0;
        }
        let t = self.entered.elapsed().as_secs_f64() - 0.045 * i.min(12) as f64;
        squad_holo::anim::spring(t.max(0.0), 0.36) as f32
    }

    /// A progress bar's shown value, gliding toward `to` instead of jumping.
    fn glide_bar(&mut self, id: u64, to: f32) -> f32 {
        let v = self.bars.entry(id).or_insert(to);
        *v += (to - *v) * 0.18;
        if (to - *v).abs() < 0.002 {
            *v = to;
        }
        *v
    }

    /// Where the rail's highlight is now, between its last two stops.
    fn nav_y(&self) -> f32 {
        let k = squad_holo::anim::spring(self.nav_at.elapsed().as_secs_f64(), 0.34) as f32;
        self.nav_from + (self.nav_to - self.nav_from) * k
    }

    /// Marks a finished job seen once it's opened.
    fn visit(&mut self, place: Place, cx: &mut Context<Self>) {
        if let Place::Live(id) = place {
            if let Some(j) = self.crew.store.job_mut(id) {
                if j.status != Status::Running && !j.seen {
                    j.seen = true;
                    self.crew.store.save();
                }
            }
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.mode_pinned = true;
        self.composer.update(cx, |i, cx| {
            i.set_placeholder(mode.placeholder(), window, cx)
        });
        cx.notify();
    }

    /// Once a job is sent (and not held for Fin's offer), the next one's kind
    /// of work is read from its text again.
    fn unpin_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.offer.is_some() || !self.mode_pinned {
            return;
        }
        self.mode_pinned = false;
        self.target = None;
        self.composer.update(cx, |i, cx| {
            i.set_placeholder(Mode::ANY_PLACEHOLDER, window, cx)
        });
    }

    fn say(&mut self, text: impl Into<SharedString>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Asks for a folder and makes it the one the crew works in.
    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        self.folder_menu = false;
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a project folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.first().cloned() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.crew.store.add_folder(path);
                this.crew.store.save();
                let name = this
                    .crew
                    .store
                    .folder()
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                this.say(format!("The crew now works in {name}."));
                cx.notify();
            });
        })
        .detach();
    }

    /// Deletes a thread for good, once confirmed: what was said goes, and its
    /// jobs move to #general.
    pub(super) fn delete_thread(&mut self, id: u64, cx: &mut Context<Self>) {
        self.deleting = None;
        let name = self.crew.store.thread_label(id);
        if id != 0 && self.crew.store.delete_thread(id) {
            self.crew.store.save();
            self.say(format!(
                "Deleted #{name}. Its jobs are in #general, so no work is lost."
            ));
            self.go(Place::Today, cx);
        }
        cx.notify();
    }

    /// Puts a thread away: its jobs show in #general from then on, and making a
    /// thread with its name brings it back.
    pub(super) fn archive_thread(&mut self, id: u64, cx: &mut Context<Self>) {
        if id == 0 {
            self.say("#general always stays. Archive any other thread.");
            return;
        }
        let name = self.crew.store.thread_label(id);
        if let Some(t) = self.crew.store.threads.iter_mut().find(|t| t.id == id) {
            t.archived = true;
        }
        if self.crew.store.thread_now == id {
            self.crew.store.thread_now = 0;
        }
        self.crew.store.save();
        self.say(format!(
            "Archived #{name}. Its jobs are in #general now; /thread {name} brings it back."
        ));
        self.go(Place::Today, cx);
    }

    /// The text from Today's box becomes a job for the bot its mode goes to, or a
    /// question for Explainer.
    pub(super) fn hand_over(&mut self, text: String, cx: &mut Context<Self>) {
        // The person spoke here: the bots may talk among themselves again.
        if let Some(f) = self.crew.store.folder().map(|f| f.path.clone()) {
            let here = self.crew.store.thread_here();
            self.crew.person_spoke(&f, here);
        }
        // "@Builder do this": that bot joins the thread and takes the ask,
        // whatever its words would otherwise suggest.
        let mention = (!text.starts_with('/')).then(|| mentioned(&text)).flatten();
        let Some((who, rest)) = mention else {
            return self.read_first(text, cx);
        };
        if self.place == Place::Today {
            let here = self.crew.store.thread_here();
            if let Some(t) = self.crew.store.threads.iter_mut().find(|t| t.id == here) {
                if who != Member::Fin && !t.bots.contains(&who) {
                    t.bots.push(who);
                }
            }
        }
        // In a thread Peek still reads it, so "@Pixel who are you" gets an
        // answer from Pixel, and only real work becomes Pixel's job.
        if self.place == Place::Today
            && !rest.starts_with('/')
            && !rest.trim().is_empty()
            && !self.sampling
            && self.crew.store.folder().is_some()
            && self.reading.is_none()
        {
            self.crew.store.save();
            return self.peek_reads(rest, Some(who), cx);
        }
        let (target, mode, pinned) = (self.target, self.mode, self.mode_pinned);
        self.target = Some(who);
        self.mode = who.mode();
        self.mode_pinned = true;
        self.hand_over_inner(rest, cx);
        self.target = target;
        self.mode = mode;
        self.mode_pinned = pinned;
        self.crew.store.save();
    }

    /// Peek reads a message in a thread before anyone works on it: small talk
    /// it answers itself, a question goes to an engine to answer here, a vague
    /// ask gets a question back, and real work goes to the bot it's for. A
    /// slash command, a picked bot or a pinned kind of work skips the read.
    fn read_first(&mut self, text: String, cx: &mut Context<Self>) {
        let skip = text.starts_with('/')
            || self.place != Place::Today
            || self.target.is_some()
            || self.mode_pinned
            || self.crew.store.folder().is_none();
        if skip {
            return self.hand_over_inner(text, cx);
        }
        self.peek_reads(text, None, cx)
    }

    /// Peek's engine reads `text`; `pinned` is the bot it was addressed to
    /// with @, if any.
    fn peek_reads(&mut self, text: String, pinned: Option<Member>, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        let folder = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        // Just "#name": go to that thread, as in Slack.
        let refs = self.crew.store.thread_refs(&folder, &text);
        if text.split_whitespace().count() == 1 && refs.len() == 1 {
            self.crew.store.thread_now = refs[0];
            self.crew.store.save();
            self.go(Place::Today, cx);
            return;
        }
        // Threads named with # are read too, before this one.
        let here = self.crew.store.thread_here();
        let mut recent = Vec::new();
        for other in refs.iter().filter(|t| **t != here) {
            recent.extend(self.crew.store.recent(&folder, *other));
        }
        recent.extend(self.crew.store.recent(&folder, here));
        // Peek reads it with the person's own engine. Nothing guesses when
        // the engine can't: Peek says so, and offers to try again.
        if self.sampling {
            return self.hand_over_inner(text, cx);
        }
        self.reading_for = pinned;
        {
            let project = self
                .crew
                .store
                .folder()
                .map(|f| f.name.clone())
                .unwrap_or_default();
            let crew = Member::all();
            let running: Vec<String> = self
                .crew
                .store
                .jobs
                .iter()
                .filter(|j| j.status == Status::Running && j.folder.path == folder)
                .map(|j| format!("#{} {}: {}", j.id, j.bot.name(), j.label()))
                .collect();
            let in_thread = self.open_here_bots();
            let server = self.server_note(&folder);
            let thread = self.crew.store.thread_label(here);
            let summary = self.crew.store.summary_brief(&folder, here);
            let seen = brain::think::Seen {
                project: &project,
                thread: &thread,
                summary: &summary,
                recent: &recent,
                crew: &crew,
                running: &running,
                in_thread: &in_thread,
                server: &server,
            };
            match brain::think::ask(&st, &folder, &text, &seen) {
                Ok(rx) => {
                    // Peek's message shows as reading until the decision lands.
                    let talk = self.crew.peek_reading(text.clone());
                    self.reading = Some((talk, text, recent, rx));
                    self.reading_since = Some(std::time::Instant::now());
                    cx.notify();
                }
                Err(why) => self.read_missed(&st, text, &why, None, cx),
            }
        }
    }

    /// Peek's engine couldn't read `text`. Addressed to a bot with @, the bot
    /// answers it, read-only; else Peek says so and offers to try again.
    fn read_missed(
        &mut self,
        st: &crate::settings::AppSettings,
        text: String,
        why: &str,
        pending: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        match self.reading_for.take() {
            Some(who) => {
                if let Some(id) = pending {
                    self.crew.store.talks.retain(|t| t.id != id);
                }
                self.crew.ask_in_thread(st, who, text);
                cx.notify();
            }
            None => self.engine_missed(st, text, why, pending, cx),
        }
    }

    /// A message to a finished job, read by Peek's engine before the bot
    /// gets it. False when the engine can't read it; it goes to the bot then.
    fn read_follow_up(&mut self, job: u64, text: String, cx: &mut Context<Self>) -> bool {
        if self.sampling || self.reading.is_some() || self.follow_reading.is_some() {
            return false;
        }
        let st = Self::app_settings(cx);
        let Some(j) = self.crew.store.job(job) else {
            return false;
        };
        let (project, folder) = (j.folder.name.clone(), j.folder.path.clone());
        let recent = vec![
            ("You".to_string(), j.title.clone()),
            (j.bot.name().to_string(), j.label().to_string()),
        ];
        let crew = Member::all();
        let in_thread = self.open_here_bots();
        let server = self.server_note(&folder);
        let seen = brain::think::Seen {
            project: &project,
            thread: "",
            summary: "",
            recent: &recent,
            crew: &crew,
            running: &[],
            in_thread: &in_thread,
            server: &server,
        };
        match brain::think::ask(&st, &folder, &text, &seen) {
            Ok(rx) => {
                self.follow_reading = Some((job, text, rx));
                cx.notify();
                true
            }
            Err(_) => false,
        }
    }

    /// What Peek's engine made of a message to a finished job, once it has.
    fn poll_follow_reading(&mut self, cx: &mut Context<Self>) {
        let Some((_, _, rx)) = &self.follow_reading else {
            return;
        };
        let reply = match rx.try_recv() {
            Ok(crate::agent::Event::Output(r)) => r.ok(),
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let Some((job, text, _)) = self.follow_reading.take() else {
            return;
        };
        let decided = reply
            .as_deref()
            .map(crate::agent::strip_ansi)
            .and_then(|r| brain::think::parse(&r, &text, &Member::all()))
            .map(|(f, _)| f);
        match decided {
            Some(brain::front::Front::RunApp) => {
                self.run_wanted = Some((text, None, Some(job)));
                cx.notify();
            }
            Some(brain::front::Front::Stop) => self.stop_here(text, cx),
            // Anything else is the bot's to answer.
            _ => self.follow_up(job, text, cx),
        }
    }

    /// "run the app" in a thread: runs the app of this thread's latest job
    /// (its branch), else the project's. False when there's none to run.
    fn run_latest_app(
        &mut self,
        text: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.crew.store.folder().is_none() || self.sampling {
            return false;
        }
        let here = self.crew.store.thread_here();
        let folder = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let job = self
            .crew
            .store
            .jobs
            .iter()
            .rev()
            .find(|j| {
                j.thread == here
                    && j.folder.path == folder
                    && j.status != Status::Running
                    && j.mode.writes()
            })
            .map(|j| j.id);
        // Peek's message about it, which its card sits under.
        let talk = self
            .crew
            .store
            .talks
            .iter()
            .map(|t| t.id)
            .max()
            .unwrap_or(0)
            + 1;
        match self.run_app(job, Some(talk), cx) {
            Some(line) => {
                self.crew.peek_says(text, line, Vec::new());
                true
            }
            None => false,
        }
    }

    /// Peek's message that shows it reading, while its engine reads.
    pub(super) fn reading_talk(&self) -> Option<u64> {
        self.reading.as_ref().map(|r| r.0)
    }

    /// Peek's engine couldn't read `text` (`why` says what went wrong). Peek
    /// doesn't guess: it says so in its message (`pending`, if it showed one)
    /// and offers to try again, or to pick a bot with @.
    fn engine_missed(
        &mut self,
        st: &crate::settings::AppSettings,
        text: String,
        why: &str,
        pending: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let engine = brain::think::engine_name(st);
        let why = why.trim().lines().next().unwrap_or("").trim();
        let why = if why.is_empty() {
            String::new()
        } else {
            format!(" ({})", why.chars().take(120).collect::<String>())
        };
        let reply = format!(
            "I couldn't reach {engine} to read that{why}, so I haven't started anything. Try again, or pick a bot yourself with @, like @Builder."
        );
        let front = brain::front::Front::AskBack {
            question: reply,
            choices: vec!["Try again".into()],
        };
        self.act_on(
            front,
            text.clone(),
            None,
            pending,
            Some("Peek, offline".into()),
            cx,
        );
        let id = self.crew.store.talks.last().map(|t| t.id).unwrap_or(0);
        let id = pending.unwrap_or(id);
        self.retry = Some((id, text));
    }

    /// Keeps Peek's engine warm for the project open now, checked every few
    /// seconds, so a message reads in a second or two.
    fn keep_warm(&mut self, st: &crate::settings::AppSettings) {
        if self.sampling || self.warmed_at.is_some_and(|at| at.elapsed().as_secs() < 5) {
            return;
        }
        self.warmed_at = Some(std::time::Instant::now());
        if let Some(f) = self.crew.store.folder() {
            brain::think::prepare(st, &f.path);
        }
    }

    /// What Peek's engine decided, once it has: acted on, or the rules'
    /// decision when it failed or made no sense.
    fn poll_reading(&mut self, cx: &mut Context<Self>) {
        let Some((_, _, _, rx)) = &self.reading else {
            return;
        };
        let (reply, err) = match rx.try_recv() {
            Ok(crate::agent::Event::Output(Ok(r))) => (Some(r), None),
            Ok(crate::agent::Event::Output(Err(e))) => (None, Some(e)),
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => (None, None),
        };
        let Some((talk, text, recent, _)) = self.reading.take() else {
            return;
        };
        let crew = Member::all();
        let decided = reply
            .as_deref()
            .map(crate::agent::strip_ansi)
            .and_then(|r| brain::think::parse(&r, &text, &crew));
        let st = Self::app_settings(cx);
        let failed = err.is_some();
        let took = self
            .reading_since
            .map(|at| at.elapsed().as_secs_f32())
            .unwrap_or(0.);
        let _ = recent;
        match decided {
            Some((f, say)) => {
                let via = format!("{} \u{b7} {took:.1}s", brain::think::engine_name(&st));
                let pinned = self.reading_for.take();
                let f = match pinned {
                    Some(who) => brain::front::pinned(f, who, &text),
                    None => f,
                };
                self.answer_by = pinned;
                self.act_on(
                    f,
                    text,
                    Some(say).filter(|s| !s.is_empty()),
                    Some(talk),
                    Some(via),
                    cx,
                );
                self.answer_by = None;
            }
            None => {
                let why = match (failed, err) {
                    (true, Some(e)) => e,
                    _ => "its answer didn't make sense".to_string(),
                };
                self.read_missed(&st, text, &why, Some(talk), cx);
            }
        }
    }

    /// Does what Peek decided about `text`. `line` is Peek's own words for a
    /// hand-off; `pending` its message that showed it reading, which the
    /// reply fills or which goes once a bot takes over.
    fn act_on(
        &mut self,
        front: brain::front::Front,
        text: String,
        line: Option<String>,
        pending: Option<u64>,
        via: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let st = Self::app_settings(cx);
        let says = |this: &mut Self, reply: String, choices: Vec<String>| -> u64 {
            let id = match pending {
                Some(id) if this.crew.fill_talk(id, reply.clone(), choices.clone()) => id,
                _ => {
                    this.crew.peek_says(text.clone(), reply, choices);
                    this.crew.store.talks.last().map(|t| t.id).unwrap_or(0)
                }
            };
            // Who decided, so Peek's word is honest about it.
            if let Some(t) = this.crew.store.talks.iter_mut().find(|t| t.id == id) {
                if let Some(a) = t.answer.as_mut() {
                    a.via = via.clone();
                }
            }
            id
        };
        let drop_pending = |this: &mut Self| {
            if let Some(id) = pending {
                this.crew.store.talks.retain(|t| t.id != id);
            }
        };
        match front {
            brain::front::Front::Chat { reply } => {
                says(self, reply, Vec::new());
                cx.notify();
            }
            brain::front::Front::AskBack { question, choices } => {
                says(self, question, choices);
                cx.notify();
            }
            brain::front::Front::Answer { question, .. } => {
                drop_pending(self);
                if !self.sample_guard(cx) {
                    let who = self.answer_by.unwrap_or_else(|| self.ask_who());
                    self.crew.ask_in_thread(&st, who, question);
                }
                cx.notify();
            }
            brain::front::Front::Confirm { why, route } => {
                // Switching to another bot reads as what it does.
                let yes = if route.by == brain::front::SWITCH {
                    format!("Give it to {}", route.bot.name())
                } else {
                    "Go ahead".to_string()
                };
                let id = says(self, why, vec![yes, "Cancel".into()]);
                self.confirming = Some((id, text, route));
                cx.notify();
            }
            brain::front::Front::Stop => {
                drop_pending(self);
                self.stop_here(text, cx)
            }
            brain::front::Front::RunApp => {
                self.run_wanted = Some((text, pending, None));
                cx.notify();
            }
            brain::front::Front::Hand(route) => {
                drop_pending(self);
                // Peek says why it's this bot, on the job's card.
                let line = self.handoff_line(&route, line);
                self.hand_with_line(route, text, line, cx);
            }
        }
    }

    /// A choice under one of Peek's messages: the answer to a risky ask it
    /// held, or else a reply in its own right.
    pub(super) fn pick_choice(&mut self, talk: u64, choice: String, cx: &mut Context<Self>) {
        if choice == "Try again" {
            if let Some((id, text)) = self.retry.take() {
                if id == talk {
                    self.crew.store.talks.retain(|t| t.id != id);
                    return self.read_first(text, cx);
                }
                self.retry = Some((id, text));
            }
        }
        match self.confirming.take() {
            Some((id, text, route)) if id == talk => {
                if choice == "Go ahead" || choice == format!("Give it to {}", route.bot.name()) {
                    let said = matches!(route.by.as_str(), "engine" | "learned" | "switch")
                        .then(|| route.why.clone());
                    let line = self.handoff_line(&route, said);
                    self.hand_with_line(route, text, line, cx);
                } else {
                    self.say("Okay, nothing started.");
                    cx.notify();
                }
            }
            held => {
                self.confirming = held;
                self.hand_over(choice, cx);
            }
        }
    }

    /// Where jobs on the project at `folder` can run besides here, for Peek's
    /// engine: the server, whether it answers and is signed in, whether it
    /// has the project, and the person's usual choice. Empty with no server.
    fn server_note(&self, folder: &str) -> String {
        let Some(link) = self.crew.link.as_ref() else {
            return String::new();
        };
        let s = link.server();
        let name = s.name.clone();
        let state = if !s.reachable {
            "not answering right now, so jobs run here until it is".to_string()
        } else if s.engines.is_empty() {
            "connected, but nothing is signed in there yet".to_string()
        } else {
            format!("connected, with {} signed in", s.engines.join(", "))
        };
        let f = self
            .crew
            .store
            .folders
            .iter()
            .find(|f| f.path == folder)
            .cloned()
            .unwrap_or_default();
        let remote = ship::remote_url(folder).unwrap_or_default();
        let has = if link.path_for(&f, &remote).is_some() {
            "It has this project.".to_string()
        } else {
            "It doesn't have this project yet; Squad Code sets it up there through git first, by itself."
                .to_string()
        };
        let usual = match link.cfg.run.as_str() {
            "server" => format!("Jobs usually run on {name}."),
            "here" => "Jobs usually run on this computer.".to_string(),
            _ => "Squad Code picks where each job runs.".to_string(),
        };
        format!("this computer, or the person's server {name} ({state}). {has} {usual}")
    }

    /// The bots with an open job in the thread you're in.
    fn open_here_bots(&self) -> Vec<Member> {
        Member::all()
            .into_iter()
            .filter(|b| self.open_here(*b).is_some())
            .collect()
    }

    /// Peek's line on a hand-off: why this bot (the engine's reason, unless
    /// that names the bot), and where it runs when that was asked for.
    fn handoff_line(&self, route: &brain::route::Route, said: Option<String>) -> String {
        let reason = said
            .map(|s| s.trim().to_string())
            .filter(|s| {
                !s.is_empty() && !s.to_lowercase().contains(&route.bot.name().to_lowercase())
            })
            .unwrap_or_else(|| route.reason());
        let place = match (route.site.as_str(), self.crew.link.as_ref()) {
            ("server", Some(l)) => format!(" It runs on {}.", l.cfg.label()),
            ("here", _) => " It runs on this computer.".to_string(),
            _ => String::new(),
        };
        format!("{reason}{place}")
    }

    /// Starts an ask the way Peek routed it, with Peek's line on the new
    /// job's card. When the bot isn't on the crew yet, the line waits on
    /// the offer to add it, and goes on the job once that's taken.
    fn hand_with_line(
        &mut self,
        route: brain::route::Route,
        text: String,
        line: String,
        cx: &mut Context<Self>,
    ) {
        let before = self.last_job();
        self.hand_to(route, text, cx);
        if !self.mark_new_jobs(before, &line) {
            if let Some(o) = self.offer.as_mut() {
                o.handoff = line;
            }
        }
    }

    /// The newest job's id, or 0.
    fn last_job(&self) -> u64 {
        self.crew.store.jobs.iter().map(|j| j.id).max().unwrap_or(0)
    }

    /// Puts Peek's hand-off line on the jobs started after `before`; false
    /// when none started.
    fn mark_new_jobs(&mut self, before: u64, line: &str) -> bool {
        let mut any = false;
        for j in self.crew.store.jobs.iter_mut().filter(|j| j.id > before) {
            j.handoff = line.to_string();
            any = true;
        }
        if any {
            self.crew.store.save();
        }
        any
    }

    /// Starts an ask the way Peek routed it, without reading it again.
    pub(super) fn hand_to(
        &mut self,
        route: brain::route::Route,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let (target, mode, pinned) = (self.target, self.mode, self.mode_pinned);
        // The bot Peek picked takes it, doing what that bot does.
        self.target = Some(route.bot);
        self.mode = route.bot.mode();
        self.mode_pinned = true;
        // "do it on the VPS": this job runs there, whatever the usual choice.
        self.crew.where_next = (!route.site.is_empty()).then(|| route.site.clone());
        self.hand_over_inner(text, cx);
        self.crew.where_next = None;
        self.target = target;
        self.mode = mode;
        self.mode_pinned = pinned;
    }

    fn hand_over_inner(&mut self, text: String, cx: &mut Context<Self>) {
        // Slash commands from the composer.
        let mut text = text;
        let mut plan = false;
        let mut race = 0;
        let mut later: Option<u64> = None;
        let mut asked = false;
        if let Some(rest) = text.clone().strip_prefix('/') {
            let (cmd, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let arg = arg.trim().to_string();
            match cmd {
                "thread" => {
                    if arg.is_empty() {
                        self.say("Give the thread a name, like /thread gift cards.");
                        return;
                    }
                    if self.crew.store.folder().is_none() {
                        self.say("Pick a project folder first, then make threads in it.");
                        self.pick_folder(cx);
                        return;
                    }
                    let bots: Vec<Member> = Member::all()
                        .into_iter()
                        .filter(|m| {
                            *m != Member::Fin
                                && arg
                                    .to_lowercase()
                                    .contains(&format!("@{}", m.name().to_lowercase()))
                        })
                        .collect();
                    let name: String = arg
                        .split_whitespace()
                        .filter(|w| !w.starts_with('@'))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if self.crew.store.add_thread(&name, bots).is_some() {
                        self.crew.store.save();
                        let label = self.crew.store.thread_label(self.crew.store.thread_now);
                        self.say(format!("#{label} is ready. Peek joins every thread."));
                    }
                    self.go(Place::Today, cx);
                    return;
                }
                "rename" => {
                    let now = self.crew.store.thread_now;
                    let name = store::thread_name(&arg);
                    if now == 0 {
                        self.say("#general keeps its name. Rename any other thread.");
                    } else if name.is_empty() || name == "general" {
                        self.say("Give the new name, like /rename gift cards.");
                    } else if self.crew.store.rename_thread(now, &name) {
                        self.crew.store.save();
                        self.say(format!("Renamed to #{name}."));
                    } else {
                        self.say(format!("There's already a #{name} in this project."));
                    }
                    cx.notify();
                    return;
                }
                "delete" => {
                    let now = self.crew.store.thread_now;
                    if now == 0 {
                        self.say("#general always stays. Delete any other thread.");
                    } else {
                        self.deleting = Some(now);
                        self.say("Confirm in the sidebar: Delete or Cancel.");
                    }
                    cx.notify();
                    return;
                }
                "archive" => {
                    self.archive_thread(self.crew.store.thread_now, cx);
                    return;
                }
                "note" => {
                    let Some(folder) = self.crew.store.folder().map(|f| f.path.clone()) else {
                        self.say("Pick a project folder first.");
                        return;
                    };
                    if arg.is_empty() {
                        self.say("Write the note, like /note this thread is for discussion, not building.");
                        return;
                    }
                    let here = self.crew.store.thread_here();
                    let label = self.crew.store.thread_label(here);
                    self.crew.store.summary_mut(&folder, here).notes.push(arg);
                    self.crew.store.save();
                    self.about_open = true;
                    self.say(format!(
                        "Pinned to #{label}. Every bot here reads it, and it wins over the summary."
                    ));
                    cx.notify();
                    return;
                }
                "plan" => plan = true,
                "race" => race = 2,
                // "/later tonight tidy the deps": a job for the night shift.
                "later" => match shift::parse_when(&arg, chrono::Local::now()) {
                    Some((at, rest)) => {
                        later = Some(at);
                        text = rest.clone();
                        if !self.mode_pinned {
                            self.mode = Mode::guess(&rest);
                        }
                        if !self.mode.writes() {
                            self.mode = Mode::Build;
                        }
                        self.mode_pinned = true;
                    }
                    None => {
                        self.say("Say when, then the job: /later tonight tidy the deps, /later 2am …, /later in 2h …, /later tomorrow ….");
                        return;
                    }
                },
                "ask" => {
                    self.mode = Mode::Ask;
                    self.mode_pinned = true;
                    asked = true;
                }
                _ => {
                    self.say(format!(
                        "There's no /{cmd}. Try /plan, /race, /later, /ask, /note, /thread, /rename, /archive or /delete."
                    ));
                    return;
                }
            }
            if later.is_none() {
                text = arg;
            }
        }
        // A named bot does what it does; the kind of work is only guessed
        // when nobody was named or picked.
        match self.target {
            Some(t) if !asked => self.mode = t.mode(),
            _ if !self.mode_pinned => self.mode = Mode::guess(&text),
            _ => {}
        }
        if plan && !self.mode.writes() {
            self.mode = Mode::Build;
        }
        let plan =
            plan || self.splitting || cx.global::<crate::settings::AppSettings>().v2_plan_first;
        let racing = if race > 0 { race } else { self.racing };
        let text = match (text.is_empty(), self.mode.default_job()) {
            (false, _) => text,
            (true, Some(d)) => d.to_string(),
            (true, None) => return,
        };
        if self.crew.store.folder().is_none() {
            self.say("Pick a folder first, so the crew knows where to work.");
            self.pick_folder(cx);
            return;
        }
        if !Member::all().contains(&self.doer()) {
            // That bot isn't on the crew yet: Peek offers to add it first.
            self.offer = Some(Offer {
                who: self.doer(),
                mode: self.mode,
                text,
                goal: plan && self.mode.writes(),
                race: if self.mode.writes() { racing } else { 0 },
                handoff: String::new(),
                site: self.crew.where_next.clone().unwrap_or_default(),
            });
            cx.notify();
            return;
        }
        self.offer = None;
        let doer = self.doer();
        if self.mode == Mode::Ask {
            // Asked in a thread: the answer comes back in the thread.
            if self.place == Place::Today {
                if !self.sample_guard(cx) {
                    let st = Self::app_settings(cx);
                    let who = self.ask_who();
                    self.crew.ask_in_thread(&st, who, text);
                }
                cx.notify();
                return;
            }
            self.go(Place::Chat(doer), cx);
            self.ask(doer, text, cx);
            return;
        }
        let Some(folder) = self.crew.store.folder().cloned() else {
            return;
        };
        let mode = self.mode;
        if let Some(at) = later {
            self.mode_pinned = false;
            let when = shift::when_words(at, store::now());
            self.crew.queue_later(doer, mode, text.clone(), folder, at);
            self.say(format!(
                "{} starts \u{201c}{}\u{201d} {when}. It's in the Waiting list until then.",
                doer.name(),
                text.lines().next().unwrap_or("").trim()
            ));
            cx.notify();
            return;
        }
        if racing > 1 && mode.writes() {
            self.start_race(doer, mode, text, folder, racing, cx);
            return;
        }
        if plan && mode.writes() {
            self.begin_goal(text, folder, cx);
            return;
        }
        // The one rule: the bot's open job in this thread takes it, else
        // it's a new job.
        if self.place == Place::Today {
            if let Some(id) = self.open_here(doer) {
                let st = Self::app_settings(cx);
                self.crew.add_to_job(&st, id, text);
                self.crew.store.save();
                cx.notify();
                return;
            }
        }
        self.start_job_in(doer, mode, text, folder, cx);
    }

    /// The bot a job from the box goes to: the one named or picked for it,
    /// else the crew's bot for the kind of work.
    fn doer(&self) -> Member {
        self.target
            .or_else(|| Member::doer_for(self.mode))
            .unwrap_or(Member::for_mode(self.mode))
    }

    /// Who answers a question or a chat in a thread: the bot you picked, else
    /// Peek, who leads the crew and talks about anything.
    pub(super) fn ask_who(&self) -> Member {
        self.target
            .filter(|t| t.mode() == Mode::Ask && Member::all().contains(t))
            .unwrap_or(Member::Fin)
    }

    /// `who`'s open job in the thread you're in, which what you say to it
    /// goes into.
    pub(super) fn open_here(&self, who: Member) -> Option<u64> {
        let folder = self.crew.store.folder()?.path.clone();
        self.crew
            .open_job(&folder, self.crew.store.thread_here(), who)
    }

    /// Puts built-in bots on the crew, with one line saying so.
    fn add_to_crew(&mut self, who: &[Member]) {
        let who: Vec<Member> = who
            .iter()
            .copied()
            .filter(|m| !m.is_made() && !m.on_crew())
            .collect();
        if who.is_empty() {
            return;
        }
        self.crew.store.removed.retain(|m| !who.contains(m));
        data::set_removed(&self.crew.store.removed);
        self.crew.store.save();
        let names: Vec<&str> = who.iter().map(|m| m.name()).collect();
        self.say(format!("Added {} to your crew.", and_list(&names)));
    }

    /// Fin's offer taken: the bot joins the crew and the held job starts.
    pub(super) fn take_offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(o) = self.offer.take() else { return };
        self.add_to_crew(&[o.who]);
        self.target = None;
        self.splitting = o.goal;
        self.racing = o.race;
        self.set_mode(o.mode, window, cx);
        let added = self.toast.clone();
        let before = self.last_job();
        // Peek's choice of where it runs still holds once the bot is added.
        self.crew.where_next = (!o.site.is_empty()).then(|| o.site.clone());
        self.hand_over(o.text, cx);
        self.crew.where_next = None;
        if !o.handoff.is_empty() {
            self.mark_new_jobs(before, &o.handoff);
        }
        self.unpin_mode(window, cx);
        // Keep "Added Fixer" showing unless starting the job had more to say.
        if self.toast.is_none() {
            self.toast = added;
        }
        cx.notify();
    }

    /// Fin's offer turned down: the job goes back in the box, nothing added.
    pub(super) fn skip_offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(o) = self.offer.take() else { return };
        self.composer
            .update(cx, |i, cx| ui::put_text(i, o.text, window, cx));
        cx.notify();
    }

    /// The person would rather make their own bot for the job: the maker opens
    /// on Crew set to the kind of work, and the job waits in the box.
    pub(super) fn make_for_offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(o) = self.offer.take() else { return };
        let about = format!("{}, for jobs like: {}", o.who.role(), o.text);
        self.composer
            .update(cx, |i, cx| ui::put_text(i, o.text, window, cx));
        let like = Member::KINDS.contains(&o.who).then_some(o.who);
        self.open_maker(Some(&about), like, window, cx);
        self.go(Place::Crew, cx);
        self.say("Make your bot, then send the job again from Jobs.");
        cx.notify();
    }

    /// Adds a built-in bot to the crew, or removes it (with its chat; its past
    /// jobs stay in History).
    fn set_on_crew(&mut self, who: Member, on: bool, cx: &mut Context<Self>) {
        if who.is_made() || who == Member::Fin {
            return;
        }
        if !on && Member::all().len() <= 1 {
            self.say("Keep at least one bot on the crew.");
            cx.notify();
            return;
        }
        if !on && self.crew.store.busy(who).is_some() {
            self.say(format!("{} is working. Stop the job first.", who.name()));
            cx.notify();
            return;
        }
        if !on {
            self.crew.store.chats.remove(&who);
            if self.target == Some(who) {
                self.target = None;
            }
        }
        self.crew.store.removed.retain(|m| *m != who);
        if !on {
            self.crew.store.removed.push(who);
        }
        data::set_removed(&self.crew.store.removed);
        self.crew.store.save();
        if !on && self.place == Place::Chat(who) {
            self.place = Place::Crew;
        }
        self.say(if on {
            format!("Added {} to your crew.", who.name())
        } else {
            format!("Removed {} from your crew.", who.name())
        });
        cx.notify();
    }

    /// Takes a made bot off the crew. Its past jobs stay in History.
    fn remove_made(&mut self, who: Member, cx: &mut Context<Self>) {
        let Member::Made(id) = who else { return };
        if self.crew.store.busy(who).is_some() {
            self.say(format!("{} is working. Stop the job first.", who.name()));
            cx.notify();
            return;
        }
        let name = who.name();
        self.crew.store.made.retain(|m| m.id != id);
        self.crew.store.chats.remove(&who);
        data::set_made(&self.crew.store.made);
        if self.target == Some(who) {
            self.target = None;
        }
        if self.place == Place::Chat(who) {
            self.place = Place::Crew;
        }
        self.crew.store.save();
        self.say(format!("{name} left the crew."));
        cx.notify();
    }

    /// The person is back: the crew wakes up.
    fn wake(&mut self, cx: &mut Context<Self>) {
        self.touched = Instant::now();
        if self
            .dozing
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            cx.notify();
        }
    }

    /// Folders dropped on the window join the list; the last one becomes current.
    fn drop_paths(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let mut added = Vec::new();
        for dir in folders_from(paths) {
            self.crew.store.add_folder(dir);
            if let Some(f) = self.crew.store.folder() {
                added.push(f.name.clone());
            }
        }
        if added.is_empty() {
            return;
        }
        self.crew.store.save();
        self.say(match added.as_slice() {
            [one] => format!("The crew now works in {one}."),
            many => format!(
                "Added {} folders. The crew now works in {}.",
                many.len(),
                many.last().cloned().unwrap_or_default()
            ),
        });
        cx.notify();
    }

    /// Takes a folder off the crew's list. Its files and past jobs stay.
    fn forget_folder(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.crew.store.folders.len() {
            return;
        }
        let gone = self.crew.store.folders.remove(i);
        if self.crew.store.current >= self.crew.store.folders.len() || self.crew.store.current > i {
            self.crew.store.current = self.crew.store.current.saturating_sub(1);
        }
        if self.crew.store.folders.is_empty() {
            self.folder_menu = false;
        }
        self.crew.store.save();
        self.say(format!(
            "Removed {} from the list. Its files are untouched.",
            gone.name
        ));
        cx.notify();
    }

    /// Starts a stopped or failed job over, as a new job in the same folder.
    fn retry_job(&mut self, id: u64, cx: &mut Context<Self>) {
        self.rerun(id, None, cx);
    }

    /// The settings the crew runs with, as they are now.
    fn app_settings(cx: &Context<Self>) -> crate::settings::AppSettings {
        cx.global::<crate::settings::AppSettings>().clone()
    }

    /// Goes to what starting something opened.
    fn open(&mut self, opened: runtime::Opened, cx: &mut Context<Self>) {
        // Asked in a thread: the job's card shows up right there, so stay.
        if self.place == Place::Today {
            cx.notify();
            return;
        }
        let place = match opened {
            runtime::Opened::Job(id) | runtime::Opened::Busy(id) => Place::Live(id),
            runtime::Opened::Race(id) => Place::Race(id),
            // It waits in line; what it waits for was said.
            runtime::Opened::Waiting(_) => {
                cx.notify();
                return;
            }
        };
        self.go(place, cx);
    }

    /// Runs a job again from the start, in the same branch, on its own engine or
    /// on `pick` (an agent index) when its own can't run it now.
    pub(super) fn rerun(&mut self, id: u64, pick: Option<usize>, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        if self.crew.rerun(&st, id, pick) {
            self.go(Place::Live(id), cx);
        }
    }

    /// Rewinds a job to before its turn `k`: its files go back and the
    /// later turns leave its feed.
    pub(super) fn rewind_job(&mut self, id: u64, k: usize, cx: &mut Context<Self>) {
        self.rewind_ask = None;
        if let Err(why) = self.crew.rewind(id, k) {
            self.say(why);
        }
        cx.notify();
    }

    /// Rewinds, then puts the message it went back over in the job's box, to
    /// change and send again.
    pub(super) fn rewind_and_edit(
        &mut self,
        id: u64,
        k: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let said = self.crew.store.job(id).and_then(|j| {
            let at = j.marks.get(k)?.feed_at;
            match j.feed.get(at) {
                Some(store::Item::You(t)) => Some(t.clone()),
                _ => j.marks.get(k).map(|m| m.said.clone()),
            }
        });
        self.rewind_job(id, k, cx);
        if let Some(text) = said {
            self.live_input
                .update(cx, |i, cx| i.set_value(text, window, cx));
            self.live_input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    /// Sends what's typed in a job's own box: to the job on its own page,
    /// or open beside its thread.
    pub(super) fn send_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.live_input.clone();
        let text = input.read(cx).value().trim().to_string();
        let job = match self.place {
            Place::Live(id) => Some(id),
            Place::Today => self.reply_open,
            _ => None,
        };
        if let (false, Some(id)) = (text.is_empty(), job) {
            input.update(cx, |i, cx| i.set_value("", window, cx));
            // To a finished job, Peek's engine reads it first: "run the app"
            // runs it, "stop" stops it, and the rest goes to the bot.
            let idle = self
                .crew
                .store
                .job(id)
                .is_some_and(|j| j.status != Status::Running);
            if !(idle && self.read_follow_up(id, text.clone(), cx)) {
                // Ask: it answers, and leaves the files as they are.
                let text = if self.live_ask {
                    format!("Only answer this; don't change any files. {text}")
                } else {
                    text
                };
                self.follow_up(id, text, cx);
            }
        }
    }

    /// A stopped or interrupted job picks up where it was.
    pub(super) fn carry_on(&mut self, id: u64, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        self.crew.carry_on(&st, id);
        cx.notify();
    }

    /// Hands a branch that clashes with its base back to its bot to bring up to
    /// date; it comes back for review when it's done.
    pub(super) fn resolve(&mut self, id: u64, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        self.crew.resolve(&st, id);
        cx.notify();
    }

    /// Approves a job the seatbelt held, as it is.
    pub(super) fn approve_anyway(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.approve_anyway(id);
        cx.notify();
    }

    /// Brings back your changes a merge set aside and couldn't put back, with
    /// the clash marked in the files.
    pub(super) fn bring_back(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.bring_back(id);
        cx.notify();
    }

    /// Leaves those changes in the stash and puts the card away.
    pub(super) fn leave_kept(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.leave_kept(id);
        cx.notify();
    }

    /// A check that fails on its own, not because of job `id`: Fixer gets a
    /// job of its own to make it pass.
    pub(super) fn fix_check(
        &mut self,
        id: u64,
        command: &str,
        tail: &str,
        folder: store::Folder,
        cx: &mut Context<Self>,
    ) {
        let end: Vec<&str> = tail.lines().rev().take(30).collect();
        let end: Vec<&str> = end.into_iter().rev().collect();
        let text = format!(
            "The check `{command}` fails in this project, and not because of job #{id}. Make it pass without changing what it checks.\n\nThe end of its output:\n```\n{}\n```",
            end.join("\n")
        );
        self.start_job_in(Member::Fixer, Mode::Fix, text, folder, cx);
    }

    /// Leaves a failing check out of job `id`'s verdict: it fails on its own.
    pub(super) fn ignore_check(&mut self, id: u64, command: &str, cx: &mut Context<Self>) {
        if let Some(j) = self.crew.store.job_mut(id) {
            if !j.ignored_checks.iter().any(|c| c == command) {
                j.ignored_checks.push(command.to_string());
            }
            let ignored = j.ignored_checks.clone();
            work::ignore(&mut j.checks, &ignored);
        }
        self.crew.store.save();
        cx.notify();
    }

    /// Hands what the seatbelt found back to the job's bot to take out.
    pub(super) fn take_out(&mut self, id: u64, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        self.crew.take_out(&st, id);
        cx.notify();
    }

    fn start_job_in(
        &mut self,
        bot: Member,
        mode: Mode,
        text: String,
        folder: store::Folder,
        cx: &mut Context<Self>,
    ) {
        let st = Self::app_settings(cx);
        let opened = self.crew.start_job(&st, bot, mode, text, folder);
        self.open(opened, cx);
    }

    /// Races one job on up to `n` engines at once, each try on its own branch,
    /// then shows them side by side so you can keep the best.
    fn start_race(
        &mut self,
        bot: Member,
        mode: Mode,
        text: String,
        folder: store::Folder,
        n: usize,
        cx: &mut Context<Self>,
    ) {
        let st = Self::app_settings(cx);
        let opened = self.crew.start_race(&st, bot, mode, text, folder, n);
        self.open(opened, cx);
    }

    /// Starts the same job again on other engines, so it races the first try.
    pub(super) fn race_it(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        let st = Self::app_settings(cx);
        match self.crew.race_it(&st, id) {
            Some(race) => self.go(Place::Race(race), cx),
            None => cx.notify(),
        }
    }

    /// Has Squad Code split a goal into jobs, then shows the split to look over.
    fn begin_goal(&mut self, text: String, folder: store::Folder, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        let id = self.crew.begin_goal(&st, text, folder);
        self.go(Place::Goal(id), cx);
    }

    /// Starts a goal's jobs, the first one now.
    pub(super) fn start_goal(&mut self, id: u64, cx: &mut Context<Self>) {
        // The bots its jobs need join the crew first; the page said so.
        let needs = self
            .crew
            .store
            .goals
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.needs())
            .unwrap_or_default();
        self.add_to_crew(&needs);
        let st = Self::app_settings(cx);
        self.crew.start_goal(&st, id);
        cx.notify();
    }

    /// Goes on past a job that stopped.
    pub(super) fn skip_step(&mut self, id: u64, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        self.crew.skip_step(&st, id);
        cx.notify();
    }

    pub(super) fn drop_goal(&mut self, id: u64, cx: &mut Context<Self>) {
        self.crew.drop_goal(id);
        self.go(Place::Today, cx);
    }

    /// The tries in a race, first one first.
    pub(super) fn tries(&self, race: u64) -> Vec<store::Job> {
        self.crew.tries(race)
    }

    /// Keeps one try of a race: approves it and throws the others away.
    pub(super) fn keep_try(&mut self, id: u64, cx: &mut Context<Self>) {
        self.crew.keep_try(id);
        cx.notify();
    }

    /// Opens a job: its race when it is one try in a race.
    pub(super) fn open_job(&mut self, id: u64, cx: &mut Context<Self>) {
        let race = self.crew.store.job(id).map(|j| j.race).unwrap_or(0);
        if race != 0 && self.tries(race).len() > 1 {
            self.go(Place::Race(race), cx);
        } else {
            self.go(Place::Live(id), cx);
        }
    }

    /// More words for a job: while it runs they wait; once it's done they start a
    /// follow-up that continues the same conversation when the engine can.
    fn follow_up(&mut self, id: u64, text: String, cx: &mut Context<Self>) {
        let st = Self::app_settings(cx);
        self.crew.follow_up(&st, id, text);
        cx.notify();
    }

    /// Runs the checks on a job's changes again.
    pub(super) fn recheck(&mut self, id: u64, cx: &mut Context<Self>) {
        self.crew.recheck(id);
        cx.notify();
    }

    /// What Approve does for this job's project.
    pub(super) fn way(&mut self, job: &store::Job) -> ship::Way {
        self.crew.way(job)
    }

    /// The Approve button's words for this job.
    pub(super) fn approve_label(&mut self, job: &store::Job) -> String {
        match self.way(job) {
            ship::Way::PullRequest => "Approve & open PR".into(),
            ship::Way::Merge => format!("Approve & merge into {}", job.base),
        }
    }

    /// Switches what Approve does in a job's project: pull request or local merge.
    pub(super) fn set_way(&mut self, id: u64, way: ship::Way, cx: &mut Context<Self>) {
        self.crew.set_way(id, way);
        cx.notify();
    }

    /// Approves a job's changes: a pull request from Shipper on GitHub projects,
    /// otherwise merged into the branch it started from.
    pub(super) fn approve(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.approve(id);
        // Hands off: a pull request Shipper opens merges once its checks pass.
        if cx.global::<crate::settings::AppSettings>().v2_auto_merge
            && self
                .crew
                .store
                .job(id)
                .is_some_and(|j| j.status == store::Status::Shipping)
        {
            self.crew.set_auto_merge(id, true);
        }
        cx.notify();
    }

    /// Throws a job's changes away, with its branch and worktree.
    pub(super) fn discard_job(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.discard(id);
        self.diff_open = None;
        cx.notify();
    }

    /// Merges (or closes) a job's pull request on GitHub now.
    pub(super) fn land_pr(&mut self, id: u64, merge: bool, cx: &mut Context<Self>) {
        self.crew.land_pr(id, merge);
        cx.notify();
    }

    /// Turns "merge when checks pass" on or off for a job's pull request.
    pub(super) fn set_auto_merge(&mut self, id: u64, on: bool, cx: &mut Context<Self>) {
        self.crew.set_auto_merge(id, on);
        cx.notify();
    }

    /// Stops what's running where the person is: the jobs in this thread,
    /// else the bot they're talking to, else every job in this project.
    fn stop_here(&mut self, text: String, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        let folder = self
            .crew
            .store
            .folder()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let here = self.crew.store.thread_here();
        let target = self.target;
        let running: Vec<&store::Job> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && j.folder.path == folder)
            .collect();
        let pick: Vec<u64> = {
            let in_thread: Vec<u64> = running
                .iter()
                .filter(|j| j.thread == here)
                .map(|j| j.id)
                .collect();
            let by_bot: Vec<u64> = running
                .iter()
                .filter(|j| Some(j.bot) == target)
                .map(|j| j.id)
                .collect();
            if !in_thread.is_empty() {
                in_thread
            } else if !by_bot.is_empty() {
                by_bot
            } else {
                running.iter().map(|j| j.id).collect()
            }
        };
        let names: Vec<String> = pick
            .iter()
            .filter_map(|id| self.crew.store.job(*id))
            .map(|j| format!("{} on #{}", j.bot.name(), j.id))
            .collect();
        for id in &pick {
            self.crew.stop(*id);
        }
        let apps = self.stop_apps();
        let reply = match names.len() {
            0 if apps > 0 => "Stopped the app.".to_string(),
            0 => "Nothing's running here right now, so there's nothing to stop.".to_string(),
            _ => format!("Stopped {}, and anything it had running.", names.join(", ")),
        };
        self.crew.peek_says(text, reply, Vec::new());
        cx.notify();
    }

    pub(super) fn stop_job(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        self.crew.stop(id);
        cx.notify();
    }

    /// Asks `who` for real, on its own engine, in the current folder.
    fn ask(&mut self, who: Member, text: String, cx: &mut Context<Self>) {
        if self.sample_guard(cx) {
            return;
        }
        let st = Self::app_settings(cx);
        self.crew.ask(&st, who, text);
        cx.notify();
    }

    /// Whether `who` is answering a question right now.
    fn thinking(&self, who: Member) -> bool {
        self.crew.thinking(who)
    }

    /// Moves the crew along, then says what happened: what it wants said, and
    /// jobs that ended while the person was looking elsewhere.
    fn poll(&mut self, cx: &mut Context<Self>) {
        // Borrowed, not cloned: this runs up to 60 times a second.
        let viewing = match self.place {
            Place::Live(id) => Some(id),
            _ => None,
        };
        // Saves asked for since the last tick, at most about once a second.
        self.crew.store.flush(false);
        // Settings changed from a terminal (`squad brain install`): take them,
        // so the next save here doesn't put the old ones back.
        if let Some(st) = crate::settings::changed_elsewhere() {
            st.apply();
            cx.set_global(st);
        }
        self.poll_server_setup(cx);
        self.poll_machine(cx);
        self.poll_web(cx);
        self.poll_runs();
        let finished = self
            .crew
            .poll(cx.global::<crate::settings::AppSettings>(), viewing);
        self.chime(cx.global::<crate::settings::AppSettings>());
        self.poll_reading(cx);
        self.poll_follow_reading(cx);
        self.keep_warm(cx.global::<crate::settings::AppSettings>());
        for note in std::mem::take(&mut self.crew.notes) {
            self.say(note);
        }
        for id in &finished {
            let Some(j) = self.crew.store.job(*id).cloned() else {
                continue;
            };
            if self.place != Place::Live(*id) {
                // A job that changed files is waiting for review by now.
                let msg = match j.status {
                    Status::Done | Status::Review => format!(
                        "{} finished \u{201c}{}\u{201d}. {}",
                        j.bot.name(),
                        j.label(),
                        brain::tasks::facts(&j)
                    ),
                    _ => format!("{} stopped: \u{201c}{}\u{201d}.", j.bot.name(), j.label()),
                };
                self.say(msg);
            }
            // Squad Code is behind other windows: tell the person on the desktop.
            if !self.active.load(std::sync::atomic::Ordering::Relaxed) {
                let title = match j.status {
                    Status::Done | Status::Review => format!("{} finished", j.bot.name()),
                    _ => format!("{} stopped", j.bot.name()),
                };
                crate::settings::notify(
                    &title,
                    &format!("{}: {}", j.label(), brain::tasks::facts(&j)),
                );
            }
        }
    }

    /// Plays a chime when something new needs the person: a bot asking for an OK
    /// or a choice, a job that failed, or one finished or ready to review. Not for
    /// what is on screen in the focused window, which the person sees anyway.
    fn chime(&mut self, st: &crate::settings::AppSettings) {
        if self.crew.store.demo {
            return;
        }
        let active = self.active.load(std::sync::atomic::Ordering::Relaxed);
        let thread_here = match self.place {
            Place::Today => Some(self.crew.store.thread_here()),
            _ => None,
        };
        let on_screen = |job: Option<u64>, thread: u64| {
            active
                && (job.is_some_and(|id| self.place == Place::Live(id))
                    || (thread != 0 && thread_here == Some(thread)))
        };
        let mut now = std::collections::HashSet::new();
        let mut fresh = Vec::new();
        let mut note = |key: String, cue: crate::sounds::Cue, here: bool| {
            if !here && !self.heard.as_ref().is_none_or(|heard| heard.contains(&key)) {
                fresh.push(cue);
            }
            now.insert(key);
        };
        for j in &self.crew.store.jobs {
            let here = on_screen(Some(j.id), j.thread);
            if j.needs_you() {
                let cue = match j.status {
                    Status::Failed | Status::Interrupted => crate::sounds::Cue::Failed,
                    _ => crate::sounds::Cue::Done,
                };
                note(format!("job:{}:{}", j.id, j.status.label()), cue, here);
            }
            // A running bot asking for an OK.
            for r in self.crew.asks_for(j) {
                note(format!("ask:{}", r.id), crate::sounds::Cue::NeedsYou, here);
            }
        }
        // Peek asking a question back, with answers to pick.
        for t in &self.crew.store.talks {
            if t.answer.is_some() && !t.choices.is_empty() {
                note(
                    format!("talk:{}", t.id),
                    crate::sounds::Cue::NeedsYou,
                    on_screen(None, t.thread),
                );
            }
        }
        self.heard = Some(now);
        if let Some(cue) = crate::sounds::pick(fresh, st) {
            crate::sounds::play(cue, st);
        }
    }

    /// How a bot feels right now, from what it is really doing.
    fn mood(&self, who: Member) -> Mood {
        if self.thinking(who) {
            return Mood::Thinking;
        }
        if self.crew.store.busy(who).is_some() {
            return Mood::Working;
        }
        // The face follows the bot's latest update: its last finished job or
        // its last answer, whichever came later.
        let store = &self.crew.store;
        let job = store
            .jobs
            .iter()
            .filter(|j| j.bot == who && j.ended.is_some())
            .max_by_key(|j| j.ended);
        // Drawn every frame for each bot on screen: nothing is copied.
        let said = store
            .chat_lines(who)
            .filter(|m| !m.mine && !m.note)
            .map(|m| (m.at, m.text.as_str()))
            .chain(store.talks.iter().filter(|t| t.bot == who).filter_map(|t| {
                t.answer
                    .as_ref()
                    .filter(|a| !a.note)
                    .map(|a| (t.at, a.text.as_str()))
            }))
            .max_by_key(|(at, _)| *at);
        let job_at = job.and_then(|j| j.ended).unwrap_or(0);
        let fresh = |at: u64| store::now().saturating_sub(at) < 30 * 60;
        match (job, said) {
            (_, Some((at, text))) if at > job_at => {
                let mood = text_mood(text);
                if fresh(at) {
                    mood
                } else {
                    Mood::Signature
                }
            }
            (Some(j), _) => {
                let mood = job_mood(j);
                // What waits on you holds its face; good news fades.
                if j.needs_you() || fresh(job_at) {
                    mood
                } else {
                    Mood::Signature
                }
            }
            _ => Mood::Signature,
        }
    }

    /// A bot's face drawn still, in `mood`, for a message.
    fn face(&mut self, who: Member, mood: Mood, size: f32) -> Img {
        let image = self.holos.image(
            mascot::figure(who, mood),
            size,
            Framing::Bust,
            2.0,
            self.scale,
            false,
        );
        img(image).size(px(size)).flex_none()
    }

    /// One word for what a bot is doing, and its colour.
    fn status_of(&self, who: Member) -> (String, u32) {
        if self.thinking(who) {
            return ("answering".into(), who.accent());
        }
        if let Some(j) = self.crew.store.busy(who) {
            return (
                match j.progress() {
                    Some((d, n)) => format!("working {d}/{n}"),
                    None => "working".into(),
                },
                who.accent(),
            );
        }
        let mine = || self.crew.store.jobs.iter().filter(move |j| j.bot == who);
        if mine().any(|j| j.status == Status::Review) {
            return ("to review".into(), ui::AMBER);
        }
        if mine().any(|j| j.needs_you() && matches!(j.status, Status::Failed | Status::Interrupted))
        {
            return ("stopped".into(), ui::RED);
        }
        (who.role().to_lowercase(), ui::faint())
    }

    /// What a bot in the sidebar shows: its real state when it has one (answering,
    /// working, waiting on you, just done), else an idle of its own that changes
    /// now and then, so the crew doesn't move in lockstep. Hovered, it waves.
    // Unused since the split view (#15) replaced the sidebar crew list that
    // called it; kept for the Crew Chat sidebar to pick up.
    #[allow(dead_code)]
    fn side_mood(&self, who: Member) -> Mood {
        let mood = self.mood(who);
        if mood != Mood::Signature {
            return mood;
        }
        if self.hovered == Some(who) {
            return Mood::Signature;
        }
        idle_mood(who.key(), self.now(), self.touched.elapsed().as_secs())
    }

    /// Where `who`'s eyes look: always towards the pointer.
    fn gaze(&self, who: Member) -> (i8, i8) {
        let Some(b) = self.faces.borrow().get(&who).copied() else {
            return (0, 0);
        };
        gaze_towards(
            (f32::from(b.center().x), f32::from(b.center().y)),
            (f32::from(self.mouse.x), f32::from(self.mouse.y)),
        )
    }

    /// The pointer moved: redraw if any sidebar bot would now look elsewhere.
    fn pointer_moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let who: Vec<Member> = self.faces.borrow().keys().copied().collect();
        let before: Vec<_> = who.iter().map(|w| self.gaze(*w)).collect();
        self.mouse = at;
        if who.iter().map(|w| self.gaze(*w)).ne(before) {
            cx.notify();
        }
    }

    /// A bot drawn at `size` logical pixels, in `mood`.
    fn bot(&mut self, who: Member, mood: Mood, size: f32, framing: Framing) -> Img {
        let pose = mascot::figure(who, mood).shine(self.hovered == Some(who));
        let pose = self.eased(pose, (who, size.to_bits(), framing == Framing::Bust));
        self.pose_img(pose, size, framing)
    }

    /// `pose`, easing in from the last face shown at `slot` when its mood
    /// just changed.
    fn eased(&mut self, pose: Figure, slot: (Member, u32, bool)) -> Figure {
        let Some(state) = pose.fin_state() else {
            return pose;
        };
        let now = self.now();
        let (shown, was, since) = self.moods.entry(slot).or_insert((state, state, -1.0));
        if *shown != state {
            // Two faces drawn at one size share this slot. When the other one
            // just took it from this mood, leave it be: swapping back each
            // frame restarts the cross-fade, and the face flickers between
            // two bots drawn over each other.
            if *since >= 0.0 && now - *since < 0.5 && *was == state {
                return pose;
            }
            *was = *shown;
            *shown = state;
            *since = now;
        }
        if self.motion && *since >= 0.0 && now - *since < squad_holo::fin::MOOD_EASE {
            pose.easing_from(*was, *since)
        } else {
            pose
        }
    }

    fn hover_bot(&mut self, who: Member, on: bool, cx: &mut Context<Self>) {
        let was = self.hovered;
        if on {
            if was != Some(who) {
                self.hover_at = Instant::now();
            }
            self.hovered = Some(who);
        } else if self.hovered == Some(who) {
            self.hovered = None;
        }
        if was != self.hovered {
            cx.notify();
        }
    }

    fn pose_img(&mut self, pose: Figure, size: f32, framing: Framing) -> Img {
        // With motion off, or while dozing, every bot holds one calm pose.
        let moving = self.motion && !self.dozing.load(std::sync::atomic::Ordering::Relaxed);
        let t = if moving { self.now() } else { 2.0 };
        let image = self.holos.image(pose, size, framing, t, self.scale, moving);
        img(image).size(px(size)).flex_none()
    }

    /// A small avatar, like the ones beside chat lines: still, so it costs nothing.
    fn avatar(&mut self, who: Member, size: f32) -> Img {
        img(self.avatar_image(who, size)).size(px(size)).flex_none()
    }

    /// The still picture behind `avatar`, for views that draw it themselves,
    /// like a bot's hover card.
    fn avatar_image(&mut self, who: Member, size: f32) -> Arc<RenderImage> {
        self.holos.image(
            mascot::figure(who, Mood::Idle),
            size,
            Framing::Bust,
            2.0,
            self.scale,
            false,
        )
    }

    fn inbox_count(&self) -> usize {
        self.crew
            .store
            .listed()
            .into_iter()
            .filter(|j| j.needs_you())
            .count()
    }

    /// The bot whose chat was open last, else Fin.
    fn last_chat(&self) -> Member {
        Member::all()
            .into_iter()
            .max_by_key(|m| {
                (
                    self.crew.store.chat_lines(*m).next().is_some() as usize,
                    (*m == Member::Fin) as u8,
                )
            })
            .unwrap_or(Member::Fin)
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::framestats::begin();
        crate::pattern::clear_chrome();
        self.scale = window.scale_factor();
        self.view_w = f32::from(window.viewport_size().width);
        self.term_focused = self.term_focus.is_focused(window);
        if self.run_typing.is_some() && !self.run_focus.is_focused(window) {
            self.run_typing = None;
        }
        if let Some((text, pending, job)) = self.run_wanted.take() {
            if let Some(id) = pending {
                self.crew.store.talks.retain(|t| t.id != id);
            }
            let started = match job {
                Some(job) => self.run_app(Some(job), None, cx).is_some(),
                None => self.run_latest_app(text.clone(), window, cx),
            };
            if !started {
                self.crew.peek_says(
                    text,
                    "There's nothing here I know how to start yet. Want me to have Builder make it runnable?".into(),
                    vec!["Yes, make it runnable".into(), "Not now".into()],
                );
            }
        }
        if self.term_open.is_some() && window.is_window_active() {
            // The shell's output arrives on its own thread: keep drawing it.
            window.request_animation_frame();
        }
        self.poll(cx);
        let board_shown = match self.place {
            Place::Live(id) if self.ws_tab == peek::Tab::Design => Some(id),
            _ => None,
        };
        self.keep_boards(board_shown, window, cx);
        self.poll_maker(window, cx);
        data::set_here(self.crew.store.folder().map_or("", |f| f.path.as_str()));
        theme::follow_system(system_dark(window.appearance()));
        if self.applied_theme != look() {
            self.applied_theme = look();
            apply_theme(window, cx);
        }
        // A box that had focus may be gone (a new place opened): take focus back,
        // so shortcuts keep working without a click.
        // (Not just after focus moved on purpose: the new box may not be drawn yet.)
        if self.term_grab > 0 && self.term_open.is_some() {
            self.term_grab -= 1;
            self.focus_moved = Instant::now();
            self.term_focus.focus(window, cx);
        }
        if self.focus_moved.elapsed() > Duration::from_millis(300)
            && !self.focus.contains_focused(window, cx)
        {
            self.focus.focus(window, cx);
        }
        // What the last frame needed sets the clock for the next one.
        let busy = self.entered.elapsed() < Duration::from_millis(1000)
            || (self.palette && self.palette_at.elapsed() < Duration::from_millis(600))
            || self.inbox_at.elapsed() < Duration::from_millis(700)
            || self.chat_at.elapsed() < Duration::from_millis(500)
            || self.nav_at.elapsed() < Duration::from_millis(600)
            || motion::settling()
            || (self.hovered.is_some() && self.hover_at.elapsed() < Duration::from_millis(400))
            || self
                .toast
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < Duration::from_millis(3600));
        // Nobody at the keyboard: the crew slows down, then nearly stops.
        let idle = self.touched.elapsed();
        let working = self.crew.busy();
        // While something on screen is gliding, draw every frame the display
        // shows (60 or 120 a second), not on the crew's slower clock.
        if busy && self.motion {
            window.request_animation_frame();
        }
        let want = if busy {
            FPS as u32
        } else if working && idle < Duration::from_secs(20) {
            // Progress sweeps and breathing dots stay smooth while you watch.
            FPS as u32
        } else if working {
            self.holos.asked.max(4)
        } else {
            self.holos.asked.max(2)
        };
        let want = if idle > Duration::from_secs(120) {
            want.min(15)
        } else if idle > Duration::from_secs(20) {
            want.min(30)
        } else {
            want
        };
        // Bots moving at full rate follow the display's own refresh, so every
        // frame lands on a vsync instead of a timer that drifts against it.
        if self.motion && want >= FPS as u32 && window.is_window_active() {
            window.request_animation_frame();
        }
        // The first frame on screen takes focus: asking before the window is
        // up is ignored on recent macOS.
        static FRONT: std::sync::Once = std::sync::Once::new();
        FRONT.call_once(|| {
            window.activate_window();
            cx.activate(true);
        });
        self.rate.store(want, std::sync::atomic::Ordering::Relaxed);
        // Five minutes with nobody around and nothing running: doze.
        let doze = self.motion
            && idle > doze_after()
            && !working
            && !self.palette
            // The server's page and its screen keep reading the server: their
            // news only shows on a redraw, so they keep the app awake.
            && !matches!(self.place, Place::Server | Place::Screen | Place::Browser(_))
            && self
                .toast
                .as_ref()
                .is_none_or(|(_, at)| at.elapsed() > Duration::from_secs(4));
        self.dozing
            .store(doze, std::sync::atomic::Ordering::Relaxed);
        self.holos.begin(window, cx);
        // Settings → Animations, and Reduce motion in macOS, both calm everything.
        self.motion =
            cx.global::<crate::settings::AppSettings>().animations && !crate::motion::calm(cx);
        motion::begin(self.motion);
        mascot::sync(cx.global::<crate::settings::AppSettings>());
        let body = match self.place {
            Place::Today => self.channel(cx),
            Place::Live(id) => self.workspace(id, window, cx),
            Place::Race(id) => self.race(id, cx),
            Place::Goal(id) => self.goal_page(id, cx),
            Place::Crew => {
                let teach = self.crew_pick.map(|who| {
                    let here = self
                        .crew
                        .store
                        .folder()
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| "this project".into());
                    format!("Teach {} something about {here}\u{2026}", who.name())
                });
                if let Some(t) = teach.filter(|t| self.memory_hint.as_ref() != Some(t)) {
                    self.memory_hint = Some(t.clone());
                    self.memory_input
                        .update(cx, |i, cx| i.set_placeholder(t, window, cx));
                }
                self.crew(cx)
            }
            Place::Chat(who) => {
                if self.chat_hint != Some(who) {
                    self.chat_hint = Some(who);
                    let hint = format!("Message {}\u{2026}", who.name());
                    self.chat_input
                        .update(cx, |i, cx| i.set_placeholder(hint, window, cx));
                }
                self.chat(who, cx)
            }
            Place::Inbox => self.inbox(cx),
            Place::History => self.history(cx),
            Place::Overview => self.overview(cx),
            Place::Settings(pane) => self.settings(pane, cx),
            Place::Server => self.machines(cx),
            Place::Screen => self.screen_page(window, cx),
            // A job's browser is a tab of its workspace.
            Place::Browser(id) => {
                self.ws_tab = peek::Tab::Browser;
                self.workspace(id, window, cx)
            }
            Place::Welcome => self.welcome(cx),
        };
        // Each place eases in: a short fade and a lift of a few pixels.
        let k = motion::ease(self.entered, motion::GENTLE, 0.0);
        let body = div()
            .size_full()
            .opacity(0.35 + 0.65 * k)
            .relative()
            .top(px(10. * (1. - k)))
            .child(body);
        self.active.store(
            window.is_window_active(),
            std::sync::atomic::Ordering::Relaxed,
        );
        let toast = self
            .toast
            .clone()
            .filter(|(_, at)| at.elapsed() < Duration::from_millis(3500));
        let toast_el = toast.map(|(text, at)| {
            let k = motion::ease(at, motion::BASE, 0.0);
            let fade = ((3.5 - at.elapsed().as_secs_f32()) / 0.4).clamp(0.0, 1.0);
            div()
                .absolute()
                .bottom(px(28. + 10. * (1. - k)))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .opacity(k.min(fade))
                .child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .rounded_full()
                        .bg(c(ui::fill()))
                        .text_color(c(ui::on_fill()))
                        .text_size(px(13.))
                        .child(text),
                )
        });
        let setup = self.place == Place::Welcome;
        let project_drawer = self.project_drawer(cx);
        let root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                this.wake(cx);
                this.pointer_moved(e.position, cx);
            }))
            .capture_key_down(cx.listener(|this, _: &KeyDownEvent, _, cx| {
                this.wake(cx);
            }))
            // Drop a folder (or a file in it) anywhere on the window to work in it.
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(ca(ui::ORANGE, 0.06)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.drop_paths(paths.paths(), cx);
            }))
            // ⌘1…⌘6 (Ctrl on Linux and Windows) jump between places, even from a text box;
            // ⌘K opens the palette and ⌘[ goes back.
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let m = &e.keystroke.modifiers;
                let key = e.keystroke.key.as_str();
                if this.menu.is_some() && !m.platform && !m.control {
                    if this.menu_key(key, window, cx) {
                        cx.stop_propagation();
                    }
                    return;
                }
                if (this.deleting.is_some() || this.removing.is_some() || this.gone.is_some())
                    && key == "escape"
                {
                    this.deleting = None;
                    this.removing = None;
                    if !this.gone.as_ref().is_some_and(|g| g.busy) {
                        this.gone = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if this.palette && !m.platform && !m.control {
                    if this.palette_key(key, window, cx) {
                        cx.stop_propagation();
                    }
                    return;
                }
                if this.viewer.is_some() && key == "escape" {
                    this.close_file(cx);
                    cx.stop_propagation();
                    return;
                }
                // Ctrl+` opens and closes a terminal: the job's own on a job,
                // the project's everywhere else (or when that one is open).
                if m.control && key == "`" && this.place != Place::Welcome {
                    match this.place {
                        Place::Live(id) if !this.term_open.is_some_and(term::is_shared) => {
                            this.toggle_term(id, window, cx)
                        }
                        Place::Server if this.crew.link.is_some() => {
                            this.toggle_server_term(window, cx)
                        }
                        _ => this.toggle_project_term(window, cx),
                    }
                    cx.stop_propagation();
                    return;
                }
                // Control keys belong to the shell while the terminal has focus.
                if this.term_focus.is_focused(window) && !m.platform {
                    return;
                }
                // Alt+Up and Alt+Down step through the threads (with Shift,
                // only the unread ones), like Slack.
                if m.alt
                    && !(m.platform || m.control)
                    && matches!(key, "up" | "down")
                    && this.place != Place::Welcome
                    && this.step_thread(key == "down", m.shift, cx)
                {
                    cx.stop_propagation();
                    return;
                }
                // Needs you works from the keyboard while no box has focus.
                if this.place == Place::Inbox
                    && !(m.platform || m.control || m.alt || m.shift)
                    && this.focus.is_focused(window)
                    && this.inbox_key(key, cx)
                {
                    cx.stop_propagation();
                    return;
                }
                if !(m.platform || m.control) || m.shift || m.alt {
                    return;
                }
                match key {
                    "k" => {
                        if this.palette {
                            this.close_palette(window, cx);
                        } else {
                            this.open_palette(window, cx);
                        }
                        cx.stop_propagation();
                        return;
                    }
                    "[" => {
                        this.palette = false;
                        this.back(cx);
                        cx.stop_propagation();
                        return;
                    }
                    _ => {}
                }
                this.palette = false;
                let to = match key {
                    "1" => Place::Today,
                    "2" => Place::Inbox,
                    "3" => Place::History,
                    "4" => Place::Chat(this.last_chat()),
                    "5" => Place::Crew,
                    "6" => Place::Settings(this.settings_pane),
                    "7" => Place::Settings(Pane::Usage),
                    _ => return,
                };
                this.go(to, cx);
                cx.stop_propagation();
            }))
            .font_family("Geist")
            .text_color(c(ui::ink()))
            // The ground the sidebar sits on; the page floats over it as a canvas.
            // When the desktop shows through, the sidebar and the page each paint
            // their own ground instead: stacked, two see-through layers would add
            // up to nearly solid.
            .when(!theme::layered(), |d| {
                d.bg(c(if setup { ui::bg() } else { ui::ground() }))
            })
            .when(theme::layered() && setup, |d| d.bg(c(ui::bg())))
            // The title bar runs across the whole window, the Mac window buttons
            // at its left end; the walkthrough has the window to itself.
            .when(!setup, |d| d.child(self.topbar(cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    // The sidebar comes after the walkthrough.
                    .when(!setup, |d| d.child(self.sidebar(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            // Full height on its own; as a canvas, it stretches between its margins.
                            .when(setup, |d| d.h_full())
                            .when(!setup, |d| d.bg(c(ui::bg())))
                            .relative()
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .relative()
                                    .overflow_hidden()
                                    // The pattern lies under the page only, not the top bar:
                                    // the bar is a plain strip of the page's own ground, so
                                    // the pattern never runs into the bar's edge and no grid
                                    // line lands a few pixels off it.
                                    .when(!setup, |d| {
                                        d.children(ui::pattern(crate::pattern::Area::Content))
                                    })
                                    .child(div().size_full().child(body))
                                    .children(self.file_view(cx)),
                            )
                            .when(!setup, |d| d.children(project_drawer))
                            .when(!setup, |d| d.child(self.statusbar(cx))),
                    ),
            )
            .children(toast_el)
            // Deferred, so it paints over the crew's frames too.
            .when(self.usage_open && !setup, |d| {
                // Positioned after the motion: it makes what it moves
                // relative, and this has to stay over the window.
                d.child(
                    motion::panel(
                        div().inset_0().child(self.usage_popover(cx)),
                        motion::key("usage-popover"),
                        0.,
                        6.,
                    )
                    .absolute(),
                )
            })
            .when(self.palette, |d| d.child(self.palette_view(cx)))
            .children(self.delete_dialog(cx).map(|d| dialog(d, "delete")))
            .children(self.remove_dialog(cx).map(|d| dialog(d, "remove")))
            .children(self.gone_dialog(cx).map(|d| dialog(d, "gone")))
            .children(self.menu_view(cx))
            // Painted last: with SQUADCODE_FRAME_STATS set, the frame's timing is noted.
            .child(
                canvas(|_, _, _| (), |_, _, _, _| crate::framestats::painted())
                    .absolute()
                    .size_0(),
            );
        // Anything that started easing while the page was built (a hover, a
        // panel coming in) gets its next frame straight away.
        if motion::settling() {
            window.request_animation_frame();
        }
        motion::end();
        crate::framestats::rendered();
        // A stand-in desktop under a see-through window, for screenshots taken where
        // the window can't really be see-through (Xvfb): SQUADCODE_BACKDROP names an image.
        // A background image, under Settings > Appearance, lies under everything.
        match wallpaper::layer().or_else(backdrop) {
            Some(desk) => div()
                .size_full()
                .relative()
                .child(desk)
                .child(div().absolute().inset_0().flex().child(root))
                .into_any_element(),
            None => root.into_any_element(),
        }
    }
}

fn backdrop() -> Option<AnyElement> {
    let path = std::env::var_os("SQUADCODE_BACKDROP")?;
    theme::window_clear().then(|| {
        img(std::path::PathBuf::from(path))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element()
    })
}

/// What the window looks like: the theme, whether transparency is on, and how
/// see-through it is.
type Look = (&'static str, bool, u8, bool, u8, bool);

fn look() -> Look {
    (
        ui::theme().id,
        theme::see_through(),
        (theme::amount() * 100.).round() as u8,
        theme::glass_popovers(),
        (theme::pop_amount() * 100.).round() as u8,
        theme::image_on(),
    )
}

/// The window's background for the theme in use. Solid themes keep it opaque: the
/// app paints every pixel, and blending what's behind the window costs every frame.
/// Frosted themes, and any theme with window transparency on, let the desktop show
/// through, blurred (vibrancy on macOS, acrylic on Windows, the compositor's blur on
/// Linux; without one the window stays solid).
///
/// A background image covers the desktop, so the window stays opaque under it.
fn background() -> WindowBackgroundAppearance {
    if theme::window_clear() && !theme::image_on() {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// A project's open issues, as last read.
pub(crate) struct IssueList {
    pub folder: String,
    pub at: Instant,
    pub loading: bool,
    pub items: Vec<issues::Listed>,
    pub error: Option<String>,
}

/// The system is using a dark appearance.
pub(crate) fn system_dark(a: gpui::WindowAppearance) -> bool {
    matches!(
        a,
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
    )
}

/// Puts the theme in use on the window and on the kit's text boxes and menus.
fn apply_theme(window: &mut Window, cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let t = ui::theme();
    window.set_background_appearance(background());
    Theme::change(
        if t.light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        Some(window),
        cx,
    );
    Theme::update(cx, |k| {
        k.background = ui::c(ui::bg());
        k.foreground = ui::c(ui::ink());
        k.border = ui::c(ui::line());
        k.input = ui::c(ui::line());
        k.muted_foreground = ui::c(ui::muted());
        k.popover = ui::c(ui::float());
        k.popover_foreground = ui::c(ui::ink());
        k.caret = ui::c(ui::ink());
        k.ring = ui::ca(ui::VIOLET, 0.6);
        k.selection = ui::ca(ui::VIOLET, if t.light { 0.22 } else { 0.38 });
    });
    window.refresh();
}

/// Opens the v2 window.
pub fn open_window(cx: &mut App) {
    // The look first: the window opens with the background it needs.
    let st = cx.global::<crate::settings::AppSettings>();
    theme::set_pref(&st.v2_theme, system_dark(cx.window_appearance()));
    issues::set_close_on_merge(st.v2_close_issues);
    work::set_belt(st.v2_belt_secrets, st.v2_belt_checks);
    theme::set_see_through(st.v2_see_through, st.v2_clear);
    theme::set_glass_popovers(st.v2_glass_popovers, st.v2_pop_clear);
    theme::set_image(!st.v2_bg_image.trim().is_empty(), st.v2_bg_show);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1440.), px(900.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Squad Code".into()),
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(LIGHTS_X), px(LIGHTS_Y))),
        }),
        app_id: Some("squadcode".into()),
        window_background: background(),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        apply_theme(window, cx);
        let shell = cx.new(|cx| Shell::new(window, cx));
        // The menu bar panel reads and acts on the crew through this.
        cx.set_global(tray::Here(shell.downgrade()));
        shell
    });
    // Come to the front when opened, not behind whatever had focus (the first
    // frame asks again, once the window is showing).
    cx.activate(true);
    if let Ok((handle, _)) = opened {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        cx.set_global(OpenWindow(handle));
    }
    cx.activate(true);
}

/// An idle bot's mood for the moment: mostly hovering, now and then a wave,
/// and dozing off once the person has been away a while. Each bot keeps its own
/// rhythm, picked from `seed`.
#[allow(dead_code)]
fn idle_mood(seed: u64, now: f64, away_secs: u64) -> Mood {
    if away_secs > 120 {
        return Mood::Asleep;
    }
    let span = 6.0 + (seed % 5) as f64;
    let slot = ((now + seed as f64 * 1.9) / span) as u64;
    let mut h =
        (seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ slot).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    match h % 10 {
        0..=6 => Mood::Idle,
        _ => Mood::Signature,
    }
}

/// Which way eyes at `eye` look to find the pointer at `at` (window pixels), in
/// tenths each way. The eyes always point along the line to the pointer; how
/// far they turn grows quickly near the face and eases out further away, the
/// way real eyes swing most of the way round and then hold.
fn gaze_towards(eye: (f32, f32), at: (f32, f32)) -> (i8, i8) {
    let (dx, dy) = (at.0 - eye.0, at.1 - eye.1);
    let dist = dx.hypot(dy);
    if dist < 1.0 {
        return (0, 0);
    }
    let reach = 1.0 - (-dist / 90.0).exp();
    let step = |d: f32| (d / dist * reach * 10.0).round().clamp(-10.0, 10.0) as i8;
    (step(dx), step(dy))
}

/// The Squad Code window, once one has been opened.
struct OpenWindow(AnyWindowHandle);

impl Global for OpenWindow {}

/// Brings the Squad Code window forward, opening a new one if it was closed.
/// The Squad Code window, while it's open.
pub fn open_handle(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<OpenWindow>().map(|w| w.0)
}

pub fn to_front(cx: &mut App) {
    let shown = cx.try_global::<OpenWindow>().map(|w| w.0).is_some_and(|w| {
        w.update(cx, |_, window, _| window.activate_window())
            .is_ok()
    });
    if !shown {
        open_window(cx);
    }
}

#[cfg(test)]
mod side_tests {
    use super::gaze_towards;

    #[test]
    fn eyes_follow_the_pointer_wherever_it_is() {
        assert_eq!(gaze_towards((100., 100.), (180., 100.)), (6, 0));
        assert_eq!(gaze_towards((100., 100.), (100., 72.)), (0, -3));
        assert_eq!(gaze_towards((100., 100.), (900., 100.)), (10, 0));
        // Along the diagonal both axes turn alike.
        let (x, y) = gaze_towards((100., 100.), (400., 400.));
        assert!(x == y && x > 5, "{x} {y}");
    }
}

#[cfg(test)]
mod drop_tests {
    use super::{and_list, folders_from};

    #[test]
    fn names_read_as_a_list() {
        assert_eq!(and_list(&[]), "");
        assert_eq!(and_list(&["Fixer"]), "Fixer");
        assert_eq!(and_list(&["Fixer", "Checker"]), "Fixer and Checker");
        assert_eq!(and_list(&["A", "B", "C"]), "A, B and C");
    }

    #[test]
    fn dropped_files_mean_their_folder() {
        let dir = std::env::temp_dir().join(format!("squadcode-drop-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let got = folders_from(&[
            dir.join("a.txt"),
            dir.clone(),
            dir.join("sub"),
            dir.join("missing/nope.txt"),
        ]);
        assert_eq!(got, vec![dir.clone(), dir.join("sub")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The first crew bot named with @ in a message, and the message without it.
fn mentioned(text: &str) -> Option<(Member, String)> {
    let crew = Member::all();
    let mut found = None;
    let mut rest = Vec::new();
    for word in text.split_whitespace() {
        let name = word
            .strip_prefix('@')
            .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()));
        match name.and_then(|n| crew.iter().find(|m| m.name().eq_ignore_ascii_case(n))) {
            Some(m) if found.is_none() => found = Some(*m),
            _ => rest.push(word),
        }
    }
    found.map(|m| (m, rest.join(" ")))
}

/// The face for a job's latest news: busy while it runs, pleased when it
/// finished well, concerned when it failed or its report reads badly.
pub(super) fn job_mood(j: &store::Job) -> Mood {
    match j.status {
        Status::Running | Status::Shipping => Mood::Working,
        Status::Failed | Status::Interrupted => Mood::NeedsYou,
        Status::Review | Status::Done | Status::Shipped => match text_mood(&j.summary) {
            Mood::NeedsYou => Mood::NeedsYou,
            _ => Mood::Done,
        },
        Status::Stopped | Status::Discarded => Mood::Signature,
    }
}

/// The face for what a bot said: pleased at good news, concerned when
/// something's wrong or it's asking you, else its own look.
pub(super) fn text_mood(text: &str) -> Mood {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&[
        "good news",
        "all good",
        "nothing to fix",
        "no issues",
        "looks good",
    ]) {
        return Mood::Done;
    }
    if has(&[
        "fail",
        "broke",
        "error",
        "crash",
        "couldn't",
        "could not",
        "found a bug",
        "problem",
        "blocked",
        "went wrong",
        "doesn't work",
        "does not work",
    ]) {
        return Mood::NeedsYou;
    }
    if t.trim_end().ends_with('?') {
        return Mood::NeedsYou;
    }
    if has(&[
        "done", "fixed", "passes", "passed", "works", "finished", "shipped", "ready",
    ]) {
        return Mood::Done;
    }
    Mood::Signature
}

#[cfg(test)]
mod mood_tests {
    use super::text_mood;
    use squad_holo::Mood;

    #[test]
    fn a_face_matches_the_news() {
        assert_eq!(
            text_mood("Good news: the counter can't go negative, so job #8 has nothing to fix."),
            Mood::Done
        );
        assert_eq!(text_mood("The login test fails on Safari."), Mood::NeedsYou);
        assert_eq!(text_mood("Should I keep the old API?"), Mood::NeedsYou);
        assert_eq!(text_mood("Fixed it. All tests passed."), Mood::Done);
        assert_eq!(text_mood("The app has three screens."), Mood::Signature);
    }
}

/// A dialog over the window, fading in with its scrim.
fn dialog(el: AnyElement, name: &'static str) -> Div {
    // Absolute after the motion, which makes what it moves relative: a
    // relative dialog would be laid out beside the page, off the window.
    motion::panel(
        div().inset_0().child(el),
        motion::key(("dialog", name)),
        0.,
        0.,
    )
    .absolute()
}
