//! The file viewer: a file you click (in a message, a job's files, the Files
//! tab) opens here, over the page, instead of in another app. Code shows with
//! its colours and line numbers, Markdown reads as a page (or as its source),
//! pictures and PDFs show as they look. "Open with" hands it to another app
//! when you want one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::ui::{self, c};
use super::Shell;

/// Text files larger than this aren't shown: they'd be slow, and no one reads them here.
const MAX_TEXT: u64 = 4 * 1024 * 1024;
/// PDF pages drawn at most, and how wide, in pixels.
const PDF_PAGES: usize = 40;
const PDF_WIDTH: u32 = 1400;

/// What kind of file this is, by its name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Text,
    Markdown,
    Html,
    Image,
    Pdf,
    /// Something only another app can show: a video, a spreadsheet, an archive.
    Other,
}

const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "tif", "tiff", "avif",
];
const OTHER: &[&str] = &[
    "mp4", "mov", "m4v", "webm", "avi", "mkv", "mp3", "wav", "m4a", "aac", "flac", "ogg", "zip",
    "gz", "tgz", "bz2", "xz", "7z", "rar", "dmg", "pkg", "iso", "exe", "dll", "so", "dylib", "a",
    "o", "class", "jar", "wasm", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "pages",
    "numbers", "psd", "sketch", "fig", "ttf", "otf", "woff", "woff2", "heic", "sqlite", "db",
];

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub(super) fn kind_of(path: &Path) -> Kind {
    let ext = ext_of(path);
    match ext.as_str() {
        "md" | "markdown" | "mdx" | "mdown" => Kind::Markdown,
        "html" | "htm" | "xhtml" => Kind::Html,
        "pdf" => Kind::Pdf,
        e if IMAGES.contains(&e) => Kind::Image,
        e if OTHER.contains(&e) => Kind::Other,
        _ => Kind::Text,
    }
}

/// The editor's language for a file: the colours it's drawn with. Plain text
/// when there's no grammar for it.
pub(super) fn language_of(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "makefile" | "gnumakefile" => return "make",
        "dockerfile" | ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" => return "bash",
        "cargo.lock" | "pipfile" => return "toml",
        _ => {}
    }
    match ext_of(path).as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" | "json5" | "webmanifest" => "json",
        "py" | "pyi" => "python",
        "go" => "go",
        "html" | "htm" | "xhtml" | "vue" | "svelte" => "html",
        "css" | "scss" | "less" => "css",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "sh" | "bash" | "zsh" | "fish" | "env" => "bash",
        "sql" => "sql",
        "md" | "markdown" | "mdx" => "markdown",
        "mk" => "make",
        _ => "text",
    }
}

/// Whether bytes read as text: no NUL in the first few thousand, and UTF-8.
fn looks_like_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    !head.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// A PDF's pages, drawn, and how many it has in all; or why it couldn't be.
type Pages = Result<(Vec<Arc<RenderImage>>, usize), String>;

/// What the viewer shows.
enum Body {
    /// Text, in a read-only editor. Markdown also keeps its words, to read as a page.
    Text {
        editor: Entity<EditorState>,
        lines: usize,
        markdown: Option<SharedString>,
    },
    Image,
    /// Pages of a PDF, drawn off the window's thread: `None` while they're coming.
    Pdf(Option<Pages>),
    /// Why it can't be shown here.
    Cannot(String),
}

/// A file open in the viewer.
pub(super) struct Viewer {
    pub(super) path: PathBuf,
    kind: Kind,
    body: Body,
    /// Markdown shown as its source rather than as a page.
    source: bool,
    /// The job the file came from, whose browser can show a page.
    job: Option<u64>,
    /// The apps that can open it, default first, looked up the first time
    /// you ask, as (name, the app's path).
    pub(super) apps: Option<Vec<(String, PathBuf)>>,
    /// What the job did to the file, like "MODIFIED", when it changed it.
    change: Option<&'static str>,
    /// The file's size in bytes.
    bytes: u64,
}

