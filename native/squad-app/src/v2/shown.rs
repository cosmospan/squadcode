//! Pictures a bot shows in its job's feed: a screenshot, a mockup or a diagram, or two
//! to four directions to pick from. The bot calls `show_picture` or `show_directions` on
//! Squad Code's MCP server, which copies the pictures into Squad Code's own folder and
//! leaves a note for the job. The job picks the notes up as feed items while it runs.

use super::store::{Direction, Item};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const PICTURE: &str = "show_picture";
pub(crate) const DIRECTIONS: &str = "show_directions";
/// Every tool this module serves.
pub(crate) const TOOLS: [&str; 2] = [PICTURE, DIRECTIONS];

/// The largest picture a bot may show.
const MAX_BYTES: u64 = 20 * 1024 * 1024;
/// How many directions one card holds at most.
const MAX_OPTIONS: usize = 4;

/// Where shown pictures and the notes about them are kept: in Squad Code's own data
/// folder, so they stay after a job's worktree is gone.
pub(crate) fn base() -> PathBuf {
    crate::store::data_dir().join("shown")
}

/// A stable name for a job's folder (FNV-1a), so the server and the app agree on it.
fn key(dir: &str) -> String {
    let dir = dir.trim_end_matches(['/', '\\']);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in dir.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn outbox(base: &Path, dir: &str) -> PathBuf {
    base.join(key(dir))
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Whether a file is a picture the feed can show.
fn is_picture(path: &Path) -> bool {
    crate::turns::is_image(path)
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// Copies the picture at `path` (absolute, or relative to the job's folder) into the
/// job's outbox and returns where the copy is.
fn keep(base: &Path, dir: &str, path: &str) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Give the picture's path.".into());
    }
    let src = match Path::new(path) {
        p if p.is_absolute() => p.to_path_buf(),
        p => Path::new(dir).join(p),
    };
    if !src.is_file() {
        return Err(format!(
            "There's no file at {path}. Save the picture first, then show it."
        ));
    }
    if !is_picture(&src) {
        return Err(format!(
            "{path} isn't a picture. Show a PNG, JPEG, GIF, WebP or SVG file."
        ));
    }
    let size = src.metadata().map(|m| m.len()).unwrap_or(0);
    if size > MAX_BYTES {
        return Err(format!("{path} is too big to show; keep it under 20 MB."));
    }
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "picture.png".into());
    let files = outbox(base, dir).join("files");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    let file = files.join(crate::design::file_name(&nanos().to_string(), &name));
    std::fs::copy(&src, &file).map_err(|e| format!("Squad Code couldn't copy {path}: {e}"))?;
    Ok(file.to_string_lossy().into_owned())
}

/// Leaves `item` for the job in `dir` to pick up.
fn leave(base: &Path, dir: &str, item: &Item) -> Result<(), String> {
    let out = outbox(base, dir);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let name = format!("{:032}-{}", nanos(), std::process::id());
    let tmp = out.join(format!("{name}.tmp"));
    let text = serde_json::to_vec(item).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out.join(format!("{name}.json"))).map_err(|e| e.to_string())
}

fn words(args: &Value, key: &str, max: usize) -> String {
    let t = args[key].as_str().unwrap_or("").trim();
    match t.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", t[..i].trim_end()),
        None => t.to_string(),
    }
}

/// The feed item for one `show_picture` call, with its picture copied.
fn picture(base: &Path, dir: &str, args: &Value) -> Result<Item, String> {
    let file = keep(base, dir, args["path"].as_str().unwrap_or(""))?;
    Ok(Item::Picture {
        file,
        title: words(args, "title", 80),
        caption: words(args, "caption", 400),
    })
}

/// The feed item for one `show_directions` call, with every picture copied.
fn directions(base: &Path, dir: &str, args: &Value) -> Result<Item, String> {
    let Some(list) = args["options"].as_array() else {
        return Err("Give options: a list of {name, path, note}.".into());
    };
    if !(2..=MAX_OPTIONS).contains(&list.len()) {
        return Err(format!(
            "Give two to {MAX_OPTIONS} directions; you gave {}.",
            list.len()
        ));
    }
    let mut options = Vec::with_capacity(list.len());
    for (i, o) in list.iter().enumerate() {
        let name = words(o, "name", 40);
        if name.is_empty() {
            return Err(format!("Direction {} has no name.", i + 1));
        }
        let file = keep(base, dir, o["path"].as_str().unwrap_or(""))?;
        options.push(Direction {
            name,
            file,
            note: words(o, "note", 200),
        });
    }
    let pick = args["recommended"]
        .as_u64()
        .map(|n| n as usize)
        .filter(|&n| n < options.len())
        .ok_or("Give recommended: the index of the direction you'd pick, from 0.")?;
    Ok(Item::Directions {
        title: words(args, "title", 80),
        options,
        pick,
        chosen: None,
    })
}

/// Answers a call to one of [`TOOLS`] from the bot working in `dir`.
pub(crate) fn call(dir: &str, name: &str, args: &Value) -> Value {
    call_in(&base(), dir, name, args)
}

