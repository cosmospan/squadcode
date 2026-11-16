//! Thin wrappers over the `git` CLI for worktrees, status and diffs.

use std::path::Path;
use std::process::Command;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Change {
    pub path: String,
    pub status: String,
    pub added: usize,
    pub removed: usize,
    /// Some of the change is in the index.
    pub staged: bool,
    /// Some of the change is only in the working tree.
    pub unstaged: bool,
    /// Git treats the file as binary, so there are no line counts.
    pub binary: bool,
}

/// Run git in `dir` with extra environment and optional stdin.
fn git_with(
    dir: &str,
    args: &[&str],
    env: &[(&str, &str)],
    input: Option<&[u8]>,
) -> Result<String, String> {
    use std::io::Write;
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        // Never wait on a password prompt nobody can see.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Reads such as `status` leave the index alone, so they never hold its lock
        // while the person or an agent commits.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not run git: {error}"))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(bytes);
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            err
        })
    }
}

fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    git_with(dir, args, &[], None)
}

/// The files in `dir` called `name`, in any folder, from `dir`: the ones git
/// tracks and new ones it doesn't ignore. None when `dir` isn't in a repository.
pub fn tracked_named(dir: &str, name: &str) -> Option<Vec<String>> {
    let pattern = format!(":(glob)**/{name}");
    git(
        dir,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            &pattern,
        ],
    )
    .ok()
    .map(|out| split_z(&out))
}

/// Whether `rev` names an object in the repository.
fn exists(dir: &str, rev: &str) -> bool {
    git(
        dir,
        &["rev-parse", "-q", "--verify", &format!("{rev}^{{commit}}")],
    )
    .is_ok()
}

/// The commit the folder is on, if there is one yet.
pub fn head(dir: &str) -> Option<String> {
    rev(dir, "HEAD")
}

pub fn rev(dir: &str, rev: &str) -> Option<String> {
    git(
        dir,
        &["rev-parse", "-q", "--verify", &format!("{rev}^{{commit}}")],
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Whether `a` is `b` or one of its ancestors.
pub fn is_ancestor(dir: &str, a: &str, b: &str) -> bool {
    git(dir, &["merge-base", "--is-ancestor", a, b]).is_ok()
}

fn count(dir: &str, args: &[&str]) -> Option<usize> {
    git(dir, args).ok().and_then(|s| s.trim().parse().ok())
}

/// Keep `pattern` out of `git status` for this repository and all its worktrees,
/// without touching any tracked file (.git/info/exclude).
pub fn exclude(dir: &str, pattern: &str) {
    let Ok(common) = git(dir, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let common = Path::new(dir).join(common.trim());
    let file = common.join("info").join("exclude");
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    if current.lines().any(|l| l.trim() == pattern) {
        return;
    }
    let _ = std::fs::create_dir_all(common.join("info"));
    let sep = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let _ = std::fs::write(&file, format!("{current}{sep}{pattern}\n"));
}

/// Whether git tracks `path` (relative to `dir`).
pub fn is_tracked(dir: &str, path: &str) -> bool {
    git(dir, &["ls-files", "--error-unmatch", "--", path]).is_ok()
}

/// Keep one file in `dir` (given relative to it) out of `git status`.
pub fn exclude_path(dir: &str, path: &str) {
    let Ok(prefix) = git(dir, &["rev-parse", "--show-prefix"]) else {
        return;
    };
    exclude(dir, &format!("/{}{path}", prefix.trim()));
}

pub fn is_repo(dir: &str) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok()
}

pub fn current_branch(dir: &str) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Where an isolated session's worktree lives: `<repo-parent>/.squadcode-worktrees/<repo>/<branch>`.
/// A branch with slashes gets nested folders, so `a/b` and `a-b` never share one.
pub fn worktree_path(repo: &str, branch: &str) -> std::path::PathBuf {
    let repo_path = Path::new(repo);
    let name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    let mut path = repo_path
        .parent()
        .unwrap_or(repo_path)
        .join(".squadcode-worktrees")
        .join(name);
    for part in branch.split('/').filter(|p| !p.is_empty() && *p != "..") {
        path.push(part);
    }
    path
}

/// A branch name drawn from a task, such as `squad/fix-login-redirect` for "Fix the login
/// redirect". Uses the first line's first few words; empty when it has no usable words.
pub fn branch_slug(task: &str) -> String {
    const SKIP: &[&str] = &[
        "a", "an", "the", "please", "can", "could", "you", "to", "of", "for", "and", "in", "on",
        "with",
    ];
    let line = task.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut slug = String::new();
    for word in line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| !SKIP.contains(&w.as_str()))
        .take(5)
    {
        if !slug.is_empty() && slug.len() + 1 + word.len() > 40 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word[..word.len().min(40)]);
    }
    if slug.is_empty() {
        slug
    } else {
        format!("squad/{slug}")
    }
}

