//! The night shift, rewinding, issues, review comments on open pull requests
//! and lessons from send-backs: the parts of the crew added with those
//! features, kept apart from the rest of `Runtime`.

use super::*;
use crate::github::{Feedback, PrStatus};
use crate::v2::{issues, learn, rewind, shift};

impl Runtime {
    // ---------- the night shift ----------

    /// Puts a job in line to start at `at` (seconds since 1970; 0 as soon as
    /// its bot is free). A job that names an issue waits for it to be read
    /// first. Returns its id in `Store::later`.
    pub fn queue_later(
        &mut self,
        bot: Member,
        mode: Mode,
        text: String,
        folder: store::Folder,
        at: u64,
    ) -> u64 {
        self.store.next_id += 1;
        let id = self.store.next_id;
        let reading = issues::find(&text).is_some();
        self.store.later.push(shift::Later {
            id,
            bot,
            mode,
            text,
            folder,
            thread: self.store.thread_here(),
            at,
            made: store::now(),
            reading,
            ..Default::default()
        });
        self.store.save();
        id
    }

    /// Starts a waiting job now, whatever time it was waiting for. False when
    /// its bot is busy or it's still reading its issue.
    pub fn start_later_now(&mut self, st: &AppSettings, id: u64) -> bool {
        let Some(l) = self.store.later.iter().find(|l| l.id == id).cloned() else {
            return false;
        };
        if l.reading || self.store.busy(l.bot).is_some() {
            return false;
        }
        self.store.later.retain(|x| x.id != id);
        self.start_later(st, l);
        self.store.save();
        true
    }

    /// Takes a job out of the line before it starts.
    pub fn cancel_later(&mut self, id: u64) -> bool {
        let before = self.store.later.len();
        self.store.later.retain(|l| l.id != id);
        self.reading.retain(|(l, _)| *l != id);
        let gone = self.store.later.len() != before;
        if gone {
            self.store.save();
        }
        gone
    }

    /// The handed-over branch for a job that went into the line instead of
    /// starting: it waits with the job, not for whichever starts next.
    pub(super) fn keep_handoff(&mut self, later: u64) {
        let h = self.handoff.take();
        if let Some(l) = self.store.later.iter_mut().find(|l| l.id == later) {
            l.handoff = h;
            self.store.save();
        }
    }

    fn start_later(&mut self, st: &AppSettings, l: shift::Later) -> u64 {
        let night = l.at > l.made + 60;
        self.handoff = l.handoff.clone();
        let mut job = self.make_job(st, l.bot, l.mode, &l.text, l.folder.clone(), None, None);
        self.handoff = None;
        job.thread = l.thread;
        job.issue = l.issue.clone();
        job.issue_url = l.issue_url.clone();
        job.shift = night;
        if l.trigger != 0 {
            self.started_by_trigger(&mut job, &l);
        }
        if night {
            job.feed.insert(
                0,
                store::Item::Note(
                    "Started on the night shift, as you asked. If its engine runs out of plan, another one takes over.".into(),
                ),
            );
        }
        let id = job.id;
        self.store.jobs.push(job);
        id
    }

