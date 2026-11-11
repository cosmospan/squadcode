//! The crew itself, apart from any window: the saved store and everything
//! that runs for it. Jobs on their engines, Checker's checks, Shipper's pull
//! requests, goals moving from one job to the next, questions to a bot, and
//! jobs sent from `squad` in a terminal.
//!
//! The window (`Shell`) is one client of a `Runtime`: it draws the store and
//! calls the methods here, then says what happened. Nothing here touches the
//! UI, so the same crew can run with no window at all, such as on a server.
//! What the crew wants said to the person (a merge landed, a job came in from
//! the terminal) goes in `notes` for whichever client is showing it.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::settings::AppSettings;

use super::data::{Member, Mode};
use super::store::{self, Said, Status, Store, Talk};
use super::{brain, cli, engine, goal, jobs, ship, trouble, work};

mod more;

type Landing = Receiver<Result<(), String>>;
/// A pull request read again, with its review feedback.
type Reviewed = (crate::github::PrStatus, Vec<crate::github::Feedback>);

/// What starting something opened, so a client can go and show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    Job(u64),
    Race(u64),
    /// The bot is still on another job, which this one waits for.
    Busy(u64),
    /// It waits to start (for its bot, a time, or its issue to be read), by
    /// its id in `Store::later`.
    Waiting(u64),
}

/// Where work given to a bot went: into its open job in the thread, or a
/// new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Added(u64),
    Opened(Opened),
}

/// Who takes a change picked on `job`'s page: its own bot when it changes
/// code, else the bot that builds.
pub fn page_bot(job: &store::Job) -> Member {
    if job.bot.mode().writes() {
        job.bot
    } else {
        Member::doer_for(Mode::Build).unwrap_or(Member::Builder)
    }
}

/// One bot's message to another, on its way.
#[derive(Clone, Debug)]
struct Relay {
    to: Member,
    from: Member,
    from_job: u64,
    folder: store::Folder,
    thread: u64,
    said: String,
    /// The whole reply the @mention ends.
    whole: String,
}

/// A thread's summary being written by Peek's engine.
struct Summing {
    folder: String,
    thread: u64,
    /// The messages it will cover once written.
    covers: usize,
    upto: u64,
    /// Written afresh from the thread, not updated.
    fresh: bool,
    rx: Receiver<crate::agent::Event>,
}

pub struct Runtime {
    pub store: Store,
    /// Where the next job started for a client starts: the branch and commit
    /// it handed over ([`work::Handoff`]). Taken by the job that starts next,
    /// or kept with it while it waits in line.
    pub handoff: Option<work::Handoff>,
    /// A thread summary being written, one at a time.
    summing: Option<Summing>,
    /// When summaries were last checked for being due.
    summed_at: Option<Instant>,
    /// A thread whose summary the person asked to write afresh now.
    pub summary_now: Option<(String, u64)>,
    /// Jobs' agent processes while they run.
    pub runners: Vec<jobs::Runner>,
    /// A question on its way to a bot's engine.
    pub asking: Option<engine::Ask>,
    /// The thread question the answer being read goes to, if it's for one.
    pub asking_talk: Option<u64>,
    /// What the crew wants said, oldest first, until a client takes it.
    pub notes: Vec<String>,
    /// When the store was last kept while jobs ran.
    saved: Instant,
    /// Checker's runs of the checks on finished jobs, by job.
    checking: Vec<(u64, Receiver<Vec<crate::proof::CheckRun>>)>,
    /// Shipper opening pull requests, by job.
    shipping: Vec<(u64, Receiver<ship::Step>)>,
    /// Looks at open pull requests, by job, and when the last round started.
    watching: Vec<(u64, Receiver<Option<crate::github::PrStatus>>)>,
    watched: Instant,
    /// Pull requests being merged (true) or closed (false) on GitHub, by job.
    landing: Vec<(u64, bool, Landing)>,
    /// What Approve does in each folder, worked out once per folder.
    ways: HashMap<String, ship::Way>,
    /// When the queue from `squad` in a terminal was last read.
    queue_at: Instant,
    /// Goals Squad Code is splitting now.
    planning: Vec<(u64, Receiver<crate::agent::Event>)>,
    /// Where jobs can run besides here, for the intelligence layer to choose.
    pub sites: brain::Sites,
    /// Your own server, when this app is connected to one.
    pub link: Option<super::remote::Link>,
    /// Where the next job runs, when the person said so for it ("server" or
    /// "here"); their usual choice otherwise.
    pub where_next: Option<String>,
    /// Messages between bots in each thread (folder, thread) since the
    /// person last spoke there.
    pub relay_hops: HashMap<(String, u64), u32>,
    /// Bots' messages to a bot that's busy, delivered once it's free.
    relays: Vec<Relay>,
    /// Server jobs whose work is coming home to be approved here.
    bringing: Vec<(u64, Receiver<super::remote::Home>)>,
    /// Titles and recaps Peek is writing, by job.
    peeking: Vec<(u64, Receiver<brain::tasks::Said>)>,
    /// New bots the crew's engine is setting up for the project, by made id,
    /// with whether the person set its skills and its rules (kept as they are).
    designing: Vec<(u16, (bool, bool), Receiver<crate::agent::Event>)>,
    /// The bot the maker on Crew is drafting: the engine's reply on its way, and
    /// the sentence and name it was asked about.
    drafting: Option<(Receiver<crate::agent::Event>, String, String)>,
    /// The maker's draft once it's back (or the quick one when the engine failed).
    pub drafted: Option<brain::maker::Full>,
    /// New skill files the engine is writing for the bot in the maker: the
    /// reply, and what the person asked for.
    skilling: Option<(Receiver<crate::agent::Event>, String)>,
    /// A skill file written, for the maker to add: (name, description, body).
    pub skilled: Option<Result<(String, String, String), String>>,
    /// Tool uses running jobs wait on your yes for, oldest first.
    pub asks: Vec<crate::approvals::Request>,
    asks_at: Instant,
    /// The git look at jobs' branches running now, and when the last one began:
    /// commits ahead and behind, and jobs whose work clashes.
    git_look: Option<Receiver<GitLook>>,
    git_looked: Instant,
    /// Each project folder's git state, from the last look.
    pub repos: HashMap<String, RepoLine>,
    /// Fetches from the remote running now.
    fetching: Vec<Receiver<()>>,
    /// When the night shift last looked at the jobs waiting to start.
    shift_at: Instant,
    /// Issues being read for waiting jobs, by their id in `Store::later`.
    reading: Vec<(u64, Receiver<Result<super::issues::Issue, String>>)>,
    /// Review feedback being read from open pull requests, by job.
    feedback: Vec<(u64, Receiver<Reviewed>)>,
    /// Pull requests whose new review feedback is waiting to be read, by job.
    feedback_due: Vec<(u64, crate::github::PrStatus)>,
    /// Lessons the engine is looking for in a send-back: the job and what
    /// the person said.
    learning: Vec<(u64, Receiver<crate::agent::Event>)>,
    /// Shipper pushing a review fix to an open pull request, by job.
    updating: Vec<(u64, Receiver<ship::Step>)>,
}

/// What one git look found: each job's commits ahead and behind, and the
/// pairs of jobs that change the same lines.
struct GitLook {
    counts: Vec<(u64, usize, usize)>,
    clashes: Vec<(u64, u64, String)>,
    repos: Vec<(String, RepoLine)>,
}

/// Where a project folder stands, for the Git side of the Jobs panel: its
/// branch against the remote's copy as of the last fetch, and whether it has
/// uncommitted work.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepoLine {
    /// False for a folder that isn't a git repository: there is nothing else.
    pub is_repo: bool,
    pub state: crate::git::RepoState,
    pub dirty: bool,
    /// Uncommitted files (git's status letters and the path), the first few.
    pub changes: Vec<(String, String)>,
    /// How many files are uncommitted in all.
    pub changed: usize,
    /// The newest commits on the checked-out branch: hash, subject, how long ago.
    pub commits: Vec<(String, String, String)>,
}

impl Runtime {
    pub fn new(store: Store) -> Self {
        Runtime {
            store,
            summing: None,
            summed_at: None,
            summary_now: None,
            runners: Vec::new(),
            asking: None,
            asking_talk: None,
            notes: Vec::new(),
            saved: Instant::now(),
            checking: Vec::new(),
            shipping: Vec::new(),
            watching: Vec::new(),
            watched: Instant::now() - Duration::from_secs(3600),
            landing: Vec::new(),
            ways: HashMap::new(),
            queue_at: Instant::now(),
            planning: Vec::new(),
            sites: brain::Sites::default(),
            link: None,
            where_next: None,
            relay_hops: HashMap::new(),
            relays: Vec::new(),
            bringing: Vec::new(),
            peeking: Vec::new(),
            designing: Vec::new(),
            drafting: None,
            drafted: None,
            skilling: None,
            skilled: None,
            asks: Vec::new(),
            asks_at: Instant::now(),
            git_look: None,
            git_looked: Instant::now() - Duration::from_secs(3600),
            repos: HashMap::new(),
            fetching: Vec::new(),
            shift_at: Instant::now() - Duration::from_secs(3600),
            reading: Vec::new(),
            feedback: Vec::new(),
            feedback_due: Vec::new(),
            learning: Vec::new(),
            updating: Vec::new(),
            handoff: None,
        }
    }

    /// What a running job is waiting for you to allow, oldest first.
    pub fn asks_for(&self, job: &store::Job) -> Vec<crate::approvals::Request> {
        if job.status != store::Status::Running {
            return Vec::new();
        }
        self.asks
            .iter()
            .filter(|r| r.dir == job.dir())
            .cloned()
            .collect()
    }

    /// Answers a job's request: `yes` lets the tool run; no tells the bot to
    /// find another way, with your reason when you gave one.
    pub fn answer_ask(&mut self, id: &str, yes: bool, why: &str) {
        let Some(req) = self.asks.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        let answer = if yes {
            crate::approvals::Answer::Allow {
                updated_input: req.input.clone(),
            }
        } else {
            let why = why.trim();
            crate::approvals::Answer::Deny {
                message: if why.is_empty() {
                    "The person said no. Don't try this again; say what you'd need instead.".into()
                } else {
                    format!("The person said no: {why}")
                },
            }
        };
        let _ = crate::approvals::answer(&crate::approvals::folder(), id, &answer);
        self.asks.retain(|r| r.id != id);
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }

    /// Something is running or on its way back, so the crew needs polling often.
    pub fn busy(&self) -> bool {
        !self.runners.is_empty()
            || self.asking.is_some()
            || !self.checking.is_empty()
            || !self.shipping.is_empty()
            || !self.landing.is_empty()
            || !self.planning.is_empty()
            || !self.peeking.is_empty()
            || !self.designing.is_empty()
            || self.drafting.is_some()
            || self.skilling.is_some()
            || self
                .store
                .jobs
                .iter()
                .any(|j| !j.remote_on.is_empty() && j.status == Status::Running)
    }

    /// Whether `who` is answering a question right now.
    pub fn thinking(&self, who: Member) -> bool {
        self.asking.as_ref().is_some_and(|a| a.who == who)
    }

    // ---------- starting work ----------

    /// A new job for `bot`, unless it's still on another one.
    pub fn start_job(
        &mut self,
        st: &AppSettings,
        bot: Member,
        mode: Mode,
        text: String,
        folder: store::Folder,
    ) -> Opened {
        // An issue it names is read first; the job starts once it's in.
        if let Some(r) = super::issues::find(&text) {
            let id = self.queue_later(bot, mode, text, folder, 0);
            self.keep_handoff(id);
            self.say(format!(
                "Reading {} first. {} starts as soon as it's read.",
                r.label(),
                bot.name()
            ));
            return Opened::Waiting(id);
        }
        if let Some(busy) = self.store.busy(bot) {
            let (title, id) = (busy.title.clone(), busy.id);
            let later = self.queue_later(bot, mode, text, folder, 0);
            self.keep_handoff(later);
            self.say(format!(
                "{} is still on \u{201c}{title}\u{201d}. This one is next in line and starts as soon as it's free.",
                bot.name()
            ));
            return Opened::Busy(id);
        }
        let job = self.make_job(st, bot, mode, &text, folder, None, None);
        let id = job.id;
        self.store.jobs.push(job);
        self.store.save();
        Opened::Job(id)
    }

    /// `who`'s open job in this thread: one still working or waiting for
    /// review, or, for a bot that doesn't change code, one that finished,
    /// since there's nothing of it to approve. A bot has at most one; what's
    /// said to that bot here goes into it until it's approved or thrown away.
    /// Plan steps and race tries are their own work and never count.
    pub fn open_job(&self, folder: &str, thread: u64, who: Member) -> Option<u64> {
        self.store
            .jobs_in(folder, thread)
            .into_iter()
            .rev()
            .find(|j| {
                j.bot == who
                    && j.goal == 0
                    && j.race == 0
                    && match j.status {
                        Status::Running | Status::Review => true,
                        Status::Done => !j.mode.writes(),
                        _ => false,
                    }
            })
            .map(|j| j.id)
    }

    /// The one rule for work given to a bot in a thread: its open job there
    /// takes it as the next turn, else a new job for it starts there.
    pub fn ask_bot(
        &mut self,
        st: &AppSettings,
        bot: Member,
        text: String,
        folder: store::Folder,
        thread: u64,
    ) -> Asked {
        if let Some(id) = self.open_job(&folder.path, thread, bot) {
            self.add_to_job(st, id, text);
            return Asked::Added(id);
        }
        // It starts in that thread, not wherever the person last looked.
        let was = std::mem::replace(&mut self.store.thread_now, thread);
        let opened = self.start_job(st, bot, bot.mode(), text, folder);
        self.store.thread_now = was;
        Asked::Opened(opened)
    }

    /// A change the person picked on job `from`'s page in its browser. It
    /// goes to that job's bot by the one rule, or, when that bot doesn't
    /// change code, to the bot that builds, as the pick bar says.
    pub fn ask_from_page(&mut self, st: &AppSettings, from: u64, text: String) -> Option<Asked> {
        let job = self.store.job(from)?;
        let bot = page_bot(job);
        let (folder, thread) = (job.folder.clone(), job.thread);
        let text = if self.open_job(&folder.path, thread, bot) == Some(from) {
            text
        } else {
            format!("{text}\n\nThe page was open in #{from}'s browser.")
        };
        Some(self.ask_bot(st, bot, text, folder, thread))
    }

    /// A new job, on its own branch when it may change files, started on its
    /// bot's engine, or on `pick` (an agent index) for a try in a race. The
    /// caller adds it to the store.
    #[allow(clippy::too_many_arguments)]
    pub fn make_job(
        &mut self,
        st: &AppSettings,
        bot: Member,
        mode: Mode,
        text: &str,
        folder: store::Folder,
        pick: Option<(usize, u64)>,
        part: Option<(u64, String)>,
    ) -> store::Job {
        self.make_placed_job(st, bot, mode, text, folder, pick, part)
            .0
    }

