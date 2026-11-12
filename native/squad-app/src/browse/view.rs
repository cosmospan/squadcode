//! The person's side of a job's browser: the page the bot uses, streamed into Squad as
//! it changes (DevTools screencast frames, decoded off the UI thread), with clicks,
//! scrolling and keys sent back. It attaches to the same page as the bot, so both see
//! and touch one page.

use super::cdp::{self, Cdp};
use gpui_kit::RenderImage;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Connecting,
    Live,
    Closed(String),
}

#[derive(Default)]
struct Shared {
    state: Option<State>,
    frame: Option<Arc<RenderImage>>,
    /// The CSS size of the page the frame shows.
    css: (f32, f32),
    url: String,
    /// The address that couldn't be loaded, while the page is Chrome's
    /// error page for it ("This site can't be reached").
    unreachable: String,
    loading: bool,
    session: String,
    cdp: Option<Arc<Cdp>>,
}

pub struct Viewer {
    shared: Arc<Mutex<Shared>>,
    /// Bumped on every new frame or page change, so the UI knows to redraw.
    pub version: Arc<AtomicU64>,
}

/// An element picked on the page, to ask a bot to change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picked {
    pub tag: String,
    pub selector: String,
    pub text: String,
    pub html: String,
    pub url: String,
    /// Where it sits, when that helps find it: its table row or heading, and
    /// the frame or shadow DOM it's inside.
    pub where_: String,
}

impl Picked {
    fn from_json(v: &Value) -> Picked {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        Picked {
            tag: s("tag"),
            selector: s("sel"),
            text: s("text"),
            html: s("html"),
            url: s("url"),
            where_: s("where"),
        }
    }

    /// "the button \u{201c}Pay now\u{201d}", for the bar under the page.
    pub fn short(&self) -> String {
        let kind = match self.tag.as_str() {
            "a" => "link",
            "button" => "button",
            "input" | "textarea" | "select" => "field",
            "img" | "svg" => "image",
            "h1" | "h2" | "h3" | "h4" => "heading",
            "p" | "span" | "label" => "text",
            "nav" => "navigation",
            "form" => "form",
            _ => "part",
        };
        if self.text.is_empty() {
            format!("this {kind} ({})", self.selector)
        } else {
            format!("the {kind} \u{201c}{}\u{201d}", self.text)
        }
    }

    /// What the bot is told: where it is and what the person wants.
    pub fn ask(&self, words: &str) -> String {
        format!(
            "On the page {url}, change {short}{where_}: {words}\n\nThe element: `{sel}`\n```html\n{html}\n```\nFind it in the code, make the change, and check it in the job's browser.",
            url = self.url,
            short = self.short(),
            where_ = if self.where_.is_empty() {
                String::new()
            } else {
                format!(" ({})", self.where_)
            },
            words = words.trim(),
            sel = self.selector,
            html = self.html,
        )
    }
}

