//! Where a writing job works and what happens to its changes.
//!
//! A job that may change files gets its own branch and worktree, so your own
//! checkout is never touched while the crew works and several jobs can run at
//! once. When it finishes, Squad Code lists what changed and Checker runs the
//! project's checks in that worktree. Then you decide: approve it (merged into
//! the branch it started from), send it back with more words, or discard it.

use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use crate::git;
use crate::proof::{self, CheckRun};
use crate::seatbelt::{self, Snag};

use super::store::{Changed, Job};

use std::sync::atomic::{AtomicBool, Ordering};

/// The seatbelt's switches from Settings > How work runs.
static BELT_SECRETS: AtomicBool = AtomicBool::new(true);
static BELT_CHECKS: AtomicBool = AtomicBool::new(true);

/// Sets whether changes are searched for secrets before they leave, and
/// whether the checks run before a job comes to you.
pub fn set_belt(secrets: bool, checks: bool) {
    BELT_SECRETS.store(secrets, Ordering::Relaxed);
    BELT_CHECKS.store(checks, Ordering::Relaxed);
}

/// Whether the seatbelt looks for secrets.
pub fn belt_secrets() -> bool {
    BELT_SECRETS.load(Ordering::Relaxed)
}

/// Whether the checks run before review.
pub fn belt_checks() -> bool {
    BELT_CHECKS.load(Ordering::Relaxed)
}

/// A job's own branch and worktree.
#[derive(Clone, Debug, PartialEq)]
pub struct Isolated {
    pub branch: String,
    pub worktree: String,
    pub base: String,
    pub base_sha: String,
}

/// The branch name for a job: `squad/<a few words of the title>`.
pub fn branch_for(title: &str, id: u64) -> String {
    let slug = git::branch_slug(title);
    let slug = slug.strip_prefix("squad/").unwrap_or(&slug).to_string();
    if slug.is_empty() {
        format!("squad/job-{id}")
    } else {
        format!("squad/{slug}")
    }
}

/// Gives a job its own branch and worktree in `folder`, starting from the branch
/// checked out there. `None` when the folder isn't a git repository with a
/// commit: the job then works in the folder itself.
pub fn isolate(folder: &str, title: &str, id: u64) -> Result<Option<Isolated>, String> {
    if !git::is_repo(folder) || !git::has_commits(folder) {
        return Ok(None);
    }
    let base = git::current_branch(folder);
    let base = if base.is_empty() || base == "HEAD" {
        git::default_branch(folder)
    } else {
        base
    };
    let base_sha = git::head(folder).unwrap_or_default();
    let wanted = branch_for(title, id);
    let branch = git::unique_branch(&wanted, |b| {
        git::has_branch(folder, b) || git::worktree_path(folder, b).exists()
    });
    let start = (!base_sha.is_empty()).then_some(base_sha.as_str());
    let worktree = git::add_worktree(folder, &branch, start)?;
    // Local secrets and config a fresh checkout lacks (.worktreeinclude).
    let _ = git::copy_included(folder, &worktree);
    // And the ones the project file asks for.
    if let Some(p) = super::project::load(&worktree) {
        super::project::copy_files(&p, folder, &worktree);
    }
    Ok(Some(Isolated {
        branch,
        worktree,
        base,
        base_sha,
    }))
}

/// Where a job sent from another computer starts: the branch that computer
/// pushed to the hub for it, and the exact commit it starts from. The server
/// starts there rather than from whatever its own copy has checked out.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Handoff {
    pub branch: String,
    pub base_sha: String,
    /// The branch it was on there, which the job lands back on.
    pub base: String,
}