/// `wanted`, or `wanted-2`, `wanted-3`… when `taken` says the name is in use.
pub fn unique_branch(wanted: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|n| format!("{wanted}-{n}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// Whether the repository has a local branch called `name`.
pub fn has_branch(repo: &str, name: &str) -> bool {
    exists(repo, &format!("refs/heads/{name}"))
}

/// The remote to fetch from and push to: `origin`, else the first one, else empty.
pub fn remote_name(dir: &str) -> String {
    let remotes = git(dir, &["remote"]).unwrap_or_default();
    if remotes.lines().any(|r| r == "origin") {
        "origin".to_string()
    } else {
        remotes.lines().next().unwrap_or("").to_string()
    }
}

/// The repository's main branch: what the remote's HEAD points at, else `main` or
/// `master`, else the branch checked out now.
pub fn default_branch(dir: &str) -> String {
    let remote = remote_name(dir);
    if !remote.is_empty() {
        if let Ok(head) = git(
            dir,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                &format!("refs/remotes/{remote}/HEAD"),
            ],
        ) {
            if let Some(branch) = head.trim().strip_prefix(&format!("{remote}/")) {
                return branch.to_string();
            }
        }
    }
    for name in ["main", "master", "trunk", "develop"] {
        if exists(dir, &format!("refs/heads/{name}"))
            || (!remote.is_empty() && exists(dir, &format!("refs/remotes/{remote}/{name}")))
        {
            return name.to_string();
        }
    }
    let current = current_branch(dir);
    if current == "HEAD" {
        String::new()
    } else {
        current
    }
}

/// Bring the remote's branches up to date. Fine when there is no remote.
pub fn fetch(dir: &str) -> Result<(), String> {
    let remote = remote_name(dir);
    if remote.is_empty() {
        return Ok(());
    }
    git(dir, &["fetch", "--quiet", "--prune", &remote])
        .map(|_| ())
        .map_err(|e| auth_hint(&e))
}

/// Git's error, with a pointer to the fix when it failed for want of credentials.
pub fn auth_hint(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("terminal prompts disabled")
        || lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied (publickey")
    {
        format!(
            "{}\nGit needs credentials. Run `gh auth setup-git` (or push once in a shell tab) so it can sign in without asking.",
            error.lines().next().unwrap_or(error)
        )
    } else {
        error.to_string()
    }
}

/// What the session branch is compared and merged against: the remote's copy of
/// `base` when it exists, otherwise the local branch.
pub fn base_ref(dir: &str, base: &str) -> String {
    let remote = remote_name(dir);
    let remote_ref = format!("{remote}/{base}");
    let has_local = exists(dir, &format!("refs/heads/{base}"));
    let has_remote = !remote.is_empty() && exists(dir, &format!("refs/remotes/{remote_ref}"));
    match (has_local, has_remote) {
        // The local branch may hold commits not pushed yet; use it then.
        (true, true) if !is_ancestor(dir, base, &remote_ref) => base.to_string(),
        (_, true) => remote_ref,
        (true, false) => base.to_string(),
        _ => "HEAD".to_string(),
    }
}

/// Whether the repository has a commit checked out; a fresh `git init` has none.
pub fn has_commits(dir: &str) -> bool {
    exists(dir, "HEAD")
}

/// Create the session worktree on a new (or existing) branch. A new branch starts at
/// `start` (a commit-ish such as `origin/main`), or at HEAD when `None`.
pub fn add_worktree(repo: &str, branch: &str, start: Option<&str>) -> Result<String, String> {
    if !has_commits(repo) {
        return Err(
            "the repository has no commits yet. Make a first commit, then start the session again."
                .into(),
        );
    }
    let dest = worktree_path(repo, branch);
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    let dest_str = dest.to_string_lossy().into_owned();
    if exists(repo, &format!("refs/heads/{branch}")) {
        git(repo, &["worktree", "add", &dest_str, branch])?;
    } else {
        let mut args = vec!["worktree", "add", "--no-track", "-b", branch, &dest_str];
        if let Some(start) = start {
            args.push(start);
        }
        git(repo, &args)?;
    }
    Ok(dest_str)
}

/// Push the checked-out branch and make it track the remote branch of the same name.
pub fn push(dir: &str, remote: &str) -> Result<String, String> {
    git(dir, &["push", "-u", remote, "HEAD"]).map_err(|e| auth_hint(&e))
}

pub fn remove_worktree(repo: &str, dir: &str) -> Result<(), String> {
    git(repo, &["worktree", "remove", "--force", dir]).map(|_| ())
}

/// Forget worktrees whose folder is gone.
pub fn prune_worktrees(repo: &str) -> Result<(), String> {
    git(repo, &["worktree", "prune"]).map(|_| ())
}

/// Whether the worktree has uncommitted changes, untracked files included. None when
/// git can't tell, such as for a folder that is not a worktree.
pub fn has_uncommitted(dir: &str) -> Option<bool> {
    git(dir, &["status", "--porcelain"])
        .ok()
        .map(|s| !s.trim().is_empty())
}

/// The last `n` commits on the checked-out branch: short hash, subject and how
/// long ago, newest first. Empty for a repository with no commits yet.
pub fn log_lines(dir: &str, n: usize) -> Vec<(String, String, String)> {
    git(
        dir,
        &[
            "log",
            &format!("-{n}"),
            "--no-decorate",
            "--format=%h%x1f%s%x1f%cr",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|l| {
        let mut p = l.splitn(3, '\x1f');
        Some((
            p.next()?.to_string(),
            p.next()?.to_string(),
            p.next().unwrap_or("").to_string(),
        ))
    })
    .collect()
}

/// Uncommitted files: git's two-letter status and the path, at most `n` of
/// them, and how many there are in all.
pub fn changed_paths(dir: &str, n: usize) -> (Vec<(String, String)>, usize) {
    let out = git(dir, &["status", "--porcelain"]).unwrap_or_default();
    let all: Vec<&str> = out.lines().filter(|l| l.len() > 3).collect();
    let some = all
        .iter()
        .take(n)
        .map(|l| (l[..2].trim().to_string(), l[3..].trim().to_string()))
        .collect();
    (some, all.len())
}

/// Delete a branch. Without `force` git refuses when it holds unmerged work.
pub fn delete_branch(repo: &str, branch: &str, force: bool) -> Result<(), String> {
    git(repo, &["branch", if force { "-D" } else { "-d" }, branch]).map(|_| ())
}

/// Commits on `branch` that `base` does not have.
pub fn commits_ahead(dir: &str, branch: &str, base: &str) -> usize {
    count(dir, &["rev-list", "--count", &format!("{base}..{branch}")]).unwrap_or(0)
}

/// Delete a session's branch unless that would lose work: it goes when `base` already
/// has all of its commits. Returns whether it was deleted.
pub fn delete_branch_if_merged(repo: &str, branch: &str, base: &str) -> bool {
    if branch.is_empty() || branch == base || !exists(repo, &format!("refs/heads/{branch}")) {
        return false;
    }
    let base = base_ref(repo, base);
    commits_ahead(repo, branch, &base) == 0 && delete_branch(repo, branch, true).is_ok()
}

/// How bringing a branch up to date went.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    UpToDate,
    Updated,
    /// Stopped on conflicts in these files; the worktree is left mid-rebase or mid-merge.
    Conflicts(Vec<String>),
}

/// Files with unresolved conflicts.
pub fn conflicted(dir: &str) -> Vec<String> {
    git(dir, &["diff", "--name-only", "-z", "--diff-filter=U"])
        .map(|s| split_z(&s))
        .unwrap_or_default()
}

fn split_z(s: &str) -> Vec<String> {
    s.split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Rebase (or merge) the checked-out branch onto `onto`, stashing uncommitted work
/// around it. Conflicts leave the worktree as git left it, for someone to resolve.
pub fn update_from(dir: &str, onto: &str, rebase: bool) -> Result<Update, String> {
    if count(dir, &["rev-list", "--count", &format!("HEAD..{onto}")]) == Some(0) {
        return Ok(Update::UpToDate);
    }
    let editor = [("GIT_EDITOR", "true")];
    let result = if rebase {
        git_with(dir, &["rebase", "--autostash", onto], &editor, None)
    } else {
        git_with(
            dir,
            &["merge", "--autostash", "--no-edit", onto],
            &editor,
            None,
        )
    };
    match result {
        Ok(_) => Ok(Update::Updated),
        Err(error) => {
            let files = conflicted(dir);
            if files.is_empty() {
                Err(error)
            } else {
                Ok(Update::Conflicts(files))
            }
        }
    }
}

/// A rebase or merge stopped half way: "rebase" or "merge".
pub fn in_progress(dir: &str) -> Option<&'static str> {
    let path = |name: &str| {
        git(dir, &["rev-parse", "--git-path", name])
            .map(|p| Path::new(dir).join(p.trim()))
            .ok()
    };
    if ["rebase-merge", "rebase-apply"]
        .iter()
        .any(|n| path(n).is_some_and(|p| p.exists()))
    {
        Some("rebase")
    } else if path("MERGE_HEAD").is_some_and(|p| p.exists()) {
        Some("merge")
    } else {
        None
    }
}

/// A file two lines of work both change the same lines of, and where: the first line of
/// each clashing spot in the file as the two would merge.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlap {
    pub path: String,
    pub lines: Vec<usize>,
}

/// Merge `a` and `b` in memory, touching no folder, and say where they clash. Empty when
/// they come together cleanly.
pub fn test_merge(dir: &str, a: &str, b: &str) -> Result<Vec<Overlap>, String> {
    let out = match git(dir, &["merge-tree", "--write-tree", "--no-messages", a, b]) {
        Ok(_) => return Ok(Vec::new()),
        // With clashes git exits 1 and prints the merged tree, then the clashing files.
        Err(out) => out,
    };
    let mut lines = out.lines();
    let tree = lines.next().unwrap_or("").trim().to_string();
    if tree.len() < 40 || !tree.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(out);
    }
    let mut paths: Vec<String> = lines
        .filter_map(|l| l.split_once('\t').map(|(_, p)| p.to_string()))
        .collect();
    paths.dedup();
    Ok(paths
        .into_iter()
        .map(|path| {
            let merged =
                git(dir, &["cat-file", "-p", &format!("{tree}:{path}")]).unwrap_or_default();
            let lines = merged
                .lines()
                .enumerate()
                .filter(|(_, l)| l.starts_with("<<<<<<<"))
                .map(|(i, _)| i + 1)
                .collect();
            Overlap { path, lines }
        })
        .collect())
}

