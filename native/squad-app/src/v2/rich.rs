//! A bot's words, made actionable: links and local ports open in the browser,
//! commands run inside Squad as a card of their own, files open, and `code` reads as code.
//! Under the text, the things you'd act on sit as chips, so nothing needs
//! copying out of a message.

use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use super::ui::{self, c, ca};
use super::{Place, Shell};

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
    fn chip(&self, there: bool) -> (IconName, String) {
        match self {
            Act::Link(u) => {
                let short = u
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/');
                let on = if there && local_port(u).is_some() {
                    " on the server"
                } else {
                    ""
                };
                (IconName::Globe, format!("Open {short}{on}"))
            }
            Act::Run(cmd) if there => (IconName::SquareTerminal, format!("Run {cmd} there")),
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

/// The port of a link to this machine itself (localhost, 127.0.0.1,
/// 0.0.0.0), and the rest of it after the port.
pub fn local_port(url: &str) -> Option<(u16, &str)> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let rest = ["localhost:", "127.0.0.1:", "0.0.0.0:"]
        .iter()
        .find_map(|h| rest.strip_prefix(h))?;
    let end = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    Some((rest[..end].parse().ok()?, &rest[end..]))
}

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
    // A path on this computer. A bare "/hello/Anish" is more often a web
    // route than a file, so an absolute path needs a real root or a file
    // extension to count.
    let real_root = [
        "/Users/",
        "/home/",
        "/tmp/",
        "/var/",
        "/etc/",
        "/opt/",
        "/private/",
        "/Volumes/",
    ]
    .iter()
    .any(|r| s.starts_with(r));
    let has_ext = s
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()));
    if !s.contains(' ')
        && s.len() > 2
        && (s.starts_with("~/")
            || s.starts_with("./")
            || (s.starts_with('/') && (real_root || has_ext)))
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
        // Where these words were said, so a command run from them shows its
        // card there: Peek's message ("talk-12"), a job's feed ("feed-7-…").
        let from: (Option<u64>, Option<u64>) = {
            let num = |s: &str| s.split('-').next().and_then(|n| n.parse::<u64>().ok());
            if let Some(rest) = key.strip_prefix("talk-") {
                (num(rest), None)
            } else if let Some(rest) = key.strip_prefix("feed-") {
                (None, num(rest))
            } else {
                (None, None)
            }
        };
        let me = cx.entity().downgrade();
        let there = self.rich_there.clone();
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
                if !all.iter().any(|b| same(a, b, dir)) {
                    all.push(a.clone());
                }
            }
            let me = me.clone();
            let dir = there.clone().unwrap_or_else(|| dir.to_string());
            let on = there.is_some();
            out = out.child(
                InteractiveText::new(
                    SharedString::from(format!("{key}-p{n}")),
                    StyledText::new(p.text.clone()).with_highlights(runs),
                )
                .on_click(ranges, move |i, window, cx| {
                    if let Some(a) = acts.get(i) {
                        let a = a.clone();
                        let dir = dir.clone();
                        let _ = me.update(cx, |this, cx| {
                            this.act_from = from;
                            this.act_where(&a, &dir, on, window, cx)
                        });
                    }
                }),
            );
        }
        if !all.is_empty() {
            let mut chips = div().flex().flex_wrap().gap(px(6.)).pt(px(2.));
            for (n, a) in all.into_iter().take(4).enumerate() {
                let (icon, label) = a.chip(there.is_some());
                let label: String = label.chars().take(48).collect();
                let on = there.is_some();
                let dir = there.clone().unwrap_or_else(|| dir.to_string());
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
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.act_from = from;
                            this.act_where(&a, &dir, on, window, cx)
                        })),
                );
            }
            out = out.child(chips);
        }
        out
    }

    /// Does what a message offered: on the server when `there` (`dir` is then
    /// the job's folder on the server), here otherwise.
    pub(super) fn act_where(
        &mut self,
        a: &Act,
        dir: &str,
        there: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !there {
            return self.act(a, dir, window, cx);
        }
        // The app on the server opens in the job's browser there, in Squad.
        if let (Act::Link(u), Some(id)) = (a, self.job_in_view()) {
            if local_port(u).is_some() {
                self.open_job_browser(id, Some(u.clone()), cx);
                return;
            }
        }
        let Some(cfg) = self.crew.link.as_ref().map(|l| l.cfg.clone()) else {
            self.say("The server isn't connected, so this can't be opened from here.");
            cx.notify();
            return;
        };
        match a {
            // A port on the server: tunnel to it, then open it here.
            Act::Link(u) => match local_port(u) {
                Some((port, rest)) => {
                    let alive = self.forwards.get_mut(&port).is_some_and(|f| f.alive());
                    if !alive {
                        match super::remote::forward(&cfg, port) {
                            Ok(f) => {
                                self.forwards.insert(port, f);
                            }
                            Err(why) => {
                                self.say(format!(
                                    "Couldn't reach port {port} on the server: {why}"
                                ));
                                cx.notify();
                                return;
                            }
                        }
                    }
                    let here = self.forwards[&port].port();
                    cx.open_url(&format!("http://127.0.0.1:{here}{rest}"));
                }
                None => cx.open_url(u),
            },
            Act::Run(cmd) => {
                let (talk, job) = std::mem::take(&mut self.act_from);
                let spec = super::apps::RunSpec {
                    cmd: cmd.clone(),
                    dir: dir.to_string(),
                    folder: self
                        .crew
                        .store
                        .folder()
                        .map(|f| f.path.clone())
                        .unwrap_or_default(),
                    job: job.or(self.job_in_view()),
                    talk,
                    hint: None,
                    keeps: super::apps::keeps_running(cmd),
                    remote: Some(cfg),
                    setup: false,
                };
                if let Err(why) = self.start_run(spec, cx) {
                    self.say(format!("Couldn't run it on the server: {why}"));
                }
            }
            Act::File(_) => {
                self.say("That file is on the server until you approve the job.");
            }
        }
        cx.notify();
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
            // The app a job made opens in that job's browser, inside Squad.
            Act::Link(u) => match self.job_in_view().filter(|_| local_port(u).is_some()) {
                Some(id) => self.open_job_browser(id, Some(u.clone()), cx),
                None => cx.open_url(u),
            },
            // Runs inside Squad, as a card where it was offered: never in
            // your terminal, which stays yours.
            Act::Run(cmd) => {
                let (talk, job) = std::mem::take(&mut self.act_from);
                if let Some(folder) = self.crew.store.folder().map(|f| f.path.clone()) {
                    let spec = super::apps::RunSpec {
                        cmd: cmd.clone(),
                        dir: if dir.is_empty() {
                            folder.clone()
                        } else {
                            dir.to_string()
                        },
                        folder,
                        job: job.or(self.job_in_view()),
                        talk,
                        hint: None,
                        keeps: super::apps::keeps_running(cmd),
                        remote: None,
                        setup: false,
                    };
                    if let Err(why) = self.start_run(spec, cx) {
                        self.say(format!("Couldn't run it: {why}"));
                    }
                } else {
                    cx.write_to_clipboard(ClipboardItem::new_string(cmd.clone()));
                    self.say("Copied. Open a folder to run it here.");
                }
            }
            // Opens in Squad's own viewer; "Open with" there hands it to another app.
            Act::File(p) => {
                let path = file_path(p, dir);
                if std::path::Path::new(&path).exists() {
                    let job = self.act_from.1.or(self.job_in_view());
                    self.open_file(path.into(), job, window, cx);
                } else {
                    self.say(format!("{p} isn't there yet."));
                }
            }
        }
        cx.notify();
    }
}

