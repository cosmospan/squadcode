//! What v2 keeps between launches: the folders the crew works in, every job
//! and what happened in it, and each bot's chat. Saved as JSON in Squad Code's data
//! folder (`~/.squadcode/v2.json`), apart from v1's state.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::data::{Member, Mode};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    /// The shape this file was saved in ([`FORMAT`]). A file from a newer Squad
    /// Code is copied aside before this one saves over it.
    pub format: u32,
    pub folders: Vec<Folder>,
    /// The folder new jobs and questions go to, by index into `folders`.
    pub current: usize,
    /// A job this build can't read (saved by a newer one) is left out, and the
    /// file is copied aside first: see [`lenient`].
    #[serde(deserialize_with = "lenient")]
    pub jobs: Vec<Job>,
    /// Saved as a list of pairs: a made bot can't be a JSON object key.
    #[serde(with = "chat_list")]
    pub chats: HashMap<Member, Vec<Said>>,
    pub next_id: u64,
    /// Bots the person made in the maker on Crew.
    pub made: Vec<super::data::Made>,
    /// Built-in bots removed from the crew (or never added).
    #[serde(alias = "hidden")]
    pub removed: Vec<Member>,
    /// The first-run walkthrough is done (or was skipped).
    pub set_up: bool,
    /// Bots pinned near the top of the sidebar.
    pub pinned: Vec<Member>,
    /// Goals split into jobs.
    pub goals: Vec<super::goal::Goal>,
    /// Getting-started steps done that leave no other trace (opening the
    /// terminal); the rest are read from the jobs themselves.
    pub steps: Vec<Step>,
    /// The getting-started card was put away.
    pub guide_hidden: bool,
    /// The sidebar's Try list was put away.
    pub try_hidden: bool,
    /// Bot ideas Fin already offered (`brain::ideas`), so each is offered once.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub offered: Vec<String>,
    /// The # threads in each project: group chats with a few bots around one
    /// piece of work. Every project also has #general, which isn't stored.
    pub threads: Vec<Thread>,
    /// The thread new work goes to, by id; 0 is the project's #general.
    pub thread_now: u64,
    /// When you last looked at each thread, by "folder#id", for unread dots.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub seen_at: HashMap<String, u64>,
    /// Reactions you put on messages in threads, by "job-<id>" or "talk-<id>".
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub reactions: HashMap<String, Vec<String>>,
    /// Questions asked in threads, with the answers that came back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "lenient")]
    pub talks: Vec<Talk>,
    /// Each thread's running summary, which bots read (`brain::summary`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub summaries: Vec<super::brain::summary::Summary>,
    /// What each bot can do beyond its kind of work (instructions, skill files,
    /// connected apps, permissions, memory), by `engine::bot_key`.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub setups: HashMap<String, super::setup::Setup>,
    /// Jobs waiting to start: at a time you picked (the night shift), or
    /// once their bot is free.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub later: Vec<super::shift::Later>,
    /// What bots learned from your send-backs, each for one project.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lessons: Vec<super::learn::Lesson>,
    /// A made-up store for looking at every screen (SQUADCODE_DEMO): never saved.
    #[serde(skip)]
    pub demo: bool,
    /// Saves wait for the window's next [`Store::flush`] instead of writing
    /// right away: set by the window, which flushes on every tick and on quit.
    #[serde(skip)]
    pub deferred: bool,
    /// A save was asked for and hasn't been written yet.
    #[serde(skip)]
    pub dirty: std::cell::Cell<bool>,
    #[serde(skip)]
    pub flushed_at: std::cell::Cell<Option<std::time::Instant>>,
}

/// A question asked in a thread and the bot's answer, kept in the thread.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Talk {
    pub id: u64,
    /// The project's folder path.
    pub folder: String,
    pub thread: u64,
    pub bot: Member,
    pub question: String,
    /// None while the bot is still reading the code.
    pub answer: Option<Said>,
    pub at: u64,
    /// Quick answers Peek offers with a question back, as buttons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl Default for Talk {
    fn default() -> Self {
        Talk {
            id: 0,
            folder: String::new(),
            thread: 0,
            bot: Member::Explainer,
            question: String::new(),
            answer: None,
            at: 0,
            choices: Vec::new(),
        }
    }
}

/// A # thread inside a project: a group chat with some bots, around one piece
/// of work. Jobs and plans started in it stay in it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Thread {
    pub id: u64,
    pub name: String,
    /// The project's folder path.
    pub folder: String,
    /// The bots in it besides Peek, who is in every thread.
    pub bots: Vec<Member>,
    pub archived: bool,
    pub made: u64,
}

/// A thread name as it reads after the #: lower case, words joined by dashes.
pub fn thread_name(raw: &str) -> String {
    let mut out = String::new();
    for w in raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&w.to_lowercase());
    }
    out.chars().take(40).collect()
}

/// The getting-started list, in the order it's shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Engine,
    Folder,
    Ask,
    Terminal,
    Race,
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Engine,
        Step::Folder,
        Step::Ask,
        Step::Terminal,
        Step::Race,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Step::Engine => "Connect an engine",
            Step::Folder => "Pick a folder",
            Step::Ask => "Give the crew a first job",
            Step::Terminal => "Open the terminal",
            Step::Race => "Try a race",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub name: String,
    pub path: String,
    /// What Approve does here: "pr", "merge", or "" to decide from the remote.
    pub ship: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Running,
    Done,
    Failed,
    Stopped,
    /// Squad Code closed while it ran.
    Interrupted,
    /// It changed files in its own worktree and waits for you to approve them.
    Review,
    /// Approved in a GitHub project: Shipper has (or is opening) a pull request
    /// and watches it.
    Shipping,
    /// Approved and landed: merged here, or its pull request merged.
    Shipped,
    /// You threw its changes away.
    Discarded,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Running => "Working",
            Status::Done => "Done",
            Status::Failed => "Stopped with a problem",
            Status::Stopped => "Stopped by you",
            Status::Interrupted => "Interrupted",
            Status::Review => "Ready for review",
            Status::Shipping => "Pull request open",
            Status::Shipped => "Shipped",
            Status::Discarded => "Discarded",
        }
    }
}

/// One line in a job's feed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    /// What the bot said.
    Text(String),
    /// A step it took: "Read src/main.rs", "Run cargo test". `state`: 0 running,
    /// 1 done, 2 failed.
    Tool {
        id: String,
        label: String,
        kind: String,
        state: u8,
    },
    /// What the person added.
    You(String),
    /// Squad Code speaking: a hint or a warning.
    Note(String),
    /// Another bot on the crew speaking about this job, such as Shipper
    /// reporting a step.
    By(Member, String),
    Error(String),
    /// A picture the bot showed: a screenshot, a mockup or a diagram. `file` is its
    /// copy in Squad Code's own folder, so it stays after the job's worktree goes.
    Picture {
        file: String,
        title: String,
        caption: String,
    },
    /// Two to four looks for the person to pick from. `pick` is the one the bot
    /// recommends; `chosen` the one the person picked, once they have.
    Directions {
        title: String,
        options: Vec<Direction>,
        pick: usize,
        chosen: Option<usize>,
    },
}