impl Shell {
    /// Opens `path` in the viewer. `job` is the job it came from, if any.
    pub(super) fn open_file(
        &mut self,
        path: PathBuf,
        job: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.is_dir() {
            cx.reveal_path(&path);
            return;
        }
        let kind = kind_of(&path);
        let body = match kind {
            Kind::Image => Body::Image,
            Kind::Other => Body::Cannot("Squad Code can't show this kind of file.".into()),
            Kind::Pdf => {
                if cfg!(target_os = "macos") {
                    let at = path.clone();
                    let task = cx.background_spawn(async move { pdf_pages(&at) });
                    cx.spawn(async move |this, cx| {
                        let pages = task.await;
                        let _ = this.update(cx, |this, cx| {
                            if let Some(v) = this.viewer.as_mut() {
                                if v.kind == Kind::Pdf && matches!(v.body, Body::Pdf(None)) {
                                    v.body = Body::Pdf(Some(pages));
                                    cx.notify();
                                }
                            }
                        });
                    })
                    .detach();
                    Body::Pdf(None)
                } else {
                    Body::Cannot("PDFs show here on a Mac.".into())
                }
            }
            Kind::Text | Kind::Markdown | Kind::Html => text_body(&path, kind, window, cx),
        };
        let change = job.and_then(|id| self.crew.store.job(id)).and_then(|j| {
            let rel = path
                .strip_prefix(j.dir())
                .ok()?
                .to_string_lossy()
                .into_owned();
            j.changes
                .iter()
                .find(|c| c.path == rel)
                .map(|c| change_word(&c.status))
        });
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        self.viewer = Some(Viewer {
            path,
            kind,
            body,
            source: false,
            job,
            apps: None,
            change,
            bytes,
        });
        self.menu = None;
        cx.notify();
    }

    pub(super) fn close_file(&mut self, cx: &mut Context<Self>) {
        if self.viewer.take().is_some() {
            cx.notify();
        }
    }

    /// The apps that can open the file in the viewer, looked up once.
    pub(super) fn viewer_apps(&mut self) -> Vec<(String, PathBuf)> {
        let Some(v) = self.viewer.as_mut() else {
            return Vec::new();
        };
        v.apps.get_or_insert_with(|| apps_for(&v.path)).clone()
    }

    /// Shows the page in the job's own browser, inside Squad, or in your
    /// browser when it didn't come from a job here.
    fn preview_page(&mut self, cx: &mut Context<Self>) {
        let Some(v) = self.viewer.as_ref() else {
            return;
        };
        let url = format!("file://{}", v.path.display());
        let job = v.job.filter(|id| {
            self.crew
                .store
                .job(*id)
                .is_some_and(|j| j.remote_on.is_empty())
        });
        match job {
            Some(id) => {
                self.viewer = None;
                self.open_job_browser(id, Some(url), cx);
            }
            None => cx.open_url(&url),
        }
        cx.notify();
    }