/// Finds and describes the element at __X__, __Y__, outlining it briefly.
const PICK_JS: &str = r##"(() => {
  let x = __X__, y = __Y__, doc = document, el = doc.elementFromPoint(x, y);
  const inside = [];
  const esc = (v) => (window.CSS && CSS.escape) ? CSS.escape(v) : String(v).replace(/[^\w-]/g, '\\$&');
  const sel = (e) => {
    const root = e.getRootNode();
    const one = (s) => { try { return root.querySelectorAll(s).length === 1; } catch (_) { return false; } };
    const parts = [];
    while (e && e.nodeType === 1) {
      const tag = e.tagName.toLowerCase();
      if (e.id && !/^\d/.test(e.id) && e.id.length < 40) {
        parts.unshift('#' + esc(e.id));
        if (one(parts.join(' > '))) break;
      } else {
        let s = tag;
        const attr = ['data-testid', 'data-test', 'data-cy', 'name', 'aria-label']
          .find((a) => e.getAttribute(a));
        if (attr) {
          s += '[' + attr + '="' + e.getAttribute(attr).replace(/"/g, '\\"').slice(0, 60) + '"]';
        } else {
          const cls = [...e.classList].filter((c) => c.length < 30 && !/^\d/.test(c)).slice(0, 2);
          if (cls.length) s += '.' + cls.map(esc).join('.');
        }
        const p = e.parentElement || (e.parentNode && e.parentNode.host ? null : e.parentNode);
        if (p && p.children) {
          const same = [...p.children].filter((c) => c.tagName === e.tagName);
          if (same.length > 1) s += ':nth-of-type(' + (same.indexOf(e) + 1) + ')';
        }
        parts.unshift(s);
        // A bare tag alone says too little to find it in the code.
        if (one(parts.join(' > ')) && (parts.length > 1 || s !== tag)) break;
      }
      if (parts.length >= 8) break;
      e = e.parentElement;
    }
    return parts.join(' > ');
  };
  // Into same-origin frames and open shadow roots, to the element really there.
  for (let i = 0; el && i < 10; i++) {
    if (el.shadowRoot) {
      const inner = el.shadowRoot.elementFromPoint(x, y);
      if (inner && inner !== el) {
        inside.push('the shadow DOM of `' + sel(el) + '`');
        el = inner;
        continue;
      }
    }
    if (el.tagName === 'IFRAME' || el.tagName === 'FRAME') {
      let d = null;
      try { d = el.contentDocument; } catch (_) {}
      if (d) {
        const r = el.getBoundingClientRect();
        x -= r.left + el.clientLeft; y -= r.top + el.clientTop;
        inside.push('the frame `' + sel(el) + '`' + (el.src ? ' (' + el.src + ')' : ''));
        doc = d; el = d.elementFromPoint(x, y);
        continue;
      }
    }
    break;
  }
  if (!el) return null;
  // A click on an icon or a word inside a button means the button.
  if (el instanceof (doc.defaultView || window).SVGElement && el.ownerSVGElement) el = el.closest('svg') || el;
  const act = el.closest && el.closest('a,button,label,summary,select,textarea,input,[role=button],[role=link],[role=tab],[role=menuitem]');
  if (act && act !== el && act.contains(el)) {
    let depth = 0;
    for (let e = el; e && e !== act; e = e.parentElement) depth++;
    if (depth <= 4) el = act;
  }
  const clean = (t) => (t || '').trim().replace(/\s+/g, ' ');
  const text = clean(el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('title') ||
    el.getAttribute('alt') || el.getAttribute('placeholder')).slice(0, 80);
  // Where it sits: the row it's in, or the nearest heading of the box around it.
  let near = '';
  const row = el.closest('tr');
  if (row && row !== el) near = 'in the table row “' + clean(row.innerText).slice(0, 80) + '”';
  for (let a = el.parentElement, i = 0; !near && a && a !== doc.body && i < 6; a = a.parentElement, i++) {
    const hs = [...a.querySelectorAll('h1,h2,h3,h4,h5,h6,[role=heading],legend,caption')]
      .filter((h) => !el.contains(h) && (h.compareDocumentPosition(el) & 4) && clean(h.innerText));
    const h = hs[hs.length - 1];
    if (h) near = 'under “' + clean(h.innerText).slice(0, 60) + '”';
  }
  const html = el.outerHTML.slice(0, 400);
  const was = el.style.outline;
  el.style.outline = '3px solid #f59e0b';
  setTimeout(() => { el.style.outline = was; }, 1600);
  const where = [near, ...inside.reverse().map((s) => 'inside ' + s)].filter(Boolean).join(', ');
  return { tag: el.tagName.toLowerCase(), sel: sel(el), text, html, url: location.href, where };
})()"##;

/// A point in the view (from its top-left) as page CSS pixels, when it's on the page.
/// The page is fitted into `area` keeping its shape, and centred.
pub fn to_page(local: (f32, f32), area: (f32, f32), css: (f32, f32)) -> Option<(f64, f64)> {
    let (pw, ph) = css;
    if pw <= 0. || ph <= 0. || area.0 <= 0. || area.1 <= 0. {
        return None;
    }
    let k = (area.0 / pw).min(area.1 / ph);
    let (ox, oy) = ((area.0 - pw * k) / 2., (area.1 - ph * k) / 2.);
    let (x, y) = ((local.0 - ox) / k, (local.1 - oy) / k);
    (x >= 0. && y >= 0. && x < pw && y < ph).then_some((x as f64, y as f64))
}

