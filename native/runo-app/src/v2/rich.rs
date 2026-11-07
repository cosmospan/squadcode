//! A bot's words, made actionable: links and local ports open in the browser,
//! commands run in the terminal drawer, files open, and `code` reads as code.
//! Under the text, the things you'd act on sit as chips, so nothing needs
//! copying out of a message.

use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use super::ui::{self, c, ca};
use super::Shell;

/// Something in a message you can act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// A web address, local port included.
    Link(String),
    /// A shell command.
    Run(String),
    /// A file or folder, as written (relative to the job's folder or absolute).
    File(String),
}

impl Act {
    fn chip(&self) -> (IconName, String) {
        match self {
            Act::Link(u) => {
                let short = u
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/');
                (IconName::Globe, format!("Open {short}"))
            }
            Act::Run(cmd) => (IconName::SquareTerminal, format!("Run {cmd}")),
            Act::File(p) => {
                let name = p.rsplit('/').find(|x| !x.is_empty()).unwrap_or(p);
                (IconName::FileText, format!("Open {name}"))
            }
        }
    }
}

/// One paragraph laid out: its text without backticks, the code spans and
/// actionable spans in it.
#[derive(Debug, Default, PartialEq)]
pub struct Para {
    pub text: String,
    pub code: Vec<Range<usize>>,
    pub acts: Vec<(Range<usize>, Act)>,
}

const COMMANDS: &[&str] = &[
    "npm", "npx", "node", "yarn", "pnpm", "bun", "deno", "cargo", "python", "python3", "pip",
    "pip3", "uv", "go", "make", "git", "ls", "cd", "curl", "docker", "pytest", "ruby", "rails",
    "bundle", "php", "composer", "mvn", "gradle", "swift", "flutter", "dotnet", "brew",
];

/// What a bit of text is, if you can act on it.
pub fn classify(s: &str) -> Option<Act> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(Act::Link(s.to_string()));
    }
    if s.starts_with("localhost:") || s.starts_with("127.0.0.1:") {
        return Some(Act::Link(format!("http://{s}")));
    }
    let first = s.split_whitespace().next().unwrap_or("");
    if COMMANDS.contains(&first) && s.contains(' ') {
        return Some(Act::Run(s.to_string()));
    }
    if !s.contains(' ')
        && (s.starts_with('/') || s.starts_with("~/") || s.starts_with("./"))
        && s.len() > 2
    {
        return Some(Act::File(s.to_string()));
    }
    // "server.js", "src/app.ts": a name with an extension.
    if !s.contains(' ') && !s.starts_with('/') {
        if let Some((stem, ext)) = s.rsplit_once('.') {
            let known = [
                "js", "ts", "tsx", "jsx", "json", "rs", "py", "go", "md", "html", "css", "toml",
                "yml", "yaml", "rb", "java", "kt", "swift", "sh", "txt", "env", "lock",
            ];
            if !stem.is_empty() && known.contains(&ext) {
                return Some(Act::File(s.to_string()));
            }
        }
    }
    None
}

/// Lays out one paragraph: strips backticks into code spans and finds links,
/// commands and files.
pub fn parse(src: &str) -> Para {
    let mut p = Para::default();
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(open) = rest.find('`') {
            words(&rest[..open], &mut p);
            let after = &rest[open + 1..];
            match after.find('`') {
                Some(close) => {
                    let code = &after[..close];
                    let start = p.text.len();
                    p.text.push_str(code);
                    let r = start..p.text.len();
                    if let Some(a) = classify(code) {
                        p.acts.push((r.clone(), a));
                    }
                    p.code.push(r);
                    rest = &after[close + 1..];
                }
                None => {
                    words(&rest[open..], &mut p);
                    rest = "";
                }
            }
        } else {
            words(rest, &mut p);
            rest = "";
        }
    }
    p
}