    /// The viewer, over the page, when a file is open.
    pub(super) fn file_view(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open_label = match self.viewer_apps().first() {
            Some((app, _)) => format!("Open with {app}"),
            None => "Open with".to_string(),
        };
        let v = self.viewer.as_ref()?;
        let path = v.path.clone();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let folder = path
            .parent()
            .map(|p| tidy_home(&p.display().to_string()))
            .unwrap_or_default();
        let kind = v.kind;
        let source = v.source;
        let has_job_browser = v.job.is_some_and(|id| {
            self.crew
                .store
                .job(id)
                .is_some_and(|j| j.remote_on.is_empty())
        });

        let detail = match &v.body {
            Body::Text { lines, .. } => Some(match lines {
                1 => "1 line".to_string(),
                n => format!("{n} lines"),
            }),
            Body::Pdf(Some(Ok((_, total)))) => Some(match total {
                1 => "1 page".to_string(),
                n => format!("{n} pages"),
            }),
            _ => None,
        };

        let icon = match kind {
            Kind::Image => IconName::FileImage,
            Kind::Markdown | Kind::Pdf => IconName::FileText,
            Kind::Other => IconName::File,
            _ => IconName::FileCode,
        };

        let mut tools = div().flex().items_center().gap(px(6.)).flex_none();
        if kind == Kind::Markdown {
            let mut seg = div()
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(6.))
                .bg(c(ui::hover()));
            for (k, label) in ["Page", "Source"].into_iter().enumerate() {
                let on = source == (k == 1);
                seg = seg.child(
                    div()
                        .id(("file-mode", k))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .map(|d| {
                            if on {
                                d.bg(c(ui::float()))
                                    .text_color(c(ui::ink()))
                                    .border_1()
                                    .border_color(c(ui::line()))
                            } else {
                                d.text_color(c(ui::muted()))
                                    .hover(|s| s.text_color(c(ui::ink())))
                            }
                        })
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(v) = this.viewer.as_mut() {
                                v.source = k == 1;
                            }
                            cx.notify();
                        })),
                );
            }
            tools = tools.child(seg);
        }
        if kind == Kind::Html {
            tools = tools.child(
                ui::ghost_i(
                    "file-preview",
                    if has_job_browser {
                        IconName::Eye
                    } else {
                        IconName::Globe
                    },
                    if has_job_browser {
                        "Preview"
                    } else {
                        "Open in browser"
                    },
                )
                .h(px(28.))
                .text_size(px(12.5))
                .on_click(cx.listener(|this, _, _, cx| this.preview_page(cx))),
            );
        }
        let menu_path = path.clone();
        tools = tools
            .child(
                self.drop_menu(
                    super::menu::Ctx::File(menu_path),
                    ui::dbtn("file-open-with", open_label, ui::Btn::Plain)
                        .h(px(30.))
                        .gap(px(6.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            Icon::new(IconName::ChevronDown)
                                .size(px(12.))
                                .text_color(c(ui::muted())),
                        ),
                    cx,
                ),
            )
            .child(
                ui::icon_btn("file-close", IconName::X)
                    .on_click(cx.listener(|this, _, _, cx| this.close_file(cx))),
            );

        let change = v.change;
        let head = div()
            .flex_none()
            .h(px(48.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(c(ui::line()))
            .child(
                Icon::new(icon)
                    .size(px(15.))
                    .flex_none()
                    .text_color(c(ui::body())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(320.))
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(ui::ink()))
                            .child(name),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(ui::MONO)
                            .text_size(px(11.5))
                            .text_color(c(ui::muted()))
                            .child(folder),
                    )
                    .when_some(change, |d, word| {
                        let tone = match word {
                            "NEW" => ui::t_green(),
                            "DELETED" => ui::t_red(),
                            _ => ui::t_amber(),
                        };
                        d.child(
                            div()
                                .flex_none()
                                .font_family(ui::MONO)
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c(tone))
                                .child(word),
                        )
                    }),
            )
            .child(tools);

        // The line along the bottom: what the file is, and how to leave or ask.
        let mut facts = vec![kind_word(kind, &path)];
        facts.extend(detail);
        if v.bytes > 0 {
            facts.push(size_words(v.bytes));
        }
        let asker = v.job.and_then(|id| self.crew.store.job(id)).map(|j| j.bot);
        let foot = div()
            .flex_none()
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(16.))
            .border_t_1()
            .border_color(c(ui::line()))
            .font_family(ui::MONO)
            .text_size(px(11.))
            .text_color(c(ui::muted()))
            .child(div().flex_1().min_w_0().truncate().child(facts.join(" · ")))
            .child(div().flex_none().child("Esc to close"))
            .when_some(asker, |d, bot| {
                let about = path.clone();
                d.child(
                    div()
                        .id("file-ask")
                        .flex_none()
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(ui::ink())))
                        .child(format!("Ask {} about this file", bot.name()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.ask_about_file(bot, &about, window, cx)
                        })),
                )
            });

        let quiet = |msg: String| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .text_size(px(13.))
                .text_color(c(ui::muted()))
                .child(msg)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(ui::faint()))
                        .child("Use Open with to see it in another app."),
                )
                .into_any_element()
        };

        let v = self.viewer.as_ref()?;
        let content: AnyElement = match &v.body {
            Body::Text {
                markdown: Some(md), ..
            } if !source => div()
                .id("file-markdown")
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .max_w(px(684.))
                        .mx_auto()
                        .px(px(32.))
                        .pt(px(28.))
                        .pb(px(32.))
                        .text_size(px(14.))
                        .line_height(px(22.))
                        .text_color(c(ui::body()))
                        .child(
                            TextView::markdown("file-markdown-text", md.clone())
                                .style(page_style(cx))
                                .selectable(true),
                        ),
                )
                .into_any_element(),
            Body::Text { editor, .. } => div()
                .size_full()
                .child(
                    Editor::new(editor)
                        .readonly(true)
                        .bordered(false)
                        .appearance(false)
                        .h_full(),
                )
                .into_any_element(),
            Body::Image => div()
                .size_full()
                .p(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(path.clone())
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(|| {
                            div()
                                .text_size(px(13.))
                                .text_color(c(ui::muted()))
                                .child("This picture couldn't be shown.")
                                .into_any_element()
                        }),
                )
                .into_any_element(),
            Body::Pdf(None) => quiet("Opening…".into()),
            Body::Pdf(Some(Err(why))) => quiet(why.clone()),
            Body::Pdf(Some(Ok((pages, total)))) => {
                let mut list = div()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .py(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.));
                for (n, page) in pages.iter().enumerate() {
                    let size = page.size(0);
                    let ratio = size.height.0 as f32 / size.width.0.max(1) as f32;
                    list = list.child(
                        div()
                            .id(("pdf-page", n))
                            .w_full()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(c(ui::line()))
                            .overflow_hidden()
                            .child(
                                img(ImageSource::Render(page.clone()))
                                    .w_full()
                                    // Keeps the page's shape at any width.
                                    .h(px(780. * ratio))
                                    .object_fit(ObjectFit::Contain),
                            ),
                    );
                }
                if *total > pages.len() {
                    list = list.child(
                        div()
                            .py(px(8.))
                            .text_center()
                            .text_size(px(12.5))
                            .text_color(c(ui::faint()))
                            .child(format!(
                                "Showing the first {} of {total} pages. Use Open with for the rest.",
                                pages.len()
                            )),
                    );
                }
                div()
                    .id("file-pdf")
                    .size_full()
                    .overflow_y_scroll()
                    .child(list)
                    .into_any_element()
            }
            Body::Cannot(why) => quiet(why.clone()),
        };

        let card = div()
            .id("file-viewer-card")
            .w(px(900.))
            .max_w_full()
            .h(px(640.))
            .max_h_full()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(c(ui::float()))
            .border_1()
            .border_color(c(ui::line()))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.18),
                offset: point(px(0.), px(12.)),
                blur_radius: px(40.),
                spread_radius: px(0.),
                inset: false,
            }])
            // A click on the card stays on it; one beside it closes the viewer.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(head)
            .child(div().flex_1().min_h_0().bg(c(ui::bg())).child(content))
            .child(foot);
        Some(
            div()
                .id("file-viewer")
                .absolute()
                .inset_0()
                .p(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.28))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_file(cx)),
                )
                .child(card)
                .into_any_element(),
        )
    }

    /// Opens a chat with `bot`, its box started with a question about `path`.
    fn ask_about_file(
        &mut self,
        bot: super::data::Member,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.close_file(cx);
        self.go(super::Place::Chat(bot), cx);
        let ask = format!("About {name}: ");
        self.chat_input.update(cx, |i, cx| {
            i.set_value(ask, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }
}

/// The word for what a job did to a file, from its git status.
fn change_word(status: &str) -> &'static str {
    match status.trim() {
        "A" | "??" => "NEW",
        "D" => "DELETED",
        _ => "MODIFIED",
    }
}

/// What kind of file this is, in a word, for the viewer's bottom line.
fn kind_word(kind: Kind, path: &Path) -> String {
    match kind {
        Kind::Markdown => "Markdown".into(),
        Kind::Html => "HTML".into(),
        Kind::Image => "Picture".into(),
        Kind::Pdf => "PDF".into(),
        _ => super::filetab::language(&path.to_string_lossy()).into(),
    }
}

/// A file size the way people say it: "812 B", "2.1 KB", "3.4 MB".
fn size_words(bytes: u64) -> String {
    const K: f64 = 1024.;
    let b = bytes as f64;
    if b < K {
        format!("{bytes} B")
    } else if b < K * K {
        format!("{:.1} KB", b / K)
    } else {
        format!("{:.1} MB", b / (K * K))
    }
}

/// How a Markdown file reads as a page: headings with weight, code in a
/// framed well, and tables as plain ruled rows under a small mono head.
fn page_style(cx: &App) -> gpui_kit::component::text::TextViewStyle {
    use gpui_kit::component::ActiveTheme;
    let line = c(ui::line());
    let theme = cx.theme();
    let mut style = gpui_kit::component::text::TextViewStyle::default()
        .heading_font_size(|level, _| match level {
            1 => px(26.),
            2 => px(17.),
            _ => px(15.),
        })
        .inline_code(HighlightStyle {
            background_color: Some(c(ui::well())),
            ..Default::default()
        })
        .code_block(
            StyleRefinement::default()
                .bg(c(ui::well()))
                .border_1()
                .border_color(line)
                .rounded(px(6.))
                .px(px(12.))
                .py(px(10.)),
        )
        .table(StyleRefinement::default().border_0().bg(c(ui::float())))
        .table_head(
            StyleRefinement::default()
                .bg(c(ui::float()))
                .text_color(c(ui::muted()))
                .font_family(ui::MONO)
                .text_size(px(11.)),
        )
        .table_cell(StyleRefinement::default().border_r_0().px_0().py(px(8.)));
    style.highlight_theme = theme.highlight_theme.clone();
    style.is_dark = theme.is_dark();
    style
}

/// A text file's body: read, checked, and put in a read-only editor.
fn text_body(path: &Path, kind: Kind, window: &mut Window, cx: &mut Context<Shell>) -> Body {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return Body::Cannot("This file isn't there anymore.".into()),
    };
    if size > MAX_TEXT {
        return Body::Cannot(format!(
            "This file is too big to show here ({} MB).",
            size / (1024 * 1024)
        ));
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Body::Cannot(format!("Couldn't read it: {e}")),
    };
    if !looks_like_text(&bytes) {
        return Body::Cannot("This file isn't text, and Squad Code can't show it.".into());
    }
    let text = String::from_utf8(bytes).unwrap_or_default();
    let lines = text.lines().count().max(1);
    let lang = language_of(path);
    let markdown = (kind == Kind::Markdown).then(|| SharedString::from(text.clone()));
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(lang)
            .line_number(true)
            .default_value(text)
    });
    Body::Text {
        editor,
        lines,
        markdown,
    }
}

