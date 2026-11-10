//! Squad Code in the macOS menu bar: a glyph with the crew's count, and the
//! crew's panel under it (`v2::tray`). With the menu bar on, closing the window
//! or ⌘Q hides Squad Code to it so the crew keeps working; Quit in the panel quits.
//!
//! gpui has no tray support, so other platforms show nothing for now.

use crate::*;

/// What the menu bar shows.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Snapshot {
    /// Beside the glyph: what needs the person, else how many are working; empty for none.
    pub count: String,
    /// The glyph carries a dot while something needs the person.
    pub attention: bool,
    /// In the glyph's tooltip: "3 working · 2 need you".
    pub summary: String,
    /// No window is open.
    pub closed: bool,
}

impl Snapshot {
    /// Squad Code is running with its window closed.
    pub(crate) fn closed() -> Self {
        Self {
            summary: "No window open".into(),
            closed: true,
            ..Default::default()
        }
    }
}

/// "run `cargo test`" as ("run", Some("cargo test")); text without a code span stays whole.
pub(crate) fn split_code(what: &str) -> (String, Option<String>) {
    match what.split_once('`') {
        Some((verb, rest)) => (
            verb.trim().to_string(),
            Some(rest.trim_end_matches('`').to_string()),
        ),
        None => (what.to_string(), None),
    }
}

/// Whether ⌘Q and closing the window hide Squad Code to the menu bar instead of quitting.
pub(crate) fn keeps_running(cx: &App) -> bool {
    let st = cx.global::<AppSettings>();
    platform::SUPPORTED && st.menu_bar && st.run_in_background
}

/// Hide Squad Code to the menu bar: the window and the Dock icon go, the crew keeps
/// working. The first time, a notification says where it went.
pub(crate) fn to_background(cx: &mut App) {
    panel::close(cx);
    // Deferred: the close button and ⌘Q land here from inside the window's own update,
    // where updating that window again fails and would leave it on screen.
    if let Some(window) = crate::v2::open_handle(cx) {
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, _| platform::order_out(window));
        });
    }
    platform::set_dock_icon(false);
    if !cx.global::<AppSettings>().background_told {
        settings::update(cx, |s| s.background_told = true);
        crate::desktop_notify::send(
            cx,
            None,
            "Squad Code is still running",
            "It's in the menu bar, and your crew keeps working. Quit from there to quit.",
        );
    }
}

/// Quit for real, from the panel.
pub(crate) fn quit(cx: &mut App) {
    panel::close(cx);
    cx.quit();
}

/// What the menu bar shows now: the crew's count and dot beside the glyph.
pub(crate) fn snapshot(cx: &App) -> Snapshot {
    match crate::v2::tray::glyph(cx) {
        Some((count, attention, summary)) => Snapshot {
            count,
            attention,
            summary,
            ..Default::default()
        },
        None => Snapshot::closed(),
    }
}

/// Open the panel at the top left of the screen, with no glyph to hang
/// under: for screenshots of it.
pub(crate) fn open_panel(cx: &mut App) {
    panel::toggle(None, cx);
}

/// Close the panel, if it is open.
pub(crate) fn close_panel(cx: &mut App) {
    panel::close(cx);
}

/// The panel lost focus: it closes, as a menu would.
pub(crate) fn panel_dismissed(window: &mut Window) {
    panel::dismissed(window);
}

/// Size the panel to `width` by `height`, still hanging under the glyph.
pub(crate) fn fit_panel(window: &Window, width: f32, height: f32) {
    platform::place(window, width, height);
}

/// Show, refresh or hide the menu bar item, and open or close the panel when the glyph
/// was clicked.
fn tick(refresh: bool, cx: &mut App) {
    if !cx.global::<AppSettings>().menu_bar {
        panel::close(cx);
        platform::hide();
        return;
    }
    if refresh || !platform::shown() {
        platform::show(snapshot(cx));
    }
    if platform::take_toggle() {
        panel::toggle(platform::anchor(), cx);
    }
}