/// A branch name another computer may hand over: Squad's own `squad/…`,
/// nothing git or a shell could read as anything else.
pub fn handoff_branch_ok(b: &str) -> bool {
    b.starts_with("squad/")
        && b.len() < 200
        && !b.contains("..")
        && !b.ends_with('/')
        && !b.ends_with(".lock")
        && b.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// [`isolate`] for a job handed over from another computer: its own branch
/// and worktree at `h.base_sha`, which must already be in `folder`. Keeps
/// the branch name it was given unless something here already has it.
pub fn isolate_at(folder: &str, h: &Handoff) -> Result<Isolated, String> {
    if !git::is_repo(folder) || !git::has_commits(folder) {
        return Err("the server's copy isn't a git repository".into());
    }
    let sha_ok = h.base_sha.len() >= 7 && h.base_sha.chars().all(|c| c.is_ascii_hexdigit());
    if !sha_ok || git::rev(folder, &h.base_sha).is_none() {
        return Err(format!(
            "the server doesn't have commit {}",
            h.base_sha.chars().take(7).collect::<String>()
        ));
    }
    let wanted = if handoff_branch_ok(&h.branch) {
        h.branch.clone()
    } else {
        "squad/job".to_string()
    };
    let branch = git::unique_branch(&wanted, |b| {
        git::has_branch(folder, b) || git::worktree_path(folder, b).exists()
    });
    let worktree = git::add_worktree(folder, &branch, Some(&h.base_sha))?;
    let _ = git::copy_included(folder, &worktree);
    if let Some(p) = super::project::load(&worktree) {
        super::project::copy_files(&p, folder, &worktree);
    }
    let base = if h.base.trim().is_empty() {
        git::default_branch(folder)
    } else {
        h.base.trim().to_string()
    };
    Ok(Isolated {
        branch,
        worktree,
        base,
        base_sha: h.base_sha.clone(),
    })
}

/// What the job changed in its worktree since it started: commits it made and
/// work it left uncommitted.
pub fn changed(job: &Job) -> Vec<Changed> {
    if job.worktree.is_empty() || !Path::new(&job.worktree).is_dir() {
        return Vec::new();
    }
    let against = if job.base_sha.is_empty() {
        job.base.as_str()
    } else {
        job.base_sha.as_str()
    };
    git::changes_since(&job.worktree, against)
        .into_iter()
        .map(|c| Changed {
            path: c.path,
            status: c.status,
            added: c.added,
            removed: c.removed,
        })
        .collect()
}

/// One file's diff in the job's worktree, against where the job started.
pub fn diff(job: &Job, path: &str) -> String {
    let against = if job.base_sha.is_empty() {
        "HEAD"
    } else {
        job.base_sha.as_str()
    };
    git::file_diff_context(&job.worktree, path, false, against)
}

/// The project's checks for the folder, as Checker would run them: the
/// ones its `squad.toml` names, else the ones Squad finds.
pub fn checks_for(dir: &str) -> Vec<String> {
    match super::project::load(dir) {
        Some(p) if !p.checks.is_empty() => p.checks,
        _ => detect_checks(dir),
    }
}

/// The checks Squad finds in the folder by itself.
pub fn detect_checks(dir: &str) -> Vec<String> {
    proof::detect(Path::new(dir))
}

/// Runs the project's checks in `dir` on another thread. Nothing comes back
/// (an empty list) when the project has no checks Squad Code can find.
pub fn run_checks(dir: String) -> Receiver<Vec<CheckRun>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let commands = checks_for(&dir);
        let results = if commands.is_empty() {
            Vec::new()
        } else {
            proof::run_all(&dir, &commands, Duration::from_secs(15 * 60), true, &[])
        };
        let _ = tx.send(results);
    });
    rx
}

/// "641 / 641 passed" style counts are the check tool's business; this is the
/// one-line verdict Squad Code shows.
/// What an ignored check says in place of why it couldn't run.
pub const IGNORED: &str = "ignored for this job";

/// Marks the checks the person said to ignore on this job, so they don't
/// fail it or start a fix turn.
pub fn ignore(checks: &mut [CheckRun], ignored: &[String]) {
    for c in checks.iter_mut() {
        if !c.ok && ignored.contains(&c.command) {
            c.infra = IGNORED.into();
        }
    }
}