/// Plain text: adds it, marking bare web addresses.
fn words(s: &str, p: &mut Para) {
    let base = p.text.len();
    p.text.push_str(s);
    let mut at = 0;
    while let Some(i) = s[at..].find("http") {
        let start = at + i;
        let tail = &s[start..];
        if !(tail.starts_with("http://") || tail.starts_with("https://")) {
            at = start + 4;
            continue;
        }
        let end = tail
            .find(|ch: char| ch.is_whitespace() || ch == ')' || ch == '"' || ch == '>')
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if url.len() > 8 {
            let r = base + start..base + start + url.len();
            p.acts.push((r, Act::Link(url.to_string())));
        }
        at = start + end.max(1);
    }
}

/// The text without markdown fuss: heading marks and bold stars go.
fn tidy(text: &str) -> String {
    text.lines()
        .map(|l| {
            let l = l.trim_end();
            let l = l.trim_start_matches('#').trim_start();
            let l = l.replace("**", "").replace("__", "");
            // A Markdown list item reads as a bullet.
            match l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
                Some(rest) => format!("\u{2022}  {rest}"),
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// A piece of a message: words, or fenced code with its language.
enum Block {
    Words(String),
    Code(String, String),
}

impl Shell {
    /// A bot's message: paragraphs with live links, then chips for what to do.
    /// `dir` is the folder its files are relative to.
    pub(super) fn rich(
        &mut self,
        key: impl Into<SharedString>,
        text: &str,
        dir: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let key: SharedString = key.into();
        let me = cx.entity().downgrade();
        let mut out = div().flex().flex_col().gap(px(8.));
        let mut all: Vec<Act> = Vec::new();
        // Fenced code reads as a block of its own with a Copy button; the
        // words around it as paragraphs.
        let mut paras: Vec<Block> = Vec::new();
        for (k, part) in text.split("```").enumerate() {
            if k % 2 == 1 {
                let (lang, code) = match part.split_once('\n') {
                    Some((first, rest)) if !first.trim().contains(' ') => {
                        (first.trim().to_string(), rest)
                    }
                    _ => (String::new(), part),
                };
                let code = code.trim_matches('\n').to_string();
                if !code.is_empty() {
                    paras.push(Block::Code(lang, code));
                }
            } else {
                paras.extend(
                    tidy(part)
                        .split("\n\n")
                        .filter(|p| !p.trim().is_empty())
                        .map(|p| Block::Words(p.to_string())),
                );
            }
        }
        for (n, block) in paras.into_iter().enumerate() {
            let para = match block {
                Block::Words(w) => w,
                Block::Code(lang, code) => {
                    out = out.child(self.code_block(format!("{key}-code{n}"), &lang, code, cx));
                    continue;
                }
            };
            let para = para.as_str();
            let p = parse(para);
            let code_bg = ca(ui::ink(), 0.07);
            let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
            for r in &p.code {
                if p.acts.iter().any(|(a, _)| a == r) {
                    continue;
                }
                runs.push((
                    r.clone(),
                    HighlightStyle {
                        background_color: Some(code_bg),
                        color: Some(c(ui::ink())),
                        ..Default::default()
                    },
                ));
            }
            for (r, _) in &p.acts {
                runs.push((
                    r.clone(),
                    HighlightStyle {
                        color: Some(c(ui::t_blue())),
                        background_color: p.code.contains(r).then(|| ca(ui::t_blue(), 0.08)),
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            color: Some(ca(ui::t_blue(), 0.4)),
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                ));
            }
            runs.sort_by_key(|(r, _)| r.start);
            let ranges: Vec<Range<usize>> = p.acts.iter().map(|(r, _)| r.clone()).collect();
            let acts: Vec<Act> = p.acts.iter().map(|(_, a)| a.clone()).collect();
            for a in &acts {
                if !all.contains(a) {
                    all.push(a.clone());
                }
            }
            let me = me.clone();
            let dir = dir.to_string();
            out = out.child(
                InteractiveText::new(
                    SharedString::from(format!("{key}-p{n}")),
                    StyledText::new(p.text.clone()).with_highlights(runs),
                )
                .on_click(ranges, move |i, window, cx| {
                    if let Some(a) = acts.get(i) {
                        let a = a.clone();
                        let dir = dir.clone();
                        let _ = me.update(cx, |this, cx| this.act(&a, &dir, window, cx));
                    }
                }),
            );
        }
        if !all.is_empty() {
            let mut chips = div().flex().flex_wrap().gap(px(6.)).pt(px(2.));
            for (n, a) in all.into_iter().take(4).enumerate() {
                let (icon, label) = a.chip();
                let label: String = label.chars().take(48).collect();
                let dir = dir.to_string();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("{key}-act{n}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(c(ui::line()))
                        .bg(c(ui::panel()))
                        .text_size(px(12.5))
                        .text_color(c(ui::ink()))
                        .cursor_pointer()
                        .hover(|s| s.bg(c(ui::hover())).border_color(c(ui::line_hi())))
                        .child(Icon::new(icon).size(px(13.)).text_color(c(ui::muted())))
                        .child(label)
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.act(&a, &dir, window, cx)),
                        ),
                );
            }
            out = out.child(chips);
        }
        out
    }

    /// A block of code in a message: monospace on a sunken ground, its
    /// language and a Copy button on top.
    fn code_block(&mut self, id: String, lang: &str, code: String, cx: &mut Context<Self>) -> Div {
        let copy = code.clone();
        div()
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(c(ui::line()))
            .bg(ca(ui::ink(), 0.04))
            .overflow_hidden()
            .child(
                div()
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(c(ui::line2()))
                    .text_size(px(11.))
                    .text_color(c(ui::faint()))
                    .child(div().flex_1().child(if lang.is_empty() {
                        "code".to_string()
                    } else {
                        lang.to_string()
                    }))
                    .child(
                        div()
                            .id(SharedString::from(id))
                            .px(px(6.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(|s| s.bg(c(ui::hover())).text_color(c(ui::ink())))
                            .child(Icon::new(IconName::Copy).size(px(12.)))
                            .child("Copy")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                this.say("Copied.");
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .font_family(ui::MONO)
                    .text_size(px(12.5))
                    .line_height(px(19.))
                    .text_color(c(ui::ink()))
                    .child(code),
            )
    }

    /// Does what a message offered.
    pub(super) fn act(&mut self, a: &Act, dir: &str, window: &mut Window, cx: &mut Context<Self>) {
        match a {
            Act::Link(u) => cx.open_url(u),
            Act::Run(cmd) => {
                if self.crew.store.folder().is_some() {
                    let id = super::term::project_id(self.crew.store.current);
                    let line = if dir.is_empty() {
                        cmd.clone()
                    } else {
                        format!("cd {} && {cmd}", shell_quote(dir))
                    };
                    self.term_run(id, &line, window, cx);
                } else {
                    cx.write_to_clipboard(ClipboardItem::new_string(cmd.clone()));
                    self.say("Copied. Open a folder to run it here.");
                }
            }
            Act::File(p) => {
                let path = if p.starts_with('/') {
                    p.clone()
                } else if let Some(rest) = p.strip_prefix("~/") {
                    std::env::var("HOME")
                        .map(|h| format!("{h}/{rest}"))
                        .unwrap_or_else(|_| p.clone())
                } else {
                    format!(
                        "{}/{}",
                        dir.trim_end_matches('/'),
                        p.trim_start_matches("./")
                    )
                };
                if std::path::Path::new(&path).exists() {
                    cx.open_url(&format!("file://{path}"));
                } else {
                    self.say(format!("{p} isn't there yet."));
                }
            }
        }
        cx.notify();
    }
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|ch| ch.is_alphanumeric() || "/._-~".contains(ch))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{classify, parse, Act};

    #[test]
    fn a_message_turns_into_things_to_do() {
        let p = parse("Run `npm test`, then open `http://localhost:3000` or see server.js.");
        assert_eq!(
            p.text,
            "Run npm test, then open http://localhost:3000 or see server.js."
        );
        let acts: Vec<&Act> = p.acts.iter().map(|(_, a)| a).collect();
        assert!(acts.contains(&&Act::Run("npm test".into())));
        assert!(acts.contains(&&Act::Link("http://localhost:3000".into())));
        assert_eq!(p.code.len(), 2);
    }

    #[test]
    fn bare_links_and_files_are_found() {
        let p = parse("Docs at https://example.com/a. Edited `/Users/me/app/server.js`.");
        let acts: Vec<&Act> = p.acts.iter().map(|(_, a)| a).collect();
        assert_eq!(acts[0], &Act::Link("https://example.com/a".into()));
        assert_eq!(acts[1], &Act::File("/Users/me/app/server.js".into()));
        assert_eq!(classify("hello world"), None);
        assert_eq!(
            classify("localhost:5173"),
            Some(Act::Link("http://localhost:5173".into()))
        );
    }
}

/// The bot narrating its own tools ("I'll create the files with the Write
/// tool instead."): noise in a chat, so it isn't shown.
pub fn tool_chatter(text: &str) -> bool {
    let t = text.trim();
    if t.len() > 180 {
        return false;
    }
    let tools = [
        "Write tool",
        "Bash tool",
        "Edit tool",
        "Read tool",
        "Grep tool",
        "Glob tool",
        "MultiEdit",
        "TodoWrite",
        "the tool",
        "a tool",
        "tool instead",
    ];
    tools.iter().any(|w| t.contains(w))
}

/// What a bot's last word is, so it reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Done,
    NeedsYou,
    Question,
    Warning,
}

impl Kind {
    pub fn of(text: &str, status: super::store::Status) -> Kind {
        use super::store::Status;
        let low = text.to_lowercase();
        if matches!(status, Status::Failed | Status::Interrupted) {
            return Kind::Warning;
        }
        let stuck = [
            "couldn't",
            "could not",
            "can't",
            "cannot",
            "needed an approval",
            "need your",
            "blocked",
            "permission",
            "untested",
            "wasn't able",
            "was not able",
        ];
        if stuck.iter().any(|w| low.contains(w)) {
            return Kind::NeedsYou;
        }
        if text.trim_end().ends_with('?') {
            return Kind::Question;
        }
        Kind::Done
    }

    fn look(self) -> (IconName, &'static str, ui::Tone) {
        match self {
            Kind::Done => (IconName::CircleCheck, "Done", ui::ok_tone()),
            Kind::NeedsYou => (IconName::Hand, "Needs you", ui::ask_tone()),
            Kind::Question => (IconName::MessageCircle, "Question for you", ui::run_tone()),
            Kind::Warning => (IconName::TriangleAlert, "Didn't finish", ui::bad_tone()),
        }
    }
}

/// A bot's speech bubble; its last word gets a header saying what kind it is,
/// in the kind's colour.
pub fn bubble(kind: Option<Kind>, body: impl IntoElement) -> Div {
    let base = div()
        .rounded(px(16.))
        .bg(ca(ui::panel(), 0.85))
        .border_1()
        .border_color(c(ui::line()))
        .overflow_hidden()
        .flex()
        .flex_col();
    let Some(kind) = kind else {
        return base.px(px(18.)).py(px(13.)).child(body);
    };
    let (icon, label, tone) = kind.look();
    base.border_color(c(tone.2))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .px(px(16.))
                .py(px(7.))
                .bg(c(tone.1))
                .border_b_1()
                .border_color(c(tone.2))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(tone.0))
                .child(Icon::new(icon).size(px(13.)).text_color(c(tone.0)))
                .child(label),
        )
        .child(div().px(px(18.)).py(px(12.)).child(body))
}