/// One look in a [`Item::Directions`] card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    pub name: String,
    /// Its picture, kept in Squad Code's own folder.
    pub file: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub id: u64,
    /// The # thread it was started in; 0 is the project's #general.
    pub thread: u64,
    pub bot: Member,
    pub mode: Mode,
    /// What the person typed: the bot's instructions.
    pub title: String,
    /// Peek's line on why it brought this bot in; empty when you picked the bot.
    pub handoff: String,
    /// Peek's short title for the card, when Smart Peek is on; empty before.
    pub short: String,
    /// Peek's sentence or two on how it went, once it ended.
    pub recap: String,
    /// Times its failing checks were sent back to its bot to fix.
    pub fix_turns: u8,
    pub folder: Folder,
    /// "Claude Code · Sonnet 4.5": what ran it.
    pub engine: String,
    pub agent: String,
    pub started: u64,
    pub ended: Option<u64>,
    /// Seconds its finished turns actually ran. Follow-ups move `ended`
    /// on, so `ended - started` would count the waiting in between too.
    pub worked: u64,
    /// When the turn running now began; 0 when none is.
    pub turn: u64,
    /// Where the latest turn's items start in the feed.
    pub turn_from: usize,
    /// Other jobs' worktrees it may read: unmerged threads it was pointed at.
    pub reads: Vec<String>,
    pub status: Status,
    /// Saved in its own file (`jobs/<id>.json`), not in v2.json: see
    /// [`Store::save`]. Read from v2.json too, where builds before that kept it.
    #[serde(deserialize_with = "lenient", skip_serializing_if = "feeds_apart")]
    pub feed: Vec<Item>,
    /// The agent's own todo list: (step, 0 todo / 1 doing / 2 done).
    pub plan: Vec<(String, u8)>,
    /// Files it changed.
    pub files: Vec<String>,
    /// "12s · 3 turns · $0.04".
    pub summary: String,
    /// Tokens the agent reported for its turns, read and written.
    pub tokens: u64,
    /// The agent's conversation id, so a follow-up continues it.
    pub conversation: String,
    /// The person has opened it since it finished.
    pub seen: bool,
    /// The branch and worktree a writing job works in, apart from your own
    /// checkout, and the branch it started from. Empty when it worked in the folder.
    pub branch: String,
    pub worktree: String,
    pub base: String,
    /// Where `base` was when the job started, for its diff.
    pub base_sha: String,
    /// What it changed, once it finished.
    pub changes: Vec<Changed>,
    /// Checker's run of the project's checks on those changes.
    pub checks: Vec<crate::proof::CheckRun>,
    /// Checker is running them now.
    pub checking: bool,
    /// What happened when it was approved: "Merged into main as 3f2a1c9", or the
    /// pull request's address.
    pub shipped: String,
    /// Its pull request's address, once Shipper opened one.
    pub pr: String,
    pub pr_number: u64,
    /// "Checks running: 3 of 5 done", from the last look at the pull request.
    pub pr_line: String,
    /// OPEN, MERGED or CLOSED.
    pub pr_state: String,
    /// Failed checks already reported, so each failure is told once.
    pub pr_failed: Vec<String>,
    /// Merge the pull request as soon as its checks pass.
    pub auto_merge: bool,
    /// Shipper is pushing and opening the pull request now.
    pub shipping: bool,
    /// The race this job is one try in: the id of its first try. 0 when it
    /// isn't racing.
    pub race: u64,
    /// The engine this try runs on, when a race picked it: an agent id and a
    /// model ("" for the agent's own). Empty means the bot's engine.
    pub pick: String,
    pub pick_model: String,
    /// Files that clash with the base branch, when approving it stopped on them.
    pub conflict: Vec<String>,
    /// The goal this job is one part of, and what the bot is told about it.
    pub goal: u64,
    pub context: String,
    /// The server running it, when it was sent to one ("" when it runs here),
    /// and its id there (0 while it's on its way).
    pub remote_on: String,
    pub remote: u64,
    /// Its worktree on the server, so its work can be brought home.
    pub remote_dir: String,
    /// What the seatbelt found when Approve was pressed, one line each
    /// ("src/config.ts:14 · Looks like a Stripe key"). Approving is held while
    /// any of them is serious, until the person says to go ahead anyway.
    pub held: Vec<String>,
    /// The person said to push or merge in spite of the seatbelt.
    pub belt_ok: bool,
    /// Files with your uncommitted changes in the checkout of its base that
    /// were set aside to merge it and clash with what it brought in.
    pub in_the_way: Vec<String>,
    /// The stash commit holding those changes until you bring them back or
    /// leave them there; empty when nothing is waiting.
    pub kept: String,
    /// Set by builds that asked before setting changes aside; merging now
    /// always does. Read and ignored.
    pub set_aside: bool,
    /// Commits on its branch since it started, and commits its base got since.
    pub ahead: usize,
    pub behind: usize,
    /// Other jobs in the same project whose work clashes with this one's:
    /// their id and where ("total.ts around line 40").
    pub clash_with: Vec<(u64, String)>,
    /// A lesson the crew's engine found in a send-back, waiting for the person
    /// to keep it, keep it for the whole crew, or let it go. Empty when none.
    pub lesson_offer: String,
    /// Checks the person said to ignore on this job: they fail on their own,
    /// not because of it. By command.
    pub ignored_checks: Vec<String>,
    /// Merge it the moment every check passes: approved ahead of the checks.
    pub merge_on_green: bool,
    /// Where its files stood before each turn, so it can be rewound to any of
    /// them (see `rewind`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<super::rewind::Mark>,
    /// It was rewound and its engine's conversation dropped: the next turn is
    /// told what's still true.
    pub rewound: bool,
    /// When it carries on by itself after its engine's plan resets; 0 when
    /// it isn't waiting.
    pub resume_at: u64,
    /// What the person said while it was working, sent as its next turn once
    /// this one ends.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub queued: Vec<String>,
    /// Times it was picked up again after running out of plan.
    pub resumes: u8,
    /// It started on the night shift: when its engine runs out, another
    /// engine may take over rather than waiting.
    pub shift: bool,
    /// The issue it's for, as the line its pull request closes it with
    /// ("Closes #12", "Fixes ENG-42"), and the issue's address.
    pub issue: String,
    pub issue_url: String,
    /// The newest review feedback on its pull request already handed to its
    /// bot, as GitHub's time.
    pub pr_seen: String,
    /// Its bot is answering review comments on the open pull request: when
    /// the turn ends, Shipper pushes the fix to the same pull request.
    pub answering: bool,
}

/// One file a job changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Changed {
    pub path: String,
    /// Git's letter: A added, M modified, D deleted.
    pub status: String,
    pub added: usize,
    pub removed: usize,
}

impl Job {
    /// The name lists and notices show: Peek's short title when there is one,
    /// else the first line of what the person typed.
    pub fn label(&self) -> &str {
        if self.short.is_empty() {
            self.title.lines().next().unwrap_or("").trim()
        } else {
            &self.short
        }
    }
}

impl Default for Job {
    fn default() -> Self {
        Job {
            id: 0,
            thread: 0,
            handoff: String::new(),
            bot: Member::Builder,
            mode: Mode::Build,
            title: String::new(),
            short: String::new(),
            recap: String::new(),
            fix_turns: 0,
            folder: Folder::default(),
            engine: String::new(),
            agent: String::new(),
            started: 0,
            ended: None,
            worked: 0,
            turn: 0,
            turn_from: 0,
            reads: Vec::new(),
            status: Status::Done,
            feed: Vec::new(),
            plan: Vec::new(),
            files: Vec::new(),
            summary: String::new(),
            tokens: 0,
            conversation: String::new(),
            seen: true,
            branch: String::new(),
            worktree: String::new(),
            base: String::new(),
            base_sha: String::new(),
            changes: Vec::new(),
            checks: Vec::new(),
            checking: false,
            shipped: String::new(),
            pr: String::new(),
            pr_number: 0,
            pr_line: String::new(),
            pr_state: String::new(),
            pr_failed: Vec::new(),
            auto_merge: true,
            shipping: false,
            race: 0,
            pick: String::new(),
            pick_model: String::new(),
            conflict: Vec::new(),
            goal: 0,
            context: String::new(),
            remote_on: String::new(),
            remote: 0,
            remote_dir: String::new(),
            held: Vec::new(),
            belt_ok: false,
            in_the_way: Vec::new(),
            kept: String::new(),
            set_aside: false,
            ahead: 0,
            behind: 0,
            clash_with: Vec::new(),
            lesson_offer: String::new(),
            ignored_checks: Vec::new(),
            merge_on_green: false,
            marks: Vec::new(),
            rewound: false,
            resume_at: 0,
            queued: Vec::new(),
            resumes: 0,
            shift: false,
            issue: String::new(),
            issue_url: String::new(),
            pr_seen: String::new(),
            answering: false,
        }
    }
}

impl Job {
    /// Seconds it has run, or ran: its turns only, not the time it sat
    /// waiting for a follow-up.
    pub fn seconds(&self) -> u64 {
        if self.worked == 0 && self.turn == 0 {
            // Saved before turns were timed.
            return self.ended.unwrap_or_else(now).saturating_sub(self.started);
        }
        let running = if self.turn > 0 {
            self.ended.unwrap_or_else(now).saturating_sub(self.turn)
        } else {
            0
        };
        self.worked + running
    }

    /// A turn starts: the first run or a follow-up.
    pub fn turn_began(&mut self) {
        self.turn = now();
        self.turn_from = self.feed.len();
    }

    /// The running turn ended; its time goes on the job.
    pub fn turn_ended(&mut self) {
        if self.turn > 0 {
            self.worked += now().saturating_sub(self.turn).max(1);
            self.turn = 0;
        }
    }

