//! Designing in the real app: what a bot is told when a job is design work, a
//! starting theme from a mood (`design_theme`), and Design's checks run on the page
//! open in the job's browser (`design_check`), so the bot sees what makes its screens
//! hard to read or use on the running app, not on a mockup beside it.

use crate::designdraw::checks::{self, Facts, Kind};
use crate::designdraw::model::{self, Device};
use crate::designdraw::tokens;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Runs Design's checks on the page in the job's browser.
pub const CHECK: &str = "design_check";
/// A starting theme from a mood.
pub const THEME: &str = "design_theme";

/// What a bot doing design work is told: how to design well, in the app's own code.
pub const SKILL: &str = "\
This job is design work: make it look and feel like a product a top team would ship, in this app's own code.
- Start from the app's theme. Find where its colours, fonts, radii and spacing live (CSS variables, a Tailwind theme or config, a theme file) and use them everywhere instead of raw values. If there is no theme yet, or the job asks for a new look, start from a mood with Squad Code's design_theme tool (calm, bold, playful, editorial or pro) and put it in the app's theme.
- Purpose first: one clear focus per screen and one primary action; everything else secondary.
- Hierarchy from size, weight and colour, not boxes around everything: two or three text sizes per area from the type scale, muted text for what's secondary.
- Spacing on an 8 px grid: related things close, groups clearly apart, a few shared edges, the same padding on every card and row.
- Colour with restraint: neutrals for most of the screen, the primary colour for the main action and the current selection, status colours only for status.
- Real content: names, numbers, dates and copy that fit the product, never lorem ipsum or \"Item 1\".
- Every state: empty, loading and error, not only the happy path. Fit the device: one column and 44 px tap targets on a phone, the width used well on a desktop.
- Look at it. Start the app, open each screen you changed in the job's browser, run Squad Code's design_check on it and fix what it finds, then take a screenshot and judge it as a demanding design lead would. Do this until it's right, at most three rounds a screen.
- New screens start with directions: before building them out, make the main screen in two or three different looks, save a screenshot of each, show them with Squad Code's show_directions tool and say which you'd pick and why. Carry on with that one; if the person picks another, it comes as your next message.
- Show the result: when a screen is right, save a screenshot and show it with show_picture, so the person sees it without opening the app.";

/// A mood: its name, what it's like, and the theme variables it sets.
type Mood = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// The moods a theme can start from: what each sets over Design's default theme.
pub const MOODS: [Mood; 5] = [
    (
        "calm",
        "Calm: warm paper, a soft green, serif headings, roomy",
        &[
            ("--primary", "#3f6f58"),
            ("--on-primary", "#ffffff"),
            ("--bg", "#faf8f4"),
            ("--surface", "#f2eee7"),
            ("--text", "#2b2a27"),
            ("--muted", "#625d55"),
            ("--border", "#e4ddd2"),
            ("--radius", "14px"),
            (
                "--font",
                "Inter, -apple-system, \"Segoe UI\", Roboto, sans-serif",
            ),
            ("--font-display", "\"Fraunces\", Georgia, serif"),
        ],
    ),
    (
        "bold",
        "Bold: dark, big type, one loud accent, tight corners",
        &[
            ("--primary", "#ff5a36"),
            ("--on-primary", "#111111"),
            ("--bg", "#0f0f10"),
            ("--surface", "#19191b"),
            ("--text", "#f5f5f4"),
            ("--muted", "#a1a1aa"),
            ("--border", "#2a2a2e"),
            ("--radius", "6px"),
            (
                "--font",
                "Inter, -apple-system, \"Segoe UI\", Roboto, sans-serif",
            ),
            ("--text-2xl", "32px"),
            ("--text-3xl", "44px"),
        ],
    ),
    (
        "playful",
        "Playful: bright, round shapes, a friendly violet",
        &[
            ("--primary", "#6d4aff"),
            ("--on-primary", "#ffffff"),
            ("--bg", "#fff7fb"),
            ("--surface", "#ffffff"),
            ("--text", "#2d1b4e"),
            ("--muted", "#6b5a86"),
            ("--border", "#f1dcea"),
            ("--radius", "22px"),
            (
                "--font",
                "\"SF Pro Rounded\", \"Nunito\", ui-rounded, sans-serif",
            ),
        ],
    ),
    (
        "editorial",
        "Editorial: black on white, large serif headlines, sharp corners",
        &[
            ("--primary", "#111111"),
            ("--on-primary", "#ffffff"),
            ("--bg", "#ffffff"),
            ("--surface", "#f6f5f2"),
            ("--text", "#111111"),
            ("--muted", "#5a5a5a"),
            ("--border", "#e5e3de"),
            ("--radius", "2px"),
            (
                "--font",
                "Inter, -apple-system, \"Segoe UI\", Roboto, sans-serif",
            ),
            ("--font-display", "Georgia, \"Times New Roman\", serif"),
            ("--text-2xl", "34px"),
            ("--text-3xl", "48px"),
        ],
    ),
    (
        "pro",
        "Pro: dense and quiet, for dashboards and tools",
        &[
            ("--primary", "#2563eb"),
            ("--on-primary", "#ffffff"),
            ("--bg", "#ffffff"),
            ("--surface", "#f7f8fa"),
            ("--text", "#0f172a"),
            ("--muted", "#56627a"),
            ("--border", "#e5e7eb"),
            ("--radius", "6px"),
            ("--space", "4px"),
            (
                "--font",
                "Inter, -apple-system, \"Segoe UI\", Roboto, sans-serif",
            ),
            ("--text-sm", "13px"),
            ("--text-md", "14px"),
            ("--text-lg", "16px"),
            ("--text-xl", "18px"),
        ],
    ),
];