pub fn verdict(checks: &[CheckRun], checking: bool) -> (String, Verdict) {
    if checking {
        return ("Checker is running the checks".into(), Verdict::Running);
    }
    if checks.is_empty() {
        return ("No checks found to run".into(), Verdict::None);
    }
    let failed = checks
        .iter()
        .filter(|c| !c.ok && c.infra.is_empty())
        .count();
    let skipped = checks
        .iter()
        .filter(|c| !c.ok && !c.infra.is_empty() && c.infra != IGNORED)
        .count();
    let ignored = checks
        .iter()
        .filter(|c| !c.ok && c.infra == IGNORED)
        .count();
    let n = checks.len();
    if failed == 0 && skipped == 0 && ignored > 0 {
        let passed = n - ignored;
        (
            format!("{passed} of {n} pass, {ignored} ignored for this job"),
            Verdict::Pass,
        )
    } else if failed == 0 && skipped == 0 {
        let text = if n == 1 {
            "The check passes".to_string()
        } else {
            format!("All {n} checks pass")
        };
        (text, Verdict::Pass)
    } else if failed == 0 {
        (format!("{skipped} of {n} couldn't run"), Verdict::Couldnt)
    } else {
        (format!("{failed} of {n} failed"), Verdict::Fail)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Running,
    None,
    Pass,
    Fail,
    Couldnt,
}

/// The commit message for an approved job.
pub fn message(job: &Job) -> String {
    let title = job.title.lines().next().unwrap_or("").trim();
    let title: String = title.chars().take(72).collect();
    let mut msg = if title.is_empty() {
        format!("Squad Code job {}", job.id)
    } else {
        title
    };
    if let Some(words) = job.last_words() {
        let words: String = words.chars().take(1200).collect();
        msg.push_str("\n\n");
        msg.push_str(words.trim());
    }
    msg
}

/// Approves a job: commits what's left in its worktree, merges its branch into
/// the branch it started from, then removes the worktree and the branch.
/// `set_aside` stashes uncommitted changes in your checkout that the merge would
/// overwrite, and puts them back after; without it they stop the merge.
pub fn merge(job: &Job, set_aside: bool) -> Result<git::Merged, git::MergeError> {
    let repo = job.folder.path.as_str();
    let dir = job.worktree.as_str();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err(git::MergeError::Failed(
            "Its worktree is gone, so there's nothing to merge.".into(),
        ));
    }
    if git::has_uncommitted(dir) == Some(true) {
        git::commit(dir, &message(job))?;
    }
    let merged = git::merge_into(repo, &job.branch, &job.base, set_aside)?;
    cleanup(job);
    Ok(merged)
}

/// Throws a job's changes away: its worktree and its branch.
pub fn discard(job: &Job) {
    cleanup(job);
    if !job.branch.is_empty() {
        let _ = git::delete_branch(&job.folder.path, &job.branch, true);
    }
}

/// Removes the job's worktree, and its branch once it's merged.
fn cleanup(job: &Job) {
    let repo = job.folder.path.as_str();
    // The job's browser goes with its worktree.
    if !job.worktree.is_empty() {
        crate::browse::host::close(&job.worktree);
    }
    if !job.worktree.is_empty() && Path::new(&job.worktree).exists() {
        let _ = git::remove_worktree(repo, &job.worktree);
    }
    let _ = git::prune_worktrees(repo);
    if !job.branch.is_empty() && !job.base.is_empty() {
        git::delete_branch_if_merged(repo, &job.branch, &job.base);
    }
}

/// What the seatbelt finds in everything a job would send: its commits and
/// the work it left uncommitted, new files included, since it started.
/// `(path, line, snag)`, line 0 for a whole file.
pub fn seatbelt(job: &Job) -> Vec<(String, usize, Snag)> {
    if job.worktree.is_empty() || job.base_sha.is_empty() || !Path::new(&job.worktree).is_dir() {
        return Vec::new();
    }
    let patch = git::patch_since(&job.worktree, &job.base_sha, 8 * 1024 * 1024);
    seatbelt::scan_patch(&patch)
}

/// Whether any of the seatbelt's finds should stop a push or a merge.
pub fn holds(found: &[(String, usize, Snag)]) -> bool {
    found.iter().any(|(_, _, s)| s.serious())
}