/// The tip of a folder's work: its uncommitted changes as a commit nothing points at,
/// or its HEAD when there are none.
pub fn work_tip(dir: &str) -> Option<String> {
    let wip = git(dir, &["stash", "create"]).unwrap_or_default();
    let wip = wip.trim();
    if wip.is_empty() {
        rev(dir, "HEAD")
    } else {
        Some(wip.to_string())
    }
}

/// Give up on a stopped rebase or merge, back to where it started.
pub fn abort_update(dir: &str) -> Result<(), String> {
    match in_progress(dir) {
        Some("rebase") => git(dir, &["rebase", "--abort"]).map(|_| ()),
        Some(_) => git(dir, &["merge", "--abort"]).map(|_| ()),
        None => Ok(()),
    }
}

/// The worktree that has `branch` checked out, if any.
pub fn checked_out_at(repo: &str, branch: &str) -> Option<String> {
    let list = git(repo, &["worktree", "list", "--porcelain", "-z"]).ok()?;
    let mut path = None;
    for field in list.split('\0') {
        if let Some(p) = field.strip_prefix("worktree ") {
            path = Some(p.to_string());
        } else if field == format!("branch refs/heads/{branch}") {
            return path;
        }
    }
    None
}

/// Why a merge didn't happen.
#[derive(Clone, Debug, PartialEq)]
pub enum MergeError {
    /// The checkout where `base` is has uncommitted changes to these files, and the
    /// merge changes them too, so merging there would overwrite them.
    InTheWay {
        dir: String,
        files: Vec<String>,
    },
    Failed(String),
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::InTheWay { dir, files } => write!(
                f,
                "{dir} has changes you haven't committed to {}, and the merge can't go ahead over {}.",
                files.join(", "),
                if files.len() == 1 { "it" } else { "them" }
            ),
            MergeError::Failed(why) => f.write_str(why),
        }
    }
}

impl From<String> for MergeError {
    fn from(why: String) -> Self {
        MergeError::Failed(why)
    }
}

/// A merge that went through.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    /// The new short hash of `base`.
    pub hash: String,
    /// Files with your uncommitted changes that were set aside for the merge.
    pub set_aside: Vec<String>,
    /// When those changes clash with what came in, they stay in the stash
    /// instead of going back into the files: the stash commit, for
    /// [`bring_back`].
    pub kept: Option<String>,
}

/// Files with changes not yet committed in `dir`, staged or not. Untracked files
/// don't count: a merge leaves them alone unless it adds the same path, and then
/// git refuses on its own.
fn uncommitted_files(dir: &str) -> Vec<String> {
    git(dir, &["diff", "HEAD", "--name-only", "-z", "--no-renames"])
        .map(|s| split_z(&s))
        .unwrap_or_default()
}

/// Files with changes staged in `dir`.
fn staged_files(dir: &str) -> Vec<String> {
    git(
        dir,
        &["diff", "--cached", "--name-only", "-z", "--no-renames"],
    )
    .map(|s| split_z(&s))
    .unwrap_or_default()
}

/// Whether merging `branch` into `base` makes a merge commit rather than
/// moving `base` ahead.
fn makes_a_commit(dir: &str, branch: &str, base: &str) -> bool {
    !is_ancestor(dir, base, branch)
        || git(dir, &["config", "--get", "merge.ff"]).is_ok_and(|v| v.trim() == "false")
}

/// Uncommitted changes in the checkout where `base` is that stop merging
/// `branch` there: that folder and the files, or `None` when nothing is in the
/// way. Those are changes to files the merge changes too, and, when the merge
/// makes a commit, any staged change, since git won't make a merge commit over
/// a staged one. Unstaged changes to other files don't stop a merge.
pub fn in_the_way(repo: &str, branch: &str, base: &str) -> Option<(String, Vec<String>)> {
    let dir = checked_out_at(repo, base)?;
    let dirty = uncommitted_files(&dir);
    if dirty.is_empty() {
        return None;
    }
    let staged = if makes_a_commit(&dir, branch, base) {
        staged_files(&dir)
    } else {
        Vec::new()
    };
    let touched = git(
        repo,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            &format!("{base}...{branch}"),
        ],
    )
    .map(|s| split_z(&s))
    .unwrap_or_default();
    let files: Vec<String> = dirty
        .into_iter()
        .filter(|f| touched.contains(f) || staged.contains(f))
        .collect();
    (!files.is_empty()).then_some((dir, files))
}

/// Merge `branch` into `base` without touching the session's worktree. Uses the
/// worktree where `base` is checked out, or a temporary one. Uncommitted changes
/// there to files the merge doesn't touch stay as they are; ones it would
/// overwrite stop it, unless `set_aside`, which stashes them first and puts them
/// back after. Conflicts abort the merge.
pub fn merge_into(
    repo: &str,
    branch: &str,
    base: &str,
    set_aside: bool,
) -> Result<Merged, MergeError> {
    if !exists(repo, &format!("refs/heads/{base}")) {
        // Only the remote has it: start a local branch there first.
        let remote = remote_name(repo);
        git(
            repo,
            &["branch", "--track", base, &format!("{remote}/{base}")],
        )?;
    }
    let editor = [("GIT_EDITOR", "true")];
    let mut stashed = None;
    let (dir, temporary) = match checked_out_at(repo, base) {
        Some(dir) => {
            if let Some((_, files)) = in_the_way(repo, branch, base) {
                if !set_aside {
                    return Err(MergeError::InTheWay { dir, files });
                }
                let label = format!("Squad Code: set aside to merge {branch}");
                let mut args = vec!["stash", "push", "--quiet", "-m", label.as_str(), "--"];
                args.extend(files.iter().map(String::as_str));
                git(&dir, &args)?;
                stashed = Some((rev(&dir, "refs/stash").unwrap_or_default(), files));
            }
            (dir, false)
        }
        None => {
            let common = git(repo, &["rev-parse", "--git-common-dir"])?;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = Path::new(repo)
                .join(common.trim())
                .join(format!("squadcode-merge-{nanos}"))
                .to_string_lossy()
                .into_owned();
            git(repo, &["worktree", "add", "--quiet", &dir, base])?;
            (dir, true)
        }
    };
    let result = git_with(&dir, &["merge", "--no-edit", branch], &editor, None);
    let outcome = match result {
        Ok(_) => git(&dir, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string()),
        Err(error) => {
            let files = conflicted(&dir);
            let _ = git(&dir, &["merge", "--abort"]);
            Err(if files.is_empty() {
                error
            } else {
                format!(
                    "Merging stopped on conflicts in {}. Update the branch from {base} first so they can be resolved there.",
                    files.join(", ")
                )
            })
        }
    };
    let kept = match &stashed {
        Some((sha, files)) => put_back(&dir, sha, files),
        None => None,
    };
    if temporary {
        let _ = remove_worktree(repo, &dir);
    }
    let set_aside = stashed.map(|(_, files)| files).unwrap_or_default();
    outcome
        .map(|hash| Merged {
            hash,
            set_aside,
            kept,
        })
        .map_err(MergeError::Failed)
}

/// The stash entry for the stash commit `sha` ("stash@{2}"), if it's still there.
fn stash_entry(dir: &str, sha: &str) -> Option<String> {
    if sha.is_empty() {
        return None;
    }
    (0..100)
        .map(|n| format!("stash@{{{n}}}"))
        .take_while(|r| rev(dir, r).is_some())
        .find(|r| rev(dir, r).as_deref() == Some(sha))
}