    /// Steps done out of steps planned, when the agent keeps a plan.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (!self.plan.is_empty()).then(|| {
            (
                self.plan.iter().filter(|s| s.1 == 2).count(),
                self.plan.len(),
            )
        })
    }

    /// The step it is on now, in words.
    pub fn doing(&self) -> String {
        if self.status != Status::Running {
            return self.status.label().to_string();
        }
        if let Some((step, _)) = self.plan.iter().find(|s| s.1 == 1) {
            return step.clone();
        }
        match self
            .feed
            .iter()
            .rev()
            .find(|i| matches!(i, Item::Tool { .. } | Item::Text(_)))
        {
            Some(Item::Tool { label, .. }) => label.clone(),
            Some(Item::Text(_)) => "Thinking it through".into(),
            _ => "Starting".into(),
        }
    }

    /// The last thing the bot said, for a one-line preview.
    pub fn last_words(&self) -> Option<&str> {
        self.feed.iter().rev().find_map(|i| match i {
            Item::Text(t) if !t.trim().is_empty() => Some(t.trim()),
            _ => None,
        })
    }

    /// Lines added and removed across its changes.
    pub fn lines(&self) -> (usize, usize) {
        self.changes
            .iter()
            .fold((0, 0), |a, c| (a.0 + c.added, a.1 + c.removed))
    }

    /// The folder it runs in: its worktree, or the project folder.
    pub fn dir(&self) -> &str {
        if self.worktree.is_empty() {
            &self.folder.path
        } else {
            &self.worktree
        }
    }

    /// The bot that started this job by @mentioning its bot in the thread;
    /// None when the person asked for it.
    pub fn asked_by(&self) -> Option<Member> {
        match self.feed.first() {
            Some(Item::By(m, _)) if self.title.starts_with(&format!("{} asked:", m.name())) => {
                Some(*m)
            }
            _ => None,
        }
    }

    pub fn needs_you(&self) -> bool {
        // Changes waiting for approval stay in Needs you until you decide.
        if self.status == Status::Review {
            return true;
        }
        !self.seen
            && matches!(
                self.status,
                Status::Done | Status::Failed | Status::Interrupted
            )
    }
}

/// A line in a chat with a bot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Said {
    pub mine: bool,
    pub text: String,
    /// Which engine answered, and how long it took.
    pub via: Option<String>,
    /// Squad Code speaking, not the bot: a problem or a hint.
    pub note: bool,
    /// The project folder the DM line belongs to; empty in older saves,
    /// which show in every project.
    pub folder: String,
    /// When it was said, to place it among the bot's jobs.
    pub at: u64,
    /// The job the bot offers to do, in its words; empty when its answer
    /// needs no code changed.
    pub job: String,
    /// Once the offer is taken: the job it started (0 when the work went
    /// another way, as a plan). The offer then can't be taken again.
    pub job_id: u64,
    pub job_taken: bool,
    /// "Not now" on the offer.
    pub job_off: bool,
}

impl Said {
    pub fn mine(text: String) -> Self {
        Said {
            mine: true,
            text,
            at: now(),
            ..Default::default()
        }
    }
    pub fn bot(text: String, via: String) -> Self {
        Said {
            text,
            via: Some(via),
            at: now(),
            ..Default::default()
        }
    }
    pub fn note(text: impl Into<String>) -> Self {
        Said {
            text: text.into(),
            note: true,
            at: now(),
            ..Default::default()
        }
    }
}

