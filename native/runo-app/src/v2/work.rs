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

use super::store::{Changed, Job};

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
    let slug = slug.strip_prefix("runo/").unwrap_or(&slug).to_string();
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
    Ok(Some(Isolated {
        branch,
        worktree,
        base,
        base_sha,
    }))
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

/// The project's checks for the folder, as Checker would run them.
pub fn checks_for(dir: &str) -> Vec<String> {
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
        .filter(|c| !c.ok && !c.infra.is_empty())
        .count();
    let n = checks.len();
    if failed == 0 && skipped == 0 {
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
/// Returns the new commit on the base branch.
pub fn merge(job: &Job) -> Result<String, String> {
    let repo = job.folder.path.as_str();
    let dir = job.worktree.as_str();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("Its worktree is gone, so there's nothing to merge.".into());
    }
    if git::has_uncommitted(dir) == Some(true) {
        git::commit(dir, &message(job))?;
    }
    let hash = git::merge_into(repo, &job.branch, &job.base)?;
    cleanup(job);
    Ok(hash)
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
    if !job.worktree.is_empty() && Path::new(&job.worktree).exists() {
        let _ = git::remove_worktree(repo, &job.worktree);
    }
    let _ = git::prune_worktrees(repo);
    if !job.branch.is_empty() && !job.base.is_empty() {
        git::delete_branch_if_merged(repo, &job.branch, &job.base);
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
        let dir = std::env::temp_dir().join(format!("runo-work-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(
            dir.parent()
                .unwrap()
                .join(".runo-worktrees")
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
        merge(&job).unwrap();
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
}

/// How to start the app in `dir`, and the address it serves on when the
/// code names a port: `npm run dev`, `npm start`, `cargo run`… None when
/// Squad Code can't tell.
pub fn start_command(dir: &str) -> Option<(String, Option<String>)> {
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
        if let Some(dev) = scripts["dev"].as_str() {
            let url = port_in(dev).or_else(|| port_in(&code)).or_else(|| {
                dev.contains("vite")
                    .then(|| "http://localhost:5173".to_string())
            });
            return Some((format!("{pm} run dev"), url));
        }
        if let Some(start) = scripts["start"].as_str() {
            let cmd = if pm == "npm" {
                "npm start".to_string()
            } else {
                format!("{pm} start")
            };
            return Some((cmd, port_in(start).or_else(|| port_in(&code))));
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
    if d.join("index.html").exists() {
        return Some((
            "python3 -m http.server 8000".into(),
            Some("http://localhost:8000".into()),
        ));
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
        let (cmd, url) = start_command(dir.to_str().unwrap()).unwrap();
        assert_eq!(cmd, "npm start");
        assert_eq!(url.as_deref(), Some("http://localhost:3000"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(start_command("/definitely/not/here").is_none());
    }
}