/// Brings changes a merge kept in the stash (see [`Merged::kept`]) back into
/// `dir`, with the clash marked in the files, and drops them from the stash:
/// the marked files hold both sides. Gives back the files that clash.
pub fn bring_back(dir: &str, sha: &str) -> Result<Vec<String>, String> {
    let at = stash_entry(dir, sha)
        .ok_or_else(|| "Those changes aren't in the stash any more.".to_string())?;
    let applied = git(dir, &["stash", "apply", "--quiet", &at]);
    let clash = conflicted(dir);
    if let (Err(why), true) = (applied, clash.is_empty()) {
        return Err(why);
    }
    let _ = git(dir, &["stash", "drop", "--quiet", &at]);
    // The files read as plain uncommitted changes with the clash marked, not
    // as a merge git waits on, which would stop the next one.
    if !clash.is_empty() {
        let mut reset = vec!["reset", "--quiet", "--"];
        reset.extend(clash.iter().map(String::as_str));
        let _ = git(dir, &reset);
    }
    Ok(clash)
}

/// Puts changes set aside before a merge back into `dir`. When they clash with
/// what the merge brought in, the files go back to the merged version and the
/// changes stay in the stash: its commit then.
fn put_back(dir: &str, sha: &str, files: &[String]) -> Option<String> {
    // The newest stash entry is ours unless something stashed in between.
    let at = stash_entry(dir, sha)?;
    // Staged changes go back staged when they can; git refuses that without
    // touching anything when the index can't take them, and then they go back
    // as plain changes.
    let applied = git(dir, &["stash", "apply", "--quiet", "--index", &at]).is_ok()
        || (conflicted(dir).is_empty() && git(dir, &["stash", "apply", "--quiet", &at]).is_ok());
    if applied && conflicted(dir).is_empty() {
        let _ = git(dir, &["stash", "drop", "--quiet", &at]);
        return None;
    }
    // A clash: leave the merged files as they are and keep the changes stashed.
    let mut reset = vec!["reset", "--quiet", "--"];
    reset.extend(files.iter().map(String::as_str));
    let _ = git(dir, &reset);
    let mut checkout = vec!["checkout", "HEAD", "--"];
    checkout.extend(files.iter().map(String::as_str));
    let _ = git(dir, &checkout);
    Some(sha.to_string())
}

/// The commit where the branch left `base`.
pub fn merge_base(dir: &str, base: &str) -> Option<String> {
    git(dir, &["merge-base", "HEAD", base])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Added and removed line counts per path from `--numstat -z` output; binary files
/// report `-` for both.
fn numstat(out: &str) -> std::collections::HashMap<String, (usize, usize, bool)> {
    let mut map = std::collections::HashMap::new();
    for entry in out.split('\0') {
        let mut parts = entry.trim_start_matches('\n').splitn(3, '\t');
        if let (Some(a), Some(r), Some(path)) = (parts.next(), parts.next(), parts.next()) {
            map.insert(
                path.to_string(),
                (a.parse().unwrap_or(0), r.parse().unwrap_or(0), a == "-"),
            );
        }
    }
    map
}

/// Line count of a new file, or `None` when it looks binary.
fn new_file_lines(path: &Path) -> Option<usize> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 8_000_000 {
        return Some(0);
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return None;
    }
    Some(bytes.split(|b| *b == b'\n').count() - usize::from(bytes.ends_with(b"\n")))
}

/// Uncommitted changes: the working tree and index against HEAD.
pub fn changes(dir: &str) -> Vec<Change> {
    let stats = numstat(
        &git(dir, &["diff", "HEAD", "--numstat", "-z", "--no-renames"]).unwrap_or_default(),
    );
    let mut result = Vec::new();
    let status = git(
        dir,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )
    .unwrap_or_default();
    let mut entries = status.split('\0');
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || !entry.is_char_boundary(3) {
            continue;
        }
        let bytes = entry.as_bytes();
        let (x, y) = (bytes[0], bytes[1]);
        let code = entry[..2].trim().to_string();
        let path = entry[3..].to_string();
        // Renames and copies carry the old path as the next entry.
        if matches!(x, b'R' | b'C') {
            entries.next();
        }
        let untracked = code == "??";
        let (added, removed, binary) = if untracked {
            match new_file_lines(&Path::new(dir).join(&path)) {
                Some(lines) => (lines, 0, false),
                None => (0, 0, true),
            }
        } else {
            stats.get(&path).copied().unwrap_or((0, 0, false))
        };
        result.push(Change {
            path,
            status: if untracked { "A".into() } else { code },
            added,
            removed,
            staged: !matches!(x, b' ' | b'?'),
            unstaged: y != b' ',
            binary,
        });
    }
    result
}

/// Everything the branch changed since it left `base`: commits plus uncommitted work,
/// so committed work counts too.
pub fn changes_since(dir: &str, base: &str) -> Vec<Change> {
    let Some(start) = merge_base(dir, base) else {
        return changes(dir);
    };
    let stats = numstat(
        &git(dir, &["diff", &start, "--numstat", "-z", "--no-renames"]).unwrap_or_default(),
    );
    let names = git(
        dir,
        &["diff", &start, "--name-status", "-z", "--no-renames"],
    )
    .unwrap_or_default();
    let mut result = Vec::new();
    let mut fields = names.split('\0');
    while let (Some(code), Some(path)) = (fields.next(), fields.next()) {
        let (added, removed, binary) = stats.get(path).copied().unwrap_or((0, 0, false));
        result.push(Change {
            path: path.to_string(),
            status: code.trim().to_string(),
            added,
            removed,
            binary,
            ..Default::default()
        });
    }
    let untracked =
        git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).unwrap_or_default();
    for path in split_z(&untracked) {
        let lines = new_file_lines(&Path::new(dir).join(&path));
        result.push(Change {
            path,
            status: "A".into(),
            added: lines.unwrap_or(0),
            binary: lines.is_none(),
            unstaged: true,
            ..Default::default()
        });
    }
    result
}

#[cfg(test)]
pub fn file_diff(dir: &str, path: &str) -> String {
    file_diff_context(dir, path, false, "HEAD")
}

/// The diff of one file against `against` (HEAD, or where the branch started); with
/// `whole` every unchanged line is included as context.
pub fn file_diff_context(dir: &str, path: &str, whole: bool, against: &str) -> String {
    let tracked = git(dir, &["ls-files", "--error-unmatch", "--", path]).is_ok()
        || git(dir, &["cat-file", "-e", &format!("{against}:{path}")]).is_ok();
    let out = if tracked {
        let context = if whole { "-U100000" } else { "-U3" };
        git(dir, &["diff", context, against, "--", path])
    } else {
        // `--no-index` exits 1 when files differ, which `git()` reports as an error.
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["diff", "--no-index", "--", "/dev/null", path])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map_err(|e| e.to_string())
    };
    out.unwrap_or_else(|e| e)
}

/// Copy git-ignored files that match the repository's `.worktreeinclude` patterns
/// (gitignore syntax, e.g. `.env` or `config/*.local.json`) into a new worktree, so
/// it has the local secrets and config a fresh checkout lacks. Returns the files copied.
pub fn copy_included(repo: &str, worktree: &str) -> Result<Vec<String>, String> {
    let include = Path::new(repo).join(".worktreeinclude");
    if !include.is_file() {
        return Ok(Vec::new());
    }
    let include = include.to_string_lossy().into_owned();
    // Ignored files matching the include patterns: `--exclude-standard` alone lists what
    // is ignored, and the include file narrows it, in one walk of the tree.
    let listed = git(
        repo,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
            "-z",
        ],
    )?;
    let patterns = git(
        repo,
        &[
            "ls-files",
            "--others",
            "--ignored",
            &format!("--exclude-from={include}"),
            "-z",
        ],
    )?;
    let ignored: Vec<String> = split_z(&listed);
    let mut copied = Vec::new();
    for path in split_z(&patterns) {
        // Only files that git ignores qualify: tracked files are already in the worktree.
        let is_ignored = ignored
            .iter()
            .any(|i| *i == path || (i.ends_with('/') && path.starts_with(i.as_str())));
        if !is_ignored {
            continue;
        }
        let dest = Path::new(worktree).join(&path);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(Path::new(repo).join(&path), &dest).map_err(|e| format!("{path}: {e}"))?;
        copied.push(path);
    }
    Ok(copied)
}