    /// [`Runtime::make_job`], with where the intelligence layer placed it. A
    /// client that can send jobs to a server starts it there when the
    /// placement's site is `Site::Server`.
    #[allow(clippy::too_many_arguments)]
    pub fn make_placed_job(
        &mut self,
        st: &AppSettings,
        bot: Member,
        mode: Mode,
        text: &str,
        folder: store::Folder,
        pick: Option<(usize, u64)>,
        part: Option<(u64, String)>,
    ) -> (store::Job, brain::place::Placement) {
        self.store.next_id += 1;
        let mut job = store::Job {
            id: self.store.next_id,
            bot,
            mode,
            title: text.to_string(),
            folder,
            started: store::now(),
            seen: false,
            ..Default::default()
        };
        job.thread = self.store.thread_here();
        if let Some((goal, context)) = part {
            if let Some(g) = self.store.goals.iter().find(|g| g.id == goal) {
                job.thread = g.thread;
            }
            job.goal = goal;
            job.context = context;
        }
        // What was said in the thread before the ask, so the bot works with
        // the thread's context, not the one line alone. A plan's parts carry
        // the plan instead.
        if job.goal == 0 {
            let talk = self
                .store
                .thread_context(&job.folder.path, job.thread, text);
            if !talk.is_empty() {
                job.context = format!("{talk}\n\n{}", job.context).trim().to_string();
            }
        }
        // The bot's notebook for this project: what it did here before.
        if job.goal == 0 {
            let notes = self.store.notebook(bot, &job.folder.path);
            if !notes.is_empty() {
                job.context = format!("{}\n\n{notes}", job.context).trim().to_string();
            }
        }
        // Threads the person points at with #, as a short digest each.
        let refs = brain::digest::context_for(text, &self.store.jobs);
        if !refs.is_empty() {
            job.context = if job.context.is_empty() {
                refs
            } else {
                format!("{}\n\n{refs}", job.context)
            };
        }
        // A thread it builds on that isn't merged yet: this job starts from
        // the base without those changes, so say so and show where they are.
        // A bot that only reads (Guard reviewing, say) is told where those
        // changes are, or it would look at the folder without them.
        if !mode.writes() {
            for j in brain::digest::unmerged(text, &self.store.jobs) {
                if j.folder.path != job.folder.path {
                    continue;
                }
                job.context = format!("{}\n\n{}", job.context, brain::digest::read_hint(j))
                    .trim()
                    .to_string();
                job.reads.push(j.worktree.clone());
            }
        }
        if mode.writes() {
            for j in brain::digest::unmerged(text, &self.store.jobs) {
                if j.folder.path != job.folder.path {
                    continue;
                }
                job.feed.push(store::Item::Note(format!(
                    "#{} isn't approved yet, so this starts without its changes. {} can read them on #{}'s branch; approve #{} first if this should build on it.",
                    j.id,
                    bot.name(),
                    j.id,
                    j.id
                )));
                job.context = format!("{}\n\n{}", job.context, brain::digest::unmerged_hint(j))
                    .trim()
                    .to_string();
                job.reads.push(j.worktree.clone());
            }
        }
        // Peek's short title for the card, when Smart Peek is on.
        if let Some(rx) = brain::tasks::ask(st, &job, false) {
            self.peeking.push((job.id, rx));
        }
        if let Some((i, race)) = pick {
            job.pick = crate::agent::PROVIDERS[i].id.to_string();
            job.race = if race == 0 { job.id } else { race };
        }
        // Where it runs, and why, when there's a choice to make. Races and
        // goals stay here for now: their tries and steps are tracked here.
        let whole = pick.is_none() && job.goal == 0;
        let none = brain::Sites::default();
        // The person's own choice of where jobs run: for this job, else
        // their usual one.
        let usual = self
            .link
            .as_ref()
            .map(|l| l.cfg.run.clone())
            .unwrap_or_default();
        let chose = self.where_next.take().unwrap_or(usual);
        if chose == "server" && self.link.is_none() {
            job.feed.push(store::Item::Note(
                "No server is connected, so this runs here. Connect one under Settings, Servers."
                    .into(),
            ));
        }
        if chose == "here" {
            job.feed.push(store::Item::Note(
                "Running here on this computer, as you chose.".into(),
            ));
            let placed = brain::place(
                st,
                &brain::JobAsk {
                    bot,
                    mode,
                    text: text.to_string(),
                    folder: job.folder.clone(),
                    race: usize::from(pick.is_some()) * 2,
                    goal: job.goal != 0,
                },
                &none,
            );
            self.launch(st, &mut job, text);
            return (job, placed);
        }
        if chose == "server" {
            if let Some(placed) = self.send_to_server(&mut job, text, whole) {
                return (job, placed);
            }
        }
        let placed = brain::place(
            st,
            &brain::JobAsk {
                bot,
                mode,
                text: text.to_string(),
                folder: job.folder.clone(),
                race: usize::from(pick.is_some()) * 2,
                goal: job.goal != 0,
            },
            if whole { &self.sites } else { &none },
        );
        if !placed.why.is_empty() {
            job.feed.push(store::Item::Note(placed.why.clone()));
        }
        if let (brain::place::Site::Server(name), Some(link)) = (&placed.site, &self.link) {
            let remote = ship::remote_url(&job.folder.path).unwrap_or_default();
            if let Some(path) = link.path_for(&job.folder, &remote) {
                job.remote_on = name.clone();
                job.status = Status::Running;
                job.engine = format!("on {name}");
                link.send(
                    job.id,
                    serde_json::json!({
                        "text": text,
                        "mode": format!("{mode:?}"),
                        "bot": bot.name(),
                        "folder": path,
                    }),
                    &job.folder.path,
                );
                return (job, placed);
            }
        }
        self.launch(st, &mut job, text);
        (job, placed)
    }

    /// Sends a job to the server because the person chose to run jobs there,
    /// copying its project over first when the server doesn't have it. `None`
    /// when it can't go there, with a note on the job saying why it runs here.
    fn send_to_server(
        &mut self,
        job: &mut store::Job,
        text: &str,
        whole: bool,
    ) -> Option<brain::place::Placement> {
        let link = self.link.as_ref()?;
        let name = link.cfg.label();
        let server = link.server();
        let why = if !whole {
            Some(
                "Races and goal steps run here for now, even with jobs set to run on your server."
                    .to_string(),
            )
        } else if !server.reachable {
            Some(format!("{name} isn't answering, so this runs here."))
        } else if server.engines.is_empty() {
            Some(format!(
                "Nothing is signed in on {name} yet, so this runs here. Sign it in under Settings > Servers."
            ))
        } else {
            None
        };
        if let Some(why) = why {
            job.feed.push(store::Item::Note(why));
            return None;
        }
        let body = serde_json::json!({
            "text": text,
            "mode": format!("{:?}", job.mode),
            "bot": job.bot.name(),
        });
        let remote = ship::remote_url(&job.folder.path).unwrap_or_default();
        match link.path_for(&job.folder, &remote) {
            Some(path) => {
                let mut body = body;
                body["folder"] = serde_json::json!(path);
                job.feed.push(store::Item::Note(format!(
                    "Running on your server, {name}, as you chose."
                )));
                link.send(job.id, body, &job.folder.path);
            }
            None => {
                job.feed.push(store::Item::Note(format!(
                    "Setting {} up on your server, {name}, through git first. Then it runs there.",
                    job.folder.name
                )));
                link.copy_then_send(job.id, &job.folder, body);
            }
        }
        job.remote_on = name.clone();
        job.status = Status::Running;
        job.engine = format!("on {name}");
        Some(brain::place::Placement {
            site: brain::place::Site::Server(name),
            engine: (0, String::new()),
            why: String::new(),
            fallback: Some(brain::place::Site::Here),
        })
    }

    /// Starts a job here: its own branch when it may change files, then its
    /// engine.
    fn launch(&mut self, st: &AppSettings, job: &mut store::Job, text: &str) {
        let mode = job.mode;
        // Sent from another computer: it starts on the branch that computer
        // handed over, at the commit it was on there.
        let handoff = self.handoff.take();
        let mut handed = false;
        if let (true, Some(h)) = (mode.writes(), handoff) {
            match work::isolate_at(&job.folder.path, &h) {
                Ok(iso) => {
                    handed = true;
                    job.feed.push(store::Item::Note(format!(
                        "Working on {}, from your commit {}, so it starts exactly where you were.",
                        iso.branch,
                        iso.base_sha.chars().take(7).collect::<String>()
                    )));
                    job.branch = iso.branch;
                    job.worktree = iso.worktree;
                    job.base = iso.base;
                    job.base_sha = iso.base_sha;
                }
                Err(why) => job.feed.push(store::Item::Note(format!(
                    "Couldn't start from the commit you sent ({why}), so it starts from the server's copy."
                ))),
            }
        }
        // A job that may change files gets its own branch and worktree, so your
        // checkout stays as it is and its changes wait for your approval.
        if mode.writes() && !handed {
            match work::isolate(&job.folder.path, text, job.id) {
                Ok(Some(iso)) => {
                    job.feed.push(store::Item::Note(format!(
                        "Working on its own branch, {}, so your checkout stays as it is.",
                        iso.branch
                    )));
                    job.branch = iso.branch;
                    job.worktree = iso.worktree;
                    job.base = iso.base;
                    job.base_sha = iso.base_sha;
                }
                Ok(None) => {}
                Err(why) => job.feed.push(store::Item::Note(format!(
                    "Couldn't give it its own branch ({why}), so it works in your folder."
                ))),
            }
        }
        match jobs::start(st, job, text, false) {
            Ok(r) => self.runners.push(r),
            Err(why) => {
                job.status = Status::Failed;
                job.ended = Some(job.started);
                let (i, model) = engine::for_job(st, job);
                job.engine = engine::label_of(i, &model);
                job.feed.push(store::Item::Error(why));
            }
        }
    }

    /// Sends what you did to a job that runs on your server there, and says
    /// so. False when the job runs here.
    fn on_server(&mut self, id: u64, action: &str, body: serde_json::Value) -> bool {
        let Some(job) = self.store.job(id) else {
            return false;
        };
        if job.remote_on.is_empty() {
            return false;
        }
        let (name, remote) = (job.remote_on.clone(), job.remote);
        match &self.link {
            Some(link) if remote != 0 && link.cfg.label() == name => link.act(remote, action, body),
            Some(_) if remote == 0 => {
                self.say(format!("#{id} is still on its way to {name}. Try again in a moment."))
            }
            _ => self.say(format!(
                "#{id} runs on {name}, and this app isn't connected to it. Connect in Settings, Servers."
            )),
        }
        true
    }

    /// Your server's side of things: where it can take jobs, the jobs sent
    /// there that started or couldn't, and the latest of the ones running
    /// there. Gives back the jobs that finished there since last time.
    fn poll_remote(&mut self, st: &AppSettings) -> Vec<u64> {
        let Some(link) = &self.link else {
            self.sites.server = None;
            return Vec::new();
        };
        self.sites.server = Some(link.server());
        let label = link.cfg.label();
        for (id, note) in link.take_job_notes() {
            if let Some(j) = self.store.job_mut(id) {
                j.feed.push(store::Item::Note(note));
            }
        }
        let sent = link.sent();
        let fresh = link.take_jobs();
        let notes = link.take_notes();
        let watch: Vec<u64> = self
            .store
            .jobs
            .iter()
            .filter(|j| j.remote != 0 && j.remote_on == label)
            .filter(|j| {
                j.status == Status::Running
                    || j.status == Status::Shipping
                    || j.status == Status::Review
                    || j.checking
                    || j.shipping
                    || j.pr_state == "OPEN"
            })
            .map(|j| j.remote)
            .collect();
        link.watch(watch);
        for n in notes {
            self.say(n);
        }
        let mut changed = !sent.is_empty();
        for (id, result) in sent {
            let Some(job) = self.store.job_mut(id) else {
                continue;
            };
            match result {
                Ok(remote) => {
                    job.remote = remote;
                    job.feed.push(store::Item::Note(format!(
                        "Running on {label} as #{remote}. It carries on there with this computer closed."
                    )));
                }
                Err(why) => {
                    // The fallback: run it here instead.
                    job.remote_on.clear();
                    job.engine.clear();
                    job.feed.push(store::Item::Note(format!(
                        "Couldn't send it to {label} ({why}), so it runs here."
                    )));
                    let mut job = job.clone();
                    let text = job.title.clone();
                    self.launch(st, &mut job, &text);
                    if let Some(j) = self.store.job_mut(id) {
                        *j = job;
                    }
                }
            }
        }
        let mut finished = Vec::new();
        for (remote, there) in fresh {
            let Some(job) = self
                .store
                .jobs
                .iter_mut()
                .find(|j| j.remote == remote && j.remote_on == label)
            else {
                continue;
            };
            let was = job.status;
            mirror(job, there);
            changed = true;
            if was == Status::Running && job.status != Status::Running {
                finished.push(job.id);
            }
        }
        if changed {
            self.store.save();
        }
        finished
    }

    /// Races one job on up to `n` engines at once, each try on its own branch.
    /// With only one engine ready it runs once.
    pub fn start_race(
        &mut self,
        st: &AppSettings,
        bot: Member,
        mode: Mode,
        text: String,
        folder: store::Folder,
        n: usize,
    ) -> Opened {
        let engines = engine::race_engines(st, bot, n);
        if engines.len() < 2 {
            self.say("Only one engine is ready on this computer, so this runs once. Add another in Settings to race.");
            return self.start_job(st, bot, mode, text, folder);
        }
        let mut race = 0;
        for i in engines {
            let job = self.make_job(st, bot, mode, &text, folder.clone(), Some((i, race)), None);
            race = job.race;
            self.store.jobs.push(job);
        }
        self.store.save();
        Opened::Race(race)
    }

    /// Starts the same job again on other engines, so it races the first try.
    /// The race's id, or None when there was nothing to race it on.
    pub fn race_it(&mut self, st: &AppSettings, id: u64) -> Option<u64> {
        if self.on_server(id, "race", serde_json::Value::Null) {
            return None;
        }
        let first = self.store.job(id).cloned()?;
        if !first.mode.writes() {
            return None;
        }
        let race = if first.race == 0 {
            first.id
        } else {
            first.race
        };
        let taken: Vec<String> = self
            .store
            .jobs
            .iter()
            .filter(|j| j.id == id || (j.race != 0 && j.race == race))
            .map(|j| j.agent.clone())
            .collect();
        let engines: Vec<usize> = engine::race_engines(st, first.bot, 8)
            .into_iter()
            .filter(|i| !taken.iter().any(|t| t == crate::agent::PROVIDERS[*i].id))
            .take(3usize.saturating_sub(taken.len()).max(1))
            .collect();
        if engines.is_empty() {
            self.say("No other engine is ready on this computer. Add one in Settings to race.");
            return None;
        }
        if let Some(j) = self.store.job_mut(id) {
            j.race = race;
            if j.pick.is_empty() {
                j.pick = j.agent.clone();
            }
        }
        for i in engines {
            let job = self.make_job(
                st,
                first.bot,
                first.mode,
                &first.title,
                first.folder.clone(),
                Some((i, race)),
                None,
            );
            self.store.jobs.push(job);
        }
        self.store.save();
        Some(race)
    }

    /// Runs a job again from the start, in the same branch, on its own engine or
    /// on `pick` (an agent index) when its own can't run it now.
    pub fn rerun(&mut self, st: &AppSettings, id: u64, pick: Option<usize>) -> bool {
        if self.on_server(id, "rerun", serde_json::Value::Null) {
            return true;
        }
        let Some(job) = self.store.job_mut(id) else {
            return false;
        };
        if job.status == Status::Running {
            return false;
        }
        if let Some(i) = pick {
            job.pick = crate::agent::PROVIDERS[i].id.to_string();
            job.pick_model.clear();
        }
        let (i, model) = engine::for_job(st, job);
        job.feed.push(store::Item::Note(format!(
            "Starting again on {}.",
            engine::label_of(i, &model)
        )));
        job.conflict.clear();
        let text = if job.worktree.is_empty() {
            job.title.clone()
        } else {
            format!(
                "{}\n\nAn earlier run may have left work in the files. Check what is already done before redoing it.",
                job.title
            )
        };
        match jobs::start(st, job, &text, false) {
            Ok(r) => self.runners.push(r),
            Err(why) => {
                job.status = Status::Failed;
                job.feed.push(store::Item::Error(why));
            }
        }
        self.store.save();
        true
    }

    /// Marks direction `k` as picked on the directions card at `at` in a job's
    /// feed, and returns what to tell the bot. None when there's no such card, or it
    /// was already picked from.
    pub fn pick_direction(&mut self, id: u64, at: usize, k: usize) -> Option<String> {
        let job = self.store.job_mut(id)?;
        let Some(store::Item::Directions {
            options, chosen, ..
        }) = job.feed.get_mut(at)
        else {
            return None;
        };
        if chosen.is_some() || k >= options.len() {
            return None;
        }
        *chosen = Some(k);
        let text = super::shown::pick_message(options, k);
        self.store.save();
        Some(text)
    }

    /// More words for a job: while it runs they wait; once it's done they start a
    /// follow-up that continues the same conversation when the engine can.
    pub fn follow_up(&mut self, st: &AppSettings, id: u64, text: String) {
        self.send_on(st, id, text, None);
    }