impl Shell {
    /// The job on screen, whose links open in its browser.
    fn job_in_view(&self) -> Option<u64> {
        match self.place {
            Place::Live(id) | Place::Browser(id) => Some(id),
            _ => None,
        }
    }
}

/// Where a file a message names is: as written when absolute, under the
/// home folder for `~/`, else in `dir`.
fn file_path(p: &str, dir: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else if let Some(rest) = p.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|h| format!("{h}/{rest}"))
            .unwrap_or_else(|_| p.to_string())
    } else {
        format!(
            "{}/{}",
            dir.trim_end_matches('/'),
            p.trim_start_matches("./")
        )
    }
}

/// Whether two things to do are the same: a file named twice, once by its
/// name and once by its whole path, is one file.
fn same(a: &Act, b: &Act, dir: &str) -> bool {
    match (a, b) {
        (Act::File(x), Act::File(y)) => file_path(x, dir) == file_path(y, dir),
        _ => a == b,
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

#[cfg(test)]
mod tests {
    use super::{classify, parse, same, Act};

    #[test]
    fn a_file_named_twice_is_one_file() {
        let dir = "/Users/a/dev/sandbox";
        let short = Act::File("index.html".into());
        let long = Act::File("/Users/a/dev/sandbox/index.html".into());
        assert!(same(&short, &long, dir));
        assert!(same(&Act::File("./index.html".into()), &long, dir));
        assert!(!same(&Act::File("other/index.html".into()), &long, dir));
    }

    #[test]
    fn a_web_route_is_not_a_file() {
        assert!(classify("/hello/Anish").is_none());
        assert!(classify("/api/users").is_none());
        assert!(matches!(
            classify("/Users/anish/dev/test/server.js"),
            Some(Act::File(_))
        ));
        assert!(matches!(classify("./server.js"), Some(Act::File(_))));
        assert!(matches!(classify("/src/app.ts"), Some(Act::File(_))));
    }

    #[test]
    fn a_link_to_this_machine_has_a_port() {
        use super::local_port;
        assert_eq!(local_port("http://localhost:5173"), Some((5173, "")));
        assert_eq!(
            local_port("http://127.0.0.1:3000/a?b=1"),
            Some((3000, "/a?b=1"))
        );
        assert_eq!(local_port("http://0.0.0.0:8080/"), Some((8080, "/")));
        assert_eq!(local_port("https://example.com:443/"), None);
        assert_eq!(local_port("http://localhost/"), None);
    }

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
