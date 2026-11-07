//! What v2 keeps between launches: the folders the crew works in, every job
//! and what happened in it, and each bot's chat. Saved as JSON in Squad Code's data
//! folder (`~/.runo/v2.json`), apart from v1's state.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::data::{Member, Mode};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub folders: Vec<Folder>,
    /// The folder new jobs and questions go to, by index into `folders`.
    pub current: usize,
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
    /// Questions asked in threads, with the answers that came back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub talks: Vec<Talk>,
    /// What each bot can do beyond its kind of work (instructions, skill files,
    /// connected apps, permissions, memory), by `engine::bot_key`.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub setups: HashMap<String, super::setup::Setup>,
    /// A made-up store for looking at every screen (RUNO_V2_DEMO): never saved.
    #[serde(skip)]
    pub demo: bool,
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
    pub status: Status,
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
        }
    }
}

impl Job {
    /// Seconds it has run, or ran.
    pub fn seconds(&self) -> u64 {
        self.ended.unwrap_or_else(now).saturating_sub(self.started)
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
        let mut s: Store = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| {
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
        for j in &mut s.jobs {
            if owner && j.status == Status::Running {
                j.status = Status::Interrupted;
                j.ended = Some(j.ended.unwrap_or(j.started));
                j.feed.push(Item::Note(
                    "Squad Code closed while this was running.".into(),
                ));
            }
            if owner && j.checking {
                j.checking = false;
            }
            // Squad Code closed while Shipper was opening the pull request.
            if owner && j.shipping {
                j.shipping = false;
                if j.pr.is_empty() && j.status == Status::Shipping {
                    j.status = Status::Review;
                }
            }
        }
        s.folders.retain(|f| std::path::Path::new(&f.path).is_dir());
        // People who used v2 before the walkthrough existed don't need it.
        if !s.set_up && (!s.jobs.is_empty() || !s.chats.is_empty()) {
            s.set_up = true;
        }
        super::data::set_made(&s.made);
        super::setup::set_all(&s.setups);
        super::data::set_removed(&s.removed);
        if s.current >= s.folders.len() {
            s.current = 0;
        }
        s
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

    pub fn save(&self) {
        if self.demo {
            return;
        }
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, p);
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
        for j in self.jobs_in(folder, thread) {
            said.push((j.started, "you".into(), j.title.clone()));
        }
        said.sort_by_key(|s| s.0);
        let n = said.len();
        said.into_iter()
            .skip(n.saturating_sub(12))
            .map(|(_, who, words)| (who, words))
            .collect()
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
        let here = self.folder().map(|f| f.path.as_str()).unwrap_or("");
        self.chats
            .get(&who)
            .map(|log| {
                log.iter()
                    .filter(|m| m.folder.is_empty() || m.folder == here)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adds a line to the DM with `who` in the project you're in.
    pub fn say_in_dm(&mut self, who: Member, mut said: Said) {
        said.folder = self.folder().map(|f| f.path.clone()).unwrap_or_default();
        self.chats.entry(who).or_default().push(said);
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
}