    /// Sends `text` to a job's bot as its next turn. `note` is None when the
    /// person wrote it; Squad Code's own asks (a fix turn, carrying on) show
    /// as that note instead, so they're never taken for the person's words.
    fn send_on(&mut self, st: &AppSettings, id: u64, text: String, note: Option<String>) {
        if self.on_server(id, "follow-up", serde_json::json!({ "text": text })) {
            return;
        }
        let refs = brain::digest::context_for(&text, &self.store.jobs);
        let Some(job) = self.store.job_mut(id) else {
            return;
        };
        if job.status == Status::Running {
            // The person's words wait for this turn to end, then go as the
            // next one; Squad Code's own asks are dropped as before.
            if note.is_none() {
                job.queued.push(text);
                self.store.save();
            } else {
                self.say("It's still working. Add this once it finishes.");
            }
            return;
        }
        if note.is_none() {
            let (f, t) = (job.folder.path.clone(), job.thread);
            self.relay_hops.remove(&(f, t));
        }
        let Some(job) = self.store.job_mut(id) else {
            return;
        };
        // Sent back from review in the person's own words: there may be a
        // lesson in it for the bot.
        let sent_back = note.is_none() && job.status == Status::Review;
        match note {
            None => job.feed.push(store::Item::You(text.clone())),
            Some(n) if !n.is_empty() => job.feed.push(store::Item::Note(n)),
            Some(_) => {}
        }
        // The last turn's recap no longer tells how it ends.
        job.recap.clear();
        job.resume_at = 0;
        // After a rewind the engine starts fresh: tell it what still stands.
        let text = if job.rewound {
            job.rewound = false;
            super::rewind::context(job, &text)
        } else {
            text
        };
        let text = match fresh_branch(job) {
            Some(base) => format!(
                "{}\n\nYou already worked on this, and that work is merged into {base}. You're now on a fresh branch from {base}: check the files before changing anything.\n\nNow: {text}",
                job.title.trim()
            ),
            None => text,
        };
        if sent_back {
            let said = job
                .feed
                .last()
                .and_then(|i| match i {
                    store::Item::You(t) => Some(t.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            self.learn_from(st, id, &said);
        }
        let Some(job) = self.store.job_mut(id) else {
            return;
        };
        let text = if refs.is_empty() {
            text
        } else {
            format!("{text}\n\n{refs}")
        };
        match jobs::start(st, job, &text, true) {
            Ok(r) => self.runners.push(r),
            Err(why) => job.feed.push(store::Item::Error(why)),
        }
        self.store.save();
    }

    /// A stopped or interrupted job picks up where it was.
    pub fn carry_on(&mut self, st: &AppSettings, id: u64) {
        self.send_on(
            st,
            id,
            "Carry on from where you stopped. Check what is already done in the files first."
                .into(),
            Some("Picking up where it stopped.".into()),
        );
    }

    /// Hands a branch that clashes with its base back to its bot to bring up to
    /// date; it comes back for review when it's done.
    pub fn resolve(&mut self, st: &AppSettings, id: u64) {
        if self.on_server(id, "resolve", serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job(id) else {
            return;
        };
        let text = trouble::resolve_prompt(job);
        if let Some(j) = self.store.job_mut(id) {
            j.conflict.clear();
        }
        self.send_on(
            st,
            id,
            text,
            Some("Bringing the branch up to date with its base.".into()),
        );
    }

    pub fn stop(&mut self, id: u64) {
        if self.on_server(id, "stop", serde_json::Value::Null) {
            return;
        }
        if let Some(r) = self.runners.iter_mut().find(|r| r.id == id) {
            r.stop();
        }
    }

    // ---------- goals ----------

    /// Has Squad Code split a goal into jobs, to look over before it starts.
    pub fn begin_goal(&mut self, st: &AppSettings, text: String, folder: store::Folder) -> u64 {
        self.store.next_id += 1;
        let id = self.store.next_id;
        let mut g = goal::Goal {
            id,
            thread: self.store.thread_here(),
            text: text.clone(),
            folder: folder.clone(),
            started: store::now(),
            ..Default::default()
        };
        match goal::plan(st, &folder, &text) {
            Ok(rx) => self.planning.push((id, rx)),
            Err(why) => {
                g.state = goal::State::Failed;
                g.note = why;
            }
        }
        self.store.goals.push(g);
        self.store.save();
        id
    }

    /// Starts a goal's jobs, the first one now.
    pub fn start_goal(&mut self, st: &AppSettings, id: u64) {
        if let Some(g) = self.store.goals.iter_mut().find(|g| g.id == id) {
            g.state = goal::State::Running;
        }
        self.advance_goals(st);
        self.store.save();
    }

    /// Goes on past a job that stopped.
    pub fn skip_step(&mut self, st: &AppSettings, id: u64) {
        let jobs = self.store.jobs.clone();
        if let Some(g) = self.store.goals.iter_mut().find(|g| g.id == id) {
            if let Some(k) = g.current(&jobs) {
                g.steps[k].skipped = true;
            }
            g.state = goal::State::Running;
        }
        self.advance_goals(st);
        self.store.save();
    }

    pub fn drop_goal(&mut self, id: u64) {
        self.store.goals.retain(|g| g.id != id);
        self.planning.retain(|(g, _)| *g != id);
        self.store.save();
    }

    /// Splits coming back, and running goals moving on.
    fn poll_goals(&mut self, st: &AppSettings) {
        let mut back = Vec::new();
        self.planning.retain(|(id, rx)| match rx.try_recv() {
            Ok(crate::agent::Event::Output(r)) => {
                back.push((*id, r));
                false
            }
            Ok(_) | Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                back.push((*id, Err("it stopped before answering".into())));
                false
            }
        });
        for (id, r) in back {
            if let Some(g) = self.store.goals.iter_mut().find(|g| g.id == id) {
                match r.map(|t| goal::parse_split(&crate::agent::strip_ansi(&t))) {
                    Ok(steps) if !steps.is_empty() => {
                        g.steps = steps;
                        g.state = goal::State::Ready;
                    }
                    Ok(_) => {
                        g.state = goal::State::Failed;
                        g.note = "Squad Code couldn't split this into jobs. Say it another way, or run it as one job.".into();
                    }
                    Err(why) => {
                        g.state = goal::State::Failed;
                        let first = why
                            .lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        g.note = format!("Squad Code couldn't split this into jobs: {first}");
                    }
                }
            }
            self.store.save();
        }
        if self
            .store
            .goals
            .iter()
            .any(|g| g.state == goal::State::Running)
        {
            self.advance_goals(st);
        }
    }

    /// Starts the next job of each running goal once the one before it shipped,
    /// and pauses a goal whose job stopped.
    fn advance_goals(&mut self, st: &AppSettings) {
        let mut start = Vec::new();
        let jobs = self.store.jobs.clone();
        let mut said = Vec::new();
        for g in &mut self.store.goals {
            if g.state != goal::State::Running {
                continue;
            }
            match g.current(&jobs) {
                None => {
                    g.state = goal::State::Done;
                    said.push(format!("Goal done: \u{201c}{}\u{201d}.", g.text));
                }
                Some(k) if g.steps[k].job == 0 => start.push((g.id, k)),
                Some(k) => {
                    if jobs
                        .iter()
                        .find(|j| j.id == g.steps[k].job)
                        .is_some_and(goal::step_stuck)
                    {
                        g.state = goal::State::Paused;
                    }
                }
            }
        }
        for m in said {
            self.say(m);
        }
        for (gid, k) in start {
            let Some(g) = self.store.goals.iter().find(|g| g.id == gid).cloned() else {
                continue;
            };
            let step = g.steps[k].clone();
            let bot = Member::doer_for(step.mode).unwrap_or(Member::for_mode(step.mode));
            let part = Some((gid, goal::context(&g, k)));
            let job = self.make_job(
                st,
                bot,
                step.mode,
                &step.title,
                g.folder.clone(),
                None,
                part,
            );
            let jid = job.id;
            self.store.jobs.push(job);
            if let Some(g) = self.store.goals.iter_mut().find(|g| g.id == gid) {
                g.steps[k].job = jid;
            }
            self.store.save();
        }
    }

    // ---------- the queue from `squad` in a terminal ----------

    /// Jobs and decisions sent from `squad` in a terminal, once a second.
    fn poll_queue(&mut self, st: &AppSettings) {
        if self.queue_at.elapsed() < Duration::from_secs(1) || self.store.demo {
            return;
        }
        self.queue_at = Instant::now();
        for ask in cli::take_queue() {
            self.handle(st, ask);
        }
    }

    /// Fin suggests a bot worth making, once per idea, when the same kind of
    /// job keeps coming up and no bot is for it.
    fn offer_idea(&mut self) {
        let crew = Member::all();
        let Some(idea) = brain::ideas::next(&self.store, &crew, &self.store.offered, store::now())
        else {
            return;
        };
        self.store.offered.push(idea.key.to_string());
        self.say(brain::voice::line(&brain::voice::Moment::Idea {
            name: idea.name.to_string(),
            why: idea.why,
        }));
    }

    /// Makes the bot Peek suggested, set up the way a bot made by hand is,
    /// with the words that led to it as its skills. None when no idea fits.
    pub fn make_idea(&mut self, st: &AppSettings, key: &str) -> Option<u16> {
        let crew = Member::all();
        let idea =
            brain::ideas::next(&self.store, &crew, &[], store::now()).filter(|i| i.key == key)?;
        let skills = idea.words.iter().map(|w| w.to_string()).collect();
        let id = self.make_bot(
            st,
            idea.name,
            idea.role,
            Some(idea.like),
            (skills, Vec::new()),
            [0, 0, 0, 0],
            String::new(),
        );
        self.say(format!(
            "{} joined the crew. It takes jobs about {} from now on.",
            idea.name, idea.key
        ));
        Some(id)
    }

    /// Does one thing asked from the command line (or any other client).
    pub fn handle(&mut self, st: &AppSettings, ask: cli::Ask) {
        match ask {
            cli::Ask::New {
                text,
                mode,
                bot,
                dir,
            } => {
                let folder = self.folder_for(&dir);
                let who = Member::all()
                    .into_iter()
                    .find(|m| m.name() == bot)
                    .or_else(|| Member::doer_for(mode))
                    .unwrap_or(Member::for_mode(mode));
                let text = if text.is_empty() {
                    mode.default_job().unwrap_or("").to_string()
                } else {
                    text
                };
                if super::issues::find(&text).is_some() || self.store.busy(who).is_some() {
                    self.queue_later(who, mode, text.clone(), folder, 0);
                    self.say(format!(
                        "{} has a job from the terminal in line: \u{201c}{text}\u{201d}.",
                        who.name()
                    ));
                    return;
                }
                let job = self.make_job(st, who, mode, &text, folder, None, None);
                self.say(format!(
                    "{} took a job from the terminal: \u{201c}{}\u{201d}.",
                    who.name(),
                    job.title
                ));
                self.store.jobs.push(job);
                self.store.save();
            }
            cli::Ask::Approve { id } => self.approve(id),
            cli::Ask::Discard { id } => self.discard(id),
            cli::Ask::Stop { id } => self.stop(id),
            cli::Ask::Later {
                text,
                mode,
                bot,
                dir,
                at,
            } => {
                let folder = self.folder_for(&dir);
                let who = Member::all()
                    .into_iter()
                    .find(|m| m.name() == bot)
                    .or_else(|| Member::doer_for(mode))
                    .unwrap_or(Member::for_mode(mode));
                let when = super::shift::when_words(at, store::now());
                self.queue_later(who, mode, text.clone(), folder, at);
                self.say(format!(
                    "{} starts \u{201c}{text}\u{201d} {when}, from the terminal.",
                    who.name()
                ));
            }
            cli::Ask::Cancel { id } => {
                self.cancel_later(id);
            }
            cli::Ask::Rewind { id, to } => {
                if let Err(why) = self.rewind(id, to) {
                    self.say(why);
                }
            }
            cli::Ask::Forget { bot, dir, n } => {
                if let Some(who) = Member::all()
                    .into_iter()
                    .find(|m| m.name().eq_ignore_ascii_case(&bot))
                {
                    self.forget_lesson(who, &dir, n.saturating_sub(1));
                }
            }
        }
    }

    /// The project a terminal's folder belongs to: a known one it's inside,
    /// the project of a job's worktree it's in, or a new one.
    pub fn folder_for(&mut self, dir: &str) -> store::Folder {
        let inside = |root: &str| {
            !root.is_empty() && std::path::Path::new(dir).starts_with(std::path::Path::new(root))
        };
        if let Some(j) = self.store.jobs.iter().find(|j| inside(&j.worktree)) {
            return j.folder.clone();
        }
        if let Some(f) = self
            .store
            .folders
            .iter()
            .filter(|f| inside(&f.path))
            .max_by_key(|f| f.path.len())
        {
            return f.clone();
        }
        // Added at the top: the folder you had picked moves down one.
        let current = self.store.current;
        let before = self.store.folders.len();
        self.store.add_folder(std::path::PathBuf::from(dir));
        let f = self.store.folder().cloned().unwrap_or_default();
        if self.store.folders.len() > before && before > 0 {
            self.store.current = current + 1;
        }
        f
    }

    // ---------- review and shipping ----------

    /// The tries in a race, first one first.
    pub fn tries(&self, race: u64) -> Vec<store::Job> {
        let mut v: Vec<_> = self
            .store
            .jobs
            .iter()
            .filter(|j| j.race == race && race != 0)
            .cloned()
            .collect();
        v.sort_by_key(|j| j.id);
        v
    }

    /// Keeps one try of a race: approves it and throws the others away.
    pub fn keep_try(&mut self, id: u64) {
        let Some(race) = self.store.job(id).map(|j| j.race) else {
            return;
        };
        for other in self.tries(race) {
            if other.id == id {
                continue;
            }
            if let Some(pos) = self.runners.iter().position(|r| r.id == other.id) {
                let mut r = self.runners.remove(pos);
                r.stop();
            }
            self.checking.retain(|(j, _)| *j != other.id);
            if matches!(
                other.status,
                Status::Shipped | Status::Shipping | Status::Discarded
            ) {
                continue;
            }
            work::discard(&other);
            if let Some(j) = self.store.job_mut(other.id) {
                j.status = Status::Discarded;
                j.ended.get_or_insert_with(store::now);
                j.worktree.clear();
                j.seen = true;
                j.feed.push(store::Item::Note(
                    "Another try won the race, so this one was thrown away.".into(),
                ));
            }
        }
        self.approve_one(id);
        self.store.save();
    }

    /// A writing job finished in its own worktree: list what it changed and have
    /// Checker run the checks. With nothing changed there's nothing to review.
    fn begin_review(&mut self, id: u64) {
        let Some(job) = self.store.job_mut(id) else {
            return;
        };
        if job.status != Status::Done || job.worktree.is_empty() {
            return;
        }
        job.changes = work::changed(job);
        if job.changes.is_empty() {
            work::discard(job);
            job.worktree.clear();
            job.feed
                .push(store::Item::Note("It didn't change any files.".into()));
            return;
        }
        job.status = Status::Review;
        job.seen = false;
        job.checks.clear();
        if !work::belt_checks() {
            return;
        }
        job.checking = true;
        let dir = job.worktree.clone();
        self.checking.push((id, work::run_checks(dir)));
    }

    /// Runs the checks on a job's changes again.
    pub fn recheck(&mut self, id: u64) {
        if self.on_server(id, "recheck", serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job_mut(id) else {
            return;
        };
        if job.worktree.is_empty() || job.checking {
            return;
        }
        job.checks.clear();
        job.checking = true;
        let dir = job.worktree.clone();
        self.checking.retain(|(j, _)| *j != id);
        self.checking.push((id, work::run_checks(dir)));
    }

    /// What Approve does for this job's project.
    pub fn way(&mut self, job: &store::Job) -> ship::Way {
        let key = format!("{}\u{0}{}", job.folder.path, job.folder.ship);
        if let Some(w) = self.ways.get(&key) {
            return *w;
        }
        let ship_pref = self
            .store
            .folders
            .iter()
            .find(|f| f.path == job.folder.path)
            .map(|f| f.ship.clone())
            .unwrap_or_default();
        let w = ship::way_for(&job.folder.path, &ship_pref);
        self.ways.insert(key, w);
        w
    }

    /// Switches what Approve does in a job's project: pull request or local merge.
    pub fn set_way(&mut self, id: u64, way: ship::Way) {
        let Some(path) = self.store.job(id).map(|j| j.folder.path.clone()) else {
            return;
        };
        let pref = match way {
            ship::Way::PullRequest => "pr",
            ship::Way::Merge => "merge",
        };
        for f in &mut self.store.folders {
            if f.path == path {
                f.ship = pref.into();
            }
        }
        for j in &mut self.store.jobs {
            if j.folder.path == path {
                j.folder.ship = pref.into();
            }
        }
        self.ways.clear();
        self.store.save();
    }

    /// Approves a job's changes: a pull request from Shipper on GitHub projects,
    /// otherwise merged into the branch it started from. Approving one try of a
    /// race keeps it and throws the others away.
    pub fn approve(&mut self, id: u64) {
        if self.bring_home(id) {
            return;
        }
        if self.on_server(id, "approve", serde_json::Value::Null) {
            return;
        }
        let race = self.store.job(id).map(|j| j.race).unwrap_or(0);
        if race != 0 && self.tries(race).len() > 1 {
            self.keep_try(id);
        } else {
            self.approve_one(id);
        }
    }

    /// Approving a job that ran on your server brings its work home first:
    /// its branch comes back here, and then it merges (or becomes a pull
    /// request) exactly like a job that ran here. False when it isn't one.
    fn bring_home(&mut self, id: u64) -> bool {
        let Some(job) = self.store.job(id).cloned() else {
            return false;
        };
        let Some(link) = &self.link else {
            return false;
        };
        if job.remote_on.is_empty()
            || job.remote == 0
            || link.cfg.label() != job.remote_on
            || job.status != Status::Review
            || job.branch.is_empty()
            || !std::path::Path::new(&job.folder.path).is_dir()
            || self.bringing.iter().any(|(j, _)| *j == id)
        {
            return false;
        }
        let url = ship::remote_url(&job.folder.path).unwrap_or_default();
        let Some(there) = link.path_for(&job.folder, &url) else {
            return false;
        };
        let rx = super::remote::bring_home_later(link.cfg.clone(), job.clone(), there);
        self.bringing.push((id, rx));
        if let Some(j) = self.store.job_mut(id) {
            j.seen = true;
            j.feed.push(store::Item::Note(format!(
                "Approved. Bringing its work home from {} first.",
                job.remote_on
            )));
        }
        self.store.save();
        true
    }

    /// Server jobs whose work came home: approve them here, and let the
    /// server drop its copy.
    fn poll_bringing(&mut self) {
        let mut done = Vec::new();
        self.bringing.retain(|(id, rx)| match rx.try_recv() {
            Ok(r) => {
                done.push((*id, r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, result) in done {
            let Some(j) = self.store.job_mut(id) else {
                continue;
            };
            let name = j.remote_on.clone();
            match result {
                Ok((worktree, branch)) => {
                    let remote = j.remote;
                    j.worktree = worktree;
                    j.branch = branch;
                    j.remote_on.clear();
                    j.remote = 0;
                    j.remote_dir.clear();
                    j.feed.push(store::Item::Note(format!(
                        "Its work is home on {}, so it lands here like any job.",
                        j.branch
                    )));
                    if let Some(link) = &self.link {
                        link.act(remote, "discard", serde_json::Value::Null);
                    }
                    self.approve_one(id);
                }
                Err(why) => {
                    j.feed.push(store::Item::Error(format!(
                        "Couldn't bring it home from {name}: {why}. It's still waiting there."
                    )));
                    self.say("Couldn't bring that job home. The reason is on the job.");
                }
            }
        }
        self.store.save();
    }

    fn approve_one(&mut self, id: u64) {
        let Some(job) = self.store.job(id).cloned() else {
            return;
        };
        if job.status != Status::Review {
            return;
        }
        let way = self.way(&job);
        if !self.belt_lets_through(&job, way) {
            return;
        }
        if way == ship::Way::PullRequest {
            if let Some(j) = self.store.job_mut(id) {
                j.status = Status::Shipping;
                j.shipping = true;
                j.seen = true;
                j.feed.push(store::Item::By(
                    Member::Shipper,
                    "Approved. Opening a pull request.".into(),
                ));
            }
            self.shipping.push((id, ship::open_pr(job)));
            self.store.save();
            return;
        }
        // Your uncommitted changes in the way are set aside for the merge and
        // put back after, the way git's --autostash does; you're asked only
        // when they clash with what came in.
        match work::merge(&job, true) {
            Ok(merged) => {
                let msg = format!("Merged into {} as {}.", job.base, merged.hash);
                if let Some(j) = self.store.job_mut(id) {
                    j.status = Status::Shipped;
                    j.shipped = msg.clone();
                    j.worktree.clear();
                    j.seen = true;
                    j.feed.push(store::Item::Note(msg.clone()));
                    let files = merged.set_aside.join(", ");
                    if let Some(sha) = merged.kept {
                        j.feed.push(store::Item::Note(format!(
                            "Your uncommitted changes to {files} clash with what was merged, so they're set aside for now."
                        )));
                        j.in_the_way = merged.set_aside;
                        j.kept = sha;
                        j.seen = false;
                    } else if !files.is_empty() {
                        j.feed.push(store::Item::Note(format!(
                            "Your uncommitted changes to {files} were set aside for the merge and are back in place."
                        )));
                    }
                }
                self.say(msg);
            }
            Err(crate::git::MergeError::InTheWay { dir, files }) => {
                // Only when not setting aside, which approving always does.
                if let Some(j) = self.store.job_mut(id) {
                    j.feed.push(store::Item::Error(format!(
                        "Couldn't merge it: {}",
                        crate::git::MergeError::InTheWay { dir, files }
                    )));
                }
                self.say("Couldn't merge it. The reason is on the job.");
            }
            Err(crate::git::MergeError::Failed(why)) => {
                let files = trouble::conflict_files(&why);
                if let Some(j) = self.store.job_mut(id) {
                    let error = format!("Couldn't merge it: {why}");
                    let repeat =
                        matches!(j.feed.last(), Some(store::Item::Error(e)) if *e == error);
                    if files.is_empty() {
                        if !repeat {
                            j.feed.push(store::Item::Error(error));
                        }
                    } else {
                        j.feed.push(store::Item::Note(format!(
                            "{} moved on and {} now clash, so nothing was merged.",
                            j.base,
                            files.join(", ")
                        )));
                        j.conflict = files;
                    }
                }
                self.say("Couldn't merge it. The reason is on the job.");
            }
        }
        self.store.save();
    }

    /// The seatbelt, before anything is pushed or merged: secrets, key files
    /// and clash markers hold the job until the person decides; debugging
    /// leftovers are only pointed out. Whether Approve can go on.
    fn belt_lets_through(&mut self, job: &store::Job, way: ship::Way) -> bool {
        if job.belt_ok || !work::belt_secrets() {
            return true;
        }
        let found = work::seatbelt(job);
        let Some(j) = self.store.job_mut(job.id) else {
            return false;
        };
        j.held.clear();
        if found.is_empty() {
            return true;
        }
        let lines = work::held_lines(&found);
        if !work::holds(&found) {
            j.feed.push(store::Item::By(
                Member::Shipper,
                format!(
                    "Heads up, left in from debugging: {}. Going ahead.",
                    lines.join("; ")
                ),
            ));
            return true;
        }
        let what = match way {
            ship::Way::PullRequest => "pushing",
            ship::Way::Merge => "merging",
        };
        j.feed.push(store::Item::By(
            Member::Shipper,
            format!(
                "I stopped before {what}. Something in this change shouldn't leave your computer: {}",
                crate::seatbelt::summary(&found)
            ),
        ));
        j.held = lines;
        j.seen = false;
        self.say("Shipper held it back: the seatbelt found something. It's on the job.");
        self.store.save();
        false
    }

    /// Approves a job the seatbelt held, as it is.
    pub fn approve_anyway(&mut self, id: u64) {
        if self.on_server(id, "approve-anyway", serde_json::Value::Null) {
            return;
        }
        if let Some(j) = self.store.job_mut(id) {
            j.belt_ok = true;
            j.held.clear();
            j.feed.push(store::Item::Note(
                "You said to go ahead in spite of the seatbelt.".into(),
            ));
        }
        self.approve(id);
    }

    /// What apps that asked before setting changes aside send: approving
    /// does that now.
    pub fn set_aside_and_merge(&mut self, id: u64) {
        self.approve(id);
    }

    /// Your changes a merge set aside and couldn't put back, brought back into
    /// your checkout with the clash marked in the files.
    pub fn bring_back(&mut self, id: u64) {
        if self.on_server(id, "bring-back", serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job(id).cloned() else {
            return;
        };
        if job.kept.is_empty() {
            return;
        }
        let dir = crate::git::checked_out_at(&job.folder.path, &job.base)
            .unwrap_or_else(|| job.folder.path.clone());
        let back = crate::git::bring_back(&dir, &job.kept);
        let Some(j) = self.store.job_mut(id) else {
            return;
        };
        match back {
            Ok(clash) if clash.is_empty() => {
                j.feed.push(store::Item::Note(format!(
                    "Your changes to {} are back in {dir}.",
                    j.in_the_way.join(", ")
                )));
                j.kept.clear();
                j.in_the_way.clear();
            }
            Ok(clash) => {
                j.feed.push(store::Item::Note(format!(
                    "Your changes are back in {dir}. {} {} both your lines and the merged ones, between <<<<<<< and >>>>>>>: keep what you want there and save.",
                    clash.join(", "),
                    if clash.len() == 1 { "has" } else { "have" }
                )));
                j.kept.clear();
                j.in_the_way.clear();
            }
            Err(why) => {
                j.feed.push(store::Item::Error(format!(
                    "Couldn't bring your changes back: {why}"
                )));
                if why.contains("aren't in the stash") {
                    j.kept.clear();
                    j.in_the_way.clear();
                }
            }
        }
        self.store.save();
    }

    /// Leaves the changes a merge set aside in the stash, and puts the card
    /// away. `git stash list` shows them as "Squad Code: set aside to merge …".
    pub fn leave_kept(&mut self, id: u64) {
        if self.on_server(id, "leave-kept", serde_json::Value::Null) {
            return;
        }
        let Some(j) = self.store.job_mut(id) else {
            return;
        };
        if j.kept.is_empty() {
            return;
        }
        j.feed.push(store::Item::Note(format!(
            "Your changes to {} stay in the stash, as \"Squad Code: set aside to merge {}\".",
            j.in_the_way.join(", "),
            j.branch
        )));
        j.kept.clear();
        j.in_the_way.clear();
        self.store.save();
    }

    /// Hands what the seatbelt found back to the job's bot to take out; it
    /// comes back for review when it's done.
    pub fn take_out(&mut self, st: &AppSettings, id: u64) {
        if self.on_server(id, "take-out", serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job(id) else {
            return;
        };
        if job.held.is_empty() {
            return;
        }
        let text = format!(
            "Before this can be pushed, take these out of the change: {}. Move secrets to environment variables or a file git ignores, keep an example file with placeholder values if the project needs one, and remove debugging leftovers. Then run the project's checks and commit.",
            job.held.join("; ")
        );
        if let Some(j) = self.store.job_mut(id) {
            j.held.clear();
        }
        self.send_on(
            st,
            id,
            text,
            Some("Sent back to take out what the seatbelt found.".into()),
        );
    }

    /// Every 45 seconds, off the window's thread: how far each job's branch is
    /// ahead of where it started and how far its base moved, and which jobs in
    /// the same project change the same lines. A new clash is told once, by
    /// Peek, on both jobs.
    fn poll_git(&mut self) {
        let before = self.fetching.len();
        self.fetching
            .retain(|rx| matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
        if self.fetching.len() < before && self.git_look.is_none() {
            self.look_at_git_soon();
        }
        if let Some(rx) = &self.git_look {
            match rx.try_recv() {
                Ok(look) => {
                    self.git_look = None;
                    self.took_git_look(look);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.git_look = None,
            }
        }
        // A demo store's folders aren't real projects.
        if self.store.demo || self.git_looked.elapsed() < Duration::from_secs(45) {
            return;
        }
        self.git_looked = Instant::now();
        let live: Vec<store::Job> = self
            .store
            .jobs
            .iter()
            .filter(|j| {
                !j.worktree.is_empty()
                    && j.remote_on.is_empty()
                    && matches!(j.status, Status::Running | Status::Done | Status::Review)
            })
            .cloned()
            .collect();
        let mut folders: Vec<String> = live.iter().map(|j| j.folder.path.clone()).collect();
        if let Some(f) = self.store.folder() {
            folders.push(f.path.clone());
        }
        folders.sort();
        folders.dedup();
        folders.retain(|f| !f.is_empty());
        if live.is_empty() && folders.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let repos = folders
                .into_iter()
                .map(|f| {
                    // A folder that isn't a repository is noted too, so the Git
                    // side can say so rather than wait for a look that never comes.
                    if !crate::git::is_repo(&f) {
                        return (f, RepoLine::default());
                    }
                    let (changes, changed) = crate::git::changed_paths(&f, 8);
                    let line = RepoLine {
                        is_repo: true,
                        state: crate::git::repo_state(&f),
                        dirty: changed > 0,
                        changes,
                        changed,
                        commits: crate::git::log_lines(&f, 6),
                    };
                    (f, line)
                })
                .collect();
            let counts = live
                .iter()
                .map(|j| {
                    let (ahead, behind) = work::counts(j);
                    (j.id, ahead, behind)
                })
                .collect();
            let lanes: Vec<work::Lane> = live
                .iter()
                .map(|j| work::Lane {
                    id: j.id,
                    repo: j.folder.path.clone(),
                    worktree: j.worktree.clone(),
                })
                .collect();
            let _ = tx.send(GitLook {
                counts,
                clashes: work::clashes(&lanes),
                repos,
            });
        });
        self.git_look = Some(rx);
    }

    /// Asks the folder's remote what's new, off the window's thread, then looks
    /// again. Nothing in the folder changes.
    pub fn fetch(&mut self, folder: &str) {
        let folder = folder.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = crate::git::fetch(&folder);
            let _ = tx.send(());
        });
        self.fetching.push(rx);
    }

    /// Looks at git on the next poll rather than waiting for the next round.
    pub fn look_at_git_soon(&mut self) {
        self.git_looked = Instant::now() - Duration::from_secs(3600);
    }

    fn took_git_look(&mut self, look: GitLook) {
        let looked: Vec<u64> = look.counts.iter().map(|(id, ..)| *id).collect();
        for (folder, line) in look.repos {
            self.repos.insert(folder, line);
        }
        let mut changed = false;
        for (id, ahead, behind) in look.counts {
            if let Some(j) = self.store.job_mut(id) {
                if (j.ahead, j.behind) != (ahead, behind) {
                    (j.ahead, j.behind) = (ahead, behind);
                    changed = true;
                }
            }
        }
        // Clashes that went away are dropped; new ones are told. Only the jobs
        // this look covered: the others keep what they had.
        let pairs: Vec<(u64, u64, String)> = look.clashes;
        for id in looked {
            let now: Vec<(u64, String)> = pairs
                .iter()
                .filter_map(|(a, b, w)| {
                    if *a == id {
                        Some((*b, w.clone()))
                    } else if *b == id {
                        Some((*a, w.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            let titles: Vec<(u64, String)> = now
                .iter()
                .map(|(other, _)| {
                    let t = self
                        .store
                        .job(*other)
                        .map(|o| o.label().to_string())
                        .unwrap_or_default();
                    (*other, t)
                })
                .collect();
            let Some(j) = self.store.job_mut(id) else {
                continue;
            };
            if j.clash_with == now {
                continue;
            }
            for (other, words) in &now {
                if j.clash_with.iter().any(|(o, _)| o == other) {
                    continue;
                }
                let title = titles
                    .iter()
                    .find(|(o, _)| o == other)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                j.feed.push(store::Item::By(
                    Member::Fin,
                    format!(
                        "Heads up: #{other} {title} changes the same lines, in {words}. Whichever is approved second will need its branch brought up to date."
                    ),
                ));
                j.seen = false;
            }
            j.clash_with = now;
            changed = true;
        }
        if changed {
            self.store.save();
        }
    }

    /// Throws a job's changes away, with its branch and worktree.
    pub fn discard(&mut self, id: u64) {
        if self.on_server(id, "discard", serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job(id).cloned() else {
            return;
        };
        work::discard(&job);
        if let Some(j) = self.store.job_mut(id) {
            j.status = Status::Discarded;
            j.worktree.clear();
            j.seen = true;
            j.feed
                .push(store::Item::Note("You discarded its changes.".into()));
        }
        self.say("Discarded. Your checkout was never touched.");
        self.store.save();
    }

    /// Merges (or closes) a job's pull request on GitHub now.
    pub fn land_pr(&mut self, id: u64, merge: bool) {
        let action = if merge { "merge-pr" } else { "close-pr" };
        if self.on_server(id, action, serde_json::Value::Null) {
            return;
        }
        let Some(job) = self.store.job(id) else {
            return;
        };
        if job.pr.is_empty() || self.landing.iter().any(|(j, ..)| *j == id) {
            return;
        }
        let (dir, url) = (job.folder.path.clone(), job.pr.clone());
        let rx = if merge {
            ship::merge_pr(dir, url)
        } else {
            ship::close_pr(dir, url)
        };
        self.landing.push((id, merge, rx));
        if let Some(j) = self.store.job_mut(id) {
            j.feed.push(store::Item::By(
                Member::Shipper,
                if merge {
                    "Merging the pull request.".into()
                } else {
                    "Closing the pull request.".into()
                },
            ));
        }
    }

    /// Turns "merge when checks pass" on or off for a job's pull request.
    pub fn set_auto_merge(&mut self, id: u64, on: bool) {
        if let Some(j) = self.store.job_mut(id) {
            j.auto_merge = on;
        }
        self.store.save();
    }

    /// Shipper's side of the poll: steps while opening pull requests, a look at
    /// open ones every half minute, and merges or closes coming back.
    fn poll_ship(&mut self) {
        let mut steps = Vec::new();
        self.shipping.retain(|(id, rx)| loop {
            match rx.try_recv() {
                Ok(step) => {
                    let end = !matches!(step, ship::Step::Did(_));
                    steps.push((*id, step));
                    if end {
                        break false;
                    }
                }
                Err(TryRecvError::Empty) => break true,
                Err(TryRecvError::Disconnected) => break false,
            }
        });
        let mut look_now = false;
        for (id, step) in steps {
            let Some(j) = self.store.job_mut(id) else {
                continue;
            };
            match step {
                ship::Step::Did(what) => j.feed.push(store::Item::By(Member::Shipper, what)),
                ship::Step::Opened(url) => {
                    j.shipping = false;
                    j.pr_number = url
                        .rsplit('/')
                        .next()
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    j.pr = url.clone();
                    j.pr_state = "OPEN".into();
                    j.feed.push(store::Item::By(
                        Member::Shipper,
                        format!("Opened the pull request: {url}"),
                    ));
                    look_now = true;
                }
                ship::Step::Conflict(files) => {
                    j.shipping = false;
                    j.status = Status::Review;
                    j.seen = false;
                    j.feed.push(store::Item::By(
                        Member::Shipper,
                        format!(
                            "{} moved while this was waiting, and {} now clash with it, so I stopped before pushing.",
                            j.base,
                            files.join(", ")
                        ),
                    ));
                    j.conflict = files;
                }
                ship::Step::Failed(why) => {
                    j.shipping = false;
                    j.status = Status::Review;
                    j.seen = false;
                    j.feed.push(store::Item::Error(format!(
                        "Shipper couldn't open the pull request: {why}"
                    )));
                }
            }
            self.store.save();
        }
        // A look at every open pull request every 30 seconds.
        if look_now || self.watched.elapsed() > Duration::from_secs(30) {
            self.watched = Instant::now();
            let open: Vec<(u64, String, String)> = self
                .store
                .jobs
                .iter()
                .filter(|j| j.status == Status::Shipping && !j.pr.is_empty())
                .map(|j| (j.id, j.folder.path.clone(), j.pr.clone()))
                .collect();
            for (id, dir, url) in open {
                if !self.watching.iter().any(|(j, _)| *j == id) {
                    self.watching.push((id, ship::watch(dir, url)));
                }
            }
        }
        let mut seen = Vec::new();
        self.watching.retain(|(id, rx)| match rx.try_recv() {
            Ok(pr) => {
                seen.push((*id, pr));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        let mut merge_now = Vec::new();
        let mut landed = Vec::new();
        let mut due_feedback = Vec::new();
        for (id, pr) in seen {
            let Some(pr) = pr else { continue };
            let busy = self.landing.iter().any(|(j, ..)| *j == id);
            let Some(j) = self.store.job_mut(id) else {
                continue;
            };
            j.pr_number = pr.number;
            j.pr_line = ship::checks_line(&pr);
            j.pr_state = pr.state.clone();
            // New review feedback from the repository's people goes to the
            // job's bot. What was there before Squad started watching for it
            // stays with you.
            if pr.state == "OPEN" {
                if j.pr_seen.is_empty() {
                    j.pr_seen = pr
                        .voices
                        .iter()
                        .map(|v| v.at.clone())
                        .max()
                        .unwrap_or_else(|| "0".into());
                } else if let Some(at) = crate::prwatch::new_feedback(&pr, &j.pr_seen) {
                    if !j.answering && !busy {
                        j.pr_seen = at;
                        due_feedback.push((id, pr.clone()));
                    }
                }
            }
            for name in &pr.failed {
                if !j.pr_failed.contains(name) {
                    j.pr_failed.push(name.clone());
                    j.seen = false;
                    j.feed.push(store::Item::By(
                        Member::Shipper,
                        format!("{name} failed on GitHub. Write below to send it back and fix it."),
                    ));
                }
            }
            match pr.state.as_str() {
                "MERGED" => landed.push((id, true)),
                "CLOSED" => landed.push((id, false)),
                _ => {
                    let green = pr.failed.is_empty() && pr.pending == 0;
                    if j.auto_merge
                        && green
                        && pr.mergeable != "CONFLICTING"
                        && !busy
                        && !j.answering
                    {
                        merge_now.push(id);
                    }
                }
            }
        }
        self.feedback_due.extend(due_feedback);
        for id in merge_now {
            if let Some(j) = self.store.job_mut(id) {
                j.feed.push(store::Item::By(
                    Member::Shipper,
                    "Checks pass. Merging, as you asked.".into(),
                ));
                let (dir, url) = (j.folder.path.clone(), j.pr.clone());
                self.landing.push((id, true, ship::merge_pr(dir, url)));
            }
        }
        let mut came_back = Vec::new();
        self.landing.retain(|(id, merge, rx)| match rx.try_recv() {
            Ok(r) => {
                came_back.push((*id, *merge, r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, merge, r) in came_back {
            match r {
                Ok(()) => landed.push((id, merge)),
                Err(why) => {
                    if let Some(j) = self.store.job_mut(id) {
                        j.feed
                            .push(store::Item::Error(format!("GitHub said no: {why}")));
                    }
                }
            }
        }
        for (id, merged) in landed {
            let Some(job) = self.store.job(id).cloned() else {
                continue;
            };
            if !matches!(job.status, Status::Shipping) {
                continue;
            }
            // The branch lives on GitHub now (or is gone): the local copy can go.
            work::discard(&job);
            let msg = if merged {
                format!(
                    "Merged #{} into {}. Cleaned up the branch and worktree.",
                    job.pr_number, job.base
                )
            } else {
                format!(
                    "Closed #{} without merging. Cleaned up the branch and worktree.",
                    job.pr_number
                )
            };
            if let Some(j) = self.store.job_mut(id) {
                j.status = if merged {
                    Status::Shipped
                } else {
                    Status::Discarded
                };
                j.pr_state = if merged { "MERGED" } else { "CLOSED" }.into();
                j.worktree.clear();
                j.shipped = msg.clone();
                j.feed.push(store::Item::By(Member::Shipper, msg.clone()));
            }
            self.say(msg);
            self.store.save();
        }
    }

    // ---------- thread summaries ----------

    /// Whether a thread's summary is being written now.
    pub fn summing(&self, folder: &str, thread: u64) -> bool {
        self.summing
            .as_ref()
            .is_some_and(|s| s.folder == folder && s.thread == thread)
    }

    /// Keeps each thread's summary current, in the background and one at a
    /// time: it takes in a written summary, then, every few seconds, starts
    /// the next one that's due in the project open now.
    fn poll_summaries(&mut self, st: &AppSettings) {
        if let Some(s) = &self.summing {
            let reply = match s.rx.try_recv() {
                Ok(crate::agent::Event::Output(r)) => r.ok(),
                Ok(_) | Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            };
            let Some(s) = self.summing.take() else {
                return;
            };
            let text = reply
                .map(|r| brain::summary::clean(&crate::agent::strip_ansi(&r)))
                .unwrap_or_default();
            // A summary that didn't come back keeps the old one; it's tried
            // again once more is said.
            if !text.is_empty() {
                let sum = self.store.summary_mut(&s.folder, s.thread);
                sum.text = text;
                sum.covers = s.covers;
                sum.upto = s.upto;
                sum.updates = if s.fresh { 0 } else { sum.updates + 1 };
                sum.at = store::now();
                self.store.save();
            }
            return;
        }
        let now_asked = self.summary_now.is_some();
        if cfg!(test)
            || self.store.demo
            || (!now_asked
                && self
                    .summed_at
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(15)))
        {
            return;
        }
        self.summed_at = Some(Instant::now());
        let Some(folder) = self.store.folder().cloned() else {
            return;
        };
        let mut threads = vec![0];
        threads.extend(
            self.store
                .threads
                .iter()
                .filter(|t| t.folder == folder.path && !t.archived)
                .map(|t| t.id),
        );
        // Asked for now: that thread first, written afresh.
        let asked = self
            .summary_now
            .take()
            .filter(|(f, _)| *f == folder.path)
            .map(|(_, t)| t);
        if let Some(t) = asked {
            threads.retain(|x| *x != t);
            threads.insert(0, t);
        }
        for thread in threads {
            // Not while a question here waits for its answer: the answer
            // would land among words already summed up.
            let waiting = self.store.talks.iter().any(|t| {
                t.folder == folder.path
                    && self.store.thread_for(&folder.path, t.thread) == thread
                    && t.answer.is_none()
            });
            if waiting {
                continue;
            }
            let said = self.store.said_in(&folder.path, thread);
            let sum = self
                .store
                .summary(&folder.path, thread)
                .cloned()
                .unwrap_or_default();
            let new = said.len().saturating_sub(sum.covers);
            let ended = self
                .store
                .jobs_in(&folder.path, thread)
                .iter()
                .any(|j| j.ended.is_some_and(|e| e > sum.at));
            let forced = asked == Some(thread) && !said.is_empty();
            if !forced && !brain::summary::due(new, ended) {
                continue;
            }
            let fresh = forced || sum.text.is_empty() || sum.updates >= brain::summary::REBUILD;
            let take = if fresh { 60 } else { new.max(1) };
            let input: Vec<(String, String)> = said
                .iter()
                .skip(said.len().saturating_sub(take))
                .map(|(_, who, w)| (who.clone(), w.clone()))
                .collect();
            let label = self.store.thread_label(thread);
            let prompt = brain::summary::prompt(
                &folder.name,
                &label,
                if fresh { "" } else { &sum.text },
                &sum.notes,
                &input,
            );
            let i = crate::settings::assistant_provider(st);
            let p = &crate::agent::PROVIDERS[i];
            let configured = st.agent(p.id).binary;
            if !crate::agent::available(p.bin, &configured) {
                return;
            }
            let model = crate::settings::assistant_model(st, p.id);
            let command = crate::agent::ask_command_on(p.id, &configured, &prompt, &model);
            let (tx, rx) = std::sync::mpsc::channel();
            crate::agent::oneshot_within(command, &folder.path, Duration::from_secs(90), tx);
            self.summing = Some(Summing {
                folder: folder.path.clone(),
                thread,
                covers: said.len(),
                upto: said.last().map(|m| m.0).unwrap_or(0),
                fresh,
                rx,
            });
            return;
        }
    }

    // ---------- questions ----------

    /// Asks `who` for real, on its own engine, in the current folder.
    pub fn ask(&mut self, st: &AppSettings, who: Member, text: String) {
        let history: Vec<(bool, String)> = self
            .store
            .chat(who)
            .iter()
            .filter(|m| !m.note)
            .map(|m| (m.mine, m.text.clone()))
            .collect();
        self.store.say_in_dm(who, Said::mine(text.clone()));
        if self.asking.is_some() {
            self.store.say_in_dm(
                who,
                Said::note("One question at a time: ask again when the last answer is in."),
            );
            return;
        }
        let Some(folder) = self.store.folders.get(self.store.current).cloned() else {
            self.store.say_in_dm(
                who,
                Said::note("Pick a folder first (on Today), so the crew knows which code to read."),
            );
            return;
        };
        // One conversation per bot: it reads its notebook for this project.
        let notes = self.store.notebook(who, &folder.path);
        let prompt = if notes.is_empty() {
            text.clone()
        } else {
            format!("({notes}\n)\n\n{text}")
        };
        match engine::Ask::start(st, who, &folder, &prompt, &history) {
            Ok(ask) => self.asking = Some(ask),
            Err(why) => self.store.say_in_dm(who, Said::note(why)),
        }
        self.store.save();
    }

    /// Asks `who` a question in the thread you're in. The answer lands in the
    /// thread, with the thread's earlier questions as context.
    pub fn ask_in_thread(&mut self, st: &AppSettings, who: Member, text: String) {
        let Some(folder) = self.store.folders.get(self.store.current).cloned() else {
            return;
        };
        let thread = self.store.thread_here();
        let mut history: Vec<(bool, String)> = Vec::new();
        // Threads named with #: the bot reads what was said there first.
        for other in self.store.thread_refs(&folder.path, &text) {
            if other == thread {
                continue;
            }
            let said = self.store.recent(&folder.path, other);
            if said.is_empty() {
                continue;
            }
            let label = self.store.thread_label(other);
            let lines: Vec<String> = said
                .iter()
                .map(|(who, words)| format!("{who}: {words}"))
                .collect();
            history.push((
                true,
                format!(
                    "For context, here is what was said in #{label}:\n{}",
                    lines.join("\n")
                ),
            ));
            history.push((false, format!("Got it, I've read #{label}.")));
        }
        // The thread's notes, summary, jobs and latest messages, so a reply
        // like "no, this is for discussion" makes sense.
        let talk_so_far = self.store.thread_context(&folder.path, thread, &text);
        let id = self.store.talks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let mut talk = Talk {
            id,
            folder: folder.path.clone(),
            thread,
            bot: who,
            question: text.clone(),
            answer: None,
            at: store::now(),
            choices: Vec::new(),
        };
        if self.asking.is_some() {
            talk.answer = Some(Said::note(
                "One question at a time: ask again when the last answer is in.",
            ));
        } else {
            // Jobs named with #12 or #their-slug: a short digest of each goes first.
            let digest = super::brain::digest::context_for(&text, &self.store.jobs);
            let asked = [talk_so_far, digest, text.clone()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            match engine::Ask::start(st, who, &folder, &asked, &history) {
                Ok(ask) => {
                    self.asking = Some(ask);
                    self.asking_talk = Some(id);
                }
                Err(why) => talk.answer = Some(Said::note(why)),
            }
        }
        self.store.talks.push(talk);
        self.store.save();
    }

    /// Peek says something in the thread you're in without an engine: a reply
    /// to small talk, or a question back with quick answers.
    /// Peek's message for `text` while its engine reads it: no answer yet.
    /// Returns its id, for `fill_talk`.
    pub fn peek_reading(&mut self, text: String) -> u64 {
        let Some(folder) = self.store.folders.get(self.store.current).cloned() else {
            return 0;
        };
        let id = self.store.talks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let thread = self.store.thread_here();
        self.store.talks.push(Talk {
            id,
            folder: folder.path,
            thread,
            bot: Member::Fin,
            question: text,
            answer: None,
            at: store::now(),
            choices: Vec::new(),
        });
        id
    }

    /// Puts Peek's reply into its message `id`; false when it's gone.
    pub fn fill_talk(&mut self, id: u64, reply: String, choices: Vec<String>) -> bool {
        let Some(t) = self.store.talks.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        t.answer = Some(Said {
            text: reply,
            ..Default::default()
        });
        t.choices = choices;
        self.store.save();
        true
    }

    pub fn peek_says(&mut self, text: String, reply: String, choices: Vec<String>) {
        let Some(folder) = self.store.folders.get(self.store.current).cloned() else {
            return;
        };
        let id = self.store.talks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let thread = self.store.thread_here();
        self.store.talks.push(Talk {
            id,
            folder: folder.path,
            thread,
            bot: Member::Fin,
            question: text,
            answer: Some(Said {
                text: reply,
                ..Default::default()
            }),
            at: store::now(),
            choices,
        });
        self.store.save();
    }

    // ---------- the poll ----------

    /// Moves everything along: answers and job output that have arrived, checks
    /// and pull requests coming back, goals, and the terminal's queue. `viewing`
    /// is the job a client has open, which counts as seen when it ends. Returns
    /// the jobs whose runs ended, for the client to say so.
    pub fn poll(&mut self, st: &AppSettings, viewing: Option<u64>) -> Vec<u64> {
        self.poll_summaries(st);
        // Requests from running jobs' agents, read about once a second.
        if self.asks_at.elapsed() > Duration::from_millis(800) {
            self.asks_at = Instant::now();
            self.asks = if self.runners.is_empty() {
                Vec::new()
            } else {
                let now = store::now();
                crate::approvals::pending(&crate::approvals::folder())
                    .into_iter()
                    .filter(|r| !r.expired(now))
                    .collect()
            };
            // An hour without an answer: the bot hears no, rather than waiting on.
            let stale: Vec<String> = self
                .asks
                .iter()
                .filter(|r| store::now().saturating_sub(r.asked) > 3600)
                .map(|r| r.id.clone())
                .collect();
            for id in stale {
                self.answer_ask(&id, false, "No answer");
            }
        }
        if let Some(ask) = &self.asking {
            if let Some(reply) = ask.poll() {
                let (who, engine) = (ask.who, ask.engine.clone());
                let secs = ask.started.elapsed().as_secs_f32();
                let said = match reply {
                    engine::Reply::Answer(text) if !text.is_empty() => {
                        let (words, job) = engine::split_job(&text);
                        let words = if words.is_empty() {
                            "I can do that as a job.".to_string()
                        } else {
                            words
                        };
                        let mut said = Said::bot(words, format!("{engine} · {secs:.0}s"));
                        said.job = job;
                        said
                    }
                    engine::Reply::Answer(_) => {
                        Said::note("The engine answered with nothing. Try asking again.")
                    }
                    engine::Reply::Failed(why) => Said::note(why),
                };
                // A thread question's answer goes back to its thread.
                match self
                    .asking_talk
                    .take()
                    .and_then(|id| self.store.talks.iter_mut().find(|t| t.id == id))
                {
                    Some(t) => t.answer = Some(said),
                    None => self.store.say_in_dm(who, said),
                }
                self.asking = None;
                self.store.save();
            }
        }
        let mut finished = Vec::new();
        for r in &mut self.runners {
            if let Some(job) = self.store.jobs.iter_mut().find(|j| j.id == r.id) {
                if r.pump(job) {
                    finished.push(r.id);
                    if viewing == Some(job.id) {
                        job.seen = true;
                    }
                }
            } else {
                finished.push(r.id);
            }
        }
        // While jobs run, what they've done so far is kept every few seconds.
        if !self.runners.is_empty() && self.saved.elapsed() > Duration::from_secs(5) {
            self.saved = Instant::now();
            self.store.save();
        }
        self.poll_ship();
        self.poll_git();
        // Checker's runs of the checks coming back.
        let mut back = Vec::new();
        self.checking.retain(|(id, rx)| match rx.try_recv() {
            Ok(results) => {
                back.push((*id, results));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                back.push((*id, Vec::new()));
                false
            }
        });
        for (id, results) in back {
            let mut retry = None;
            let mut merge = false;
            if let Some(job) = self.store.job_mut(id) {
                job.checks = results;
                super::work::ignore(&mut job.checks, &job.ignored_checks);
                job.checking = false;
                merge = job.merge_on_green
                    && job.status == Status::Review
                    && job.held.is_empty()
                    && job.conflict.is_empty()
                    && super::work::verdict(&job.checks, false).1 == super::work::Verdict::Pass;
                retry = if merge { None } else { fix_turn(job) };
            }
            if merge {
                // Approved ahead of the checks: they're green, so it goes in.
                if let Some(job) = self.store.job_mut(id) {
                    job.merge_on_green = false;
                    job.feed.push(store::Item::By(
                        Member::Checker,
                        "Every check passes, so it merges now, as you asked.".into(),
                    ));
                }
                self.approve(id);
            }
            if let Some(prompt) = retry {
                // fix_turn already said why in the feed.
                self.send_on(st, id, prompt, Some(String::new()));
            }
            self.store.save();
        }
        if !finished.is_empty() {
            self.runners.retain(|r| !finished.contains(&r.id));
            for id in &finished {
                // A fix for review comments goes to its pull request; a turn
                // that ran out of plan waits for it, or moves engines.
                if self.after_answering(*id) || self.after_running_out(st, *id) {
                    self.peek(st, *id, true);
                    continue;
                }
                self.begin_review(*id);
                self.peek(st, *id, true);
                self.post_turn(*id);
                self.relay_mentions(*id);
            }
            self.deliver_relays(st);
            self.offer_idea();
            self.store.save();
        }
        self.poll_peek();
        self.poll_designs();
        self.poll_draft();
        self.poll_goals(st);
        self.poll_queue(st);
        self.poll_queued(st);
        self.poll_shift(st);
        self.poll_feedback(st);
        self.poll_learning();
        self.poll_bringing();
        finished.extend(self.poll_remote(st));
        finished
    }

    /// Words the person added while a job worked, sent as its next turn once
    /// the turn has ended and its checks are back. A turn waiting for its
    /// engine's plan to reset keeps them until it carries on.
    fn poll_queued(&mut self, st: &AppSettings) {
        let ready: Vec<u64> = self
            .store
            .jobs
            .iter()
            .filter(|j| {
                !j.queued.is_empty()
                    && j.status != Status::Running
                    && !j.checking
                    && j.resume_at == 0
                    && !matches!(j.status, Status::Discarded | Status::Shipped)
            })
            .map(|j| j.id)
            .filter(|id| !self.runners.iter().any(|r| r.id == *id))
            .collect();
        for id in ready {
            let Some(job) = self.store.job_mut(id) else {
                continue;
            };
            let text = std::mem::take(&mut job.queued).join("\n\n");
            self.send_on(st, id, text, None);
        }
    }

    /// Takes back one of the words waiting for a job's next turn.
    pub fn unqueue(&mut self, id: u64, n: usize) {
        if let Some(job) = self.store.job_mut(id) {
            if n < job.queued.len() {
                job.queued.remove(n);
                self.store.save();
            }
        }
    }

    /// The @mentions in a finished job's last reply, posted in its thread and
    /// queued for the bots they name. Past [`brain::talk::HOPS`] messages
    /// between bots with no word from the person, they stop and say so.
    fn relay_mentions(&mut self, id: u64) {
        let Some(job) = self.store.job(id) else {
            return;
        };
        // A turn the person stopped, or one that failed, says nothing to the crew.
        if job.goal != 0
            || job.race != 0
            || matches!(
                job.status,
                Status::Failed | Status::Stopped | Status::Interrupted
            )
        {
            return;
        }
        // Only this turn's last words: an @mention from an earlier turn has
        // been passed on already.
        let Some(last) = job
            .feed
            .get(job.turn_from..)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|i| match i {
                store::Item::Text(t) => Some(t.clone()),
                _ => None,
            })
        else {
            return;
        };
        let said = brain::talk::mentions(&last, job.bot, &Member::all());
        if said.is_empty() {
            return;
        }
        let (from, folder, thread) = (job.bot, job.folder.clone(), job.thread);
        let key = (folder.path.clone(), thread);
        for (to, words) in said {
            let hops = self.relay_hops.entry(key.clone()).or_insert(0);
            if *hops >= brain::talk::HOPS {
                let note = format!(
                    "The bots have messaged each other {} times without you, so they've stopped here. {} was about to ask {}. Tell one of them what to do next, like \"@{} go ahead\", and they'll carry on.",
                    brain::talk::HOPS,
                    from.name(),
                    to.name(),
                    from.name(),
                );
                if let Some(j) = self.store.job_mut(id) {
                    j.feed.push(store::Item::Note(note.clone()));
                }
                self.crew_says(&folder, thread, Member::Fin, note, None);
                return;
            }
            *hops += 1;
            // An answer it passed on to another bot isn't waiting on the
            // person; changes to approve still are.
            if let Some(j) = self.store.job_mut(id) {
                if j.status == Status::Done {
                    j.seen = true;
                }
            }
            self.crew_says(
                &folder,
                thread,
                from,
                format!("@{} {words}", to.name()),
                Some(format!("to {} \u{b7} from #{id}", to.name())),
            );
            self.relays.push(Relay {
                to,
                from,
                from_job: id,
                folder: folder.clone(),
                thread,
                said: words,
                whole: last.clone(),
            });
        }
    }

    /// Hands bots the messages waiting for them, once they're free, by the
    /// one rule: the next turn of their open job in that thread, else a new
    /// job there.
    fn deliver_relays(&mut self, st: &AppSettings) {
        let waiting = std::mem::take(&mut self.relays);
        for r in waiting {
            if self.store.busy(r.to).is_some() {
                self.relays.push(r);
                continue;
            }
            let text = brain::talk::relay_text(r.from, r.from_job, &r.said, &r.whole);
            let theirs = self.open_job(&r.folder.path, r.thread, r.to);
            match theirs {
                Some(tid) => {
                    if let Some(j) = self.store.job_mut(tid) {
                        j.feed.push(store::Item::By(r.from, r.said.clone()));
                    }
                    self.send_on(st, tid, text, Some(String::new()));
                }
                None => {
                    let mut job =
                        self.make_job(st, r.to, r.to.mode(), &text, r.folder.clone(), None, None);
                    job.thread = r.thread;
                    job.title = format!("{} asked: {}", r.from.name(), r.said);
                    job.feed.insert(0, store::Item::By(r.from, r.said.clone()));
                    job.turn_from += 1;
                    self.store.jobs.push(job);
                }
            }
        }
        self.store.save();
    }

    /// A bot speaking in a thread, as a message of its own: `from` says
    /// `text`, with `via` under it.
    fn crew_says(
        &mut self,
        folder: &store::Folder,
        thread: u64,
        from: Member,
        text: String,
        via: Option<String>,
    ) {
        let id = self.store.talks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.store.talks.push(Talk {
            id,
            folder: folder.path.clone(),
            thread,
            bot: from,
            question: String::new(),
            answer: Some(Said {
                text,
                via,
                ..Default::default()
            }),
            at: store::now(),
            choices: Vec::new(),
        });
    }

    /// Whether a job has been talked to in its thread since it began: a bot
    /// messaged it or it messaged one, or the person added to it there. Its
    /// later turns then show in the thread, where the talk is.
    fn talked_in_thread(&self, j: &store::Job) -> bool {
        let (from, added) = (format!("from #{}", j.id), format!("added to #{}", j.id));
        j.feed.iter().any(|i| matches!(i, store::Item::By(..)))
            || self.store.talks.iter().any(|t| {
                t.folder == j.folder.path
                    && t.answer
                        .as_ref()
                        .and_then(|a| a.via.as_deref())
                        .is_some_and(|v| v.ends_with(&from) || v == added)
            })
    }

    /// A later turn of a job talked to in its thread ends: what it said goes
    /// in the thread as the bot's message, since its card sits further up
    /// where the job began. Its @mentions are posted on their own after it.
    fn post_turn(&mut self, id: u64) {
        let Some(job) = self.store.job(id) else {
            return;
        };
        if job.goal != 0
            || job.race != 0
            // Its first turn shows in its card, right where it began.
            || !job
                .feed
                .get(..job.turn_from)
                .unwrap_or_default()
                .iter()
                .any(|i| matches!(i, store::Item::Text(_)))
            || matches!(
                job.status,
                Status::Failed | Status::Stopped | Status::Interrupted
            )
            || !self.talked_in_thread(job)
        {
            return;
        }
        let Some(last) = job
            .feed
            .get(job.turn_from..)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|i| match i {
                store::Item::Text(t) => Some(t.clone()),
                _ => None,
            })
        else {
            return;
        };
        let crew = Member::all();
        let body = last
            .lines()
            .filter(|l| brain::talk::mentions(l, job.bot, &crew).is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if body.is_empty() {
            return;
        }
        let (folder, thread, bot) = (job.folder.clone(), job.thread, job.bot);
        self.crew_says(&folder, thread, bot, body, Some(format!("in #{id}")));
    }

    /// The person's words for a bot go into its open job `id`: they show in
    /// the thread, and what the bot says back is posted there too.
    pub fn add_to_job(&mut self, st: &AppSettings, id: u64, text: String) {
        let Some(job) = self.store.job(id) else {
            return;
        };
        let (folder, thread, who) = (job.folder.path.clone(), job.thread, job.bot);
        let tid = self.store.talks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let mut said = Said::note(format!("{} adds this to #{id}.", who.name()));
        said.via = Some(format!("added to #{id}"));
        self.store.talks.push(Talk {
            id: tid,
            folder,
            thread,
            bot: who,
            question: text.clone(),
            answer: Some(said),
            at: store::now(),
            choices: Vec::new(),
        });
        self.follow_up(st, id, text);
    }

    /// The person spoke in a thread: its bots may talk to each other again.
    pub fn person_spoke(&mut self, folder: &str, thread: u64) {
        self.relay_hops.remove(&(folder.to_string(), thread));
    }

    /// Asks Peek for a job's title, or its recap once it ended, when Smart
    /// Peek is on.
    fn peek(&mut self, st: &AppSettings, id: u64, recap: bool) {
        let Some(job) = self.store.job(id) else {
            return;
        };
        // The small model mixes up a job of several turns (a fix turn, a
        // follow-up); there the bot's own last words and the facts line
        // tell it better.
        if recap && several_turns(job) {
            return;
        }
        if let Some(rx) = brain::tasks::ask(st, job, recap) {
            self.peeking.push((id, rx));
        }
    }

    /// Adds a bot to the crew. What the person left out (the kind of work, its
    /// skills, its rules) is filled in at once from what it's for, and then the
    /// person's engine reads the project and drafts the rest of it: its
    /// instructions, skill files, connected apps, permissions and memory. This
    /// is how Peek makes the bots it suggests.
    #[allow(clippy::too_many_arguments)]
    pub fn make_bot(
        &mut self,
        st: &AppSettings,
        name: &str,
        role: &str,
        like: Option<Member>,
        (skills, rules): (Vec<String>, Vec<String>),
        look: [u8; 4],
        outfit: String,
    ) -> u16 {
        let id = self.next_made_id();
        let mut d = brain::maker::draft(name, role, like);
        let set = (!skills.is_empty(), !rules.is_empty());
        if set.0 {
            d.skills = skills;
        }
        if set.1 {
            d.rules = rules;
        }
        self.store.made.push(super::data::Made {
            id,
            name: name.to_string(),
            role: d.role.clone(),
            like: d.like,
            look,
            outfit,
            skills: d.skills.clone(),
            rules: d.rules.clone(),
            project: String::new(),
        });
        super::data::set_made(&self.store.made);
        self.store.save();
        // A bot for every project: drafted from what it's for, not the open project.
        let found = crate::settings::discover(None);
        let (have, apps) = brain::maker::on_computer(&found);
        let prompt = brain::maker::full_prompt(&d.role, name, None, &have, &apps);
        if let Ok(rx) = brain::maker::ask_full(st, None, &prompt) {
            self.designing.push((id, set, rx));
        }
        id
    }

    /// The id the next made bot gets.
    pub fn next_made_id(&self) -> u16 {
        self.store.made.iter().map(|m| m.id).max().unwrap_or(0) + 1
    }

    /// Adds a bot the person reviewed in the maker, as it is: no second draft.
    #[allow(clippy::too_many_arguments)]
    pub fn add_reviewed(
        &mut self,
        id: u16,
        name: &str,
        role: &str,
        like: Member,
        (skills, rules): (Vec<String>, Vec<String>),
        look: [u8; 4],
        setup: super::setup::Setup,
        project: String,
    ) {
        self.store.made.retain(|m| m.id != id);
        self.store.made.push(super::data::Made {
            id,
            name: name.to_string(),
            role: role.to_string(),
            like,
            look,
            outfit: String::new(),
            skills,
            rules,
            project,
        });
        super::data::set_made(&self.store.made);
        self.set_setup(Member::Made(id), setup);
    }

    /// Saves what `who` can do.
    pub fn set_setup(&mut self, who: Member, setup: super::setup::Setup) {
        let key = super::engine::bot_key(who);
        if setup.is_empty() {
            self.store.setups.remove(&key);
        } else {
            self.store.setups.insert(key, setup);
        }
        super::setup::set_all(&self.store.setups);
        self.store.save();
    }

    /// Asks the person's engine to draft a whole bot from `about`, for the
    /// maker. Without an open project or an engine, the quick draft is ready
    /// at once.
    pub fn draft_bot(
        &mut self,
        st: &AppSettings,
        about: &str,
        name: &str,
        project: Option<&store::Folder>,
    ) {
        self.drafted = None;
        let found = crate::settings::discover(project.map(|f| f.path.as_str()));
        let (have, apps) = brain::maker::on_computer(&found);
        let prompt =
            brain::maker::full_prompt(about, name, project.map(|f| f.name.as_str()), &have, &apps);
        match brain::maker::ask_full(st, project, &prompt) {
            Ok(rx) => self.drafting = Some((rx, about.to_string(), name.to_string())),
            Err(_) => {
                self.drafting = None;
                self.drafted = Some(brain::maker::quick(about, name));
            }
        }
    }

    /// Whether the maker's draft is still on its way.
    pub fn is_drafting(&self) -> bool {
        self.drafting.is_some()
    }

    /// Stops waiting for the maker's draft.
    pub fn cancel_draft(&mut self) {
        self.drafting = None;
        self.skilling = None;
    }

    /// Asks the person's engine to write one skill file for a bot, from what
    /// the person wants it to do.
    /// `project` is the one the bot is only for, read so the steps fit it;
    /// None writes a skill that works in any project.
    pub fn draft_skill(
        &mut self,
        st: &AppSettings,
        about: &str,
        bot: &str,
        project: Option<&store::Folder>,
    ) -> Result<(), String> {
        let (whose, fit) = match project {
            Some(f) => (
                format!("for the project \"{}\" (the current folder)", f.name),
                "Read the project so the steps fit it (its commands, folders and conventions). Change no files.",
            ),
            None => (
                "that works across all of the person's projects".to_string(),
                "Don't read any project: write steps that work in any codebase, saying how to find project-specific commands.",
            ),
        };
        let prompt = format!(
            "Write a skill file for {bot}, a bot in Squad Code's crew {whose}. \
             The skill: {about}\n\n{fit}\n\n\
             Reply with JSON only: {{\"name\": \"short-kebab-name\", \"description\": \"when to use it, one line\", \
             \"body\": \"Markdown: the steps, commands and checks, 10 to 40 lines\"}}"
        );
        let rx = brain::maker::ask_full(st, project, &prompt)?;
        self.skilling = Some((rx, about.to_string()));
        self.skilled = None;
        Ok(())
    }

    /// Whether a skill file is being written.
    pub fn is_skilling(&self) -> bool {
        self.skilling.is_some()
    }

    /// The maker's draft and skill coming back from the engine.
    fn poll_draft(&mut self) {
        if let Some((rx, about, name)) = &self.drafting {
            let back = match rx.try_recv() {
                Ok(crate::agent::Event::Output(r)) => Some(r.ok()),
                Ok(_) | Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            };
            if let Some(reply) = back {
                let full = reply
                    .as_deref()
                    .map(crate::agent::strip_ansi)
                    .and_then(|t| brain::maker::parse_full(&t))
                    .map(|mut f| {
                        if !name.trim().is_empty() || f.name.is_empty() {
                            f.name = brain::maker::quick(about, name).name;
                        }
                        f
                    });
                let full = full.unwrap_or_else(|| {
                    self.notes.push(
                        "The engine's draft didn't come back, so this one is built from your words. Redraft any time."
                            .into(),
                    );
                    brain::maker::quick(about, name)
                });
                self.drafted = Some(full);
                self.drafting = None;
            }
        }
        if let Some((rx, about)) = &self.skilling {
            let back = match rx.try_recv() {
                Ok(crate::agent::Event::Output(r)) => Some(r.ok()),
                Ok(_) | Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            };
            if let Some(reply) = back {
                let parsed = reply
                    .as_deref()
                    .map(crate::agent::strip_ansi)
                    .and_then(|t| {
                        let (a, b) = (t.find('{')?, t.rfind('}')?);
                        serde_json::from_str::<serde_json::Value>(t.get(a..=b)?).ok()
                    })
                    .and_then(|v| {
                        let f = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
                        let (n, d, b) = (f("name"), f("description"), f("body"));
                        (!b.is_empty()).then(|| {
                            (
                                if n.is_empty() { about.clone() } else { n },
                                if d.is_empty() { about.clone() } else { d },
                                b,
                            )
                        })
                    });
                self.skilled = Some(parsed.ok_or_else(|| {
                    "The engine didn't write the skill. Try again, or paste a path to a SKILL.md."
                        .to_string()
                }));
                self.skilling = None;
            }
        }
    }

    /// Bot setups coming back from the engine, for bots made by Peek or
    /// without the maker's review: what the person set stays as it is.
    fn poll_designs(&mut self) {
        let mut back = Vec::new();
        self.designing.retain(|(id, set, rx)| match rx.try_recv() {
            Ok(crate::agent::Event::Output(r)) => {
                back.push((*id, *set, r.ok()));
                false
            }
            Ok(_) | Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, set, reply) in back {
            let parsed = reply
                .as_deref()
                .map(crate::agent::strip_ansi)
                .and_then(|t| brain::maker::parse_full(&t));
            let Some(full) = parsed else {
                continue;
            };
            let Some(m) = self.store.made.iter_mut().find(|m| m.id == id) else {
                continue;
            };
            m.role = full.role.clone();
            if !set.0 && !full.skills.is_empty() {
                m.skills = full.skills.clone();
            }
            if !set.1 && !full.rules.is_empty() {
                m.rules = full.rules.clone();
            }
            let name = m.name.clone();
            super::data::set_made(&self.store.made);
            let who = Member::Made(id);
            // Only a bot with no setup yet gets one: never over the person's edits.
            if !self.store.setups.contains_key(&super::engine::bot_key(who)) {
                let found = crate::settings::discover(None);
                let setup = brain::maker::setup_from(who, &full, &found);
                self.set_setup(who, setup);
            }
            self.store.save();
            self.say(format!(
                "{name} is set up for this project: its instructions, skills, tools and memory are in its Setup on Crew."
            ));
        }
    }

    /// Titles and recaps coming back from Peek.
    fn poll_peek(&mut self) {
        let mut back = Vec::new();
        self.peeking.retain(|(id, rx)| match rx.try_recv() {
            Ok(said) => {
                back.push((*id, said));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, said) in back {
            let Some(job) = self.store.job_mut(id) else {
                continue;
            };
            match said {
                brain::tasks::Said::Title(t) => job.short = t,
                brain::tasks::Said::Recap(r, _) => {
                    job.feed.push(store::Item::Note(r.clone()));
                    job.recap = r;
                }
            }
            self.store.save();
        }
    }
}

/// Whether the bot has had more than one turn on `job`.
fn several_turns(job: &store::Job) -> bool {
    job.fix_turns > 0 || job.feed.iter().any(|i| matches!(i, store::Item::You(_)))
}

/// How many times a job's failing checks go back to its bot before the
/// person is asked.
pub const FIX_TURNS: u8 = 1;

/// When a job's checks failed and it hasn't used its fix turns, notes that in
/// its feed and gives the words to send its bot. Checks the computer couldn't
/// run (a full disk, a timeout) aren't the code's fault and send nothing back.
fn fix_turn(job: &mut store::Job) -> Option<String> {
    let failed: Vec<&crate::proof::CheckRun> = job
        .checks
        .iter()
        .filter(|c| !c.ok && c.infra.is_empty() && !c.timed_out)
        .collect();
    if failed.is_empty() || job.fix_turns >= FIX_TURNS || job.status != Status::Review {
        return None;
    }
    let names: Vec<String> = failed.iter().map(|c| format!("`{}`", c.command)).collect();
    job.fix_turns += 1;
    job.feed.push(store::Item::Note(format!(
        "{} failed, so {} is fixing it before it comes to you.",
        names.join(" and "),
        job.bot.name()
    )));
    let mut prompt = crate::proof::fix_prompt(&job.checks);
    // Checks can fail before a job touches anything; fixing that here would
    // redo other work and clash with it later.
    prompt.push_str(
        "\nFirst check whether each failure comes from your change: compare with the code before it \
         (git stash, or the base branch). Fix only what your change broke. If a failure was already \
         there, leave it and say so in one line.",
    );
    Some(prompt)
}

/// A job's copy here takes the server's latest, keeping what only makes sense
/// here: its id, thread, folder and whether you've seen it.
fn mirror(job: &mut store::Job, there: store::Job) {
    let keep = (
        job.id,
        job.thread,
        job.folder.clone(),
        job.seen,
        job.goal,
        std::mem::take(&mut job.remote_on),
        job.remote,
        job.short.clone(),
    );
    let ended_now = job.status == Status::Running && there.status != Status::Running;
    // The notes this side wrote before sending it (where it runs and why) stay
    // on top of the server's feed.
    let ours: Vec<store::Item> = job
        .feed
        .iter()
        .take_while(|i| matches!(i, store::Item::Note(_)) && !there.feed.contains(i))
        .cloned()
        .collect();
    *job = there;
    job.feed.splice(0..0, ours);
    job.id = keep.0;
    job.thread = keep.1;
    job.folder = keep.2;
    job.seen = keep.3 && !ended_now;
    job.goal = keep.4;
    job.race = 0;
    job.pick.clear();
    // Its worktree is on the server, not here.
    job.remote_dir = std::mem::take(&mut job.worktree);
    if !job.engine.contains(&keep.5) {
        job.engine = if job.engine.is_empty() {
            format!("on {}", keep.5)
        } else {
            format!("{} on {}", job.engine, keep.5)
        };
    }
    job.remote_on = keep.5;
    job.remote = keep.6;
    if job.short.is_empty() {
        job.short = keep.7;
    }
}

/// A job that changes files and whose branch was merged or thrown away gets a
/// fresh branch and worktree from where its base is now before it works again,
/// so more words for it never land in your checkout. Gives back the base when
/// it did. Its engine's conversation lived in the old folder, so it starts a
/// new one.
fn fresh_branch(job: &mut store::Job) -> Option<String> {
    if !job.mode.writes()
        || !job.worktree.is_empty()
        || job.branch.is_empty()
        || !job.remote_on.is_empty()
    {
        return None;
    }
    match work::isolate(&job.folder.path, &job.title, job.id) {
        Ok(Some(iso)) => {
            job.feed.push(store::Item::Note(format!(
                "Its last branch is done with, so this goes on a new one, {}, and your checkout stays as it is.",
                iso.branch
            )));
            job.branch = iso.branch;
            job.worktree = iso.worktree;
            job.base = iso.base.clone();
            job.base_sha = iso.base_sha;
            job.changes.clear();
            job.checks.clear();
            job.conflict.clear();
            job.held.clear();
            job.belt_ok = false;
            job.shipped.clear();
            job.shipping = false;
            job.pr.clear();
            job.pr_number = 0;
            job.pr_line.clear();
            job.pr_state.clear();
            job.pr_failed.clear();
            job.auto_merge = false;
            job.marks.clear();
            job.conversation.clear();
            Some(iso.base)
        }
        Ok(None) => None,
        Err(why) => {
            job.feed.push(store::Item::Note(format!(
                "Couldn't give it a new branch ({why}), so it works in your folder."
            )));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_checks_go_back_to_the_bot_once() {
        let failing = crate::proof::CheckRun {
            command: "cargo test".into(),
            ok: false,
            tail: "test parse ... FAILED".into(),
            ..Default::default()
        };
        let mut job = store::Job {
            bot: Member::Fixer,
            status: Status::Review,
            checks: vec![failing.clone()],
            ..Default::default()
        };
        let prompt = fix_turn(&mut job).expect("one fix turn");
        assert!(prompt.contains("cargo test") && prompt.contains("FAILED"));
        assert!(prompt.starts_with("Squad Code ran"));
        assert_eq!(job.fix_turns, 1);
        // The second failure is the person's to see.
        assert!(fix_turn(&mut job).is_none());
        // A check the computer couldn't run sends nothing back.
        let mut job = store::Job {
            status: Status::Review,
            checks: vec![crate::proof::CheckRun {
                infra: "disk full".into(),
                ..failing
            }],
            ..Default::default()
        };
        assert!(fix_turn(&mut job).is_none());
    }

    pub(super) fn quiet_store() -> Store {
        // A demo store is never saved, so tests leave ~/.squadcode alone.
        Store {
            demo: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_bot_mentioning_another_reaches_it_in_the_thread() {
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = "/nonexistent/squad-test-agent".into();
            st.agents.insert(p.id.to_string(), a);
        }
        let dir = std::env::temp_dir().join(format!("squad-talk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let folder = store::Folder {
            name: "t".into(),
            path: dir.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        let mut rt = Runtime::new(quiet_store());
        rt.store.jobs.push(store::Job {
            id: 1,
            thread: 3,
            bot: Member::Builder,
            mode: Mode::Build,
            title: "Add a login page".into(),
            folder: folder.clone(),
            status: Status::Review,
            feed: vec![store::Item::Text(
                "The login page works.\n@Guard: is the session cookie safe?".into(),
            )],
            ..Default::default()
        });
        rt.relay_mentions(1);
        // Builder's message shows in the thread, from Builder.
        let t = rt.store.talks.last().unwrap();
        assert_eq!((t.bot, t.thread), (Member::Builder, 3));
        assert!(t.question.is_empty());
        assert!(t.answer.as_ref().unwrap().text.starts_with("@Guard"));
        // Guard has no job in the thread, so it gets one there.
        rt.deliver_relays(&st);
        let g = rt
            .store
            .jobs
            .iter()
            .find(|j| j.bot == Member::Guard)
            .unwrap();
        assert_eq!(g.thread, 3);
        assert!(g.title.starts_with("Builder asked"));
        assert!(matches!(&g.feed[0], store::Item::By(Member::Builder, _)));
        // Past the limit they stop, and the person is asked to step in.
        rt.relay_hops
            .insert((folder.path.clone(), 3), brain::talk::HOPS);
        rt.relay_mentions(1);
        let last = rt.store.talks.last().unwrap();
        assert_eq!(last.bot, Member::Fin);
        assert!(rt.relays.is_empty());
        // Once the person speaks, they may talk again.
        rt.person_spoke(&folder.path, 3);
        assert!(rt.relay_hops.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bot_keeps_one_open_job_per_thread() {
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = "/nonexistent/squad-test-agent".into();
            st.agents.insert(p.id.to_string(), a);
        }
        let dir = std::env::temp_dir().join(format!("squad-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let folder = store::Folder {
            name: "t".into(),
            path: dir.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        let mut rt = Runtime::new(quiet_store());
        rt.store.folders = vec![folder.clone()];
        rt.store.current = 0;
        let other = rt.store.add_thread("other", Vec::new()).unwrap();
        let t = rt.store.add_thread("checkout", Vec::new()).unwrap();
        rt.store.thread_now = 0;
        let job = |id, bot: Member, status| store::Job {
            id,
            thread: t,
            bot,
            mode: bot.mode(),
            status,
            folder: folder.clone(),
            ..Default::default()
        };
        rt.store.jobs.push(job(9, Member::Builder, Status::Review));
        rt.store.jobs.push(job(10, Member::Guard, Status::Done));
        rt.store.jobs.push(job(11, Member::Checker, Status::Done));
        rt.store.next_id = 11;
        // Waiting for review: Builder's words here go into it.
        let p = folder.path.clone();
        assert_eq!(rt.open_job(&p, t, Member::Builder), Some(9));
        // A bot that doesn't change code keeps its finished job open; one
        // that does starts again once it's done.
        assert_eq!(rt.open_job(&p, t, Member::Guard), Some(10));
        assert_eq!(rt.open_job(&p, t, Member::Checker), None);
        // Another thread is another job.
        assert_eq!(rt.open_job(&p, other, Member::Builder), None);
        let asked = rt.ask_bot(
            &st,
            Member::Builder,
            "make it blue too".into(),
            folder.clone(),
            t,
        );
        assert_eq!(asked, Asked::Added(9));
        assert!(rt.store.talks.last().unwrap().question.contains("blue"));
        // Approved: the next ask is a new job for Builder, in that thread.
        rt.store.job_mut(9).unwrap().status = Status::Shipped;
        let Asked::Opened(Opened::Job(id)) = rt.ask_bot(
            &st,
            Member::Builder,
            "add a footer".into(),
            folder.clone(),
            t,
        ) else {
            panic!("expected a new job");
        };
        let j = rt.store.job(id).unwrap();
        assert_eq!((j.bot, j.mode, j.thread), (Member::Builder, Mode::Build, t));
        // A pick on Guard's page goes to the bot that builds, in its thread.
        rt.store.job_mut(id).unwrap().status = Status::Shipped;
        let Some(Asked::Opened(Opened::Job(id))) =
            rt.ask_from_page(&st, 10, "make the button green".into())
        else {
            panic!("expected a new job");
        };
        let j = rt.store.job(id).unwrap();
        assert_eq!((j.bot, j.thread), (Member::Builder, t));
        assert!(j.title.contains("#10"));
        assert!(rt.ask_from_page(&st, 404, "nothing".into()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_mention_isnt_sent_again() {
        let mut rt = Runtime::new(quiet_store());
        let folder = store::Folder {
            name: "t".into(),
            path: "/tmp/squad-old-mention".into(),
            ship: String::new(),
        };
        rt.store.jobs.push(store::Job {
            id: 1,
            thread: 3,
            bot: Member::Builder,
            folder: folder.clone(),
            status: Status::Done,
            feed: vec![
                store::Item::Text("Done.\n@Guard: is the cookie safe?".into()),
                store::Item::You("Also add a logout button".into()),
            ],
            // The person's follow-up began a turn that said nothing.
            turn_from: 1,
            ..Default::default()
        });
        rt.relay_mentions(1);
        assert!(rt.store.talks.is_empty() && rt.relays.is_empty());
        // Nor does a turn the person stopped.
        let j = rt.store.job_mut(1).unwrap();
        j.turn_from = 0;
        j.status = Status::Stopped;
        rt.relay_mentions(1);
        assert!(rt.store.talks.is_empty() && rt.relays.is_empty());
    }

    #[test]
    fn a_fresh_crew_is_idle() {
        let rt = Runtime::new(quiet_store());
        assert!(!rt.busy());
        assert!(rt.notes.is_empty());
        assert!(!rt.thinking(Member::Explainer));
    }

    #[test]
    fn a_job_that_cant_start_fails_without_a_window() {
        // No agent can run here: the job is made, fails with a reason, and
        // nothing is left running.
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = "/nonexistent/squad-test-agent".into();
            st.agents.insert(p.id.to_string(), a);
        }
        let dir = std::env::temp_dir().join(format!("squad-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut rt = Runtime::new(quiet_store());
        let folder = store::Folder {
            name: "t".into(),
            path: dir.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        let opened = rt.start_job(&st, Member::Builder, Mode::Ask, "Read me".into(), folder);
        let Opened::Job(id) = opened else {
            panic!("expected a job, got {opened:?}");
        };
        let job = rt.store.job(id).unwrap();
        assert_eq!(job.status, Status::Failed);
        assert!(matches!(job.feed.last(), Some(store::Item::Error(_))));
        assert!(rt.runners.is_empty());
        assert!(rt.poll(&st, None).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_reads_what_was_said_in_its_thread() {
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = "/nonexistent/squad-test-agent".into();
            st.agents.insert(p.id.to_string(), a);
        }
        let dir = std::env::temp_dir().join(format!("squad-ctx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut rt = Runtime::new(quiet_store());
        let folder = store::Folder {
            name: "t".into(),
            path: dir.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        let thread = rt.store.thread_here();
        rt.store.talks.push(Talk {
            id: 1,
            folder: folder.path.clone(),
            thread,
            bot: Member::Fin,
            question: "research teams sign up with a university email".into(),
            answer: Some(Said::bot(
                "Got it, .edu addresses only.".into(),
                String::new(),
            )),
            at: store::now(),
            choices: Vec::new(),
        });
        let opened = rt.start_job(
            &st,
            Member::Builder,
            Mode::Build,
            "add the sign-up page".into(),
            folder,
        );
        let Opened::Job(id) = opened else {
            panic!("expected a job, got {opened:?}");
        };
        let ctx = &rt.store.job(id).unwrap().context;
        assert!(ctx.contains("university email") && ctx.contains(".edu addresses"));
        assert!(!ctx.contains("add the sign-up page"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn words_for_a_running_job_wait_for_its_next_turn() {
        let mut rt = Runtime::new(quiet_store());
        rt.store.jobs.push(store::Job {
            id: 9,
            bot: Member::Builder,
            status: Status::Running,
            ..Default::default()
        });
        let st = AppSettings::default();
        rt.follow_up(&st, 9, "use the blue button".into());
        rt.follow_up(&st, 9, "and keep the old one".into());
        let job = rt.store.job(9).unwrap();
        assert_eq!(job.queued, ["use the blue button", "and keep the old one"]);
        assert!(job.feed.is_empty());
        assert!(rt.notes.is_empty());
        // Still running, or done with its checks still going: they keep waiting.
        rt.poll_queued(&st);
        assert_eq!(rt.store.job(9).unwrap().queued.len(), 2);
        let job = rt.store.job_mut(9).unwrap();
        job.status = Status::Review;
        job.checking = true;
        rt.poll_queued(&st);
        assert_eq!(rt.store.job(9).unwrap().queued.len(), 2);
        // Taken back one at a time.
        rt.unqueue(9, 0);
        assert_eq!(rt.store.job(9).unwrap().queued, ["and keep the old one"]);
    }

    #[test]
    fn a_busy_bot_says_so_and_points_at_its_job() {
        let mut rt = Runtime::new(quiet_store());
        rt.store.jobs.push(store::Job {
            id: 7,
            bot: Member::Builder,
            title: "Already going".into(),
            status: Status::Running,
            ..Default::default()
        });
        let st = AppSettings::default();
        let opened = rt.start_job(
            &st,
            Member::Builder,
            Mode::Build,
            "Another".into(),
            store::Folder::default(),
        );
        assert_eq!(opened, Opened::Busy(7));
        assert_eq!(rt.store.jobs.len(), 1);
        assert!(rt.notes[0].contains("Already going"));
    }

    #[test]
    fn a_dropped_goal_is_gone() {
        let mut rt = Runtime::new(quiet_store());
        rt.store.goals.push(goal::Goal {
            id: 3,
            text: "Big thing".into(),
            ..Default::default()
        });
        rt.drop_goal(3);
        assert!(rt.store.goals.is_empty());
    }

    #[test]
    fn a_job_on_the_server_mirrors_here_and_keeps_its_own_notes() {
        let mut here = store::Job {
            id: 7,
            thread: 3,
            remote_on: "vps".into(),
            remote: 41,
            status: Status::Running,
            seen: true,
            feed: vec![store::Item::Note("Running on vps: long work.".into())],
            ..Default::default()
        };
        here.folder.name = "app".into();
        let mut there = store::Job {
            id: 41,
            status: Status::Review,
            engine: "Claude Code".into(),
            worktree: "/home/me/.squadcode/worktrees/x".into(),
            feed: vec![store::Item::Note("Working on its own branch.".into())],
            ..Default::default()
        };
        there.folder.name = "srv".into();
        mirror(&mut here, there.clone());
        assert_eq!((here.id, here.thread, here.remote), (7, 3, 41));
        assert_eq!(here.folder.name, "app");
        assert_eq!(here.status, Status::Review);
        assert!(here.worktree.is_empty());
        assert_eq!(here.engine, "Claude Code on vps");
        assert!(!here.seen, "it just ended, so it's new to you");
        assert_eq!(here.feed.len(), 2);
        // Reading it again doesn't pile the notes up.
        mirror(&mut here, there);
        assert_eq!(here.feed.len(), 2);
        assert_eq!(here.engine, "Claude Code on vps");
    }
}

#[cfg(all(test, unix))]
mod talk_e2e {
    use super::*;

    /// A stand-in for Claude Code: answers in stream-json, scripted by what
    /// it's told, so bots can be watched talking with no real engine.
    const FAKE: &str = r#"#!/bin/bash
all="$*"
sid="sess-$$"
if [[ "$all" == *"another bot on your crew"* ]]; then
  who=$(printf '%s' "$all" | grep -o '[A-Za-z]* (another bot on your crew' | head -1 | cut -d' ' -f1)
  text="I disagree: the cookie needs httpOnly.\\n@$who: please set httpOnly on the session cookie."
elif [[ "$all" == *"TALK-START"* ]]; then
  echo '<form>login</form>' > login.html
  text="The login page works.\\n@Guard: is the session cookie safe?"
else
  text="Done, nothing to add."
fi
echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$sid\"}"
echo "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"$text\"}]},\"session_id\":\"$sid\"}"
echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"$text\",\"session_id\":\"$sid\"}"
"#;

    fn run_until_idle(rt: &mut Runtime, st: &AppSettings) {
        let t = Instant::now();
        loop {
            rt.poll(st, None);
            if rt.runners.is_empty() && rt.relays.is_empty() {
                // Let a finished turn's relays go out.
                rt.poll(st, None);
                if rt.runners.is_empty() {
                    return;
                }
            }
            assert!(t.elapsed() < Duration::from_secs(60), "jobs never settled");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn two_bots_argue_until_the_person_is_needed() {
        let base = std::env::temp_dir().join(format!("squad-talk-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let fake = base.join("claude");
        std::fs::write(&fake, FAKE).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let sh = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
        };
        sh(&["init", "-q"]);
        std::fs::write(repo.join("README.md"), "demo\n").unwrap();
        sh(&["add", "."]);
        sh(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ]);

        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = if p.id == "claude" {
                fake.to_string_lossy().into_owned()
            } else {
                "/nonexistent/squad-test-agent".into()
            };
            st.agents.insert(p.id.to_string(), a);
        }
        let folder = store::Folder {
            name: "repo".into(),
            path: repo.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        let mut rt = Runtime::new(super::tests::quiet_store());
        let job = rt.make_job(
            &st,
            Member::Builder,
            Mode::Build,
            "TALK-START build a login page and have Guard check it",
            folder.clone(),
            None,
            None,
        );
        let thread = job.thread;
        rt.store.jobs.push(job);
        run_until_idle(&mut rt, &st);

        // Each bot's words show in the thread as its own message.
        let said: Vec<(Member, String)> = rt
            .store
            .talks
            .iter()
            .filter(|t| t.thread == thread)
            .map(|t| (t.bot, t.answer.clone().unwrap().text))
            .collect();
        assert_eq!(said[0].0, Member::Builder);
        assert!(said[0].1.starts_with("@Guard"));
        assert_eq!(said[1].0, Member::Guard);
        assert!(said[1].1.starts_with("@Builder") && said[1].1.contains("httpOnly"));
        // Guard reads Builder's unmerged work where it is.
        let builder = rt.store.jobs[0].clone();
        let guard = rt
            .store
            .jobs
            .iter()
            .find(|j| j.bot == Member::Guard)
            .unwrap();
        assert_eq!(guard.thread, thread);
        assert!(guard.context.contains(&builder.worktree));
        assert_eq!(guard.reads, vec![builder.worktree.clone()]);
        // Guard's job was Builder's ask, not the person's, and its answer
        // went on to Builder, so it isn't waiting on the person.
        assert_eq!(guard.asked_by(), Some(Member::Builder));
        assert_eq!(builder.asked_by(), None);
        assert!(guard.seen && !guard.needs_you());
        let recent = rt.store.recent(&folder.path, thread);
        assert!(!recent
            .iter()
            .any(|(who, w)| who == "you" && w.contains("asked:")));
        let via = |t: &Talk| {
            t.answer
                .as_ref()
                .and_then(|a| a.via.clone())
                .unwrap_or_default()
        };
        let bot_msgs = rt
            .store
            .talks
            .iter()
            .filter(|t| t.thread == thread && via(t).starts_with("to "))
            .count();
        assert_eq!(bot_msgs as u32, brain::talk::HOPS);
        // A later turn's words show in the thread too, without the @line.
        let turn = rt
            .store
            .talks
            .iter()
            .find(|t| via(t).starts_with("in #"))
            .unwrap();
        let text = &turn.answer.as_ref().unwrap().text;
        assert!(text.contains("I disagree") && !text.contains('@'));
        assert_eq!(rt.store.talks.last().unwrap().bot, Member::Fin);
        let paused = rt.store.talks.len();
        // Nothing more goes on while they wait for the person.
        run_until_idle(&mut rt, &st);
        assert_eq!(rt.store.talks.len(), paused);
        // The person steps in, answering Builder: the bots may talk again.
        // "@Builder ..." goes into Builder's open job here, not a new one.
        let id = rt.open_job(&folder.path, thread, Member::Builder);
        assert_eq!(id, Some(builder.id));
        let guard_id = rt
            .store
            .jobs
            .iter()
            .find(|j| j.bot == Member::Guard)
            .unwrap()
            .id;
        assert_eq!(
            rt.open_job(&folder.path, thread, Member::Guard),
            Some(guard_id)
        );
        assert_eq!(rt.open_job(&folder.path, thread, Member::Checker), None);
        // Each bot kept to one job in the thread, however often it was asked.
        for who in [Member::Builder, Member::Guard] {
            assert_eq!(rt.store.jobs.iter().filter(|j| j.bot == who).count(), 1);
        }
        let asked = rt.ask_bot(
            &st,
            Member::Builder,
            "TALK-START Fine, add httpOnly.".into(),
            folder.clone(),
            thread,
        );
        assert_eq!(asked, Asked::Added(builder.id));
        assert!(rt.store.talks[paused]
            .question
            .contains("Fine, add httpOnly"));
        let paused = paused + 1;
        run_until_idle(&mut rt, &st);
        let after: Vec<Member> = rt.store.talks[paused..].iter().map(|t| t.bot).collect();
        assert_eq!(after.first(), Some(&Member::Builder));
        assert!(after.len() > 1);
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// A job's whole life with a stand-in engine: started, asked to do more,
/// merged or thrown away, and stopped part way.
#[cfg(all(test, unix))]
mod lifecycle_e2e {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A stand-in for Claude Code. It logs each call on one line next to itself,
    /// writes the files its prompt names, and with SLOW-TASK takes its time.
    const FAKE: &str = r#"#!/bin/bash
all="$*"
{ printf '%s' "$all" | tr '\n' ' '; echo; } >> "$(dirname "$0")/calls.log"
if [[ "$all" == *"SLOW-TASK"* ]]; then sleep 30; fi
if [[ "$all" == *"WRITE-A"* ]]; then echo a > a.txt; fi
if [[ "$all" == *"WRITE-B"* ]]; then echo b > b.txt; fi
sid="sess-life"
echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$sid\"}"
echo "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]},\"session_id\":\"$sid\"}"
echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"Done.\",\"session_id\":\"$sid\"}"
"#;

    struct Bench {
        base: PathBuf,
        repo: PathBuf,
        st: AppSettings,
        folder: store::Folder,
        rt: Runtime,
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            for j in &self.rt.store.jobs {
                if !j.worktree.is_empty() {
                    let _ = crate::git::remove_worktree(&j.folder.path, &j.worktree);
                }
            }
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A git repository with one commit, and settings that run the stand-in.
    fn bench(name: &str) -> Bench {
        let base = std::env::temp_dir().join(format!("squad-life-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let fake = base.join("claude");
        std::fs::write(&fake, FAKE).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("README.md"), "demo\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "init"]);
        let mut st = AppSettings::default();
        for p in crate::agent::PROVIDERS.iter() {
            let mut a = st.agent(p.id);
            a.binary = if p.id == "claude" {
                fake.to_string_lossy().into_owned()
            } else {
                "/nonexistent/squad-test-agent".into()
            };
            st.agents.insert(p.id.to_string(), a);
        }
        let folder = store::Folder {
            name: "repo".into(),
            path: repo.to_string_lossy().into_owned(),
            ship: String::new(),
        };
        Bench {
            base,
            repo,
            st,
            folder,
            rt: Runtime::new(super::tests::quiet_store()),
        }
    }

    impl Bench {
        fn start(&mut self, task: &str) -> u64 {
            let opened = self.rt.start_job(
                &self.st,
                Member::Builder,
                Mode::Build,
                task.into(),
                self.folder.clone(),
            );
            let Opened::Job(id) = opened else {
                panic!("expected a job, got {opened:?}");
            };
            id
        }

        /// Moves the crew along until nothing is running or being checked.
        fn settle(&mut self) {
            let t = Instant::now();
            loop {
                self.rt.poll(&self.st, None);
                let checking = self.rt.store.jobs.iter().any(|j| j.checking);
                if self.rt.runners.is_empty() && self.rt.checking.is_empty() && !checking {
                    return;
                }
                assert!(
                    t.elapsed() < Duration::from_secs(60),
                    "the job never settled"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        fn job(&self, id: u64) -> store::Job {
            self.rt.store.job(id).cloned().unwrap()
        }

        /// Moves the crew along until the stand-in has been called `n` times.
        fn wait_for_calls(&mut self, n: usize) {
            let t = Instant::now();
            while self.calls().len() < n {
                self.rt.poll(&self.st, None);
                assert!(
                    t.elapsed() < Duration::from_secs(20),
                    "the engine never started"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(self.base.join("calls.log"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[test]
    fn a_job_goes_from_start_to_merged() {
        let mut b = bench("merge");
        let id = b.start("WRITE-A Add a file called a");
        assert_eq!(b.job(id).status, Status::Running);
        b.settle();

        // It worked on its own branch, in its own worktree, and waits for review.
        let job = b.job(id);
        assert_eq!(job.status, Status::Review, "feed: {:?}", job.feed);
        assert!(job.branch.starts_with("squad/"), "branch {}", job.branch);
        assert!(Path::new(&job.worktree).join("a.txt").is_file());
        assert!(!b.repo.join("a.txt").exists(), "the checkout is untouched");
        assert!(job.changes.iter().any(|c| c.path.ends_with("a.txt")));
        assert_eq!(job.conversation, "sess-life");

        // A follow-up goes on in the same worktree and the same conversation.
        b.rt.follow_up(&b.st, id, "WRITE-B Also add b".into());
        assert_eq!(b.job(id).status, Status::Running);
        b.settle();
        let job = b.job(id);
        assert_eq!(job.status, Status::Review, "feed: {:?}", job.feed);
        assert!(job
            .feed
            .iter()
            .any(|i| matches!(i, store::Item::You(t) if t == "WRITE-B Also add b")));
        assert!(Path::new(&job.worktree).join("b.txt").is_file());
        let calls = b.calls();
        assert_eq!(calls.len(), 2, "calls: {calls:?}");
        assert!(calls[1].contains("sess-life"), "not resumed: {}", calls[1]);

        // Approving merges both files into main and clears the worktree away.
        let worktree = job.worktree.clone();
        b.rt.approve(id);
        let job = b.job(id);
        assert_eq!(job.status, Status::Shipped, "feed: {:?}", job.feed);
        assert!(job.worktree.is_empty());
        assert!(!Path::new(&worktree).exists());
        assert_eq!(
            git(&b.repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
            "main"
        );
        let files = git(&b.repo, &["ls-tree", "--name-only", "HEAD"]);
        assert!(
            files.contains("a.txt") && files.contains("b.txt"),
            "{files}"
        );
    }

    #[test]
    fn more_words_for_a_merged_job_go_on_a_new_branch() {
        let mut b = bench("after-merge");
        let id = b.start("WRITE-A Add a file called a");
        b.settle();
        b.rt.approve(id);
        assert_eq!(b.job(id).status, Status::Shipped);

        // Asked for more after the merge: it works apart again, not in the checkout.
        b.rt.follow_up(&b.st, id, "WRITE-B Also add b".into());
        b.settle();
        let job = b.job(id);
        assert_eq!(job.status, Status::Review, "feed: {:?}", job.feed);
        assert!(!job.worktree.is_empty());
        assert!(Path::new(&job.worktree).join("b.txt").is_file());
        assert!(!b.repo.join("b.txt").exists(), "the checkout is untouched");
        assert!(git(&b.repo, &["status", "--porcelain"]).trim().is_empty());

        // And it merges like any other.
        b.rt.approve(id);
        assert_eq!(
            b.job(id).status,
            Status::Shipped,
            "feed: {:?}",
            b.job(id).feed
        );
        assert!(git(&b.repo, &["ls-tree", "--name-only", "HEAD"]).contains("b.txt"));
    }

    #[test]
    fn your_changes_in_the_way_are_set_aside_and_a_clash_waits_for_you() {
        let mut b = bench("in-the-way");
        let id = b.start("WRITE-A Add a file called a");
        b.settle();
        // You made the same file in your checkout meanwhile.
        std::fs::write(b.repo.join("a.txt"), "mine\n").unwrap();
        git(&b.repo, &["add", "a.txt"]);
        // Approve merges without asking; yours clash, so they wait aside.
        b.rt.approve(id);
        let job = b.job(id);
        assert_eq!(job.status, Status::Shipped, "feed: {:?}", job.feed);
        assert_eq!(job.in_the_way, vec!["a.txt".to_string()]);
        assert!(!job.kept.is_empty());
        assert!(!job.feed.iter().any(|i| matches!(i, store::Item::Error(_))));
        assert!(git(&b.repo, &["status", "--porcelain"]).trim().is_empty());

        // Brought back, both sides are in the file to sort out.
        b.rt.bring_back(id);
        let job = b.job(id);
        assert!(job.kept.is_empty() && job.in_the_way.is_empty());
        let a = std::fs::read_to_string(b.repo.join("a.txt")).unwrap();
        assert!(a.contains("<<<<<<<") && a.contains("mine"), "{a}");
        assert!(git(&b.repo, &["stash", "list"]).trim().is_empty());
    }

    #[test]
    fn a_discarded_job_leaves_the_checkout_as_it_was() {
        let mut b = bench("discard");
        let id = b.start("WRITE-A Add a file called a");
        b.settle();
        let job = b.job(id);
        assert_eq!(job.status, Status::Review, "feed: {:?}", job.feed);
        b.rt.discard(id);
        let after = b.job(id);
        assert_eq!(after.status, Status::Discarded);
        assert!(!Path::new(&job.worktree).exists());
        assert!(git(&b.repo, &["branch", "--list", &job.branch])
            .trim()
            .is_empty());
        assert!(!b.repo.join("a.txt").exists());
        assert_eq!(git(&b.repo, &["status", "--porcelain"]).trim(), "");
    }

    #[test]
    fn a_job_that_changes_nothing_has_nothing_to_review() {
        let mut b = bench("nothing");
        let id = b.start("Look around and change nothing");
        b.settle();
        let job = b.job(id);
        assert_eq!(job.status, Status::Done, "feed: {:?}", job.feed);
        assert!(job.worktree.is_empty());
        assert!(job
            .feed
            .iter()
            .any(|i| matches!(i, store::Item::Note(n) if n.contains("didn't change"))));
    }

    #[test]
    fn a_stopped_job_stops_and_can_carry_on() {
        let mut b = bench("stop");
        let id = b.start("SLOW-TASK WRITE-A Take your time");
        // Wait for the stand-in to be called, so there's a turn to stop.
        b.wait_for_calls(1);
        b.rt.stop(id);
        let t = Instant::now();
        b.settle();
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "stopping took too long"
        );
        let job = b.job(id);
        assert_eq!(job.status, Status::Stopped, "feed: {:?}", job.feed);
        assert!(b.rt.runners.is_empty());
        // Carry on starts a new turn in the same worktree.
        b.rt.carry_on(&b.st, id);
        assert_eq!(b.job(id).status, Status::Running);
        b.wait_for_calls(2);
        b.rt.stop(id);
        b.settle();
        assert_eq!(b.job(id).status, Status::Stopped);
        assert!(b.calls()[1].contains("Carry on"));
    }
}