/// `~/…` for a path in your home folder.
fn tidy_home(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// A PDF's first pages, drawn, and how many it has.
fn pdf_pages(path: &Path) -> Pages {
    #[cfg(target_os = "macos")]
    {
        let (pages, total) = crate::macfile::render(path, PDF_PAGES, PDF_WIDTH)?;
        let pages = pages
            .into_iter()
            .map(|mut rgba| {
                // GPUI draws BGRA.
                for px in rgba.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                }
                Arc::new(RenderImage::new(vec![image::Frame::new(rgba)]))
            })
            .collect();
        Ok((pages, total))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, PDF_PAGES, PDF_WIDTH);
        Err("PDFs show here on a Mac.".into())
    }
}

/// The apps that can open `path`, the default first, as (name, app path).
fn apps_for(path: &Path) -> Vec<(String, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        crate::macfile::apps_for(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Vec::new()
    }
}

/// Opens `path` in the app at `app`, or in its default app.
pub(super) fn open_in(path: &Path, app: Option<&Path>, cx: &mut App) {
    match app {
        #[cfg(target_os = "macos")]
        Some(app) => {
            let _ = std::process::Command::new("open")
                .arg("-a")
                .arg(app)
                .arg(path)
                .spawn();
        }
        _ => {
            let _ = app;
            cx.open_url(&format!("file://{}", path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{change_word, kind_of, kind_word, language_of, looks_like_text, size_words, Kind};
    use std::path::Path;

    #[test]
    fn the_bottom_line_reads_plainly() {
        assert_eq!(size_words(663), "663 B");
        assert_eq!(size_words(2150), "2.1 KB");
        assert_eq!(size_words(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(kind_word(Kind::Text, Path::new("a/pay.ts")), "TypeScript");
        assert_eq!(kind_word(Kind::Markdown, Path::new("a.md")), "Markdown");
        assert_eq!(change_word("M"), "MODIFIED");
        assert_eq!(change_word("??"), "NEW");
        assert_eq!(change_word("D"), "DELETED");
    }

    #[test]
    fn files_are_shown_by_kind() {
        assert_eq!(kind_of(Path::new("/a/README.md")), Kind::Markdown);
        assert_eq!(kind_of(Path::new("index.html")), Kind::Html);
        assert_eq!(kind_of(Path::new("logo.PNG")), Kind::Image);
        assert_eq!(kind_of(Path::new("spec.pdf")), Kind::Pdf);
        assert_eq!(kind_of(Path::new("clip.mp4")), Kind::Other);
        assert_eq!(kind_of(Path::new("src/main.rs")), Kind::Text);
        assert_eq!(kind_of(Path::new("Makefile")), Kind::Text);
    }

    #[test]
    fn code_gets_its_language() {
        assert_eq!(language_of(Path::new("a.rs")), "rust");
        assert_eq!(language_of(Path::new("App.jsx")), "javascript");
        assert_eq!(language_of(Path::new("App.tsx")), "tsx");
        assert_eq!(language_of(Path::new("x.yml")), "yaml");
        assert_eq!(language_of(Path::new("Makefile")), "make");
        assert_eq!(language_of(Path::new("notes.txt")), "text");
    }

    #[test]
    fn binary_is_not_text() {
        assert!(looks_like_text(b"fn main() {}\n"));
        assert!(!looks_like_text(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(!looks_like_text(&[0xff, 0xfe, 0xfd]));
    }
}