fn call_in(base: &Path, dir: &str, name: &str, args: &Value) -> Value {
    let item = match name {
        PICTURE => picture(base, dir, args),
        DIRECTIONS => directions(base, dir, args),
        other => Err(format!("No tool {other}.")),
    };
    let done = item.and_then(|item| leave(base, dir, &item));
    match (done, name) {
        (Ok(()), DIRECTIONS) => text(
            "Shown to the person as a card to pick from. Keep going with the one you recommended; if they pick another, it comes as your next message.",
        ),
        (Ok(()), _) => text("Shown to the person in the job's feed."),
        (Err(e), _) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn text(t: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": t }] })
}

/// What was shown for the job working in `dir` since the last look, oldest first. Each
/// note is taken once.
pub(crate) fn take(dir: &str) -> Vec<Item> {
    take_in(&base(), dir)
}

fn take_in(base: &Path, dir: &str) -> Vec<Item> {
    let out = outbox(base, dir);
    let Ok(read) = std::fs::read_dir(&out) else {
        return Vec::new();
    };
    let mut notes: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    notes.sort();
    notes
        .into_iter()
        .filter_map(|p| {
            let item = std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice::<Item>(&b).ok());
            let _ = std::fs::remove_file(&p);
            item
        })
        .collect()
}

/// The tools as the bot sees them.
pub(crate) fn list() -> Vec<Value> {
    vec![
        json!({
            "name": PICTURE,
            "description": "Show the person a picture in this job's feed: a screenshot of a screen you built, a mockup, or an architecture diagram you drew. Save it as a file first (PNG, JPEG, GIF, WebP or SVG), then give its path. Show what you want them to look at, not every step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The picture's file, absolute or relative to the job's folder." },
                    "title": { "type": "string", "description": "A short name, like \"Home screen, phone\" or \"How the studio runs today\"." },
                    "caption": { "type": "string", "description": "A sentence or two on what to notice." }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": DIRECTIONS,
            "description": "Before designing new screens, show the person two to four directions to pick from, each a picture of the same screen in a different look, and say which you'd pick. Keep working on the one you recommend; if they pick another, it comes as your next message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "What the directions are for, like \"Home screen, phone\"." },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "A short name, like \"Sage\"." },
                                "path": { "type": "string", "description": "Its picture's file." },
                                "note": { "type": "string", "description": "One sentence on the look." }
                            },
                            "required": ["name", "path"]
                        }
                    },
                    "recommended": { "type": "integer", "description": "The index of the one you'd pick, from 0." }
                },
                "required": ["options", "recommended"]
            }
        }),
    ]
}

/// What the person's pick says to the bot.
pub(crate) fn pick_message(options: &[Direction], k: usize) -> String {
    let letter = (b'A' + k as u8) as char;
    match options.get(k) {
        Some(o) => format!(
            "Go with direction {letter}, {}. Design the rest of the screens in that look.",
            o.name
        ),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("squadcode-shown-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_picture_reaches_its_job_once() {
        let root = temp("picture");
        let (base, work) = (root.join("shown"), root.join("work"));
        std::fs::create_dir_all(work.join("shots")).unwrap();
        std::fs::write(work.join("shots/home.png"), b"png").unwrap();
        let dir = work.to_string_lossy().into_owned();
        let got = call_in(
            &base,
            &dir,
            PICTURE,
            &json!({ "path": "shots/home.png", "title": "Home", "caption": "The calm one" }),
        );
        assert!(got.get("isError").is_none(), "{got}");
        // Another job's folder sees nothing.
        assert!(take_in(&base, &root.to_string_lossy()).is_empty());
        let items = take_in(&base, &format!("{dir}/"));
        let [Item::Picture {
            file,
            title,
            caption,
        }] = items.as_slice()
        else {
            panic!("{items:?}");
        };
        assert_eq!((title.as_str(), caption.as_str()), ("Home", "The calm one"));
        // The copy stays when the job's own file goes.
        std::fs::remove_file(work.join("shots/home.png")).unwrap();
        assert_eq!(std::fs::read(file).unwrap(), b"png");
        assert!(take_in(&base, &dir).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_pictures_are_shown() {
        let root = temp("refused");
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let dir = root.to_string_lossy().into_owned();
        let base = root.join("shown");
        for args in [
            json!({ "path": "notes.txt" }),
            json!({ "path": "gone.png" }),
            json!({}),
        ] {
            let got = call_in(&base, &dir, PICTURE, &args);
            assert_eq!(got["isError"], true, "{args}");
        }
        assert!(take_in(&base, &dir).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn directions_need_two_to_four_and_a_pick() {
        let root = temp("directions");
        for n in ["a.png", "b.svg", "c.jpg"] {
            std::fs::write(root.join(n), b"img").unwrap();
        }
        let dir = root.to_string_lossy().into_owned();
        let base = root.join("shown");
        let opt = |n: &str, p: &str| json!({ "name": n, "path": p, "note": "A look" });
        let one = json!({ "options": [opt("Sage", "a.png")], "recommended": 0 });
        assert_eq!(call_in(&base, &dir, DIRECTIONS, &one)["isError"], true);
        let far =
            json!({ "options": [opt("Sage", "a.png"), opt("Studio", "b.svg")], "recommended": 2 });
        assert_eq!(call_in(&base, &dir, DIRECTIONS, &far)["isError"], true);
        let three = json!({
            "title": "Home screen",
            "options": [opt("Sage", "a.png"), opt("Studio", "b.svg"), opt("Bloom", "c.jpg")],
            "recommended": 0
        });
        assert!(call_in(&base, &dir, DIRECTIONS, &three)
            .get("isError")
            .is_none());
        let items = take_in(&base, &dir);
        let [Item::Directions {
            title,
            options,
            pick,
            chosen,
        }] = items.as_slice()
        else {
            panic!("{items:?}");
        };
        assert_eq!(
            (title.as_str(), options.len(), *pick, *chosen),
            ("Home screen", 3, 0, None)
        );
        assert_eq!(
            pick_message(options, 1),
            "Go with direction B, Studio. Design the rest of the screens in that look."
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