/// Files under `path` (relative to `dir`) that differ from the last commit: changed,
/// staged, deleted or new, relative to `dir`. Ignored files don't count.
pub fn changed_under(dir: &str, path: &str) -> Vec<String> {
    let tracked = if exists(dir, "HEAD") {
        git(
            dir,
            &[
                "diff",
                "--name-only",
                "--no-renames",
                "--relative",
                "-z",
                "HEAD",
                "--",
                path,
            ],
        )
    } else {
        git(dir, &["ls-files", "-z", "--", path])
    };
    let new = git(
        dir,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            path,
        ],
    );
    let mut out: Vec<String> = [tracked, new]
        .into_iter()
        .flat_map(|r| split_z(&r.unwrap_or_default()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A file's text as the last commit has it, `path` relative to `dir`.
pub fn committed_text(dir: &str, path: &str) -> Option<String> {
    git(dir, &["show", &format!("HEAD:./{path}")]).ok()
}

/// Put everything under `path` (relative to `dir`) back as the last commit has it, staged
/// changes too: committed files come back and files made since are removed, so git shows
/// no changes there. Ignored files are left alone.
pub fn discard_under(dir: &str, path: &str) -> Result<(), String> {
    if exists(dir, "HEAD") {
        if let Err(e) = git(
            dir,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                path,
            ],
        ) {
            // Nothing under it was ever committed or staged: only new files to remove.
            if !e.contains("did not match") {
                return Err(e);
            }
        }
    } else {
        git(
            dir,
            &["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--", path],
        )?;
    }
    git(dir, &["clean", "-f", "-d", "-q", "--", path]).map(|_| ())
}

/// Commit what is staged, or every change when nothing is. Returns the short hash of
/// the new commit.
pub fn commit(dir: &str, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message first.".into());
    }
    let staged = git(dir, &["diff", "--cached", "--quiet"]).is_err();
    if !staged {
        git(dir, &["add", "-A"])?;
    }
    git(dir, &["commit", "-q", "-m", message])?;
    git(dir, &["rev-parse", "--short", "HEAD"]).map(|s| s.trim().to_string())
}

/// Run git with a private index file, so snapshots never touch the user's staging area.
fn git_index(dir: &str, index: &Path, args: &[&str]) -> Result<String, String> {
    let index = index.to_string_lossy();
    git_with(dir, args, &[("GIT_INDEX_FILE", index.as_ref())], None)
}

/// Where a folder's repository keeps things, from one `git rev-parse`.
struct Layout {
    git_dir: String,
    /// The user's index file.
    index: std::path::PathBuf,
    /// The folder is the top of its working tree.
    top: bool,
}

fn layout(dir: &str) -> Result<Layout, String> {
    let out = git(
        dir,
        &[
            "rev-parse",
            "--absolute-git-dir",
            "--git-path",
            "index",
            "--show-prefix",
        ],
    )?;
    let mut lines = out.lines();
    let git_dir = lines.next().unwrap_or("").trim().to_string();
    let index = Path::new(dir).join(lines.next().unwrap_or("").trim());
    let top = lines.next().unwrap_or("").trim().is_empty();
    Ok(Layout {
        git_dir,
        index,
        top,
    })
}

fn temp_index_in(git_dir: &str) -> std::path::PathBuf {
    // Snapshots run side by side on background threads; each gets its own file.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Path::new(git_dir).join(format!(
        "squadcode-index-{}-{nanos}-{n}",
        std::process::id()
    ))
}

/// A private index to snapshot from, removed when dropped.
struct TempIndex(std::path::PathBuf);

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A private copy of the user's index, so `git add` reuses its saved file times and
/// hashes only the files that changed. Its `write-tree` is the user's staging. In a
/// folder below the top of the working tree the snapshot starts from HEAD, as it always
/// has, so files outside the folder don't move between snapshots.
fn index_copy(dir: &str) -> Result<(TempIndex, Option<String>), String> {
    let layout = layout(dir)?;
    let copy = TempIndex(temp_index_in(&layout.git_dir));
    // No index yet stands for an empty one, whose tree is the empty tree.
    if layout.index.exists() {
        std::fs::copy(&layout.index, &copy.0)
            .map_err(|e| format!("could not copy the index: {e}"))?;
    }
    let staged = git_index(dir, &copy.0, &["write-tree"])
        .ok()
        .map(|t| t.trim().to_string());
    if !layout.top {
        let _ = std::fs::remove_file(&copy.0);
        let _ = git_index(dir, &copy.0, &["read-tree", "HEAD"]);
    }
    Ok((copy, staged))
}

/// Tree id of the whole working tree (tracked and untracked, minus ignored files) from
/// a private index.
fn tree_from(dir: &str, index: &Path) -> Result<String, String> {
    git_index(dir, index, &["add", "-A", "--", "."])?;
    git_index(dir, index, &["write-tree"]).map(|t| t.trim().to_string())
}

/// Tree id of the whole working tree (tracked and untracked, minus ignored files).
fn snapshot_tree(dir: &str) -> Result<String, String> {
    let (index, _) = index_copy(dir)?;
    tree_from(dir, &index.0)
}

/// The patch of everything that changed since a checkpoint, files made since included,
/// cut to about `limit` bytes.
pub fn patch_since(dir: &str, checkpoint: &str, limit: usize) -> String {
    let Ok(tree) = snapshot_tree(dir) else {
        return String::new();
    };
    let mut text = git(dir, &["diff", "--stat", checkpoint, &tree]).unwrap_or_default();
    text.push('\n');
    text.push_str(&git(dir, &["diff", checkpoint, &tree]).unwrap_or_default());
    if text.len() > limit {
        let mut cut = limit;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n[diff truncated]");
    }
    text
}

/// Where a project folder stands: its branch and how it compares with the remote's copy.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RepoState {
    /// The checked-out branch, empty when HEAD is detached.
    pub branch: String,
    /// The remote branch it tracks, such as `origin/main`; empty when there is none.
    pub upstream: String,
    /// The remote to push to, empty when the repository has none.
    pub remote: String,
    /// The remote as a person would call it: "GitHub" for one there, else its name.
    pub host: String,
    /// Commits only here, and commits only on the remote branch.
    pub ahead: usize,
    pub behind: usize,
    /// Local branches, the checked-out one included, most recently committed first.
    pub branches: Vec<String>,
}

pub fn repo_state(dir: &str) -> RepoState {
    let branch = match current_branch(dir) {
        b if b == "HEAD" => String::new(),
        b => b,
    };
    let upstream = git(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_default();
    let remote = remote_name(dir);
    let host = match git(dir, &["remote", "get-url", &remote]) {
        Ok(url) if url.contains("github.com") => "GitHub".to_string(),
        _ => remote.clone(),
    };
    let (behind, ahead) = if upstream.is_empty() {
        let ahead = if remote.is_empty() {
            0
        } else {
            count(dir, &["rev-list", "--count", "HEAD", "--not", "--remotes"]).unwrap_or(0)
        };
        (0, ahead)
    } else {
        git(dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])
            .ok()
            .and_then(|s| {
                let mut n = s.split_whitespace().filter_map(|n| n.parse().ok());
                Some((n.next()?, n.next()?))
            })
            .unwrap_or((0, 0))
    };
    let branches = git(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_string)
    .collect();
    RepoState {
        branch,
        upstream,
        remote,
        host,
        ahead,
        behind,
        branches,
    }
}