/// The seatbelt's finds as lines for a card, serious ones first:
/// "src/config.ts:14 · Looks like a GitHub token".
pub fn held_lines(found: &[(String, usize, Snag)]) -> Vec<String> {
    let mut found: Vec<&(String, usize, Snag)> = found.iter().collect();
    found.sort_by_key(|(_, _, s)| !s.serious());
    found
        .into_iter()
        .map(|(path, line, snag)| match line {
            0 => format!("{path} \u{b7} {}", snag.words()),
            l => format!("{path}:{l} \u{b7} {}", snag.words()),
        })
        .collect()
}

/// Commits on a job's branch since it started, and commits its base branch
/// got since then.
pub fn counts(job: &Job) -> (usize, usize) {
    if job.worktree.is_empty() || job.base_sha.is_empty() {
        return (0, 0);
    }
    let repo = job.folder.path.as_str();
    let ahead = if Path::new(&job.worktree).is_dir() {
        git::commits_ahead(&job.worktree, "HEAD", &job.base_sha)
    } else {
        0
    };
    let behind = if job.base.is_empty() {
        0
    } else {
        git::commits_ahead(repo, &job.base, &job.base_sha)
    };
    (ahead, behind)
}

/// One job the clash check looks at: its id, its project and its worktree.
#[derive(Clone, Debug)]
pub struct Lane {
    pub id: u64,
    pub repo: String,
    pub worktree: String,
}

/// Jobs in the same project whose work, committed or not, changes the same
/// lines: each pair once, with where ("total.ts around line 40"). Merged in
/// memory; no folder is touched.
pub fn clashes(lanes: &[Lane]) -> Vec<(u64, u64, String)> {
    let tips: Vec<(&Lane, String)> = lanes
        .iter()
        .filter(|l| Path::new(&l.worktree).is_dir())
        .filter_map(|l| git::work_tip(&l.worktree).map(|t| (l, t)))
        .collect();
    let mut out = Vec::new();
    for i in 0..tips.len() {
        for j in i + 1..tips.len() {
            let ((a, ta), (b, tb)) = (&tips[i], &tips[j]);
            if a.repo != b.repo || ta == tb {
                continue;
            }
            if let Ok(files) = git::test_merge(&a.repo, ta, tb) {
                if !files.is_empty() {
                    out.push((a.id, b.id, clash_words(&files)));
                }
            }
        }
    }
    out
}