/// Start the menu bar item, once, as the app starts.
pub(crate) fn install(cx: &mut App) {
    if !platform::SUPPORTED {
        return;
    }
    // A click on the glyph is handled straight away rather than on the next tick.
    let async_cx = cx.to_async();
    platform::on_click(move || {
        let cx = async_cx.clone();
        async_cx
            .foreground_executor()
            .spawn(async move { cx.update(|cx| tick(false, cx)) })
            .detach();
    });
    // The glyph's counts, once a second (every two while the window is hidden, since the
    // menu bar still shows them), and every two seconds a look at whether it was turned
    // on while it is off.
    cx.spawn(async move |cx| loop {
        let on = cx.update(|cx| cx.global::<AppSettings>().menu_bar);
        let wait = if on {
            crate::wake::pace(Duration::from_secs(1), Duration::from_secs(2))
        } else {
            Duration::from_secs(2)
        };
        cx.background_executor().timer(wait).await;
        cx.update(|cx| tick(true, cx));
    })
    .detach();
}

#[path = "menubar_panel.rs"]
mod panel;

/// Where the glyph is on screen, in the display's own coordinates from its top left:
/// the point under its middle where the panel hangs, and the display it is on.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Anchor {
    pub x: f32,
    pub y: f32,
    pub display: Option<u32>,
}

#[cfg(target_os = "macos")]
use mac as platform;

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{Anchor, Snapshot};

    pub(super) const SUPPORTED: bool = false;

    pub(super) fn show(_: Snapshot) {}

    pub(super) fn shown() -> bool {
        false
    }

    pub(super) fn hide() {}

    pub(super) fn take_toggle() -> bool {
        false
    }

    pub(super) fn on_click(_: impl Fn() + 'static) {}

    pub(super) fn anchor() -> Option<Anchor> {
        None
    }

    pub(super) fn set_dock_icon(_: bool) {}

    pub(super) fn place(_: &gpui_kit::Window, _: f32, _: f32) {}

    pub(super) fn order_out(_: &gpui_kit::Window) {}
}

#[cfg(target_os = "macos")]
mod mac {
    //! The status item and the object its button calls. Everything here lives on the main
    //! thread, where AppKit and gpui's foreground tasks both run.