impl Viewer {
    /// Starts watching the browser at `port` (`path` is its `/devtools/browser/…`), and
    /// opens `url` there first when given.
    pub fn connect(port: u16, path: String, url: Option<String>) -> Viewer {
        let shared: Arc<Mutex<Shared>> = Arc::new(Mutex::new(Shared {
            state: Some(State::Connecting),
            ..Shared::default()
        }));
        let version = Arc::new(AtomicU64::new(0));
        let (s, v) = (shared.clone(), version.clone());
        std::thread::Builder::new()
            .name("squad-browser-view".into())
            .spawn(move || {
                if let Err(why) = watch(port, &path, url, &s, &v) {
                    let mut sh = s.lock().unwrap_or_else(|e| e.into_inner());
                    sh.state = Some(State::Closed(why));
                    v.fetch_add(1, Ordering::Relaxed);
                }
            })
            .ok();
        Viewer { shared, version }
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> State {
        let sh = self.locked();
        match (&sh.state, &sh.cdp) {
            (Some(State::Live), Some(c)) if !c.alive() => {
                State::Closed("The browser closed.".into())
            }
            (Some(s), _) => s.clone(),
            (None, _) => State::Connecting,
        }
    }

    pub fn frame(&self) -> Option<(Arc<RenderImage>, (f32, f32))> {
        let sh = self.locked();
        sh.frame.clone().map(|f| (f, sh.css))
    }

    pub fn css(&self) -> (f32, f32) {
        self.locked().css
    }

    pub fn url(&self) -> String {
        self.locked().url.clone()
    }

    pub fn loading(&self) -> bool {
        self.locked().loading
    }

    /// Some when the page is Chrome's own error page, not the app's: the
    /// address it couldn't reach (empty when Chrome didn't say), most often
    /// a local server that has since stopped.
    pub fn unreachable(&self) -> Option<String> {
        let sh = self.locked();
        is_error_page(&sh.url).then(|| sh.unreachable.clone())
    }

    fn send(&self, method: &str, params: Value) {
        let (cdp, session) = {
            let sh = self.locked();
            (sh.cdp.clone(), sh.session.clone())
        };
        if let Some(cdp) = cdp.filter(|_| !session.is_empty()) {
            cdp.send(method, params, &session);
        }
    }

    pub fn navigate(&self, url: &str) {
        self.send("Page.navigate", json!({"url": url}));
    }

    pub fn reload(&self) {
        self.send("Page.reload", json!({}));
    }

    pub fn history(&self, back: bool) {
        let js = if back {
            "history.back()"
        } else {
            "history.forward()"
        };
        self.send("Runtime.evaluate", json!({"expression": js}));
    }

    /// `kind` is mouseMoved, mousePressed or mouseReleased.
    pub fn mouse(
        &self,
        kind: &str,
        (x, y): (f64, f64),
        button: &str,
        clicks: usize,
        modifiers: u8,
    ) {
        let buttons = match (kind, button) {
            ("mousePressed", "left") => 1,
            ("mousePressed", "right") => 2,
            ("mousePressed", "middle") => 4,
            _ => 0,
        };
        self.send(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": x, "y": y, "button": button, "buttons": buttons,
                "clickCount": clicks, "modifiers": modifiers}),
        );
    }

    pub fn wheel(&self, (x, y): (f64, f64), dx: f64, dy: f64) {
        self.send(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy}),
        );
    }

    /// What's on the page at `(x, y)` (CSS pixels), described for a bot: its
    /// tag, a selector, its text and the start of its HTML. Outlines it on the
    /// page for a moment so you see what was picked. Answers on another
    /// thread.
    pub fn pick_at(&self, (x, y): (f64, f64)) -> std::sync::mpsc::Receiver<Result<Picked, String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (cdp, session) = {
            let sh = self.locked();
            (sh.cdp.clone(), sh.session.clone())
        };
        std::thread::spawn(move || {
            let got = (|| -> Result<Picked, String> {
                let cdp = cdp
                    .filter(|_| !session.is_empty())
                    .ok_or("the browser isn't ready")?;
                let js = PICK_JS
                    .replace("__X__", &x.to_string())
                    .replace("__Y__", &y.to_string());
                let r = cdp.call(
                    "Runtime.evaluate",
                    json!({"expression": js, "returnByValue": true}),
                    &session,
                )?;
                let v = &r["result"]["value"];
                if v.is_null() {
                    return Err("nothing is there".into());
                }
                Ok(Picked::from_json(v))
            })();
            let _ = tx.send(got);
        });
        rx
    }

    pub fn insert_text(&self, text: &str) {
        self.send("Input.insertText", json!({"text": text}));
    }

    /// A key with no text of its own (Enter, arrows, Backspace…), down and up.
    pub fn key(&self, k: &crate::browser_engine::KeySpec, modifiers: u8, commands: &[&str]) {
        let mut down = json!({
            "type": if k.text.is_empty() { "rawKeyDown" } else { "keyDown" },
            "key": k.key, "code": k.code, "windowsVirtualKeyCode": k.vk,
            "nativeVirtualKeyCode": k.vk, "modifiers": modifiers,
        });
        if !k.text.is_empty() {
            down["text"] = json!(k.text);
            down["unmodifiedText"] = json!(k.text);
        }
        if !commands.is_empty() {
            down["commands"] = json!(commands);
        } else if !k.command.is_empty() {
            down["commands"] = json!([k.command]);
        }
        self.send("Input.dispatchKeyEvent", down);
        self.send(
            "Input.dispatchKeyEvent",
            json!({"type": "keyUp", "key": k.key, "code": k.code,
                "windowsVirtualKeyCode": k.vk, "nativeVirtualKeyCode": k.vk, "modifiers": modifiers}),
        );
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let (cdp, session) = {
            let sh = self.locked();
            (sh.cdp.clone(), sh.session.clone())
        };
        if let Some(cdp) = cdp {
            if !session.is_empty() {
                cdp.send("Page.stopScreencast", json!({}), &session);
            }
            cdp.close();
        }
    }
}