    /// Moves the waiting jobs along: those whose time came and whose bot is
    /// free start, jobs that ran out of plan carry on once it resets, and
    /// issues are read for jobs that name one.
    pub(super) fn poll_shift(&mut self, st: &AppSettings) {
        if self.shift_at.elapsed() < Duration::from_secs(2) || self.store.demo {
            return;
        }
        self.shift_at = Instant::now();
        let now = store::now();
        let mut changed = false;
        // Plans that reset.
        let resume: Vec<u64> = self
            .store
            .jobs
            .iter()
            .filter(|j| j.resume_at > 0 && j.resume_at <= now && j.status != Status::Running)
            .map(|j| j.id)
            .collect();
        for id in resume {
            if let Some(j) = self.store.job_mut(id) {
                j.resume_at = 0;
                j.resumes += 1;
            }
            self.send_on(
                st,
                id,
                "Carry on from where you stopped. Check what is already done in the files first."
                    .into(),
                Some(
                    "Its plan should have reset by now, so it's picking up where it stopped."
                        .into(),
                ),
            );
            changed = true;
        }
        // Issues to read.
        let to_read: Vec<(u64, String, issues::Ref)> = self
            .store
            .later
            .iter()
            .filter(|l| l.reading && !self.reading.iter().any(|(id, _)| *id == l.id))
            .filter_map(|l| issues::find(&l.text).map(|r| (l.id, l.folder.path.clone(), r)))
            .collect();
        for (id, folder, r) in to_read {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(issues::fetch(&folder, &r));
            });
            self.reading.push((id, rx));
        }
        let mut read = Vec::new();
        self.reading.retain(|(id, rx)| match rx.try_recv() {
            Ok(r) => {
                read.push((*id, r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                read.push((*id, Err("it stopped before it finished".into())));
                false
            }
        });
        for (id, got) in read {
            let Some(l) = self.store.later.iter_mut().find(|l| l.id == id) else {
                continue;
            };
            l.reading = false;
            changed = true;
            let label = issues::find(&l.text)
                .map(|r| r.label())
                .unwrap_or_else(|| "the issue".into());
            match got {
                Ok(issue) => {
                    l.text = issues::brief(&l.text, &issue);
                    l.issue = issue.closes.clone();
                    l.issue_url = issue.url.clone();
                    let title = issue.title.trim().to_string();
                    self.say(format!(
                        "Read {}: \u{201c}{title}\u{201d}. Its pull request closes it when it merges.",
                        issue.key
                    ));
                }
                Err(why) => {
                    self.say(format!(
                        "Couldn't read {label} ({why}), so the job starts with your words alone."
                    ));
                }
            }
        }
        // Jobs whose time came, one per free bot.
        let mut taken: Vec<Member> = Vec::new();
        let mut start = Vec::new();
        for l in &self.store.later {
            if l.reading || !l.due(now) || taken.contains(&l.bot) {
                continue;
            }
            if self.store.busy(l.bot).is_some() {
                taken.push(l.bot);
                continue;
            }
            taken.push(l.bot);
            start.push(l.id);
        }
        for id in start {
            let Some(pos) = self.store.later.iter().position(|l| l.id == id) else {
                continue;
            };
            let l = self.store.later.remove(pos);
            let (bot, label, night) = (l.bot, l.label().to_string(), l.at > l.made + 60);
            self.start_later(st, l);
            if night {
                self.say(format!(
                    "Night shift: {} started \u{201c}{label}\u{201d}.",
                    bot.name()
                ));
            }
            changed = true;
        }
        if changed {
            self.store.save();
        }
    }

    /// A finished turn that ran out of its engine's plan: on the night shift
    /// another engine takes over; otherwise it carries on by itself once the
    /// plan resets. True when it was taken care of, so it isn't reviewed yet.
    pub(super) fn after_running_out(&mut self, st: &AppSettings, id: u64) -> bool {
        let Some(job) = self.store.job(id) else {
            return false;
        };
        if !matches!(job.status, Status::Done | Status::Failed) || !job.remote_on.is_empty() {
            return false;
        }
        let Some(reset) = shift::ran_out(job) else {
            return false;
        };
        if job.resumes >= shift::MAX_RESUMES {
            if let Some(j) = self.store.job_mut(id) {
                j.feed.push(store::Item::Note(
                    "It ran out of plan again, so it's waiting for you now.".into(),
                ));
            }
            return false;
        }
        let engine_was = job.engine.clone();
        let other = job.shift.then(|| engine::fallback(st, job)).flatten();
        if let Some(i) = other {
            if let Some(j) = self.store.job_mut(id) {
                j.resumes += 1;
                j.feed.push(store::Item::Note(format!(
                    "{engine_was} ran out of plan. On the night shift, so {} takes over rather than waiting.",
                    crate::agent::PROVIDERS[i].name
                )));
            }
            return self.rerun(st, id, Some(i));
        }
        let now = store::now();
        let at = shift::resume_time(reset, now);
        let when = shift::when_words(at, now);
        if let Some(j) = self.store.job_mut(id) {
            j.resume_at = at;
            j.status = Status::Failed;
            j.seen = false;
            j.feed.push(store::Item::Note(format!(
                "{engine_was} ran out of plan. It carries on by itself {when}, once the plan resets, or pick another engine to go on now."
            )));
        }
        self.store.save();
        true
    }

    // ---------- rewinding ----------

    /// Rewinds a job to before mark `k`: see [`rewind::rewind`]. Its changes
    /// are looked at again, and checked when there are any.
    pub fn rewind(&mut self, id: u64, k: usize) -> Result<(), String> {
        if self.runners.iter().any(|r| r.id == id) {
            return Err("It's still working. Stop it first, then rewind.".into());
        }
        let job = self.store.job_mut(id).ok_or("No such job.")?;
        let note = rewind::rewind(job, k)?;
        job.resume_at = 0;
        job.feed.push(store::Item::Note(note));
        job.changes = work::changed(job);
        if job.changes.is_empty() {
            job.feed.push(store::Item::Note(
                "Nothing is changed at this point. Write below to carry on from here.".into(),
            ));
        } else {
            job.status = Status::Review;
            job.seen = false;
            job.checking = true;
            let dir = job.worktree.clone();
            self.checking.retain(|(j, _)| *j != id);
            self.checking.push((id, work::run_checks(dir)));
        }
        self.store.save();
        Ok(())
    }

    // ---------- review comments on an open pull request ----------

    /// Reads the new review feedback on pull requests that got some, and hands
    /// what the repository's people said to the job's bot. When its turn
    /// ends, Shipper pushes the fix to the same pull request.
    pub(super) fn poll_feedback(&mut self, st: &AppSettings) {
        for (id, pr) in std::mem::take(&mut self.feedback_due) {
            let Some(dir) = self.store.job(id).map(|j| j.folder.path.clone()) else {
                continue;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let run = |args: &[&str]| {
                    std::process::Command::new("gh")
                        .args(args)
                        .current_dir(&dir)
                        .stdin(std::process::Stdio::null())
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default()
                };
                let n = pr.number.to_string();
                let view = run(&["pr", "view", &n, "--json", "reviews,comments"]);
                let lines = run(&[
                    "api",
                    &format!("repos/{{owner}}/{{repo}}/pulls/{n}/comments"),
                ]);
                let feedback = crate::github::review_feedback(&view, &lines, &pr.author);
                let _ = tx.send((pr, feedback));
            });
            self.feedback.push((id, rx));
        }
        let mut back = Vec::new();
        self.feedback.retain(|(id, rx)| match rx.try_recv() {
            Ok(got) => {
                back.push((*id, got));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, (pr, feedback)) in back {
            self.answer_review(st, id, &pr, &feedback);
        }
        // Fixes being pushed.
        let mut steps = Vec::new();
        self.updating.retain(|(id, rx)| loop {
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
        for (id, step) in steps {
            let Some(j) = self.store.job_mut(id) else {
                continue;
            };
            match step {
                ship::Step::Did(what) => j.feed.push(store::Item::By(Member::Shipper, what)),
                ship::Step::Opened(_) => {
                    j.pr_failed.clear();
                    j.feed.push(store::Item::By(
                        Member::Shipper,
                        format!(
                            "Pushed the fix to #{}. Its checks run again on GitHub.",
                            j.pr_number
                        ),
                    ));
                }
                ship::Step::Conflict(files) => {
                    j.feed.push(store::Item::Error(format!(
                        "Shipper couldn't push the fix: {} clash with {}.",
                        files.join(", "),
                        j.base
                    )));
                }
                ship::Step::Failed(why) => {
                    j.feed.push(store::Item::Error(format!(
                        "Shipper couldn't push the fix: {why}"
                    )));
                }
            }
            self.store.save();
        }
    }

    fn answer_review(&mut self, st: &AppSettings, id: u64, pr: &PrStatus, feedback: &[Feedback]) {
        let Some(job) = self.store.job(id) else {
            return;
        };
        if job.status != Status::Shipping || job.answering {
            return;
        }
        let bot = job.bot;
        let trusted: Vec<&Feedback> = feedback.iter().filter(|f| f.trusted).collect();
        if trusted.is_empty() {
            return;
        }
        let who: Vec<String> = {
            let mut w: Vec<String> = trusted.iter().map(|f| format!("@{}", f.author)).collect();
            w.dedup();
            w
        };
        if let Some(j) = self.store.job_mut(id) {
            j.answering = true;
            j.seen = false;
            j.feed.push(store::Item::By(
                Member::Shipper,
                format!(
                    "New review feedback on #{} from {}. {} is on it, and I'll push the fix to the same pull request.",
                    pr.number,
                    who.join(", "),
                    bot.name()
                ),
            ));
        }
        let prompt = crate::v2::ship::review_prompt(pr, feedback);
        self.send_on(st, id, prompt, Some(String::new()));
    }

    /// A turn that answered review comments ended: the job stays with its
    /// pull request, and Shipper pushes what changed. True when it was one.
    pub(super) fn after_answering(&mut self, id: u64) -> bool {
        let Some(j) = self.store.job_mut(id) else {
            return false;
        };
        if !j.answering {
            return false;
        }
        j.answering = false;
        let ok = j.status == Status::Done;
        j.status = Status::Shipping;
        if ok {
            let job = j.clone();
            self.updating.push((id, ship::push_update(job)));
        } else {
            j.feed.push(store::Item::By(
                Member::Shipper,
                "That didn't finish, so nothing was pushed. Write below to try again.".into(),
            ));
        }
        true
    }

    // ---------- lessons from send-backs ----------

    /// The person sent a job back with `said`: the crew's engine looks for a
    /// lasting lesson in it, for the bot to keep in this project.
    pub(super) fn learn_from(&mut self, st: &AppSettings, id: u64, said: &str) {
        if self.store.demo || !learn::worth_asking(said) {
            return;
        }
        let Some(job) = self.store.job(id) else {
            return;
        };
        let prompt = learn::prompt(job, said);
        if let Ok(rx) = brain::maker::ask_full(st, Some(&job.folder.clone()), &prompt) {
            self.learning.push((id, rx));
        }
    }

    pub(super) fn poll_learning(&mut self) {
        let mut back = Vec::new();
        self.learning.retain(|(id, rx)| match rx.try_recv() {
            Ok(crate::agent::Event::Output(r)) => {
                back.push((*id, r.ok()));
                false
            }
            Ok(_) | Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for (id, reply) in back {
            let Some(rule) = reply.as_deref().and_then(learn::parse) else {
                continue;
            };
            let Some(job) = self.store.job(id) else {
                continue;
            };
            // Nothing is kept until the person says so: the job shows the
            // lesson with Keep it, For the whole crew and Not now.
            let known = self
                .store
                .lessons
                .iter()
                .any(|l| l.folder == job.folder.path && l.text.eq_ignore_ascii_case(&rule));
            if known {
                continue;
            }
            if let Some(j) = self.store.job_mut(id) {
                j.lesson_offer = rule;
            }
            self.store.save();
        }
    }

    /// Keeps the lesson job `id` offers, for its bot or (`crew`) for every bot
    /// in the project. True when it was new.
    pub fn keep_lesson(&mut self, id: u64, text: &str, crew: bool) -> bool {
        let Some(job) = self.store.job_mut(id) else {
            return false;
        };
        job.lesson_offer.clear();
        let (bot, folder, project) = (job.bot, job.folder.path.clone(), job.folder.name.clone());
        let text = text.trim().to_string();
        let lesson = learn::Lesson {
            bot,
            folder,
            text: text.clone(),
            job: id,
            at: store::now(),
            crew,
        };
        let added = learn::add(&mut self.store.lessons, lesson);
        if added {
            learn::set_all(&self.store.lessons);
            let who = if crew {
                "The crew".to_string()
            } else {
                bot.name().to_string()
            };
            if let Some(j) = self.store.job_mut(id) {
                j.feed.push(store::Item::Note(format!(
                    "{who} will remember this in {project}: \u{201c}{text}\u{201d}"
                )));
            }
        }
        self.store.save();
        added
    }

    /// Lets the lesson job `id` offers go.
    pub fn drop_lesson(&mut self, id: u64) {
        if let Some(j) = self.store.job_mut(id) {
            j.lesson_offer.clear();
        }
        self.store.save();
    }

    /// Forgets lesson `i` of the ones `who` keeps in `folder`.
    pub fn forget_lesson(&mut self, who: Member, folder: &str, i: usize) -> bool {
        let pos = self
            .store
            .lessons
            .iter()
            .enumerate()
            .filter(|(_, l)| l.bot == who && l.folder == folder)
            .nth(i)
            .map(|(p, _)| p);
        let Some(pos) = pos else {
            return false;
        };
        self.store.lessons.remove(pos);
        learn::set_all(&self.store.lessons);
        self.store.save();
        true
    }
}