/// Whether a job's words ask for design work rather than plain code.
pub fn is_design_work(text: &str) -> bool {
    let t = text.to_lowercase();
    const WORDS: [&str; 11] = [
        "design",
        "redesign",
        "mockup",
        "mockups",
        "mock-up",
        "ui",
        "restyle",
        "styling",
        "theme",
        "beautiful",
        "gorgeous",
    ];
    const PHRASES: [&str; 8] = [
        "landing page",
        "look better",
        "looks better",
        "look nicer",
        "make it look",
        "dark mode",
        "look and feel",
        "polish the ui",
    ];
    t.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .any(|w| WORDS.contains(&w))
        || PHRASES.iter().any(|p| t.contains(p))
}

/// The theme for `mood`, with `accent` (a hex colour) as the primary when given: CSS
/// variables, and the same as a Tailwind v4 theme.
pub fn theme(mood: &str, accent: &str) -> Result<String, String> {
    let key = mood.trim().to_lowercase();
    let Some((name, about, sets)) = MOODS.iter().find(|(m, _, _)| *m == key) else {
        let names: Vec<&str> = MOODS.iter().map(|(m, _, _)| *m).collect();
        return Err(format!(
            "No mood \"{mood}\". Pick one of: {}.",
            names.join(", ")
        ));
    };
    let mut css = model::TOKENS_CSS.to_string();
    for (var, value) in sets.iter() {
        css = model::set_var(&css, var, value);
    }
    let accent = accent.trim();
    if !accent.is_empty() {
        if tokens::color(accent).is_none() {
            return Err(format!(
                "\"{accent}\" isn't a colour; give a hex such as #3f6f58."
            ));
        }
        css = model::set_var(&css, "--primary", accent);
    }
    let tw = tokens::tailwind(&tokens::read(&css));
    Ok(format!(
        "{about}. Put this in the app's theme: the CSS variables in its global stylesheet, \
         or the Tailwind block for a Tailwind v4 app (map the same values into tailwind.config \
         for v3). Keep any names the app already uses and give them these values, then use the \
         variables everywhere instead of raw colours and sizes. Load the fonts it names if the \
         app doesn't have them. The dark-mode values are a starting point to adjust.\n\n\
         Mood: {name}\n\n## CSS variables\n```css\n{css}```\n\n## Tailwind v4\n```css\n{tw}```"
    ))
}

