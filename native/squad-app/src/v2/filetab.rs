//! The Files tab of the right sidebar: the folder in view as a tree, with the
//! files that changed marked, and a box to find a file by name.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::ui::{self, c};
use super::Shell;
use crate::files::TreeRow;

/// Files listed at most, so a huge folder stays quick.
const MAX_FILES: usize = 4000;
/// Rows shown at most, in the tree or as matches.
const MAX_ROWS: usize = 400;
/// How long a listing is trusted before the folder is looked at again.
const FRESH: Duration = Duration::from_secs(12);

/// Folders never worth walking into outside git: they are generated or huge.
const SKIP: [&str; 8] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
];

/// Every file under `dir`, as paths from its top: what git knows or would add
/// in a repository, else a walk that skips hidden and generated folders.
pub(super) fn list_files(dir: &str) -> Vec<String> {
    if crate::git::is_repo(dir) {
        let mut files = crate::git::tracked_files(dir);
        files.truncate(MAX_FILES);
        return files;
    }
    let root = std::path::Path::new(dir);
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                    stack.push(path);
                }
            } else if kind.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= MAX_FILES {
                    out.sort();
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// The colour of git's status letter for a changed file.
fn mark_color(code: &str) -> u32 {
    match code {
        "A" | "U" | "?" => ui::add(),
        "D" => ui::del(),
        _ => ui::t_amber(),
    }
}

impl Shell {
    /// The files in `dir`, listed again when they're stale or the folder changed.
    fn files_in(&mut self, dir: &str) -> Arc<Vec<String>> {
        if let Some((d, at, files)) = &self.files_seen {
            if d == dir && at.elapsed() < FRESH {
                return files.clone();
            }
        }
        let files = Arc::new(list_files(dir));
        self.files_seen = Some((dir.to_string(), Instant::now(), files.clone()));
        files
    }

    /// The Files tab for `dir`. `marks` are changed files, by path, with git's
    /// one-letter status; `title` says whose files these are.
    pub(super) fn files_side(
        &mut self,
        dir: &str,
        title: String,
        marks: HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let files = if dir.is_empty() {
            Arc::new(Vec::new())
        } else {
            self.files_in(dir)
        };
        let query = self.files_input.read(cx).value().trim().to_string();

        let find = div()
            .h(px(32.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(c(ui::well()))
            .border_1()
            .border_color(c(ui::line2()))
            .child(
                Icon::new(IconName::Search)
                    .size(px(13.))
                    .text_color(c(ui::faint())),
            )
            .child(
                div().flex_1().min_w_0().text_size(px(12.5)).child(
                    Input::new(&self.files_input)
                        .appearance(false)
                        .bordered(false),
                ),
            );
        let changed = marks.len();
        let about = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(11.5))
            .text_color(c(ui::faint()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(ui::MONO)
                    .child(title),
            )
            .child(match files.len() {
                1 => "1 file".to_string(),
                n if n >= MAX_FILES => format!("{MAX_FILES}+ files"),
                n => format!("{n} files"),
            })
            .when(changed > 0, |d| {
                d.child("\u{b7}").child(
                    div()
                        .text_color(c(ui::t_amber()))
                        .child(format!("{changed} changed")),
                )
            });

        let mut list = div()
            .id("files-side")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(24. + super::DOCK_ROOM))
            .flex()
            .flex_col();
        let quiet = |t: &str| {
            div()
                .px(px(12.))
                .py(px(16.))
                .text_size(px(12.5))
                .text_color(c(ui::faint()))
                .child(t.to_string())
        };
        if files.is_empty() {
            list = list.child(quiet(if dir.is_empty() {
                "Open a project folder to see its files."
            } else {
                "No files here yet."
            }));
        } else if !query.is_empty() {
            let hits = crate::files::filter_files(&files, &query);
            if hits.is_empty() {
                list = list.child(quiet("No file matches that."));
            }
            for (n, path) in hits.into_iter().take(MAX_ROWS).enumerate() {
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                let folder = path
                    .strip_suffix(name.as_str())
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_string();
                let mark = marks.get(path.as_str()).cloned();
                list = list.child(self.file_row(
                    ("file-hit", n),
                    0,
                    name,
                    Some(folder),
                    mark,
                    path.clone(),
                    dir,
                    cx,
                ));
            }
        } else {
            // Folders holding a changed file show a dot, so a change is easy
            // to find while they're closed.
            let mut hot: std::collections::HashSet<String> = Default::default();
            for p in marks.keys() {
                let mut at = p.as_str();
                while let Some((up, _)) = at.rsplit_once('/') {
                    hot.insert(up.to_string());
                    at = up;
                }
            }
            let rows = crate::files::tree_rows(&files, &self.files_open);
            let more = rows.len().saturating_sub(MAX_ROWS);
            for (n, row) in rows.into_iter().take(MAX_ROWS).enumerate() {
                match row {
                    TreeRow::Folder {
                        path,
                        name,
                        depth,
                        open,
                    } => {
                        let warm = !open && hot.contains(&path);
                        list = list.child(
                            div()
                                .id(("file-folder", n))
                                .h(px(28.))
                                .pl(px(8. + depth as f32 * 14.))
                                .pr(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .map(|d| super::motion::hover_fill(d, None, ui::hover()))
                                .child(
                                    Icon::new(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(px(12.))
                                    .text_color(c(ui::faint())),
                                )
                                .child(
                                    Icon::new(if open {
                                        IconName::FolderOpen
                                    } else {
                                        IconName::Folder
                                    })
                                    .size(px(14.))
                                    .text_color(c(ui::muted())),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .text_color(c(ui::ink()))
                                        .child(name),
                                )
                                .when(warm, |d| {
                                    d.child(div().size(px(6.)).rounded_full().bg(c(ui::t_amber())))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.files_open.remove(&path) {
                                        this.files_open.insert(path.clone());
                                    }
                                    cx.notify();
                                })),
                        );
                    }
                    TreeRow::File { path, name, depth } => {
                        let mark = marks.get(&path).cloned();
                        list = list.child(self.file_row(
                            ("file-row", n),
                            depth,
                            name,
                            None,
                            mark,
                            path,
                            dir,
                            cx,
                        ));
                    }
                }
            }
            if more > 0 {
                list = list.child(quiet(&format!(
                    "and {more} more. Open fewer folders, or find a file by name."
                )));
            }
        }

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(find)
                    .child(about),
            )
            .child(list)
    }

    /// One file in the Files tab: its icon, name, the folder it's in when
    /// listed flat, and git's letter when it changed. A click opens it.
    #[allow(clippy::too_many_arguments)]
    fn file_row(
        &mut self,
        id: (&'static str, usize),
        depth: usize,
        name: String,
        folder: Option<String>,
        mark: Option<String>,
        path: String,
        dir: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let dir = dir.to_string();
        let act = super::rich::Act::File(path);
        let changed = mark.is_some();
        div()
            .id(id)
            .min_h(px(28.))
            .py(px(4.))
            .pl(px(8.
                + depth as f32 * 14.
                + if folder.is_some() { 0. } else { 18. }))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .map(|d| super::motion::hover_fill(d, None, ui::hover()))
            .child(
                Icon::new(IconName::FileText)
                    .size(px(13.))
                    .flex_none()
                    .text_color(c(ui::faint())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .when(changed, |d| d.font_weight(FontWeight::MEDIUM))
                            .text_color(c(if changed { ui::ink() } else { ui::body() }))
                            .child(name),
                    )
                    .when_some(folder.filter(|f| !f.is_empty()), |d, f| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(c(ui::faint()))
                                .child(f),
                        )
                    }),
            )
            .when_some(mark, |d, m| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(ui::MONO)
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(mark_color(&m)))
                        .child(m),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.act(&act, &dir, window, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::list_files;

    #[test]
    fn a_plain_folder_is_walked_without_hidden_or_generated_folders() {
        let base = std::env::temp_dir().join(format!("runo-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for p in [
            "src/main.rs",
            "README.md",
            "node_modules/x/i.js",
            ".hidden/a",
        ] {
            let f = base.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, "x").unwrap();
        }
        let files = list_files(&base.to_string_lossy());
        assert_eq!(
            files,
            vec!["README.md".to_string(), "src/main.rs".to_string()]
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// Lines shown at most in a job's file preview.
const MAX_VIEW: usize = 2000;

/// A file as a job's Files tab shows it.
pub(super) struct FileView {
    /// The job's folder and the file's path in it.
    key: (String, String),
    lines: Arc<Vec<String>>,
    /// Lines (from 1) the job added or changed.
    changed: Arc<std::collections::HashSet<usize>>,
    /// Lines in the file, past the ones shown.
    total: usize,
    read_ok: bool,
}

/// The lines a diff adds, numbered as in the new file.
pub(super) fn added_lines(diff: &str) -> std::collections::HashSet<usize> {
    let mut out = std::collections::HashSet::new();
    let mut at = 0usize;
    let mut started = false;
    for l in diff.lines() {
        if l.starts_with("@@") {
            started = true;
            at = l
                .split_whitespace()
                .find(|w| w.starts_with('+'))
                .and_then(|w| w[1..].split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            continue;
        }
        if !started || l.starts_with('-') {
            continue;
        }
        if l.starts_with('+') {
            out.insert(at);
        }
        at += 1;
    }
    out
}

/// The language a file is written in, by its name.
pub(super) fn language(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default().to_lowercase();
    match ext.as_str() {
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "rs" => "Rust",
        "py" => "Python",
        "go" => "Go",
        "rb" => "Ruby",
        "swift" => "Swift",
        "kt" => "Kotlin",
        "java" => "Java",
        "md" => "Markdown",
        "json" => "JSON",
        "toml" => "TOML",
        "yml" | "yaml" => "YAML",
        "css" | "scss" => "CSS",
        "html" => "HTML",
        "sh" => "Shell",
        _ => "Text",
    }
}

const KEYWORDS: [&str; 34] = [
    "import",
    "from",
    "export",
    "default",
    "async",
    "await",
    "function",
    "const",
    "let",
    "var",
    "return",
    "if",
    "else",
    "for",
    "while",
    "class",
    "new",
    "type",
    "interface",
    "fn",
    "pub",
    "use",
    "impl",
    "struct",
    "enum",
    "match",
    "mut",
    "def",
    "self",
    "true",
    "false",
    "null",
    "None",
    "in",
];

/// Colour runs for one line of code: keywords, strings, numbers and comments.
pub(super) fn code_runs(line: &str) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let paint = |col: u32| HighlightStyle {
        color: Some(c(col)),
        ..Default::default()
    };
    let mut runs = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let ch = b[i];
        if (ch == b'/' && b.get(i + 1) == Some(&b'/'))
            || (ch == b'#' && line[..i].trim().is_empty())
        {
            runs.push((i..b.len(), paint(ui::faint())));
            break;
        }
        if ch == b'\'' || ch == b'"' || ch == b'`' {
            let end = line[i + 1..]
                .find(ch as char)
                .map_or(b.len(), |e| i + 2 + e);
            runs.push((i..end, paint(ui::t_green())));
            i = end;
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if KEYWORDS.contains(&&line[start..i]) {
                runs.push((start..i, paint(ui::t_violet())));
            }
            continue;
        }
        if ch.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            runs.push((start..i, paint(ui::t_amber())));
            continue;
        }
        i += 1;
    }
    runs
}

impl Shell {
    /// The picked file, read once per pick.
    fn job_file(&mut self, job: &super::store::Job, path: &str) -> &FileView {
        let key = (job.dir().to_string(), path.to_string());
        if self.job_view.as_ref().is_none_or(|v| v.key != key) {
            let text = std::fs::read_to_string(std::path::Path::new(job.dir()).join(path));
            let read_ok = text.is_ok();
            let text = text.unwrap_or_default();
            let all: Vec<String> = text.lines().map(str::to_string).collect();
            let total = all.len();
            let lines = all.into_iter().take(MAX_VIEW).collect();
            let changed = if job.changes.iter().any(|ch| ch.path == path) {
                added_lines(&super::work::diff(job, path))
            } else {
                Default::default()
            };
            self.job_view = Some(FileView {
                key,
                lines: Arc::new(lines),
                changed: Arc::new(changed),
                total,
                read_ok,
            });
        }
        self.job_view.as_ref().expect("just set")
    }

    /// A job's Files tab: find a file, the tree beside the picked file with
    /// the lines the job changed marked, and a way to open it over the page.
    pub(super) fn job_files(&mut self, job: &super::store::Job, cx: &mut Context<Self>) -> Div {
        let dir = job.dir().to_string();
        let marks: HashMap<String, String> = job
            .changes
            .iter()
            .map(|ch| (ch.path.clone(), ch.status.chars().take(1).collect()))
            .collect();
        let only = self.files_changed_only;
        let mut files: Vec<String> = if only || dir.is_empty() {
            Vec::new()
        } else {
            self.files_in(&dir).as_ref().clone()
        };
        // Changed files always show, even ones not on disk yet.
        for p in marks.keys() {
            if !files.contains(p) {
                files.push(p.clone());
            }
        }
        let query = self.files_input.read(cx).value().trim().to_string();
        if self.file_pick.as_ref().is_none_or(|p| !files.contains(p)) {
            // The job's first changed file, as it lists them.
            self.file_pick = job
                .changes
                .first()
                .map(|ch| ch.path.clone())
                .or_else(|| files.first().cloned());
        }
        let pick = self.file_pick.clone();

        let find = div()
            .flex()
            .gap(px(8.))
            .items_center()
            .child(
                div()
                    .flex_1()
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(5.))
                    .bg(c(ui::raised()))
                    .border_1()
                    .border_color(c(ui::line_hi()))
                    .child(
                        Icon::new(IconName::Search)
                            .size(px(13.))
                            .text_color(c(ui::faint())),
                    )
                    .child(
                        div().flex_1().min_w_0().text_size(px(12.5)).child(
                            Input::new(&self.files_input)
                                .appearance(false)
                                .bordered(false),
                        ),
                    ),
            )
            .child(
                div()
                    .id("files-changed-only")
                    .h(px(30.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(if only {
                        ui::accent_bar()
                    } else {
                        ui::line_hi()
                    }))
                    .bg(c(if only { ui::pick_fill() } else { ui::raised() }))
                    .font_family(ui::MONO)
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(if only { ui::pick_on() } else { ui::body() }))
                    .cursor_pointer()
                    .child("Changed only")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.files_changed_only = !this.files_changed_only;
                        cx.notify();
                    })),
            );

        let (m, a) = marks.values().fold((0, 0), |(m, a), k| match k.as_str() {
            "A" | "?" | "U" => (m, a + 1),
            _ => (m + 1, a),
        });
        let about = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .font_family(ui::MONO)
            .text_size(px(11.))
            .text_color(c(ui::faint()))
            .child(div().min_w_0().truncate().child(dir.clone()))
            .child(div().flex_1())
            .when(!job.branch.is_empty(), |d| {
                d.child(div().flex_none().child(job.branch.clone()))
            })
            .when(m + a > 0, |d| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(4.))
                        .when(m > 0, |d| {
                            d.child(div().text_color(c(ui::t_amber())).child(format!("{m} M")))
                        })
                        .when(m > 0 && a > 0, |d| d.child("\u{b7}"))
                        .when(a > 0, |d| {
                            d.child(div().text_color(c(ui::t_green())).child(format!("{a} A")))
                        }),
                )
            });

        // The tree, or what matches what was typed.
        let mut tree = div()
            .id("job-files-tree")
            .w(px(186.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p(px(5.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(6.))
            .bg(c(ui::raised()))
            .border_1()
            .border_color(c(ui::line()));
        let row = |id: (&'static str, usize),
                   depth: usize,
                   folder: Option<bool>,
                   name: String,
                   mark: Option<String>,
                   on: bool| {
            div()
                .id(id)
                .flex_none()
                .h(px(26.))
                .pl(px(8. + depth as f32 * 14.))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(7.))
                .rounded(px(4.))
                .relative()
                .overflow_hidden()
                .cursor_pointer()
                .font_family(ui::MONO)
                .text_size(px(12.))
                .text_color(c(if on { ui::ink() } else { ui::body() }))
                .when(on, |d| {
                    d.bg(c(ui::raised()))
                        .font_weight(FontWeight::MEDIUM)
                        .border_1()
                        .border_color(c(ui::line()))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(4.))
                                .bg(c(ui::pick_fill())),
                        )
                })
                .when(!on, |d| d.hover(|s| s.bg(c(ui::hover()))))
                .child(match folder {
                    Some(open) => Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(11.))
                    .text_color(c(ui::faint()))
                    .into_any_element(),
                    None => div().w(px(11.)).flex_none().into_any_element(),
                })
                .child(
                    Icon::new(if folder.is_some() {
                        IconName::Folder
                    } else {
                        IconName::FileText
                    })
                    .size(px(13.))
                    .flex_none()
                    .text_color(c(ui::body())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when_some(mark.clone().filter(|_| !on), |d, m| {
                            d.text_color(c(mark_color(&m)))
                        })
                        .child(name),
                )
                .when_some(mark, |d, m| {
                    d.child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(10.5))
                            .text_color(c(mark_color(&m)))
                            .child(m),
                    )
                })
        };
        if !query.is_empty() {
            let hits = crate::files::filter_files(&files, &query);
            if hits.is_empty() {
                tree = tree.child(
                    div()
                        .px(px(8.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(c(ui::faint()))
                        .child("No file matches that."),
                );
            }
            for (n, path) in hits.into_iter().take(MAX_ROWS).enumerate() {
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                let on = pick.as_deref() == Some(path.as_str());
                let p = path.clone();
                tree = tree.child(
                    row(
                        ("jf-hit", n),
                        0,
                        None,
                        name,
                        marks.get(path.as_str()).cloned(),
                        on,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.file_pick = Some(p.clone());
                        cx.notify();
                    })),
                );
            }
        } else {
            // Folders that hold a changed file start open.
            if !self.files_seeded.contains(&dir) {
                self.files_seeded.insert(dir.clone());
                for p in marks.keys() {
                    let mut at = p.as_str();
                    while let Some((up, _)) = at.rsplit_once('/') {
                        self.files_open.insert(up.to_string());
                        at = up;
                    }
                }
            }
            for (n, r) in crate::files::tree_rows(&files, &self.files_open)
                .into_iter()
                .take(MAX_ROWS)
                .enumerate()
            {
                tree = tree.child(match r {
                    TreeRow::Folder {
                        path,
                        name,
                        depth,
                        open,
                    } => row(("jf-dir", n), depth, Some(open), name, None, false).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if !this.files_open.remove(&path) {
                                this.files_open.insert(path.clone());
                            }
                            cx.notify();
                        }),
                    ),
                    TreeRow::File { path, name, depth } => {
                        let on = pick.as_deref() == Some(path.as_str());
                        let mark = marks.get(&path).cloned();
                        row(("jf-file", n), depth, None, name, mark, on).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.file_pick = Some(path.clone());
                                cx.notify();
                            },
                        ))
                    }
                });
            }
        }

        let view = pick.map(|path| {
            let (lines, changed, total, read_ok) = {
                let v = self.job_file(job, &path);
                (v.lines.clone(), v.changed.clone(), v.total, v.read_ok)
            };
            let (add, del) = job
                .changes
                .iter()
                .find(|ch| ch.path == path)
                .map_or((0, 0), |ch| (ch.added, ch.removed));
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let id = job.id;
            let act = super::rich::Act::File(path.clone());
            let open_dir = dir.clone();
            let diff_path = path.clone();
            let mut code = div()
                .id("job-file-code")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py(px(6.))
                .font_family(ui::MONO)
                .text_size(px(11.5))
                .line_height(px(19.))
                .flex()
                .flex_col();
            if !read_ok {
                code = code.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_color(c(ui::faint()))
                        .child(
                            "This file isn't on disk here. Changes shows what the job did to it.",
                        ),
                );
            }
            for (i, l) in lines.iter().enumerate() {
                let n = i + 1;
                let on = changed.contains(&n);
                code = code.child(
                    div()
                        .flex_none()
                        .flex()
                        .when(on, |d| d.bg(super::ui::ca(ui::t_green(), 0.07)))
                        .child(
                            div()
                                .w(px(3.))
                                .flex_none()
                                .when(on, |d| d.bg(c(ui::t_green()))),
                        )
                        .child(
                            div()
                                .w(px(34.))
                                .pr(px(10.))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .text_color(c(ui::faint()))
                                .child(n.to_string()),
                        )
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_color(c(ui::ink()))
                                .child(StyledText::new(l.clone()).with_highlights(code_runs(l))),
                        ),
                );
            }
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(6.))
                .bg(c(ui::raised()))
                .border_1()
                .border_color(c(ui::line()))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pl(px(12.))
                        .pr(px(10.))
                        .py(px(8.))
                        .bg(c(ui::hover()))
                        .border_b_1()
                        .border_color(c(ui::line()))
                        .child(
                            div()
                                .font_family(ui::MONO)
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .when(add + del > 0, |d| {
                            d.child(
                                div()
                                    .flex()
                                    .gap(px(5.))
                                    .font_family(ui::MONO)
                                    .text_size(px(11.))
                                    .child(
                                        div().text_color(c(ui::t_green())).child(format!("+{add}")),
                                    )
                                    .child(
                                        div()
                                            .text_color(c(ui::t_red()))
                                            .child(format!("\u{2212}{del}")),
                                    ),
                            )
                        })
                        .child(div().flex_1())
                        .when(add + del > 0, |d| {
                            d.child(
                                ui::dbtn("jf-changes", "Changes", ui::Btn::Plain)
                                    .h(px(24.))
                                    .px(px(8.))
                                    .text_size(px(12.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.diff_text = this
                                            .crew
                                            .store
                                            .job(id)
                                            .map(|j| super::work::diff(j, &diff_path))
                                            .unwrap_or_default();
                                        this.diff_open = Some((id, diff_path.clone()));
                                        this.ws_tab = super::peek::Tab::Changes;
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            ui::dbtn_i("jf-open", IconName::Maximize2, "Open", ui::Btn::Plain)
                                .h(px(24.))
                                .px(px(8.))
                                .text_size(px(12.))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.act(&act, &open_dir, window, cx)
                                })),
                        ),
                )
                .child(code)
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(6.))
                        .border_t_1()
                        .border_color(c(ui::line()))
                        .font_family(ui::MONO)
                        .text_size(px(10.5))
                        .text_color(c(ui::faint()))
                        .child(format!(
                            "{} \u{b7} {total} line{}",
                            language(&path),
                            if total == 1 { "" } else { "s" }
                        ))
                        .child(div().flex_1())
                        .when(!changed.is_empty(), |d| {
                            d.child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(div().text_color(c(ui::t_green())).child("\u{258d}"))
                                    .child("changed in this job"),
                            )
                        }),
                )
        });

        div()
            .size_full()
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(find)
            .child(about)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(10.))
                    .child(tree)
                    .children(view),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(c(ui::faint()))
                    .child("Open shows any file over the page: code, Markdown as a page, pictures and PDFs as they look."),
            )
    }
}

#[cfg(test)]
mod view_tests {
    use super::{added_lines, language};
    use std::prelude::v1::test;

    #[test]
    fn added_lines_count_from_the_new_file() {
        let diff = "--- a/x\n+++ b/x\n@@ -1,3 +1,4 @@\n a\n-b\n+B\n+C\n c\n";
        let got = added_lines(diff);
        assert_eq!(got, [2, 3].into_iter().collect());
    }

    #[test]
    fn languages_by_extension() {
        assert_eq!(language("src/pay.ts"), "TypeScript");
        assert_eq!(language("README"), "Text");
    }
}