/// "total.ts around line 40", or "total.ts and cart.ts" for several files.
pub fn clash_words(files: &[git::Overlap]) -> String {
    match files {
        [one] => {
            let name = one.path.rsplit('/').next().unwrap_or(&one.path);
            let at = crate::radar::around(&one.lines);
            if at.is_empty() {
                name.to_string()
            } else {
                format!("{name} {at}")
            }
        }
        _ => crate::radar::file_names(files),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("squadcode-work-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(
            dir.parent()
                .unwrap()
                .join(".squadcode-worktrees")
                .join(dir.file_name().unwrap()),
        );
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&dir, &["add", "-A"]);
        run(&dir, &["commit", "-q", "-m", "first"]);
        dir
    }

    #[test]
    fn branch_names_come_from_the_title() {
        assert_eq!(
            branch_for("Add Apple Pay to checkout", 3),
            "squad/add-apple-pay-checkout"
        );
        assert_eq!(branch_for("", 7), "squad/job-7");
    }

    #[test]
    fn a_job_works_apart_then_merges_into_its_base() {
        let dir = repo("merge");
        let folder = dir.to_string_lossy().into_owned();
        let iso = isolate(&folder, "Change the greeting", 1).unwrap().unwrap();
        assert_eq!(iso.base, "main");
        assert!(Path::new(&iso.worktree).is_dir());
        // The job edits a file and adds one, in its worktree only.
        std::fs::write(Path::new(&iso.worktree).join("a.txt"), "two\n").unwrap();
        std::fs::write(Path::new(&iso.worktree).join("b.txt"), "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
        let mut job = Job {
            id: 1,
            title: "Change the greeting".into(),
            folder: super::super::store::Folder {
                name: "t".into(),
                path: folder.clone(),
                ship: String::new(),
            },
            branch: iso.branch.clone(),
            worktree: iso.worktree.clone(),
            base: iso.base.clone(),
            base_sha: iso.base_sha.clone(),
            ..Default::default()
        };
        job.changes = changed(&job);
        let mut paths: Vec<_> = job.changes.iter().map(|c| c.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
        assert!(diff(&job, "a.txt").contains("+two"));
        // Approving merges into main and cleans up.
        run(&dir, &["config", "user.name", "t"]);
        run(&dir, &["config", "user.email", "t@t"]);
        run(Path::new(&iso.worktree), &["config", "user.name", "t"]);
        run(Path::new(&iso.worktree), &["config", "user.email", "t@t"]);
        merge(&job, false).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");
        assert!(dir.join("b.txt").exists());
        assert!(!Path::new(&iso.worktree).exists());
        assert!(!git::has_branch(&folder, &iso.branch));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discarding_leaves_the_base_alone() {
        let dir = repo("discard");
        let folder = dir.to_string_lossy().into_owned();
        let iso = isolate(&folder, "Try something", 2).unwrap().unwrap();
        std::fs::write(Path::new(&iso.worktree).join("a.txt"), "nope\n").unwrap();
        let job = Job {
            folder: super::super::store::Folder {
                name: "t".into(),
                path: folder.clone(),
                ship: String::new(),
            },
            branch: iso.branch.clone(),
            worktree: iso.worktree.clone(),
            base: iso.base.clone(),
            base_sha: iso.base_sha.clone(),
            ..Default::default()
        };
        discard(&job);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
        assert!(!Path::new(&iso.worktree).exists());
        assert!(!git::has_branch(&folder, &iso.branch));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ignored_check_does_not_fail_the_job() {
        let mut checks = vec![
            CheckRun {
                command: "npm run lint".into(),
                ok: true,
                ..Default::default()
            },
            CheckRun {
                command: "npm test".into(),
                ok: false,
                ..Default::default()
            },
        ];
        assert_eq!(verdict(&checks, false).1, Verdict::Fail);
        ignore(&mut checks, &["npm test".to_string()]);
        let (said, v) = verdict(&checks, false);
        assert_eq!(v, Verdict::Pass);
        assert_eq!(said, "1 of 2 pass, 1 ignored for this job");
    }

    #[test]
    fn verdicts_read_plainly() {
        let ok = CheckRun {
            command: "cargo test".into(),
            ok: true,
            ..Default::default()
        };
        let bad = CheckRun {
            command: "npm test".into(),
            ok: false,
            ..Default::default()
        };
        assert_eq!(
            verdict(std::slice::from_ref(&ok), false).0,
            "The check passes"
        );
        assert_eq!(
            verdict(&[ok, bad], false),
            ("1 of 2 failed".into(), Verdict::Fail)
        );
        assert_eq!(verdict(&[], true).1, Verdict::Running);
        assert_eq!(verdict(&[], false).1, Verdict::None);
    }

    fn job_in(folder: &str, iso: &Isolated, id: u64) -> Job {
        Job {
            id,
            folder: super::super::store::Folder {
                name: "t".into(),
                path: folder.to_string(),
                ship: String::new(),
            },
            branch: iso.branch.clone(),
            worktree: iso.worktree.clone(),
            base: iso.base.clone(),
            base_sha: iso.base_sha.clone(),
            ..Default::default()
        }
    }

    #[test]
    fn the_seatbelt_holds_a_job_that_adds_secrets() {
        let dir = repo("belt");
        let folder = dir.to_string_lossy().into_owned();
        let iso = isolate(&folder, "Wire up payments", 4).unwrap().unwrap();
        let job = job_in(&folder, &iso, 4);
        // Nothing yet, nothing held.
        assert!(seatbelt(&job).is_empty());
        let wt = Path::new(&iso.worktree);
        std::fs::write(wt.join(".env"), "PAY_KEY=abc\n").unwrap();
        std::fs::write(wt.join("a.txt"), "one\nconsole.log(token)\n").unwrap();
        let found = seatbelt(&job);
        assert!(holds(&found), "{found:?}");
        let lines = held_lines(&found);
        assert!(lines[0].starts_with(".env \u{b7} "), "{lines:?}");
        // A leftover alone is pointed out but doesn't hold.
        std::fs::remove_file(wt.join(".env")).unwrap();
        let found = seatbelt(&job);
        assert!(!holds(&found));
        discard(&job);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counts_commits_ahead_and_behind() {
        let dir = repo("counts");
        let folder = dir.to_string_lossy().into_owned();
        let iso = isolate(&folder, "Count things", 5).unwrap().unwrap();
        let job = job_in(&folder, &iso, 5);
        let wt = Path::new(&iso.worktree);
        std::fs::write(wt.join("b.txt"), "b\n").unwrap();
        run(wt, &["add", "-A"]);
        run(wt, &["commit", "-q", "-m", "b"]);
        std::fs::write(dir.join("c.txt"), "c\n").unwrap();
        run(&dir, &["add", "-A"]);
        run(&dir, &["commit", "-q", "-m", "c"]);
        run(&dir, &["commit", "-q", "--allow-empty", "-m", "d"]);
        assert_eq!(counts(&job), (1, 2));
        discard(&job);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_two_jobs_changing_the_same_lines() {
        let dir = repo("clash");
        let folder = dir.to_string_lossy().into_owned();
        let a = isolate(&folder, "Say two", 6).unwrap().unwrap();
        let b = isolate(&folder, "Say three", 7).unwrap().unwrap();
        let c = isolate(&folder, "Add a file", 8).unwrap().unwrap();
        // Uncommitted work counts too.
        std::fs::write(Path::new(&a.worktree).join("a.txt"), "two\n").unwrap();
        std::fs::write(Path::new(&b.worktree).join("a.txt"), "three\n").unwrap();
        std::fs::write(Path::new(&c.worktree).join("z.txt"), "z\n").unwrap();
        let lanes: Vec<Lane> = [(6, &a), (7, &b), (8, &c)]
            .iter()
            .map(|(id, iso)| Lane {
                id: *id,
                repo: folder.clone(),
                worktree: iso.worktree.clone(),
            })
            .collect();
        let found = clashes(&lanes);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].0, found[0].1), (6, 7));
        assert!(found[0].2.starts_with("a.txt"), "{found:?}");
        for (id, iso) in [(6, &a), (7, &b), (8, &c)] {
            discard(&job_in(&folder, iso, id));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// How to start the app in `dir` for job `id` (0 for the project itself),
/// and the address it serves on: what the project's `squad.toml` says,
/// else what Squad finds.
pub fn start_command(dir: &str, id: u64) -> Option<(String, Option<String>)> {
    super::project::load(dir)
        .and_then(|p| p.start(id))
        .or_else(|| detect_start(dir))
}

/// How to start the app in `dir`, and the address it serves on when the
/// code names a port: `npm run dev`, `npm start`, `cargo run`… None when
/// Squad Code can't tell.
pub fn detect_start(dir: &str) -> Option<(String, Option<String>)> {
    let d = Path::new(dir);
    let read = |name: &str| std::fs::read_to_string(d.join(name)).unwrap_or_default();
    let port_in = |text: &str| -> Option<String> {
        for pat in [
            "listen(", "PORT || ", "PORT ?? ", "port=", "port: ", "--port ",
        ] {
            if let Some(at) = text.find(pat) {
                let digits: String = text[at + pat.len()..]
                    .chars()
                    .skip_while(|c| *c == ' ' || *c == '\'' || *c == '"')
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if (2..=5).contains(&digits.len()) {
                    return Some(format!("http://localhost:{digits}"));
                }
            }
        }
        None
    };
    if d.join("package.json").exists() {
        let pkg: serde_json::Value =
            serde_json::from_str(&read("package.json")).unwrap_or_default();
        let scripts = &pkg["scripts"];
        let pm = if d.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if d.join("yarn.lock").exists() {
            "yarn"
        } else if d.join("bun.lockb").exists() || d.join("bun.lock").exists() {
            "bun"
        } else {
            "npm"
        };
        let main = pkg["main"].as_str().unwrap_or("");
        let code: String = [
            main,
            "server.js",
            "index.js",
            "app.js",
            "src/index.js",
            "src/server.js",
        ]
        .iter()
        .filter(|f| !f.is_empty())
        .map(|f| read(f))
        .collect::<Vec<_>>()
        .join("\n");
        // The port a framework serves on when nothing says otherwise.
        let usual = |script: &str| -> Option<String> {
            let port = [
                ("vite", 5173),
                ("next", 3000),
                ("react-scripts", 3000),
                ("ng serve", 4200),
                ("astro", 4321),
                ("nuxt", 3000),
                ("remix", 3000),
                ("gatsby", 8000),
                ("expo start", 8081),
                ("webpack serve", 8080),
                ("vue-cli-service serve", 8080),
            ]
            .iter()
            .find(|(k, _)| script.contains(k))?
            .1;
            Some(format!("http://localhost:{port}"))
        };
        if let Some(dev) = scripts["dev"].as_str() {
            let url = port_in(dev)
                .or_else(|| port_in(&code))
                .or_else(|| usual(dev));
            return Some((format!("{pm} run dev"), url));
        }
        if let Some(start) = scripts["start"].as_str() {
            let cmd = if pm == "npm" {
                "npm start".to_string()
            } else {
                format!("{pm} start")
            };
            let url = port_in(start)
                .or_else(|| port_in(&code))
                .or_else(|| usual(start));
            return Some((cmd, url));
        }
        if let Some(serve) = scripts["serve"].as_str() {
            return Some((
                format!("{pm} run serve"),
                port_in(serve).or_else(|| usual(serve)),
            ));
        }
        if !main.is_empty() || d.join("server.js").exists() {
            let file = if main.is_empty() { "server.js" } else { main };
            return Some((format!("node {file}"), port_in(&code)));
        }
    }
    if d.join("Cargo.toml").exists() {
        return Some(("cargo run".into(), port_in(&read("src/main.rs"))));
    }
    if d.join("manage.py").exists() {
        return Some((
            "python manage.py runserver".into(),
            Some("http://localhost:8000".into()),
        ));
    }
    for f in ["app.py", "main.py", "server.py"] {
        if d.join(f).exists() {
            return Some((format!("python {f}"), port_in(&read(f))));
        }
    }
    if d.join("go.mod").exists() {
        let main = read("main.go");
        return Some(("go run .".into(), port_in(&main).or_else(|| go_port(&main))));
    }
    if d.join("pubspec.yaml").exists() && read("pubspec.yaml").contains("flutter") {
        // A Flutter app with a web build serves it, so it opens in Squad's
        // browser; otherwise it runs on whatever device there is.
        if d.join("web").is_dir() {
            return Some((
                "flutter run -d web-server --web-port 8080".into(),
                Some("http://localhost:8080".into()),
            ));
        }
        return Some(("flutter run".into(), None));
    }
    if d.join("config/routes.rb").exists() {
        return Some((
            "bin/rails server".into(),
            Some("http://localhost:3000".into()),
        ));
    }
    if d.join("artisan").exists() {
        return Some((
            "php artisan serve".into(),
            Some("http://localhost:8000".into()),
        ));
    }
    if d.join("mix.exs").exists() && read("mix.exs").contains("phoenix") {
        return Some((
            "mix phx.server".into(),
            Some("http://localhost:4000".into()),
        ));
    }
    for f in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yml",
        "docker-compose.yaml",
    ] {
        if d.join(f).exists() {
            return Some(("docker compose up".into(), compose_port(&read(f))));
        }
    }
    if d.join("Dockerfile").exists() {
        let file = read("Dockerfile");
        let port = file
            .lines()
            .find_map(|l| l.trim().strip_prefix("EXPOSE "))
            .and_then(|p| p.split_whitespace().next())
            .map(|p| p.trim_end_matches("/tcp").to_string())
            .filter(|p| p.parse::<u16>().is_ok());
        let name = d
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| "app".into())
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let publish = port
            .as_ref()
            .map(|p| format!(" -p {p}:{p}"))
            .unwrap_or_default();
        return Some((
            format!("docker build -t {name} . && docker run --rm{publish} {name}"),
            port.map(|p| format!("http://localhost:{p}")),
        ));
    }
    if d.join("index.html").exists() {
        return Some((
            "python3 -m http.server 8000".into(),
            Some("http://localhost:8000".into()),
        ));
    }
    None
}

/// The port a Go server listens on, from `":8080"` in its code.
fn go_port(code: &str) -> Option<String> {
    let at = code.find("\":")?;
    let digits: String = code[at + 2..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (2..=5)
        .contains(&digits.len())
        .then(|| format!("http://localhost:{digits}"))
}

/// The first port a Compose file publishes, from `- "8080:80"`.
fn compose_port(text: &str) -> Option<String> {
    let mut in_ports = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("ports:") {
            in_ports = true;
            continue;
        }
        if !in_ports {
            continue;
        }
        let Some(item) = t.strip_prefix('-') else {
            in_ports = false;
            continue;
        };
        let item = item.trim().trim_matches(|c| c == '"' || c == '\'');
        // "8080:80", "127.0.0.1:8080:80", "3000"
        let parts: Vec<&str> = item.split(':').collect();
        let host = match parts.len() {
            1 => parts[0],
            n => parts[n - 2],
        };
        if host.parse::<u16>().is_ok() {
            return Some(format!("http://localhost:{host}"));
        }
    }
    None
}

#[cfg(test)]
mod start_tests {
    use super::start_command;

    #[test]
    fn a_node_app_starts_with_its_script_and_port() {
        let dir = std::env::temp_dir().join(format!("squad-start-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"start":"node server.js"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("server.js"),
            "const PORT = process.env.PORT || 3000;\napp.listen(PORT)",
        )
        .unwrap();
        let (cmd, url) = start_command(dir.to_str().unwrap(), 0).unwrap();
        assert_eq!(cmd, "npm start");
        assert_eq!(url.as_deref(), Some("http://localhost:3000"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(start_command("/definitely/not/here", 0).is_none());
    }

    #[test]
    fn other_kinds_of_app_start_too() {
        let dir = std::env::temp_dir().join(format!("squad-kinds-{}", std::process::id()));
        let fresh = |name: &str| {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let d = fresh("compose");
        std::fs::write(
            d.join("docker-compose.yml"),
            "services:\n  web:\n    build: .\n    ports:\n      - \"8080:80\"\n  db:\n    image: postgres\n",
        )
        .unwrap();
        let (cmd, url) = start_command(d.to_str().unwrap(), 0).unwrap();
        assert_eq!(cmd, "docker compose up");
        assert_eq!(url.as_deref(), Some("http://localhost:8080"));

        let d = fresh("flutter");
        std::fs::write(
            d.join("pubspec.yaml"),
            "dependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join("web")).unwrap();
        let (cmd, url) = start_command(d.to_str().unwrap(), 0).unwrap();
        assert!(cmd.starts_with("flutter run"), "{cmd}");
        assert_eq!(url.as_deref(), Some("http://localhost:8080"));

        let d = fresh("react");
        std::fs::write(
            d.join("package.json"),
            r#"{"scripts":{"start":"react-scripts start"}}"#,
        )
        .unwrap();
        let (cmd, url) = start_command(d.to_str().unwrap(), 0).unwrap();
        assert_eq!(cmd, "npm start");
        assert_eq!(url.as_deref(), Some("http://localhost:3000"));

        let d = fresh("go");
        std::fs::write(d.join("go.mod"), "module x\n").unwrap();
        std::fs::write(d.join("main.go"), "http.ListenAndServe(\":9090\", nil)").unwrap();
        let (cmd, url) = start_command(d.to_str().unwrap(), 0).unwrap();
        assert_eq!(cmd, "go run .");
        assert_eq!(url.as_deref(), Some("http://localhost:9090"));

        let d = fresh("docker");
        std::fs::write(d.join("Dockerfile"), "FROM nginx\nEXPOSE 80\n").unwrap();
        let (cmd, url) = start_command(d.to_str().unwrap(), 0).unwrap();
        assert_eq!(
            cmd,
            "docker build -t docker . && docker run --rm -p 80:80 docker"
        );
        assert_eq!(url.as_deref(), Some("http://localhost:80"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