/// Connects, attaches to the shared page and starts the stream. Events keep arriving
/// on the connection's reader thread after this returns.
fn watch(
    port: u16,
    path: &str,
    url: Option<String>,
    shared: &Arc<Mutex<Shared>>,
    version: &Arc<AtomicU64>,
) -> Result<(), String> {
    let me: Arc<Mutex<Weak<Cdp>>> = Arc::new(Mutex::new(Weak::new()));
    let (s, v, m) = (shared.clone(), version.clone(), me.clone());
    let listener = Box::new(move |method: &str, p: &Value, session: &str| {
        let lock = || s.lock().unwrap_or_else(|e| e.into_inner());
        let mine = lock().session.clone();
        let ours = !mine.is_empty() && mine == session;
        match method {
            "Page.screencastFrame" if ours => {
                if let Some(cdp) = m.lock().ok().and_then(|w| w.upgrade()) {
                    cdp.send(
                        "Page.screencastFrameAck",
                        json!({"sessionId": p["sessionId"]}),
                        session,
                    );
                }
                let Some(image) = p["data"]
                    .as_str()
                    .and_then(|d| crate::browser_engine::decode_image(d, image::ImageFormat::Jpeg))
                else {
                    return;
                };
                let css = (
                    p["metadata"]["deviceWidth"].as_f64().unwrap_or(0.) as f32,
                    p["metadata"]["deviceHeight"].as_f64().unwrap_or(0.) as f32,
                );
                let mut sh = lock();
                sh.frame = Some(image);
                sh.css = css;
                drop(sh);
                v.fetch_add(1, Ordering::Relaxed);
            }
            "Page.frameNavigated" if ours && p["frame"]["parentId"].is_null() => {
                let mut sh = lock();
                sh.url = p["frame"]["url"].as_str().unwrap_or_default().to_string();
                sh.unreachable = p["frame"]["unreachableUrl"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                drop(sh);
                v.fetch_add(1, Ordering::Relaxed);
            }
            "Page.navigatedWithinDocument" if ours => {
                lock().url = p["url"].as_str().unwrap_or_default().to_string();
                v.fetch_add(1, Ordering::Relaxed);
            }
            "Page.frameStartedLoading" if ours => {
                lock().loading = true;
                v.fetch_add(1, Ordering::Relaxed);
            }
            "Page.loadEventFired" | "Page.frameStoppedLoading" if ours => {
                lock().loading = false;
                v.fetch_add(1, Ordering::Relaxed);
            }
            "Target.detachedFromTarget" if !mine.is_empty() && p["sessionId"] == mine.as_str() => {
                lock().state = Some(State::Closed("The page closed.".into()));
                v.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    });
    let cdp = Cdp::connect(port, path, listener)?;
    if let Ok(mut w) = me.lock() {
        *w = Arc::downgrade(&cdp);
    }
    let target = cdp::the_page(&cdp)?;
    let info = cdp.call("Target.getTargetInfo", json!({"targetId": target}), "")?;
    let session = cdp::attach(&cdp, &target)?;
    {
        let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
        sh.session = session.clone();
        sh.url = info["targetInfo"]["url"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        sh.cdp = Some(cdp.clone());
    }
    cdp.call("Page.enable", json!({}), &session)?;
    // Where the page really is: the target's address stays the one asked
    // for even when Chrome shows its error page for it.
    if let Ok(t) = cdp.call("Page.getFrameTree", json!({}), &session) {
        let f = &t["frameTree"]["frame"];
        let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(url) = f["url"].as_str() {
            sh.url = url.to_string();
        }
        sh.unreachable = f["unreachableUrl"].as_str().unwrap_or_default().to_string();
    }
    cdp.send(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": crate::browser_engine::IN_PLACE_JS}),
        &session,
    );
    cdp.send(
        "Emulation.setFocusEmulationEnabled",
        json!({"enabled": true}),
        &session,
    );
    cdp::fit(&cdp, &session);
    if let Some(url) = url {
        cdp.send("Page.navigate", json!({"url": url}), &session);
    }
    cdp.call(
        "Page.startScreencast",
        json!({"format": "jpeg", "quality": 75,
            "maxWidth": super::host::WIDTH * 2, "maxHeight": super::host::HEIGHT * 2}),
        &session,
    )?;
    shared.lock().unwrap_or_else(|e| e.into_inner()).state = Some(State::Live);
    version.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Chrome's own page for an address it couldn't load.
pub fn is_error_page(url: &str) -> bool {
    url.starts_with("chrome-error://")
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    #[test]
    fn a_picked_element_reads_plainly() {
        let p = Picked::from_json(
            &json!({"tag": "button", "sel": "form > button.pay", "text": "Pay now", "html": "<button class=\"pay\">Pay now</button>", "url": "http://localhost:3000/cart"}),
        );
        assert_eq!(p.short(), "the button \u{201c}Pay now\u{201d}");
        let ask = p.ask("make it green");
        assert!(
            ask.contains("http://localhost:3000/cart")
                && ask.contains("make it green")
                && ask.contains("form > button.pay")
        );
    }

    #[test]
    fn where_it_sits_goes_into_the_ask() {
        let p = Picked::from_json(
            &json!({"tag": "button", "sel": "button.cta", "text": "Claim offer",
            "where": "inside the shadow DOM of `main > promo-banner`"}),
        );
        assert!(p
            .ask("make it bigger")
            .contains("the button \u{201c}Claim offer\u{201d} (inside the shadow DOM of `main > promo-banner`): make it bigger"));
    }

    #[test]
    fn the_pick_script_has_its_placeholders() {
        assert!(PICK_JS.contains("__X__") && PICK_JS.contains("__Y__"));
        assert!(PICK_JS.contains("shadowRoot") && PICK_JS.contains("contentDocument"));
    }
}

#[cfg(test)]
mod tests {
    use super::to_page;

    #[test]
    fn points_land_on_the_fitted_page() {
        // A 1280×800 page in a 640×800 area: half size, 200 px above and below.
        assert_eq!(
            to_page((0., 200.), (640., 800.), (1280., 800.)),
            Some((0., 0.))
        );
        assert_eq!(
            to_page((320., 400.), (640., 800.), (1280., 800.)),
            Some((640., 400.))
        );
        assert_eq!(to_page((320., 100.), (640., 800.), (1280., 800.)), None);
        assert_eq!(to_page((1., 1.), (640., 800.), (0., 0.)), None);
    }
}

/// The bot and a viewer on one real page. Needs a Chromium-based browser and a free
/// port, so it runs only when asked: `cargo test -p squadcode shared_page -- --ignored`.
#[cfg(all(test, unix))]
mod live {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(what: &str, ok: impl Fn() -> bool) {
        let start = Instant::now();
        while !ok() {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    #[ignore]
    fn shared_page() {
        let home = std::env::temp_dir().join(format!("squad-view-{}", std::process::id()));
        std::env::set_var("SQUADCODE_HOME", &home);
        let dir = home.join("job");
        std::fs::create_dir_all(&dir).unwrap();
        let page = dir.join("index.html");
        std::fs::write(
            &page,
            "<title>T</title><input id=q placeholder=Search><p id=o></p>\
             <script>q.oninput=()=>o.textContent='typed '+q.value</script>",
        )
        .unwrap();
        let d = dir.to_str().unwrap();
        let url = format!("file://{}", page.display());
        let opened = crate::browse::bot::with(d, |b| b.open(&url)).unwrap();
        assert!(opened.contains("[1] textbox \"Search\""), "{opened}");
        let e = crate::browse::host::ensure(d).unwrap();
        let v = Viewer::connect(e.port, e.path.clone(), None);
        wait("live", || v.state() == State::Live);
        wait("a frame", || v.frame().is_some());
        assert_eq!(v.css(), (1280., 800.));
        assert!(v.url().ends_with("index.html"));
        // The person clicks the field and types; the bot sees it.
        let at = crate::browse::bot::with(d, |b| b.eval_js(
            "JSON.stringify((r => [r.x + r.width / 2, r.y + r.height / 2])(q.getBoundingClientRect()))",
        ))
        .unwrap();
        let xy: Vec<f64> = serde_json::from_str(at.trim()).unwrap();
        v.mouse("mousePressed", (xy[0], xy[1]), "left", 1, 0);
        v.mouse("mouseReleased", (xy[0], xy[1]), "left", 1, 0);
        std::thread::sleep(Duration::from_millis(200));
        v.insert_text("hi");
        wait("the bot sees it", || {
            crate::browse::bot::with(d, |b| b.look("typed", "", 1000))
                .is_ok_and(|t| t.contains("typed hi"))
        });
        drop(v);
        crate::browse::host::close(d);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A little app on a port of its own, answering until it's dropped.
    fn app() -> (String, std::sync::Arc<std::sync::atomic::AtomicBool>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://localhost:{}/", l.local_addr().unwrap().port());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s = stop.clone();
        std::thread::spawn(move || {
            for c in l.incoming() {
                if s.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut c) = c else { continue };
                let mut buf = [0u8; 2048];
                let _ = c.read(&mut buf);
                let body = "<title>App</title><h1>Hello</h1>";
                let _ = write!(
                    c,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, stop)
    }

    /// Stops `app`: its port refuses from now on.
    fn stop(url: &str, flag: &std::sync::atomic::AtomicBool) {
        flag.store(true, Ordering::Relaxed);
        // Wake the accept loop so it lets go of the port.
        let _ =
            std::net::TcpStream::connect(url.trim_start_matches("http://").trim_end_matches('/'));
        std::thread::sleep(Duration::from_millis(100));
    }

    #[test]
    #[ignore]
    fn says_which_address_it_couldnt_reach() {
        let home = std::env::temp_dir().join(format!("squad-dead-{}", std::process::id()));
        std::env::set_var("SQUADCODE_HOME", &home);
        let dir = home.join("job");
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_str().unwrap();
        // The bot tests the app, stops it, and the page is reloaded after:
        // all before you look.
        let (first, flag) = app();
        crate::browse::bot::with(d, |b| b.open(&first)).unwrap();
        stop(&first, &flag);
        let _ = crate::browse::bot::with(d, |b| b.eval_js("location.reload()"));
        std::thread::sleep(Duration::from_millis(500));
        let e = crate::browse::host::ensure(d).unwrap();
        let v = Viewer::connect(e.port, e.path.clone(), None);
        wait("live", || v.state() == State::Live);
        assert!(is_error_page(&v.url()), "{}", v.url());
        assert_eq!(v.unreachable().as_deref(), Some(first.as_str()));
        // And one that stops answering while you watch.
        let (next, flag) = app();
        v.navigate(&next);
        wait("the app", || v.url() == next);
        assert_eq!(v.unreachable(), None);
        stop(&next, &flag);
        v.reload();
        wait("the next one", || {
            v.unreachable().as_deref() == Some(next.as_str())
        });
        drop(v);
        crate::browse::host::close(d);
        let _ = std::fs::remove_dir_all(&home);
    }
}