/// Open a pull request for `branch` onto `base` with the GitHub CLI, returning its link.
pub fn open_pull_request(
    dir: &str,
    branch: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<String, String> {
    let out = Command::new("gh")
        .args([
            "pr", "create", "--head", branch, "--base", base, "--title", title, "--body", body,
        ])
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("The GitHub CLI (gh) is needed for pull requests: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(text
        .lines()
        .rev()
        .find(|l| l.starts_with("https://"))
        .unwrap_or(text.trim())
        .to_string())
}

/// Every file git knows or would add (tracked plus untracked, minus ignored).
pub fn tracked_files(dir: &str) -> Vec<String> {
    git(
        dir,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )
    .map(|out| {
        let mut files = split_z(&out);
        // A file both staged and changed is listed once.
        files.dedup();
        files
    })
    .unwrap_or_default()
}

/// Snapshot the working tree as a commit that no branch points at, so a turn can be
/// undone (or an undo redone). Its first parent is HEAD at the time and its second a
/// commit of the index, so both come back too. Kept alive by a ref under `refs/squadcode/`.
pub fn checkpoint(dir: &str) -> Result<String, String> {
    // One copy of the index gives both the staging (before `add`) and the folder.
    let (copy, staged) = index_copy(dir)?;
    let tree = tree_from(dir, &copy.0)?;
    drop(copy);
    let head = rev(dir, "HEAD");
    let index = staged
        .map(|t| commit_tree(dir, &t, &[], "squadcode index"))
        .transpose()?;
    let mut parents: Vec<&str> = Vec::new();
    let message = match &head {
        Some(head) => {
            parents.push(head);
            "squadcode checkpoint"
        }
        None => "squadcode checkpoint\n\nno-head",
    };
    if let Some(index) = &index {
        parents.push(index);
    }
    let commit = commit_tree(dir, &tree, &parents, message)?;
    let _ = git(
        dir,
        &[
            "update-ref",
            &format!("refs/squadcode/checkpoints/{commit}"),
            &commit,
        ],
    );
    Ok(commit)
}

/// Put the working tree back as it was at `checkpoint`: changed files are restored with
/// their modes and symlinks, files created since are deleted, and the index entries of
/// those files are restored too. Ignored files are left alone. When the branch gained
/// or lost commits since, it is moved back to the checkpoint's HEAD.
///
/// With `guard` (a later snapshot, such as the end of the last turn) only the files
/// that differ between the checkpoint and the guard are touched, and of those only the
/// ones nobody changed after it; the rest are reported as skipped.
pub fn restore(dir: &str, checkpoint: &str, guard: Option<&str>) -> Result<Restored, String> {
    let now = snapshot_tree(dir)?;
    let mut done = Restored::default();
    let paths = match guard {
        Some(guard) => {
            let later = diff_names(dir, guard, &now)?;
            let mut paths = Vec::new();
            for path in diff_names(dir, checkpoint, guard)? {
                if later.contains(&path) {
                    done.skipped.push(path);
                } else {
                    paths.push(path);
                }
            }
            paths
        }
        None => diff_names(dir, checkpoint, &now)?,
    };
    let (head, index) = checkpoint_parts(dir, checkpoint);
    // Move the branch only along its own history, never onto another line of work.
    if let (Some(target), Some(current)) = (head.as_deref(), rev(dir, "HEAD")) {
        if target != current
            && (is_ancestor(dir, target, &current) || is_ancestor(dir, &current, target))
        {
            git(dir, &["reset", "-q", "--soft", target])?;
            done.moved_head = true;
        }
    }
    if paths.is_empty() {
        return Ok(done);
    }
    let in_checkpoint = |source: &str| -> Result<Vec<(String, String)>, String> {
        // `mode type sha<TAB>path` for every file in the tree.
        let listing = git(dir, &["ls-tree", "-r", "-z", source])?;
        Ok(listing
            .split('\0')
            .filter_map(|e| e.split_once('\t'))
            .map(|(meta, path)| (path.to_string(), meta.to_string()))
            .collect())
    };
    let wanted: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();
    let saved: std::collections::HashMap<String, String> = in_checkpoint(checkpoint)?
        .into_iter()
        .filter(|(p, _)| wanted.contains(p.as_str()))
        .collect();
    // Files the checkpoint has come back through a private index, so git writes the
    // executable bit and symlinks exactly as they were.
    let restore_index = temp_index(dir);
    let back: Vec<&String> = paths.iter().filter(|p| saved.contains_key(*p)).collect();
    if !back.is_empty() {
        git_index(dir, &restore_index, &["read-tree", checkpoint])?;
        let mut list = Vec::new();
        for path in &back {
            let target = Path::new(dir).join(path);
            // A folder or link where a file was would block the checkout.
            if target.is_dir() && !target.is_symlink() {
                let _ = std::fs::remove_dir_all(&target);
            } else {
                let _ = std::fs::remove_file(&target);
            }
            list.extend_from_slice(path.as_bytes());
            list.push(0);
        }
        let index = restore_index.to_string_lossy().into_owned();
        let result = git_with(
            dir,
            &["checkout-index", "-f", "-z", "--stdin"],
            &[("GIT_INDEX_FILE", index.as_str())],
            Some(&list),
        );
        let _ = std::fs::remove_file(&restore_index);
        result?;
    }
    for path in paths.iter().filter(|p| !saved.contains_key(*p)) {
        let target = Path::new(dir).join(path);
        let _ = std::fs::remove_file(&target);
        // Drop folders the removed files leave empty.
        let mut parent = target.parent();
        while let Some(p) = parent.filter(|p| *p != Path::new(dir)) {
            if std::fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
    }
    // The index entries of these files go back to the checkpoint's index (or to HEAD
    // for older checkpoints); every other file keeps the user's staging.
    let source = index.or(head);
    let staged: std::collections::HashMap<String, String> = match &source {
        Some(source) => in_checkpoint(source)?
            .into_iter()
            .filter(|(p, _)| wanted.contains(p.as_str()))
            .collect(),
        None => Default::default(),
    };
    let mut info = Vec::new();
    for path in &paths {
        let entry = match staged.get(path) {
            // `mode blob sha` becomes `mode sha`.
            Some(meta) => {
                let parts: Vec<&str> = meta.split_whitespace().collect();
                format!("{} {}\t{path}", parts[0], parts.get(2).unwrap_or(&""))
            }
            None => format!("0 {}\t{path}", "0".repeat(40)),
        };
        info.extend_from_slice(entry.as_bytes());
        info.push(0);
    }
    git_with(
        dir,
        &["update-index", "-z", "--index-info"],
        &[],
        Some(&info),
    )?;
    done.files = paths;
    Ok(done)
}

/// What a restore did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Restored {
    /// Files put back.
    pub files: Vec<String>,
    /// Files left alone because they changed after `guard`.
    pub skipped: Vec<String>,
    /// The branch was moved back (or forward) to where it was.
    pub moved_head: bool,
}

fn commit_tree(dir: &str, tree: &str, parents: &[&str], message: &str) -> Result<String, String> {
    let mut args = vec!["commit-tree", tree, "-m", message];
    for p in parents {
        args.extend(["-p", p]);
    }
    let who = [
        ("GIT_AUTHOR_NAME", "Squad Code"),
        ("GIT_AUTHOR_EMAIL", "squadcode@localhost"),
        ("GIT_COMMITTER_NAME", "Squad Code"),
        ("GIT_COMMITTER_EMAIL", "squadcode@localhost"),
    ];
    git_with(dir, &args, &who, None).map(|s| s.trim().to_string())
}

/// Paths that differ between two trees or commits.
fn diff_names(dir: &str, a: &str, b: &str) -> Result<Vec<String>, String> {
    git(dir, &["diff", "--name-only", "--no-renames", "-z", a, b]).map(|s| split_z(&s))
}

/// HEAD and index commit recorded in a checkpoint. Older checkpoints have no index.
fn checkpoint_parts(dir: &str, checkpoint: &str) -> (Option<String>, Option<String>) {
    let parents: Vec<String> = git(dir, &["rev-list", "--parents", "-n", "1", checkpoint])
        .unwrap_or_default()
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect();
    let headless = git(dir, &["log", "-1", "--format=%B", checkpoint])
        .unwrap_or_default()
        .contains("no-head");
    if headless {
        (None, parents.first().cloned())
    } else {
        (parents.first().cloned(), parents.get(1).cloned())
    }
}

fn temp_index(dir: &str) -> std::path::PathBuf {
    let git_dir = git(dir, &["rev-parse", "--absolute-git-dir"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    temp_index_in(&git_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn repo(name: &str) -> std::path::PathBuf {
        let repo =
            std::env::temp_dir().join(format!("squadcode-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        sh(&repo, &["init", "-q"]);
        sh(&repo, &["config", "user.email", "t@t"]);
        sh(&repo, &["config", "user.name", "t"]);
        // Git reports real paths, and on a Mac the temporary folder is a symlink.
        repo.canonicalize().unwrap()
    }

    /// A repo on `main` with `f` and `g`, and a branch `feat` that changes `f`.
    fn merge_setup(name: &str) -> (String, std::path::PathBuf) {
        let repo = repo(name);
        sh(&repo, &["checkout", "-q", "-b", "main"]);
        std::fs::write(repo.join("f"), "a\nb\nc\n").unwrap();
        std::fs::write(repo.join("g"), "1\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-qm", "init"]);
        sh(&repo, &["checkout", "-q", "-b", "feat"]);
        std::fs::write(repo.join("f"), "a\nB\nc\n").unwrap();
        sh(&repo, &["commit", "-qam", "feat"]);
        sh(&repo, &["checkout", "-q", "main"]);
        (repo.to_string_lossy().into_owned(), repo)
    }

    #[test]
    fn changes_to_other_files_dont_stop_a_merge() {
        let (r, repo) = merge_setup("merge-other");
        std::fs::write(repo.join("g"), "2\n").unwrap();
        assert_eq!(in_the_way(&r, "feat", "main"), None);
        let merged = merge_into(&r, "feat", "main", false).unwrap();
        assert_eq!(merged.kept, None);
        assert_eq!(
            std::fs::read_to_string(repo.join("f")).unwrap(),
            "a\nB\nc\n"
        );
        assert_eq!(std::fs::read_to_string(repo.join("g")).unwrap(), "2\n");
    }

    #[test]
    fn changes_the_merge_would_overwrite_are_named() {
        let (r, repo) = merge_setup("merge-named");
        std::fs::write(repo.join("f"), "a\nb\nc\nd\n").unwrap();
        match merge_into(&r, "feat", "main", false) {
            Err(MergeError::InTheWay { files, .. }) => assert_eq!(files, vec!["f".to_string()]),
            other => panic!("expected in the way, got {other:?}"),
        }
        // Nothing moved.
        assert_eq!(
            std::fs::read_to_string(repo.join("f")).unwrap(),
            "a\nb\nc\nd\n"
        );
    }

    #[test]
    fn set_aside_changes_come_back_after_the_merge() {
        let (r, repo) = merge_setup("merge-aside");
        std::fs::write(repo.join("f"), "a\nb\nc\nd\n").unwrap();
        let merged = merge_into(&r, "feat", "main", true).unwrap();
        assert_eq!(merged.kept, None);
        assert_eq!(
            std::fs::read_to_string(repo.join("f")).unwrap(),
            "a\nB\nc\nd\n"
        );
        assert!(git(&r, &["stash", "list"]).unwrap().trim().is_empty());
    }

    #[test]
    fn set_aside_changes_that_clash_stay_in_the_stash() {
        let (r, repo) = merge_setup("merge-kept");
        std::fs::write(repo.join("f"), "a\nX\nc\n").unwrap();
        let merged = merge_into(&r, "feat", "main", true).unwrap();
        assert_eq!(merged.set_aside, vec!["f".to_string()]);
        assert_eq!(merged.kept, rev(&r, "stash@{0}"));
        // The merged file, with no clash markers, and the change kept.
        assert_eq!(
            std::fs::read_to_string(repo.join("f")).unwrap(),
            "a\nB\nc\n"
        );
        assert!(conflicted(&r).is_empty());
        assert!(git(&r, &["stash", "show", "-p"]).unwrap().contains("+X"));

        // Brought back, the clash is marked in the file and the stash is empty.
        let clash = bring_back(&r, merged.kept.as_deref().unwrap()).unwrap();
        assert_eq!(clash, vec!["f".to_string()]);
        let f = std::fs::read_to_string(repo.join("f")).unwrap();
        assert!(
            f.contains("<<<<<<<") && f.contains("X") && f.contains("B"),
            "{f}"
        );
        assert!(git(&r, &["stash", "list"]).unwrap().trim().is_empty());
        assert!(conflicted(&r).is_empty());
        assert!(bring_back(&r, merged.kept.as_deref().unwrap()).is_err());
    }

    /// `merge_setup`, with `main` moved on so the merge makes a commit.
    fn merge_setup_diverged(name: &str) -> (String, std::path::PathBuf) {
        let (r, repo) = merge_setup(name);
        std::fs::write(repo.join("h"), "x\n").unwrap();
        sh(&repo, &["add", "h"]);
        sh(&repo, &["commit", "-qm", "main moved on"]);
        (r, repo)
    }

    #[test]
    fn a_staged_change_to_another_file_is_in_the_way_of_a_merge_commit() {
        let (r, repo) = merge_setup_diverged("merge-staged");
        std::fs::write(repo.join("g"), "2\n").unwrap();
        sh(&repo, &["add", "g"]);
        // Git won't make a merge commit over it, so it's named, not a raw error.
        match merge_into(&r, "feat", "main", false) {
            Err(MergeError::InTheWay { files, .. }) => assert_eq!(files, vec!["g".to_string()]),
            other => panic!("expected in the way, got {other:?}"),
        }
        // Set aside, it merges and comes back staged.
        let merged = merge_into(&r, "feat", "main", true).unwrap();
        assert_eq!(merged.kept, None);
        assert_eq!(
            std::fs::read_to_string(repo.join("f")).unwrap(),
            "a\nB\nc\n"
        );
        assert_eq!(std::fs::read_to_string(repo.join("g")).unwrap(), "2\n");
        assert_eq!(staged_files(&r), vec!["g".to_string()]);
        assert!(git(&r, &["stash", "list"]).unwrap().trim().is_empty());
    }

    #[test]
    fn a_staged_change_doesnt_stop_a_fast_forward() {
        let (r, repo) = merge_setup("merge-staged-ff");
        std::fs::write(repo.join("g"), "2\n").unwrap();
        sh(&repo, &["add", "g"]);
        assert_eq!(in_the_way(&r, "feat", "main"), None);
        merge_into(&r, "feat", "main", false).unwrap();
        assert_eq!(staged_files(&r), vec!["g".to_string()]);
    }

    #[test]
    fn unstaged_changes_to_other_files_dont_stop_a_merge_commit() {
        let (r, repo) = merge_setup_diverged("merge-other-diverged");
        std::fs::write(repo.join("g"), "2\n").unwrap();
        assert_eq!(in_the_way(&r, "feat", "main"), None);
        merge_into(&r, "feat", "main", false).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("g")).unwrap(), "2\n");
    }

    #[test]
    fn worktree_paths_sit_beside_the_repository() {
        let p = worktree_path("/code/app", "feature/nav");
        assert_eq!(p, Path::new("/code/.squadcode-worktrees/app/feature/nav"));
        // `a/b` and `a-b` are different branches, so they get different folders.
        assert_ne!(
            worktree_path("/code/app", "a/b"),
            worktree_path("/code/app", "a-b")
        );
        assert_eq!(
            worktree_path("/code/app", "../x"),
            Path::new("/code/.squadcode-worktrees/app/x")
        );
    }

    #[test]
    fn worktree_on_an_existing_branch() {
        let repo = repo("existing");
        std::fs::write(repo.join("a"), "a\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-qm", "init"]);
        sh(&repo, &["branch", "topic"]);
        let r = repo.to_str().unwrap();
        let dir = add_worktree(r, "topic", None).unwrap();
        assert_eq!(current_branch(&dir), "topic");
        assert!(dir.contains(".squadcode-worktrees"));
        remove_worktree(r, &dir).unwrap();
        let _ = std::fs::remove_dir_all(
            repo.parent()
                .unwrap()
                .join(".squadcode-worktrees")
                .join(repo.file_name().unwrap()),
        );
        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn worktree_changes_and_diff() {
        let root = std::env::temp_dir().join(format!("squadcode-test-{}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let repo = repo.canonicalize().unwrap();
        sh(&repo, &["init", "-q"]);
        sh(&repo, &["config", "user.email", "t@t"]);
        sh(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-qm", "init"]);

        let repo_str = repo.to_str().unwrap();
        assert!(is_repo(repo_str));
        let dir = add_worktree(repo_str, "squad/test", None).unwrap();
        assert!(add_worktree(repo_str, "squad/test", None).is_err());
        assert_eq!(current_branch(&dir), "squad/test");

        std::fs::write(Path::new(&dir).join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(Path::new(&dir).join("new.txt"), "x\ny\n").unwrap();
        let changes = changes(&dir);
        let a = changes.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!((a.added, a.removed), (2, 1));
        let n = changes.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!((n.status.as_str(), n.added), ("A", 2));
        assert!(file_diff(&dir, "a.txt").contains("+TWO"));
        assert!(file_diff(&dir, "new.txt").contains("+x"));

        remove_worktree(repo_str, &dir).unwrap();
        assert!(!Path::new(&dir).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_goes_back_to_the_last_commit() {
        let top = repo("under");
        // The project can sit below the repository's top.
        let project = top.join("app");
        std::fs::create_dir_all(project.join("design/screens")).unwrap();
        let dir = project.to_str().unwrap();
        std::fs::write(project.join("design/canvas.toml"), "one\n").unwrap();
        std::fs::write(project.join("design/.gitignore"), "local.txt\n").unwrap();
        std::fs::write(project.join("design/screens/old.html"), "<p>old").unwrap();
        std::fs::write(project.join("code.rs"), "fn a() {}\n").unwrap();
        sh(&top, &["add", "-A"]);
        sh(&top, &["commit", "-q", "-m", "init"]);
        assert!(changed_under(dir, "design").is_empty());
        assert_eq!(
            committed_text(dir, "design/canvas.toml").as_deref(),
            Some("one\n")
        );
        assert!(committed_text(dir, "design/nope.toml").is_none());

        // Edited, staged, new and deleted files; ignored ones and other folders stay.
        std::fs::remove_file(project.join("design/screens/old.html")).unwrap();
        std::fs::write(project.join("design/canvas.toml"), "two\n").unwrap();
        std::fs::write(project.join("design/screens/home.html"), "<p>").unwrap();
        std::fs::write(project.join("design/screens/staged.html"), "<p>").unwrap();
        sh(&project, &["add", "design/screens/staged.html"]);
        std::fs::write(project.join("design/local.txt"), "mine").unwrap();
        std::fs::write(project.join("code.rs"), "fn b() {}\n").unwrap();
        assert_eq!(
            changed_under(dir, "design"),
            [
                "design/canvas.toml",
                "design/screens/home.html",
                "design/screens/old.html",
                "design/screens/staged.html"
            ]
        );
        discard_under(dir, "design").unwrap();
        assert!(changed_under(dir, "design").is_empty());
        assert_eq!(
            std::fs::read_to_string(project.join("design/canvas.toml")).unwrap(),
            "one\n"
        );
        assert!(project.join("design/screens/old.html").exists());
        assert!(!project.join("design/screens/home.html").exists());
        assert!(!project.join("design/screens/staged.html").exists());
        assert!(project.join("design/local.txt").exists());
        assert_eq!(
            std::fs::read_to_string(project.join("code.rs")).unwrap(),
            "fn b() {}\n"
        );

        // A folder never committed goes away entirely.
        std::fs::create_dir_all(project.join("architecture/views")).unwrap();
        std::fs::write(project.join("architecture/views/a.toml"), "x").unwrap();
        assert_eq!(changed_under(dir, "architecture").len(), 1);
        discard_under(dir, "architecture").unwrap();
        assert!(!project.join("architecture").exists());
        // So does one in a repository with no commit yet, staged or not.
        let fresh = repo("under-fresh");
        let fresh_dir = fresh.to_str().unwrap();
        std::fs::create_dir_all(fresh.join("design")).unwrap();
        std::fs::write(fresh.join("design/a.html"), "a").unwrap();
        std::fs::write(fresh.join("design/b.html"), "b").unwrap();
        sh(&fresh, &["add", "design/a.html"]);
        assert_eq!(changed_under(fresh_dir, "design").len(), 2);
        discard_under(fresh_dir, "design").unwrap();
        assert!(changed_under(fresh_dir, "design").is_empty());
        assert!(!fresh.join("design").exists());
        let _ = std::fs::remove_dir_all(&top);
        let _ = std::fs::remove_dir_all(&fresh);
    }

    #[test]
    fn worktreeinclude_copies_matching_ignored_files() {
        let repo = repo("include");
        let dir = repo.to_str().unwrap();
        std::fs::write(repo.join(".gitignore"), ".env\n*.log\nsecrets/\n").unwrap();
        std::fs::write(repo.join(".worktreeinclude"), ".env\nsecrets/\n").unwrap();
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-q", "-m", "init"]);
        std::fs::write(repo.join(".env"), "KEY=1").unwrap();
        std::fs::write(repo.join("debug.log"), "noise").unwrap();
        std::fs::create_dir_all(repo.join("secrets")).unwrap();
        std::fs::write(repo.join("secrets/key.pem"), "pem").unwrap();
        // Untracked but not ignored: the user's own work, never copied.
        std::fs::write(repo.join("draft.txt"), "wip").unwrap();

        let wt = add_worktree(dir, "include-test", None).unwrap();
        let mut copied = copy_included(dir, &wt).unwrap();
        copied.sort();
        assert_eq!(copied, [".env", "secrets/key.pem"]);
        assert_eq!(
            std::fs::read_to_string(Path::new(&wt).join(".env")).unwrap(),
            "KEY=1"
        );
        assert!(!Path::new(&wt).join("debug.log").exists());
        assert!(!Path::new(&wt).join("draft.txt").exists());
        // Running again leaves existing files alone.
        assert!(copy_included(dir, &wt).unwrap().is_empty());
        remove_worktree(dir, &wt).unwrap();
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn branch_names_come_from_the_task() {
        assert_eq!(
            branch_slug("Fix the login redirect\nmore detail"),
            "squad/fix-login-redirect"
        );
        assert_eq!(
            branch_slug("\n  Please add a dark-mode toggle to the Settings page"),
            "squad/add-dark-mode-toggle-settings"
        );
        assert_eq!(branch_slug("修复 bug"), "squad/bug");
        assert_eq!(branch_slug("!!! ???"), "");
        assert!(branch_slug(&"x".repeat(80)).len() <= "squad/".len() + 40);
        let taken = ["squad/a", "squad/a-2"];
        assert_eq!(
            unique_branch("squad/a", |b| taken.contains(&b)),
            "squad/a-3"
        );
        assert_eq!(unique_branch("squad/b", |b| taken.contains(&b)), "squad/b");
    }

    #[test]
    fn credential_errors_point_at_the_fix() {
        assert!(auth_hint(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
        )
        .contains("gh auth setup-git"));
        assert_eq!(auth_hint("fatal: other"), "fatal: other");
    }
}