    use super::{Anchor, Snapshot};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSImage, NSScreen, NSStatusBar, NSStatusItem,
        NSVariableStatusItemLength, NSView, NSWindow,
    };
    use objc2_foundation::{NSData, NSObjectProtocol, NSPoint, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::cell::{Cell, RefCell};

    pub(super) const SUPPORTED: bool = true;

    static GLYPH: &[u8] = include_bytes!("../assets/icon/menubar/squadcode-menubar.png");
    static GLYPH_DOT: &[u8] = include_bytes!("../assets/icon/menubar/squadcode-menubar-dot.png");

    struct Bar {
        item: Retained<NSStatusItem>,
        // Kept alive: the button holds its target weakly.
        _target: Retained<Target>,
        /// What the button shows now: the count and whether it has the dot.
        button: (String, bool),
    }

    thread_local! {
        static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
        static CLICKED: Cell<bool> = const { Cell::new(false) };
        static ON_CLICK: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and Target has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SquadCodeMenuBarTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(toggle:))]
            fn toggle(&self, _sender: &AnyObject) {
                CLICKED.set(true);
                ON_CLICK.with_borrow(|f| {
                    if let Some(f) = f {
                        f();
                    }
                });
            }
        }

        unsafe impl NSObjectProtocol for Target {}
    );

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub(super) fn on_click(f: impl Fn() + 'static) {
        ON_CLICK.with_borrow_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(super) fn take_toggle() -> bool {
        CLICKED.replace(false)
    }

    pub(super) fn shown() -> bool {
        BAR.with_borrow(|b| b.is_some())
    }

    pub(super) fn show(snapshot: Snapshot) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        BAR.with_borrow_mut(|bar| {
            let bar = bar.get_or_insert_with(|| {
                let item =
                    NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
                let target = Target::new(mtm);
                if let Some(button) = item.button(mtm) {
                    unsafe {
                        button.setTarget(Some(&target));
                        button.setAction(Some(sel!(toggle:)));
                    }
                }
                Bar {
                    item,
                    _target: target,
                    button: (String::from("unset"), false),
                }
            });
            let want = (snapshot.count.clone(), snapshot.attention);
            if bar.button != want {
                if let Some(button) = bar.item.button(mtm) {
                    button.setImage(glyph(snapshot.attention).as_deref());
                    // A thin space keeps the count off the glyph.
                    let title = if want.0.is_empty() {
                        String::new()
                    } else {
                        format!("\u{2009}{}", want.0)
                    };
                    button.setTitle(&NSString::from_str(&title));
                    button.setToolTip(Some(&NSString::from_str(&format!(
                        "Squad Code: {}",
                        snapshot.summary
                    ))));
                }
                bar.button = want;
            }
        });
    }

    pub(super) fn hide() {
        if let Some(bar) = BAR.with_borrow_mut(Option::take) {
            NSStatusBar::systemStatusBar().removeStatusItem(&bar.item);
        }
        CLICKED.set(false);
    }

    /// The glyph as a template image, so macOS draws it in the menu bar's own color.
    fn glyph(dot: bool) -> Option<Retained<NSImage>> {
        let data = NSData::with_bytes(if dot { GLYPH_DOT } else { GLYPH });
        let image = NSImage::initWithData(NSImage::alloc(), &data)?;
        image.setSize(NSSize::new(18., 18.));
        image.setTemplate(true);
        Some(image)
    }

    /// The glyph's screen: its frame, and its id as gpui names displays.
    fn screen_of(button: &NSView) -> Option<(objc2_foundation::NSRect, Retained<NSScreen>)> {
        let window = button.window()?;
        let frame = window.frame();
        let screen = window.screen()?;
        Some((frame, screen))
    }

    pub(super) fn anchor() -> Option<Anchor> {
        let mtm = MainThreadMarker::new()?;
        BAR.with_borrow(|bar| {
            let button = bar.as_ref()?.item.button(mtm)?;
            let (frame, screen) = screen_of(&button)?;
            let display = screen.frame();
            let number = screen
                .deviceDescription()
                .objectForKey(&NSString::from_str("NSScreenNumber"))
                .and_then(|n| n.downcast::<objc2_foundation::NSNumber>().ok())
                .map(|n| n.unsignedIntValue());
            Some(Anchor {
                x: (frame.origin.x + frame.size.width / 2. - display.origin.x) as f32,
                y: (display.origin.y + display.size.height - frame.origin.y) as f32,
                display: number,
            })
        })
    }

    /// Hang the panel `window` under the glyph, kept on screen, and bring it up.
    pub(super) fn place(window: &gpui_kit::Window, width: f32, height: f32) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(panel) = ns_window(window) else {
            return;
        };
        let spot = BAR.with_borrow(|bar| {
            let button = bar.as_ref()?.item.button(mtm)?;
            screen_of(&button)
        });
        let Some((frame, screen)) = spot else {
            return;
        };
        let visible = screen.visibleFrame();
        let (w, h) = (width as f64, height as f64);
        let x = (frame.origin.x + frame.size.width / 2. - w / 2.)
            .min(visible.origin.x + visible.size.width - w - 8.)
            .max(visible.origin.x + 8.);
        // Sized first, so a panel that fits its content stays hung from the top.
        panel.setContentSize(NSSize::new(w, h));
        let top = frame.origin.y - 6.;
        panel.setFrameTopLeftPoint(NSPoint::new(x, top.max(visible.origin.y + h)));
        panel.makeKeyAndOrderFront(None);
    }

    /// The AppKit window behind a gpui one.
    fn ns_window(window: &gpui_kit::Window) -> Option<Retained<NSWindow>> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: gpui hands out its own content view, alive while the window is.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// Take the window off screen without closing it; showing it again brings it back.
    pub(super) fn order_out(window: &gpui_kit::Window) {
        if let Some(window) = ns_window(window) {
            window.orderOut(None);
        }
    }

    /// Show or hide Squad Code's Dock icon: hidden while it runs only in the menu bar.
    pub(super) fn set_dock_icon(shown: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(if shown {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        });
        if shown {
            // The icon comes back as the bundle's; set Squad Code's own again, and its count.
            crate::set_dock_icon();
            crate::dock::reapply();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn commands_come_out_of_their_code_span() {
        assert_eq!(
            split_code("run `cargo test -p squadcode`"),
            (
                "run".to_string(),
                Some("cargo test -p squadcode".to_string())
            )
        );
        assert_eq!(
            split_code("edit src/main.rs"),
            ("edit src/main.rs".to_string(), None)
        );
    }

    #[test]
    fn closed_offers_only_open() {
        let s = Snapshot::closed();
        assert!(s.closed && s.count.is_empty());
    }
}