mod chat_list {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{Member, Said};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Pairs(Vec<(Member, Vec<Said>)>),
        // Stores from before made bots: a map keyed by the bot's name.
        Map(HashMap<Member, Vec<Said>>),
    }

    pub fn serialize<S: Serializer>(
        m: &HashMap<Member, Vec<Said>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut v: Vec<(&Member, &Vec<Said>)> = m.iter().collect();
        v.sort_by_key(|(k, _)| k.key());
        v.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<Member, Vec<Said>>, D::Error> {
        Ok(match Saved::deserialize(d)? {
            Saved::Pairs(v) => v.into_iter().collect(),
            Saved::Map(m) => m,
        })
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path() -> PathBuf {
    crate::store::data_dir().join("v2.json")
}

/// The copy of the store kept from before this launch's first save.
fn prev_path() -> PathBuf {
    crate::store::data_dir().join("v2.json.prev")
}

/// The shape of `v2.json` this build writes. Raise it when a change means an
/// older build would misread the file.
/// 2: job feeds are kept in `jobs/<id>.json`.
pub const FORMAT: u32 = 2;

/// The folder each job's feed is saved in, one file per job.
fn feeds_dir() -> PathBuf {
    crate::store::data_dir().join("jobs")
}

fn feed_path(id: u64) -> PathBuf {
    feeds_dir().join(format!("{id}.json"))
}

thread_local! {
    /// Set while v2.json is written, so feeds stay out of it; anywhere else a
    /// job is written out (the server's API) it keeps its feed.
    static FEEDS_APART: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn feeds_apart(_: &Vec<Item>) -> bool {
    FEEDS_APART.with(|f| f.get())
}

/// What was last written to (or read from) each job's feed file: its length
/// and a hash of its last line. A feed that matches isn't written again.
static FEEDS_WRITTEN: std::sync::Mutex<Option<HashMap<u64, (usize, u64)>>> =
    std::sync::Mutex::new(None);

fn feed_mark(feed: &[Item]) -> (usize, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", feed.last()).hash(&mut h);
    (feed.len(), h.finish())
}

/// Entries [`lenient`] left out since it was last asked.
static SKIPPED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// What happened to the saved store at load, for the person to hear once.
static LOAD_NOTE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The previous copy is taken once a launch, before the first save.
static PREV_TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A list read entry by entry: one this build can't read (a job state or feed
/// line from a newer Squad Code) is left out instead of failing the whole file.
fn lenient<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = Vec::<serde_json::Value>::deserialize(d)?;
    let mut out = Vec::with_capacity(raw.len());
    for v in raw {
        match serde_json::from_value(v) {
            Ok(t) => out.push(t),
            Err(_) => {
                SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    Ok(out)
}

/// What happened to the saved jobs and chats when they were loaded, once:
/// a file that couldn't be read in full, and where its copy is.
pub fn take_load_note() -> Option<String> {
    LOAD_NOTE.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn set_load_note(note: String) {
    crate::applog::error("load", note.clone());
    *LOAD_NOTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(note);
}

/// The store in `text`, read as fully as it can be. As the owner, a file that
/// can't be read in full is kept: moved aside when it can't be read at all,
/// copied aside when only parts of it were left out, so a save never loses it.
fn parse(text: &str, owner: bool) -> Option<Store> {
    SKIPPED.store(0, std::sync::atomic::Ordering::Relaxed);
    match serde_json::from_str::<Store>(text) {
        Ok(s) => {
            let skipped = SKIPPED.swap(0, std::sync::atomic::Ordering::Relaxed);
            if owner && (skipped > 0 || s.format > FORMAT) {
                let why = if s.format > FORMAT {
                    "saved by a newer Squad Code"
                } else {
                    "some entries couldn't be read"
                };
                crate::store::keep_aside(&path(), why);
                set_load_note(format!(
                    "Some saved jobs or chats were {}. A copy of the file is kept next to v2.json in {}.",
                    if s.format > FORMAT {
                        "saved by a newer Squad Code and may be missing here"
                    } else {
                        "left out because they couldn't be read"
                    },
                    crate::store::data_dir().display()
                ));
            }
            Some(s)
        }
        Err(e) => {
            if owner {
                crate::store::move_aside(&path(), &e.to_string());
            }
            // Whatever fields still read; None when it isn't JSON at all.
            let salvaged = serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(crate::store::salvage::<Store>);
            if owner {
                set_load_note(format!(
                    "Your saved jobs and chats couldn't be read in full, so Squad Code {}. The file is kept as it was in {}.",
                    if salvaged.is_some() {
                        "kept what it could"
                    } else {
                        "started from the copy before your last launch"
                    },
                    crate::store::data_dir().display()
                ));
            }
            salvaged
        }
    }
}

impl Store {
    /// The saved store, or a fresh one that starts from v1's projects.
    pub fn load() -> Store {
        Self::load_as(true)
    }

    /// The saved store as it stands, for a reader such as `squad ls` while the
    /// app or server may be running jobs: a running job stays running.
    pub fn load_view() -> Store {
        Self::load_as(false)
    }

    /// `owner`: this process runs the jobs, so a job still marked running was
    /// cut off when Squad Code last closed.
    fn load_as(owner: bool) -> Store {
        let saved = std::fs::read_to_string(path()).ok();
        let read = match &saved {
            Some(text) => parse(text, owner).or_else(|| {
                // Not readable at all (cut off mid-write): the copy from before
                // the last launch is the best there is.
                std::fs::read_to_string(prev_path())
                    .ok()
                    .and_then(|t| serde_json::from_str::<Store>(&t).ok())
            }),
            None => None,
        };
        let mut s = read.unwrap_or_else(|| {
            let v1 = crate::store::State::load();
            Store {
                folders: v1
                    .projects
                    .iter()
                    .rev()
                    .filter(|p| std::path::Path::new(&p.path).is_dir())
                    .map(|p| Folder {
                        name: p.name.clone(),
                        path: p.path.clone(),
                        ship: String::new(),
                    })
                    .collect(),
                ..Default::default()
            }
        });
        s.format = FORMAT;
        s.read_feeds(owner);
        if owner {
            s.after_restart();
        }
        s.folders.retain(|f| std::path::Path::new(&f.path).is_dir());
        // People who used v2 before the walkthrough existed don't need it.
        if !s.set_up && (!s.jobs.is_empty() || !s.chats.is_empty()) {
            s.set_up = true;
        }
        super::data::set_made(&s.made);
        super::setup::set_all(&s.setups);
        super::learn::set_all(&s.lessons);
        super::data::set_removed(&s.removed);
        if s.current >= s.folders.len() {
            s.current = 0;
        }
        s
    }

    /// What was going on when Squad Code last closed can't still be: a running
    /// job was cut off, its checks stopped, and a pull request it was opening
    /// may not exist. Only for the process that runs the jobs.
    fn after_restart(&mut self) {
        for j in &mut self.jobs {
            if j.status == Status::Running {
                j.status = Status::Interrupted;
                j.ended = Some(j.ended.unwrap_or(j.started));
                j.feed.push(Item::Note(
                    "Squad Code closed while this was running.".into(),
                ));
            }
            j.checking = false;
            // Squad Code closed while Shipper was opening the pull request.
            if j.shipping {
                j.shipping = false;
                if j.pr.is_empty() && j.status == Status::Shipping {
                    j.status = Status::Review;
                }
            }
        }
    }

    /// Marks a getting-started step done. True when it wasn't before.
    pub fn done_step(&mut self, step: Step) -> bool {
        if self.steps.contains(&step) {
            return false;
        }
        self.steps.push(step);
        true
    }

    /// Whether a getting-started step is done, apart from the engine, which
    /// the settings know about.
    pub fn step_done(&self, step: Step) -> bool {
        match step {
            Step::Engine => self.steps.contains(&step),
            Step::Folder => self.folder().is_some(),
            Step::Ask => !self.jobs.is_empty(),
            Step::Terminal => self.steps.contains(&step),
            Step::Race => self.jobs.iter().any(|j| j.race != 0),
        }
    }

    /// Saves now, or at the window's next flush when saves are deferred.
    pub fn save(&self) {
        if self.demo {
            return;
        }
        if self.deferred {
            self.dirty.set(true);
            return;
        }
        self.write();
    }

    /// Writes what a deferred save asked for: at most about once a second,
    /// or right away with `now` (on quit).
    pub fn flush(&self, now: bool) {
        if !self.dirty.get() {
            return;
        }
        let due = self
            .flushed_at
            .get()
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1));
        if now || due {
            self.dirty.set(false);
            self.flushed_at.set(Some(std::time::Instant::now()));
            self.write();
        }
    }

    /// Each job's feed from its own file, for jobs whose feed v2.json didn't
    /// hold. As the owner, feed files of jobs that are gone are removed.
    fn read_feeds(&mut self, owner: bool) {
        let mut written = FEEDS_WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
        let written = written.get_or_insert_with(HashMap::new);
        for j in &mut self.jobs {
            if !j.feed.is_empty() {
                // Saved inside v2.json by an older build: written apart on the next save.
                continue;
            }
            let Ok(text) = std::fs::read_to_string(feed_path(j.id)) else {
                continue;
            };
            #[derive(Deserialize)]
            struct Feed(#[serde(deserialize_with = "lenient")] Vec<Item>);
            if let Ok(Feed(feed)) = serde_json::from_str(&text) {
                j.feed = feed;
                written.insert(j.id, feed_mark(&j.feed));
            }
        }
        if owner && !self.demo {
            let ids: std::collections::HashSet<u64> = self.jobs.iter().map(|j| j.id).collect();
            for entry in std::fs::read_dir(feeds_dir())
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name
                    .strip_suffix(".json")
                    .and_then(|n| n.parse::<u64>().ok());
                if id.is_some_and(|id| !ids.contains(&id)) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// The feeds that changed since they were last written, each in its own
    /// file. A running job's feed changes in place (a step finishing), so it
    /// is always written.
    fn write_feeds(&self) {
        let mut written = FEEDS_WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
        let written = written.get_or_insert_with(HashMap::new);
        for j in &self.jobs {
            let mark = feed_mark(&j.feed);
            if j.status != Status::Running && written.get(&j.id) == Some(&mark) {
                continue;
            }
            match serde_json::to_string(&j.feed) {
                Ok(text) => match crate::store::write_private(&feed_path(j.id), text.as_bytes()) {
                    Ok(()) => {
                        written.insert(j.id, mark);
                    }
                    Err(e) => crate::applog::error_once(
                        "save",
                        format!("could not save the feed of job {}: {e}", j.id),
                    ),
                },
                Err(e) => crate::applog::error_once("save", format!("could not save a feed: {e}")),
            }
        }
    }

    fn write(&self) {
        // Feeds first: v2.json never lists a job whose feed isn't on disk.
        self.write_feeds();
        let p = path();
        // Once a launch, keep the file as it was before this launch changed it:
        // what a load falls back to when the file is found cut off.
        if !PREV_TAKEN.swap(true, std::sync::atomic::Ordering::Relaxed) && p.exists() {
            let _ = std::fs::copy(&p, prev_path());
        }
        FEEDS_APART.with(|f| f.set(true));
        let text = serde_json::to_string(self);
        FEEDS_APART.with(|f| f.set(false));
        match text {
            Ok(text) => {
                if let Err(e) = crate::store::write_private(&p, text.as_bytes()) {
                    crate::applog::error_once(
                        "save",
                        format!("could not save {}: {e}", p.display()),
                    );
                }
            }
            Err(e) => {
                crate::applog::error_once("save", format!("could not save {}: {e}", p.display()))
            }
        }
    }

    pub fn folder(&self) -> Option<&Folder> {
        self.folders.get(self.current)
    }

    /// The live threads in a project, oldest first.
    pub fn threads_in(&self, folder: &str) -> Vec<&Thread> {
        self.threads
            .iter()
            .filter(|t| t.folder == folder && !t.archived)
            .collect()
    }

    /// The thread a job or plan belongs to: its own, or #general (0) when that
    /// thread is gone or archived.
    pub fn thread_for(&self, folder: &str, thread: u64) -> u64 {
        if self
            .threads
            .iter()
            .any(|t| t.id == thread && t.folder == folder && !t.archived)
        {
            thread
        } else {
            0
        }
    }

    /// The thread new work goes to, if it belongs to the current project;
    /// else #general.
    pub fn thread_here(&self) -> u64 {
        match self.folder() {
            Some(f) => self.thread_for(&f.path, self.thread_now),
            None => 0,
        }
    }

    /// The name of a thread, without the #.
    pub fn thread_label(&self, id: u64) -> String {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "general".into())
    }

    /// Makes a thread in the current project and moves to it. A name already
    /// used there picks that thread instead.
    pub fn add_thread(&mut self, raw: &str, bots: Vec<Member>) -> Option<u64> {
        let folder = self.folder()?.path.clone();
        let name = thread_name(raw);
        if name.is_empty() || name == "general" {
            self.thread_now = 0;
            return Some(0);
        }
        if let Some(t) = self
            .threads
            .iter_mut()
            .find(|t| t.folder == folder && t.name == name)
        {
            t.archived = false;
            self.thread_now = t.id;
            return Some(t.id);
        }
        let id = self.threads.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.threads.push(Thread {
            id,
            name,
            folder,
            bots,
            archived: false,
            made: now(),
        });
        self.thread_now = id;
        Some(id)
    }

    /// Renames a thread, unless its project already has one by that name.
    pub fn rename_thread(&mut self, id: u64, name: &str) -> bool {
        let Some(folder) = self
            .threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.folder.clone())
        else {
            return false;
        };
        if self
            .threads
            .iter()
            .any(|t| t.id != id && t.folder == folder && t.name == name && !t.archived)
        {
            return false;
        }
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
            t.name = name.to_string();
        }
        true
    }

    /// A thread's conversation so far, oldest first, as (who, words): what you
    /// asked and what came back, for Peek to read a follow-up by.
    pub fn recent(&self, folder: &str, thread: u64) -> Vec<(String, String)> {
        let said = self.said_in(folder, thread);
        let n = said.len();
        said.into_iter()
            .skip(n.saturating_sub(12))
            .map(|(_, who, words)| (who, words))
            .collect()
    }

    /// Everything said in a thread, oldest first, with when: (at, who, words).
    pub fn said_in(&self, folder: &str, thread: u64) -> Vec<(u64, String, String)> {
        let mut said: Vec<(u64, String, String)> = Vec::new();
        for t in &self.talks {
            if t.folder == folder && self.thread_for(folder, t.thread) == thread {
                // A bot speaking up on its own has no question of yours.
                if !t.question.is_empty() {
                    said.push((t.at, "you".into(), t.question.clone()));
                }
                if let Some(a) = t.answer.as_ref().filter(|a| !a.note) {
                    said.push((t.at, t.bot.name().to_lowercase(), a.text.clone()));
                }
            }
        }
        // A job a bot asked for: its words are in the thread already, as its own.
        for j in self.jobs_in(folder, thread) {
            if j.asked_by().is_none() {
                said.push((j.started, "you".into(), j.title.clone()));
            }
        }
        said.sort_by_key(|s| s.0);
        said
    }

    /// A thread's summary, if it has one.
    pub fn summary(&self, folder: &str, thread: u64) -> Option<&super::brain::summary::Summary> {
        let thread = self.thread_for(folder, thread);
        self.summaries
            .iter()
            .find(|s| s.folder == folder && s.thread == thread)
    }

    /// A thread's notes and summary, for Peek as it reads a message there.
    pub fn summary_brief(&self, folder: &str, thread: u64) -> String {
        let label = self.thread_label(self.thread_for(folder, thread));
        super::brain::summary::context(&label, self.summary(folder, thread), &[], &[])
    }

    /// A thread's summary, made empty when it has none yet.
    pub fn summary_mut(
        &mut self,
        folder: &str,
        thread: u64,
    ) -> &mut super::brain::summary::Summary {
        let thread = self.thread_for(folder, thread);
        let at = self
            .summaries
            .iter()
            .position(|s| s.folder == folder && s.thread == thread);
        let i = match at {
            Some(i) => i,
            None => {
                self.summaries.push(super::brain::summary::Summary {
                    folder: folder.to_string(),
                    thread,
                    ..Default::default()
                });
                self.summaries.len() - 1
            }
        };
        &mut self.summaries[i]
    }

    /// What a bot reads about a thread before it answers or works there: the
    /// person's notes, the summary, the thread's jobs, and the latest
    /// messages word for word. `skip` leaves out the ask itself.
    pub fn thread_context(&self, folder: &str, thread: u64, skip: &str) -> String {
        let label = self.thread_label(self.thread_for(folder, thread));
        let recent: Vec<(String, String)> = self
            .recent(folder, thread)
            .into_iter()
            .filter(|(_, w)| w.trim() != skip.trim())
            .collect();
        let jobs: Vec<String> = self
            .jobs_in(folder, thread)
            .into_iter()
            .filter(|j| j.race == 0 || j.race == j.id)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(8)
            .rev()
            .map(|j| {
                format!(
                    "- #{} {}: {} ({})",
                    j.id,
                    j.bot.name(),
                    j.label(),
                    j.status.label()
                )
            })
            .collect();
        super::brain::summary::context(&label, self.summary(folder, thread), &jobs, &recent)
    }

    /// Deletes a thread and what was said in it. Its jobs and plans hold real
    /// work, so they move to #general rather than vanish.
    pub fn delete_thread(&mut self, id: u64) -> bool {
        let Some(i) = self.threads.iter().position(|t| t.id == id) else {
            return false;
        };
        let t = self.threads.remove(i);
        self.talks
            .retain(|k| !(k.folder == t.folder && k.thread == id));
        for j in self
            .jobs
            .iter_mut()
            .filter(|j| j.folder.path == t.folder && j.thread == id)
        {
            j.thread = 0;
        }
        for g in self
            .goals
            .iter_mut()
            .filter(|g| g.folder.path == t.folder && g.thread == id)
        {
            g.thread = 0;
        }
        self.seen_at.remove(&format!("{}#{id}", t.folder));
        if self.thread_now == id {
            self.thread_now = 0;
        }
        true
    }

    /// A thread of a project by its name; "general" is 0.
    pub fn thread_named(&self, folder: &str, name: &str) -> Option<u64> {
        let name = thread_name(name);
        if name == "general" {
            return Some(0);
        }
        self.threads
            .iter()
            .find(|t| t.folder == folder && t.name == name && !t.archived)
            .map(|t| t.id)
    }

    /// The threads a message points at with #name, in order, each once.
    pub fn thread_refs(&self, folder: &str, text: &str) -> Vec<u64> {
        let mut out = Vec::new();
        for word in text.split_whitespace() {
            let Some(name) = word.strip_prefix('#') else {
                continue;
            };
            let name = name.trim_end_matches(|c: char| !c.is_alphanumeric());
            if let Some(id) = self.thread_named(folder, name) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// You're looking at a thread now: it has nothing unread.
    /// Puts `emoji` on a message, or takes it off when it's already there.
    pub fn toggle_reaction(&mut self, key: &str, emoji: &str) {
        let list = self.reactions.entry(key.to_string()).or_default();
        if let Some(i) = list.iter().position(|e| e == emoji) {
            list.remove(i);
        } else {
            list.push(emoji.to_string());
        }
        if list.is_empty() {
            self.reactions.remove(key);
        }
        self.save();
    }

    pub fn mark_seen(&mut self, folder: &str, thread: u64) {
        self.seen_at.insert(format!("{folder}#{thread}"), now());
    }

    /// Something happened in a thread since you last looked: an answer came
    /// back, or a job started or ended there.
    pub fn unread(&self, folder: &str, thread: u64) -> bool {
        let Some(&seen) = self.seen_at.get(&format!("{folder}#{thread}")) else {
            return false;
        };
        let jobs = self
            .jobs_in(folder, thread)
            .into_iter()
            .any(|j| j.started > seen || j.ended.is_some_and(|e| e > seen));
        let talks = self.talks.iter().any(|t| {
            t.folder == folder
                && self.thread_for(folder, t.thread) == thread
                && t.answer.is_some()
                && t.at > seen
        });
        jobs || talks
    }

    /// Who's in a thread: Peek, the bots you brought in, and any bot that has
    /// done work or answered there. Nobody else until they're needed.
    pub fn members(&self, folder: &str, thread: u64) -> Vec<Member> {
        let mut out = vec![Member::Fin];
        let mut add = |m: Member| {
            if !out.contains(&m) {
                out.push(m);
            }
        };
        if let Some(t) = self.threads.iter().find(|t| t.id == thread) {
            t.bots.iter().copied().for_each(&mut add);
        }
        for j in self.jobs_in(folder, thread) {
            add(j.bot);
        }
        for t in &self.talks {
            if t.folder == folder && self.thread_for(folder, t.thread) == thread {
                add(t.bot);
            }
        }
        let crew = Member::all();
        out.retain(|m| *m == Member::Fin || crew.contains(m));
        out
    }

    /// Jobs in one thread of a project, oldest first.
    pub fn jobs_in(&self, folder: &str, thread: u64) -> Vec<&Job> {
        let mut out: Vec<&Job> = self
            .jobs
            .iter()
            .filter(|j| j.folder.path == folder && self.thread_for(folder, j.thread) == thread)
            .collect();
        out.sort_by_key(|j| j.started);
        out
    }

    /// Adds a folder (or picks it if it's there already) and makes it current.
    pub fn add_folder(&mut self, path: PathBuf) {
        let p = path.to_string_lossy().into_owned();
        if let Some(i) = self.folders.iter().position(|f| f.path == p) {
            self.current = i;
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone());
        self.folders.insert(
            0,
            Folder {
                name,
                path: p,
                ship: String::new(),
            },
        );
        self.current = 0;
    }

    /// Jobs newest first, with each race shown once: by the try that matters
    /// most now (kept, then waiting for review, then still working).
    pub fn listed(&self) -> Vec<&Job> {
        let rank = |j: &Job| match j.status {
            Status::Shipping | Status::Shipped => 0,
            Status::Review => 1,
            Status::Running => 2,
            Status::Done => 3,
            _ => 4,
        };
        let mut face: HashMap<u64, &Job> = HashMap::new();
        for j in self.jobs.iter().filter(|j| j.race != 0) {
            let e = face.entry(j.race).or_insert(j);
            if (rank(j), j.id) < (rank(e), e.id) {
                *e = j;
            }
        }
        self.jobs
            .iter()
            .rev()
            .filter(|j| j.race == 0 || face.get(&j.race).is_some_and(|f| f.id == j.id))
            .collect()
    }

    pub fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn job_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn running(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter().filter(|j| j.status == Status::Running)
    }

    /// The job a bot is on right now.
    pub fn busy(&self, who: Member) -> Option<&Job> {
        self.running().find(|j| j.bot == who)
    }

    /// Your DM with `who` in the project you're in: one conversation per bot
    /// per project.
    pub fn chat(&self, who: Member) -> Vec<Said> {
        self.chat_lines(who).cloned().collect()
    }

    /// The DM with `who` in the project you're in, oldest first, without
    /// copying it: for what's drawn every frame.
    pub fn chat_lines(&self, who: Member) -> impl DoubleEndedIterator<Item = &Said> {
        let here = self.folder().map(|f| f.path.as_str()).unwrap_or("");
        self.chats
            .get(&who)
            .into_iter()
            .flatten()
            .filter(move |m| m.folder.is_empty() || m.folder == here)
    }

    /// Adds a line to the DM with `who` in the project you're in.
    pub fn say_in_dm(&mut self, who: Member, mut said: Said) {
        said.folder = self.folder().map(|f| f.path.clone()).unwrap_or_default();
        self.chats.entry(who).or_default().push(said);
    }

    /// The line in the DM with `who` said at `at` with these words.
    pub fn dm_line_mut(&mut self, who: Member, at: u64, text: &str) -> Option<&mut Said> {
        self.chats
            .get_mut(&who)?
            .iter_mut()
            .rev()
            .find(|m| !m.mine && m.at == at && m.text == text)
    }

    /// Clears the DM with `who` in the project you're in; other projects keep theirs.
    pub fn clear_dm(&mut self, who: Member) {
        let here = self.folder().map(|f| f.path.clone()).unwrap_or_default();
        if let Some(log) = self.chats.get_mut(&who) {
            log.retain(|m| !m.folder.is_empty() && m.folder != here);
            if log.is_empty() {
                self.chats.remove(&who);
            }
        }
    }

    /// `who`'s notebook for the project at `folder`: a line for each of its
    /// last few jobs there, newest first, so a new job or a DM starts
    /// knowing the bot's own past work in this project. Its lessons are
    /// added where the bot's instructions are written. Empty when it has
    /// none.
    pub fn notebook(&self, who: Member, folder: &str) -> String {
        let mut mine: Vec<&Job> = self
            .jobs
            .iter()
            .filter(|j| j.bot == who && j.folder.path == folder)
            .filter(|j| j.goal == 0 && (j.race == 0 || j.race == j.id))
            .collect();
        mine.sort_by_key(|j| std::cmp::Reverse(j.started));
        let lines: Vec<String> = mine
            .into_iter()
            .take(NOTEBOOK)
            .map(|j| {
                let last: String = j
                    .last_words()
                    .and_then(|w| w.lines().next())
                    .unwrap_or("")
                    .chars()
                    .take(160)
                    .collect();
                format!(
                    "- #{} \"{}\" ({}) {last}",
                    j.id,
                    j.label(),
                    j.status.label()
                )
                .trim_end()
                .to_string()
            })
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!(
            "Your recent jobs in this project, newest first:\n{}",
            lines.join("\n")
        )
    }

    /// `who`'s jobs in the project you're in, oldest first: the cards its DM shows.
    pub fn dm_jobs(&self, who: Member) -> Vec<&Job> {
        let here = self.folder().map(|f| f.path.as_str()).unwrap_or("");
        let mut v: Vec<&Job> = self
            .jobs
            .iter()
            .filter(|j| j.bot == who && j.folder.path == here)
            .filter(|j| j.goal == 0 && (j.race == 0 || j.race == j.id))
            .collect();
        v.sort_by_key(|j| j.started);
        v
    }
}

/// Jobs a bot's notebook lists.
pub const NOTEBOOK: usize = 5;

/// A step in plain words: "Ran cat > package.json <<'EOF'" reads "Writing
/// package.json", "Ran npm install" reads "Installing packages". `now` picks
/// the running form ("Writing") over the finished one ("Wrote").
pub fn plain_step(label: &str, now: bool) -> String {
    let pick = |doing: &str, done: &str, what: &str| {
        let verb = if now { doing } else { done };
        if what.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} {what}")
        }
    };
    let base = |p: &str| -> String {
        let p = p.trim().trim_matches(|c| c == '\'' || c == '"');
        p.rsplit('/')
            .find(|x| !x.is_empty())
            .unwrap_or(p)
            .to_string()
    };
    // Steps written as orders ("Run npm test") read like the agents' own.
    for (order, done) in [
        ("Run ", "Ran "),
        ("Edit ", "Edited "),
        ("Write ", "Wrote "),
        ("Search for ", "Searched for "),
    ] {
        if let Some(rest) = label.strip_prefix(order) {
            return plain_step(&format!("{done}{rest}"), now);
        }
    }
    if let Some(p) = label.strip_prefix("Read ") {
        return pick("Reading", "Read", &base(p));
    }
    if let Some(p) = label.strip_prefix("Edited ") {
        return pick("Editing", "Edited", &base(p));
    }
    if let Some(p) = label.strip_prefix("Wrote ") {
        return pick("Writing", "Wrote", &base(p));
    }
    if label.starts_with("Searched the web") {
        return pick("Searching the web", "Searched the web", "");
    }
    if let Some(p) = label.strip_prefix("Searched for ") {
        return pick("Searching for", "Searched for", p.trim());
    }
    if label.starts_with("Listed ") {
        return pick("Looking around the folder", "Looked around the folder", "");
    }
    if label.starts_with("Fetched ") {
        return pick("Reading a web page", "Read a web page", "");
    }
    let Some(cmd) = label.strip_prefix("Ran ") else {
        return label.to_string();
    };
    // The part that matters: past any "cd somewhere &&".
    let mut cmd = cmd.trim();
    while let Some((head, rest)) = cmd.split_once("&&") {
        if head.trim_start().starts_with("cd ") {
            cmd = rest.trim();
        } else {
            break;
        }
    }
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let first = words.first().copied().unwrap_or("");
    let second = words.get(1).copied().unwrap_or("");
    // Writing a file through the shell: cat > f, tee f, echo … > f.
    if matches!(first, "cat" | "echo" | "printf" | "tee") {
        if let Some(i) = words
            .iter()
            .position(|w| *w == ">" || *w == ">>" || *w == "tee")
        {
            if let Some(f) = words.get(i + 1).filter(|f| !f.starts_with("<<")) {
                return pick("Writing", "Wrote", &base(f));
            }
        }
        if first == "cat" && !second.is_empty() && !second.starts_with('-') {
            return pick("Reading", "Read", &base(second));
        }
    }
    let tests = cmd.contains("test") || cmd.contains("pytest") || cmd.contains("jest");
    match first {
        "npm" | "yarn" | "pnpm" | "bun" | "pip" | "pip3" | "uv" | "cargo" | "go"
            if matches!(second, "install" | "i" | "add" | "ci" | "fetch" | "get") =>
        {
            pick("Installing packages", "Installed packages", "")
        }
        _ if tests => pick("Running the tests", "Ran the tests", ""),
        "npm" | "yarn" | "pnpm" | "bun" if matches!(second, "start" | "run" | "dev") => {
            pick("Running the app", "Ran the app", "")
        }
        "cargo" | "make" | "tsc" | "go"
            if cmd.contains("build") || first == "make" || first == "tsc" =>
        {
            pick("Building", "Built it", "")
        }
        "node" | "python" | "python3" | "deno" | "ruby"
            if !second.is_empty() && !second.starts_with('-') =>
        {
            pick("Running", "Ran", &base(second))
        }
        "ls" | "find" | "tree" | "pwd" | "du" | "stat" | "node" | "python" | "python3"
        | "which" => pick("Looking around the folder", "Looked around the folder", ""),
        "head" | "tail" | "sed" | "less" | "wc" => pick(
            "Reading",
            "Read",
            &base(words.last().copied().unwrap_or("")),
        ),
        "grep" | "rg" | "ag" => pick("Searching the code", "Searched the code", ""),
        "git" if matches!(second, "commit" | "add") => {
            pick("Saving the work in git", "Saved the work in git", "")
        }
        "git" => pick("Checking git", "Checked git", ""),
        "mkdir" => pick("Making folders", "Made folders", ""),
        "rm" | "mv" | "cp" => pick("Moving files around", "Moved files around", ""),
        "curl" | "wget" => pick("Fetching from the web", "Fetched from the web", ""),
        "chmod" => pick("Setting file permissions", "Set file permissions", ""),
        _ => pick("Running a command", "Ran a command", ""),
    }
}

/// "just now", "4m", "2h", "3d".
pub fn ago(at: u64) -> String {
    let s = now().saturating_sub(at);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// "45s", "3m 20s", "1h 5m".
pub fn span(s: u64) -> String {
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feeds_stay_out_of_v2_json_but_not_out_of_the_api() {
        let s = Store {
            jobs: vec![Job {
                id: 1,
                feed: vec![Item::Text("long output".into())],
                ..Default::default()
            }],
            ..Default::default()
        };
        FEEDS_APART.with(|f| f.set(true));
        let saved = serde_json::to_string(&s).unwrap();
        FEEDS_APART.with(|f| f.set(false));
        assert!(!saved.contains("long output"));
        // A job sent anywhere else, such as the server's API, keeps its feed.
        assert!(serde_json::to_string(&s.jobs[0])
            .unwrap()
            .contains("long output"));
        // And a v2.json from before, with feeds inside, still reads them.
        let old = serde_json::to_string(&s).unwrap();
        assert_eq!(parse(&old, false).unwrap().jobs[0].feed.len(), 1);
    }

    #[test]
    fn feeds_are_saved_apart_and_read_back() {
        let _lock = crate::store::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("SQUADCODE_HOME");
        let home = std::env::temp_dir().join(format!("squad-feeds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::env::set_var("SQUADCODE_HOME", &home);
        let s = Store {
            format: FORMAT,
            jobs: vec![Job {
                id: 7,
                feed: vec![Item::Text("ran the tests".into())],
                ..Default::default()
            }],
            next_id: 8,
            ..Default::default()
        };
        s.save();
        let index = std::fs::read_to_string(home.join("v2.json")).unwrap();
        assert!(!index.contains("ran the tests"));
        assert!(std::fs::read_to_string(home.join("jobs/7.json"))
            .unwrap()
            .contains("ran the tests"));
        // A feed file whose job is gone is cleared away by the next load.
        std::fs::write(home.join("jobs/3.json"), "[]").unwrap();
        let back = Store::load();
        assert_eq!(back.jobs[0].feed, vec![Item::Text("ran the tests".into())]);
        assert!(!home.join("jobs/3.json").exists());
        match old {
            Some(v) => std::env::set_var("SQUADCODE_HOME", v),
            None => std::env::remove_var("SQUADCODE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_feed_is_written_again_only_when_its_end_changed() {
        let mut feed = vec![Item::Text("a".into())];
        let before = feed_mark(&feed);
        assert_eq!(before, feed_mark(&feed.clone()));
        feed.push(Item::Note("b".into()));
        assert_ne!(before, feed_mark(&feed));
        let grown = feed_mark(&feed);
        feed[1] = Item::Note("c".into());
        assert_ne!(grown, feed_mark(&feed));
    }

    #[test]
    fn a_deferred_save_waits_for_a_flush() {
        let s = Store {
            deferred: true,
            demo: false,
            ..Default::default()
        };
        assert!(!s.dirty.get());
        s.save();
        assert!(s.dirty.get());
    }

    fn saved_with(jobs: serde_json::Value) -> String {
        serde_json::json!({ "format": 1, "jobs": jobs, "next_id": 9 }).to_string()
    }

    #[test]
    fn a_job_this_build_cant_read_costs_that_job_not_the_file() {
        let good = serde_json::to_value(Job {
            id: 1,
            title: "fix login".into(),
            ..Default::default()
        })
        .unwrap();
        let mut newer = good.clone();
        newer["id"] = 2.into();
        newer["status"] = "Waiting".into();
        let s = parse(&saved_with(serde_json::json!([good, newer])), false).unwrap();
        assert_eq!(s.jobs.len(), 1);
        assert_eq!(s.jobs[0].title, "fix login");
        assert_eq!(s.next_id, 9);
    }

    #[test]
    fn a_feed_line_this_build_cant_read_costs_that_line() {
        let mut job = serde_json::to_value(Job {
            id: 1,
            feed: vec![Item::Text("hi".into())],
            ..Default::default()
        })
        .unwrap();
        job["feed"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "Picture": "a.png" }));
        let s = parse(&saved_with(serde_json::json!([job])), false).unwrap();
        assert_eq!(s.jobs[0].feed, vec![Item::Text("hi".into())]);
    }

    #[test]
    fn a_file_cut_off_mid_write_reads_as_nothing() {
        let text = saved_with(serde_json::json!([]));
        assert!(parse(&text[..text.len() / 2], false).is_none());
    }

    #[test]
    fn one_field_in_a_shape_this_build_doesnt_know_keeps_the_rest() {
        let text = serde_json::json!({ "next_id": 9, "set_up": "maybe" }).to_string();
        let s = parse(&text, false).unwrap();
        assert_eq!(s.next_id, 9);
        assert!(!s.set_up);
    }

    #[test]
    fn a_restart_cuts_off_what_was_running() {
        let mut s = Store {
            jobs: vec![
                Job {
                    id: 1,
                    status: Status::Running,
                    started: 100,
                    checking: true,
                    ..Job::default()
                },
                Job {
                    id: 2,
                    status: Status::Shipping,
                    shipping: true,
                    ..Job::default()
                },
                Job {
                    id: 3,
                    status: Status::Shipping,
                    shipping: true,
                    pr: "https://example.test/pr/9".into(),
                    ..Job::default()
                },
                Job {
                    id: 4,
                    status: Status::Review,
                    ..Job::default()
                },
            ],
            ..Store::default()
        };
        s.after_restart();
        let j = &s.jobs[0];
        assert_eq!(j.status, Status::Interrupted);
        assert_eq!(j.ended, Some(100));
        assert!(!j.checking);
        assert!(matches!(j.feed.last(), Some(Item::Note(n)) if n.contains("closed")));
        // A pull request that wasn't opened goes back for review; one that was stays.
        assert_eq!(s.jobs[1].status, Status::Review);
        assert!(!s.jobs[1].shipping);
        assert_eq!(s.jobs[2].status, Status::Shipping);
        assert!(!s.jobs[2].shipping);
        assert_eq!(s.jobs[3].status, Status::Review);
        assert!(s.jobs[3].feed.is_empty());
    }

    #[test]
    fn time_waiting_for_a_follow_up_is_not_time_at_work() {
        let t = now();
        // Ran 6 seconds, sat for hours, then a 4 second follow-up.
        let j = Job {
            started: t - 30_000,
            ended: Some(t),
            worked: 10,
            ..Job::default()
        };
        assert_eq!(j.seconds(), 10);
        // A turn running now counts from when it began.
        let mut k = Job {
            started: t - 30_000,
            worked: 6,
            turn: t - 5,
            ..Job::default()
        };
        assert!((11..=12).contains(&k.seconds()));
        k.turn_ended();
        assert_eq!(k.turn, 0);
        assert!(k.worked >= 11);
        // Jobs saved before turns were timed keep their old span.
        let old = Job {
            started: t - 60,
            ended: Some(t),
            ..Job::default()
        };
        assert_eq!(old.seconds(), 60);
    }

    fn with_project() -> Store {
        let mut s = Store::default();
        s.add_folder(PathBuf::from("/work/shop"));
        s
    }

    #[test]
    fn steps_read_in_plain_words() {
        assert_eq!(
            plain_step("Ran cat > package.json <<'EOF'\u{2026}", true),
            "Writing package.json"
        );
        assert_eq!(
            plain_step("Ran ls -A && node -v", false),
            "Looked around the folder"
        );
        assert_eq!(
            plain_step("Ran cd app && npm install", true),
            "Installing packages"
        );
        assert_eq!(plain_step("Ran node server.js", true), "Running server.js");
        assert_eq!(plain_step("Ran npm test", false), "Ran the tests");
        assert_eq!(plain_step("Read src/main.rs", true), "Reading main.rs");
        assert_eq!(plain_step("Edited src/app.ts", false), "Edited app.ts");
        assert_eq!(plain_step("Planning", true), "Planning");
        assert_eq!(plain_step("Run git log -S key", true), "Checking git");
        assert_eq!(plain_step("Edit src/a.ts", false), "Edited a.ts");
    }

    #[test]
    fn thread_names_read_like_channels() {
        assert_eq!(thread_name("Gift Cards!"), "gift-cards");
        assert_eq!(thread_name("  flaky   tests "), "flaky-tests");
        assert_eq!(thread_name("@#$"), "");
    }

    #[test]
    fn a_new_thread_becomes_the_one_you_are_in() {
        let mut s = with_project();
        let id = s.add_thread("Gift cards", vec![Member::Builder]).unwrap();
        assert_eq!(s.thread_here(), id);
        assert_eq!(s.thread_label(id), "gift-cards");
        // The same name picks the same thread instead of making another.
        s.thread_now = 0;
        assert_eq!(s.add_thread("gift cards", Vec::new()), Some(id));
        assert_eq!(s.threads.len(), 1);
        // "general" is always there and never stored.
        assert_eq!(s.add_thread("general", Vec::new()), Some(0));
        assert_eq!(s.threads.len(), 1);
    }

    #[test]
    fn a_thread_can_be_renamed_but_not_onto_another() {
        let mut s = with_project();
        let a = s.add_thread("checkout", Vec::new()).unwrap();
        let b = s.add_thread("payments", Vec::new()).unwrap();
        assert!(s.rename_thread(a, "cart"));
        assert_eq!(s.thread_label(a), "cart");
        assert!(!s.rename_thread(b, "cart"));
        assert_eq!(s.thread_label(b), "payments");
    }

    #[test]
    fn a_thread_starts_with_just_peek() {
        let mut s = with_project();
        let folder = s.folder().unwrap().clone();
        assert_eq!(s.members(&folder.path, 0), vec![Member::Fin]);
        // A bot that works there joins.
        s.jobs.push(Job {
            id: 1,
            bot: Member::Fixer,
            folder: folder.clone(),
            ..Default::default()
        });
        assert_eq!(s.members(&folder.path, 0), vec![Member::Fin, Member::Fixer]);
    }

    #[test]
    fn deleting_a_thread_keeps_its_jobs_in_general() {
        let mut s = with_project();
        let folder = s.folder().unwrap().clone();
        let id = s.add_thread("checkout", Vec::new()).unwrap();
        s.jobs.push(Job {
            id: 1,
            thread: id,
            folder: folder.clone(),
            ..Default::default()
        });
        s.talks.push(Talk {
            id: 1,
            folder: folder.path.clone(),
            thread: id,
            ..Default::default()
        });
        assert!(s.delete_thread(id));
        assert!(s.threads.is_empty() && s.talks.is_empty());
        assert_eq!(s.jobs[0].thread, 0);
        assert_eq!(s.thread_now, 0);
        assert!(!s.delete_thread(0));
    }

    #[test]
    fn hash_names_point_at_threads() {
        let mut s = with_project();
        let folder = s.folder().unwrap().path.clone();
        let id = s.add_thread("checkout", Vec::new()).unwrap();
        assert_eq!(
            s.thread_refs(&folder, "see #checkout, and #general. Not #nope"),
            vec![id, 0]
        );
    }

    #[test]
    fn jobs_from_gone_threads_land_in_general() {
        let mut s = with_project();
        let id = s.add_thread("checkout", Vec::new()).unwrap();
        let folder = s.folder().unwrap().clone();
        for (n, thread) in [(1, id), (2, 0), (3, 99)] {
            s.jobs.push(Job {
                id: n,
                thread,
                folder: folder.clone(),
                started: n,
                ..Default::default()
            });
        }
        let ids = |s: &Store, t| {
            s.jobs_in(&folder.path, t)
                .iter()
                .map(|j| j.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&s, id), vec![1]);
        assert_eq!(ids(&s, 0), vec![2, 3]);
        s.threads[0].archived = true;
        assert_eq!(ids(&s, 0), vec![1, 2, 3]);
        assert!(s.threads_in(&folder.path).is_empty());
    }

    #[test]
    fn stores_from_before_threads_still_load() {
        let back: Store = serde_json::from_str(r#"{"jobs":[{"id":4,"title":"x"}]}"#).unwrap();
        assert!(back.threads.is_empty());
        assert_eq!(back.jobs[0].thread, 0);
    }

    #[test]
    fn each_project_has_its_own_dm_with_a_bot_and_its_jobs() {
        let mut s = Store::default();
        for (n, p) in [("a", "/a"), ("b", "/b")] {
            s.folders.push(Folder {
                name: n.into(),
                path: p.into(),
                ..Default::default()
            });
        }
        s.current = 0;
        s.say_in_dm(Member::Builder, Said::mine("in a".into()));
        s.jobs.push(Job {
            id: 1,
            bot: Member::Builder,
            folder: s.folders[0].clone(),
            ..Default::default()
        });
        s.current = 1;
        s.say_in_dm(Member::Builder, Said::mine("in b".into()));
        assert_eq!(s.chat(Member::Builder).len(), 1);
        assert_eq!(s.chat(Member::Builder)[0].text, "in b");
        assert!(s.dm_jobs(Member::Builder).is_empty());
        s.clear_dm(Member::Builder);
        s.current = 0;
        assert_eq!(s.chat(Member::Builder)[0].text, "in a");
        assert_eq!(s.dm_jobs(Member::Builder).len(), 1);
    }

    #[test]
    fn a_bots_notebook_is_its_own_recent_jobs_in_that_project() {
        let mut s = Store::default();
        let job = |id, bot, path: &str, title: &str| Job {
            id,
            bot,
            title: title.into(),
            started: id,
            folder: Folder {
                path: path.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(s.notebook(Member::Builder, "/a").is_empty());
        for id in 1..=7 {
            s.jobs
                .push(job(id, Member::Builder, "/a", &format!("task {id}")));
        }
        s.jobs.push(job(8, Member::Guard, "/a", "a review"));
        s.jobs.push(job(9, Member::Builder, "/b", "elsewhere"));
        let n = s.notebook(Member::Builder, "/a");
        // Newest first, only the last few, only Builder's, only here.
        assert!(n.find("#7").unwrap() < n.find("#6").unwrap(), "{n}");
        assert_eq!(n.matches("\n- #").count(), NOTEBOOK);
        assert!(!n.contains("#2 ") && !n.contains("review") && !n.contains("elsewhere"));
    }

    #[test]
    fn chats_with_made_bots_save_and_load() {
        let mut s = Store::default();
        s.chats
            .insert(Member::Explainer, vec![Said::mine("hi".into())]);
        s.chats
            .insert(Member::Made(3), vec![Said::mine("yo".into())]);
        let text = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&text).unwrap();
        assert_eq!(back.chat(Member::Made(3))[0].text, "yo");
        assert_eq!(back.chat(Member::Explainer)[0].text, "hi");
    }

    #[test]
    fn old_stores_still_load() {
        let old = r#"{"chats":{"Explainer":[{"mine":true,"text":"hi"}]},"next_id":4}"#;
        let back: Store = serde_json::from_str(old).unwrap();
        assert_eq!(back.chat(Member::Explainer)[0].text, "hi");
        assert_eq!(back.next_id, 4);
    }

    #[test]
    fn bots_read_a_threads_notes_summary_and_latest_words() {
        let mut s = Store::default();
        let f = "/tmp/shop";
        for i in 0..9u64 {
            s.talks.push(Talk {
                id: i + 1,
                folder: f.into(),
                thread: 0,
                bot: Member::Fin,
                question: format!("question {i}"),
                answer: Some(Said::bot(format!("answer {i}"), String::new())),
                at: 100 + i,
                choices: Vec::new(),
            });
        }
        assert_eq!(s.said_in(f, 0).len(), 18);
        let sum = s.summary_mut(f, 0);
        sum.text = "For:\n- Discussion about research teams\nDecided:\n- none".into();
        sum.notes.push("No new vendors".into());
        let c = s.thread_context(f, 0, "question 8");
        assert!(c.contains("No new vendors") && c.contains("research teams"));
        assert!(c.contains("answer 8") && !c.contains("question 8") && !c.contains("answer 4"));
        // One summary per thread, found again rather than made twice.
        s.summary_mut(f, 0);
        assert_eq!(s.summaries.len(), 1);
    }
}