/// Design's facts about the page, each element with a CSS selector the bot can use.
pub fn check_js() -> String {
    format!(
        r#"(() => {{
  const f = {facts};
  const SKIP = new Set(['SCRIPT', 'STYLE', 'LINK', 'META', 'TITLE', 'TEMPLATE', 'BR']);
  const kids = (el) => Array.from(el.children).filter((c) => !SKIP.has(c.tagName));
  const at = (path) => {{
    let el = document.body;
    for (const i of path) {{ el = kids(el)[i]; if (!el) return null; }}
    return el;
  }};
  const one = (el) => {{
    if (el.id) return '#' + CSS.escape(el.id);
    let s = el.tagName.toLowerCase();
    const cls = Array.from(el.classList).filter((c) => !/[:\[\]\/]/.test(c)).slice(0, 2);
    if (cls.length) s += '.' + cls.map((c) => CSS.escape(c)).join('.');
    const p = el.parentElement;
    if (p) {{
      const same = Array.from(p.children).filter((c) => c.tagName === el.tagName);
      if (same.length > 1) s += ':nth-of-type(' + (same.indexOf(el) + 1) + ')';
    }}
    return s;
  }};
  const sel = (el) => {{
    const parts = [];
    for (let e = el; e && e !== document.body && parts.length < 4; e = e.parentElement) {{
      parts.unshift(one(e));
      if (e.id) break;
    }}
    return parts.join(' > ');
  }};
  for (const it of f.items) {{ const el = at(it.path); it.sel = el ? sel(el) : null; }}
  f.url = location.href;
  return f;
}})()"#,
        facts = checks::FACTS.trim()
    )
}

/// The device a page is laid out for, from its width.
fn device_of(width: f32) -> Device {
    if width < 600. {
        Device::Phone
    } else if width < 1024. {
        Device::Tablet
    } else {
        Device::Desktop
    }
}

/// What the checks found on a page, from what `check_js` reported.
pub fn report(v: &Value) -> Result<String, String> {
    let facts: Facts = serde_json::from_value(v.clone())
        .map_err(|e| format!("couldn't read the page's facts: {e}"))?;
    let sels: HashMap<Vec<usize>, String> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let path: Vec<usize> = serde_json::from_value(it["path"].clone()).ok()?;
            Some((path, it["sel"].as_str()?.to_string()))
        })
        .collect();
    let device = device_of(facts.width);
    // A real app has no Design screens to link to: `data-goto` isn't checked.
    let found: Vec<_> = checks::review(&facts, device, &Default::default())
        .into_iter()
        .filter(|f| f.kind != Kind::DeadLink)
        .collect();
    let url = v["url"].as_str().unwrap_or_default();
    let head = format!(
        "Checked {} at {} px ({}).",
        if url.is_empty() { "the page" } else { url },
        facts.width.round(),
        device.label().to_lowercase()
    );
    if found.is_empty() {
        return Ok(format!(
            "{head} Nothing found: contrast, text sizes, tap targets, alt text, overflow and \
             theme use all pass. Look at a screenshot too; the checks can't judge taste."
        ));
    }
    let serious = found.iter().filter(|f| f.kind.serious()).count();
    let mut out = format!(
        "{head} {} found, {serious} of them stop people using it:\n",
        found.len()
    );
    for f in &found {
        let at = f
            .path
            .as_ref()
            .and_then(|p| sels.get(p))
            .map(|s| format!(" (`{s}`)"))
            .unwrap_or_default();
        out.push_str(&format!("- {}{at}\n", f.text));
    }
    out.push_str("Fix them with the app's theme values, then run design_check again.");
    Ok(out)
}

/// The tools, as MCP lists them.
pub fn check_tool() -> Value {
    json!({
        "name": CHECK,
        "description": "Run design checks on the page open in the job's browser (or open url first): text contrast, small text, tap targets, images without alt text, sideways overflow and colours set outside the theme. Returns each problem with a CSS selector. Use it on every screen you design or restyle, then fix what it finds.",
        "inputSchema": {
            "type": "object",
            "properties": { "url": { "type": "string", "description": "A page to open first, like http://localhost:5173/pricing. Leave out to check the page open now." } }
        }
    })
}

pub fn theme_tool() -> Value {
    let moods: Vec<&str> = MOODS.iter().map(|(m, _, _)| *m).collect();
    json!({
        "name": THEME,
        "description": "A complete starting theme from a mood: colours (light and dark), fonts, type scale, radii, spacing and shadows, as CSS variables and as a Tailwind v4 theme, for the app's theme. Use it when an app has no theme yet or the job asks for a new look.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "mood": { "type": "string", "enum": moods, "description": "calm, bold, playful, editorial or pro." },
                "accent": { "type": "string", "description": "A hex colour to use as the primary instead of the mood's own." }
            },
            "required": ["mood"]
        }
    })
}

fn text(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }] })
}

fn failure(t: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": true })
}

/// Carries out `design_theme`.
pub fn call_theme(args: &Value) -> Value {
    let s = |k: &str| args[k].as_str().unwrap_or_default();
    match theme(s("mood"), s("accent")) {
        Ok(t) => text(t),
        Err(e) => failure(e),
    }
}

/// Carries out `design_check` with the job's browser in `dir`.
pub fn call_check(dir: &str, args: &Value) -> Value {
    let url = args["url"].as_str().unwrap_or_default().trim().to_string();
    let js = check_js();
    let got = super::bot::with(dir, |d| {
        if !url.is_empty() {
            d.open(&url)?;
        }
        d.eval_value(&js)
    });
    match got.and_then(|v| report(&v)) {
        Ok(t) => text(t),
        Err(e) => failure(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mood_is_readable() {
        for (mood, _, _) in MOODS {
            let css = theme(mood, "").unwrap();
            let var = |n: &str| {
                let line = css
                    .lines()
                    .find(|l| l.trim_start().starts_with(&format!("{n}:")))
                    .unwrap();
                let v = line.split(':').nth(1).unwrap().trim().trim_end_matches(';');
                tokens::color(v).unwrap()
            };
            let c = |a: [u8; 4], b: [u8; 4]| checks::contrast_of(a, b);
            assert!(c(var("--text"), var("--bg")) >= 7., "{mood} text");
            assert!(c(var("--muted"), var("--bg")) >= 4.5, "{mood} muted on bg");
            assert!(
                c(var("--muted"), var("--surface")) >= 4.5,
                "{mood} muted on surface"
            );
            assert!(
                c(var("--on-primary"), var("--primary")) >= 4.5,
                "{mood} button text"
            );
        }
    }

    #[test]
    fn a_theme_comes_as_css_and_tailwind() {
        let t = theme("Calm", "").unwrap();
        assert!(t.contains("--primary: #3f6f58;"));
        assert!(t.contains("@theme inline"));
        assert!(t.contains("--color-primary: var(--primary);"));
        let t = theme("pro", "#0d9488").unwrap();
        assert!(t.contains("--primary: #0d9488;"));
        assert!(theme("gloomy", "").unwrap_err().contains("calm, bold"));
        assert!(theme("calm", "greenish").is_err());
    }

    #[test]
    fn design_work_is_told_from_code_work() {
        assert!(is_design_work("Design the booking app for the studio"));
        assert!(is_design_work("Make the pricing page look better"));
        assert!(is_design_work("Add dark mode"));
        assert!(!is_design_work("Fix the flaky login test"));
        assert!(!is_design_work("Bump the payment SDK"));
    }

    #[test]
    fn findings_name_their_element_by_selector() {
        let page = json!({
            "width": 390., "scroll": 390., "url": "http://localhost:5173/",
            "items": [
                { "path": [0, 1], "label": "p.hint", "sel": "main > p.hint",
                  "rect": { "x": 0., "y": 0., "w": 200., "h": 20. },
                  "text": "Faint words", "color": "rgb(200, 200, 200)",
                  "background": "rgb(255, 255, 255)", "size": 14., "weight": 400 },
                { "path": [0, 2], "label": "button", "sel": "main > button",
                  "rect": { "x": 0., "y": 40., "w": 30., "h": 30. },
                  "click": true, "color": "rgb(0, 0, 0)" },
                { "path": [0, 3], "label": "a", "sel": "main > a",
                  "rect": { "x": 0., "y": 80., "w": 200., "h": 48. },
                  "click": true, "goto": "nowhere", "color": "rgb(0, 0, 0)" }
            ]
        });
        let r = report(&page).unwrap();
        assert!(r.contains("390 px (phone)"), "{r}");
        assert!(r.contains("contrast"), "{r}");
        assert!(r.contains("(`main > p.hint`)"), "{r}");
        assert!(r.contains("small target"), "{r}");
        // Links between Design's screens mean nothing in a real app.
        assert!(!r.contains("isn't there"), "{r}");
        let clean = json!({ "width": 1280., "scroll": 1280., "items": [] });
        assert!(report(&clean).unwrap().contains("Nothing found"));
    }

    #[test]
    fn the_check_script_carries_design_facts() {
        let js = check_js();
        assert!(js.contains("getComputedStyle"));
        assert!(js.contains("it.sel"));
    }
}
