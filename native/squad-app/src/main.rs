mod accent;
mod agent;
mod agent_pick;
mod agentcmds;
mod alerts;
mod applog;
mod approvals;
// The diagram and design editors stay for the bots' design tool, which will give
// them a place in the Squad Code window; until then nothing opens them.
#[allow(dead_code)]
mod archdraw;
mod architecture;
mod askbar;
mod browse;
mod browser;
mod browser_engine;
mod caps;
mod compat;
mod connections;
mod copies;
mod cove;
mod coveserver;
mod crew;
mod design;
#[allow(dead_code)]
mod designdraw;
mod desktop_notify;
mod detach;
mod dock;
mod files;
mod framestats;
mod gemini;
mod git;
mod github;
mod hardware;
mod harness;
mod health;
mod icons;
mod install;
#[cfg(target_os = "macos")]
mod macfile;
mod mcp;
mod memory_mcp;
mod menubar;
mod modelcheck;
mod models;
mod motion;
mod pattern;
mod pbfile;
mod pbrunner;
mod persist;
mod phone;
mod planlimit;
mod playbooks;
mod proof;
mod prwatch;
mod pty;
mod radar;
mod registry;
mod runwatch;
mod safefs;
mod schedule;
mod seatbelt;
mod settings;
mod shell;
mod shellenv;
mod signin;
mod sounds;
mod startup;
mod store;
mod stream;
mod terminal;
mod themes;
mod turns;
mod updater;
mod usage;
mod v2;
mod wake;
mod workspace;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use settings::*;
use std::collections::HashMap;
use std::time::Duration;

actions!(squad, [Quit]);

/// `with_animation` that is skipped entirely when animations are switched off.
trait Animate: IntoElement + Sized + 'static {
    fn animated(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        f: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnyElement {
        if anim_on() {
            self.with_animation(id, animation, f).into_any_element()
        } else {
            self.into_any_element()
        }
    }
}
impl<T: IntoElement + 'static> Animate for T {}

/// One row of a composer menu: an optional lead, a label with an optional description
/// under it, and a check when it is the current choice.
fn menu_row(
    id: impl Into<ElementId>,
    label: String,
    desc: String,
    selected: bool,
    lead: Option<AnyElement>,
) -> Stateful<Div> {
    div()
        .id(id)
        // Rows keep their full height: a menu column with a fixed height scrolls instead
        // of squeezing them, which would draw each description over the next row.
        .flex_none()
        .min_h(px(28.))
        .px_2()
        .py(px(5.))
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(13.))
        .hover(|el| el.bg(tint(c_active())))
        .when_some(lead, |el, lead| el.child(lead))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .v_flex()
                .child(div().whitespace_normal().child(label))
                .when(!desc.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(c_muted()))
                            .max_w(px(280.))
                            .whitespace_normal()
                            .child(desc),
                    )
                }),
        )
        .when(selected, |el| {
            el.child(icon(IconName::Check, 13., c_text()))
        })
}

/// The surface of a composer menu: solid, raised and scrolling when long.
fn menu_box(key: &str, list: impl IntoElement) -> Div {
    div()
        .rounded_lg()
        .border_1()
        .border_color(divider())
        .bg(rgb(c_raised()))
        .shadow_lg()
        .occlude()
        .child(
            div()
                .id(SharedString::from(format!("popup-scroll-{key}")))
                .max_h(px(340.))
                .overflow_y_scroll()
                .child(list),
        )
}

fn icon(name: IconName, size: f32, color: u32) -> Icon {
    Icon::new(name).size(px(size)).text_color(rgb(color))
}

/// The UI typeface, bundled so the app looks the same on every platform.
/// Turn macOS font smoothing on or off for Squad Code. The renderer reads Squad Code's own
/// `AppleFontSmoothing` preference once, the first time it draws text; with smoothing on,
/// CoreGraphics thickens every glyph whose colour isn't near black, which makes grey
/// and light text look bold. Off draws text with plain antialiasing, crisp and light.
#[cfg(target_os = "macos")]
fn set_font_smoothing(on: bool) {
    use std::ffi::c_void;
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFPreferencesCurrentApplication: Ref;
        fn CFStringCreateWithCString(alloc: Ref, s: *const std::ffi::c_char, encoding: u32) -> Ref;
        fn CFNumberCreate(alloc: Ref, kind: isize, value: *const c_void) -> Ref;
        fn CFPreferencesSetAppValue(key: Ref, value: Ref, app: Ref);
        fn CFRelease(r: Ref);
    }
    const UTF8: u32 = 0x0800_0100;
    const SINT32: isize = 3;
    let level: i32 = if on { 2 } else { 0 };
    unsafe {
        let key = CFStringCreateWithCString(std::ptr::null(), c"AppleFontSmoothing".as_ptr(), UTF8);
        let value = CFNumberCreate(std::ptr::null(), SINT32, (&level as *const i32).cast());
        if !key.is_null() && !value.is_null() {
            CFPreferencesSetAppValue(key, value, kCFPreferencesCurrentApplication);
        }
        for r in [key, value] {
            if !r.is_null() {
                CFRelease(r);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_font_smoothing(_on: bool) {}

/// Show Squad Code's icon in the Dock when it runs outside an app bundle (`cargo run`), where
/// macOS would otherwise show a generic executable icon. A bundled Squad Code.app already has it.
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use std::ffi::{c_char, c_void};
    type Id = *mut c_void;
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Id;
        fn objc_msgSend();
    }
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}
    static PNG: &[u8] = include_bytes!("../assets/icon/png/squadcode-512.png");
    unsafe {
        let send0: unsafe extern "C" fn(Id, Id) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let send1: unsafe extern "C" fn(Id, Id, Id) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let send_bytes: unsafe extern "C" fn(Id, Id, *const c_void, usize) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let data = send_bytes(
            objc_getClass(c"NSData".as_ptr()),
            sel_registerName(c"dataWithBytes:length:".as_ptr()),
            PNG.as_ptr().cast(),
            PNG.len(),
        );
        if data.is_null() {
            return;
        }
        let image = send0(
            objc_getClass(c"NSImage".as_ptr()),
            sel_registerName(c"alloc".as_ptr()),
        );
        let image = send1(image, sel_registerName(c"initWithData:".as_ptr()), data);
        if image.is_null() {
            return;
        }
        let app = send0(
            objc_getClass(c"NSApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        send1(
            app,
            sel_registerName(c"setApplicationIconImage:".as_ptr()),
            image,
        );
        send0(image, sel_registerName(c"release".as_ptr()));
    }
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {}

fn load_fonts(cx: &mut App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Regular.otf")),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.otf")),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.otf")),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Bold.otf")),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

fn main() {
    // ssh asking for a server's password while the app sets that server up
    // (see v2::remote): answer with the password it was given, and nothing else.
    if let Ok(password) = std::env::var("SQUAD_ASKPASS") {
        println!("{password}");
        return;
    }
    // `--version` and `--help` answer and quit, before anything that could open a
    // window: on a server without a display a window would just hang.
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("Squad Code {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            println!(
                "Squad Code {}\n\nUsage: SquadCode [v2 <command> | --version]\n\nWith no arguments it opens the app. `SquadCode v2 help` lists the crew's terminal commands.",
                env!("CARGO_PKG_VERSION")
            );
            return;
        }
        _ => {}
    }
    startup::begin();
    // `SquadCode --mcp-memory <folder> [--thread <id>]`: serve the memory tools to an agent,
    // without a window; a Crew thread also gets the tools it reports through.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--mcp-memory") {
        let thread = (args.get(3).map(String::as_str) == Some("--thread"))
            .then(|| args.get(4).and_then(|id| id.parse().ok()))
            .flatten();
        // The key that answers to its permission requests are signed with.
        if let Some(i) = args.iter().position(|a| a == approvals::KEY_FILE_ARG) {
            approvals::load_key_file(args.get(i + 1).map(String::as_str).unwrap_or(""));
        }
        memory_mcp::serve(args.get(2).map(String::as_str).unwrap_or("."), thread);
        return;
    }
    // Every other mode may start agents or answer them: give it this launch's approval
    // key, and keep Squad Code's data folder private.
    store::adopt_runo_data();
    approvals::init_key();
    store::tighten_data_dir();
    // `SquadCode --agent-host squadcode-turn <turn folder> <agent> <args…>`: a live turn's agent,
    // detached from the window (see harness::host).
    // `SquadCode v2 <command>`: the v2 crew from a terminal (`SquadCode help`).
    if args.get(1).map(String::as_str) == Some("v2") {
        std::process::exit(v2::cli::main(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("--agent-host") {
        std::process::exit(harness::host::main(&args[2..]));
    }
    // `SquadCode --check-agents [--report <file>] [--since <file>]`: how the installed agent
    // CLIs match the manifest, for the nightly compatibility check (see compat).
    if args.get(1).map(String::as_str) == Some("--check-agents") {
        std::process::exit(compat::main(&args[2..]));
    }
    // `--run-due-playbooks`: the background job the old Runo window set up for
    // playbooks, which are gone. Take the job away, quietly.
    if args.get(1).map(String::as_str) == Some("--run-due-playbooks") {
        pbrunner::remove_job();
        std::process::exit(0);
    }
    // Find agents and Node.js where the person's terminal would. The login shell answers
    // on a thread while the window opens; whatever starts a program waits for it.
    shellenv::start();
    applog::init();
    detach::enable(store::data_dir());
    // Before settings or sessions are read, and so before either is saved again.
    store::keep_before_version_change(&store::data_dir(), env!("CARGO_PKG_VERSION"));
    let app = application().with_assets(icons::Icons);
    startup::mark("gpui application");
    // Clicking the Dock icon with the window closed (macOS keeps Squad Code running) opens it again.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            v2::open_window(cx);
        }
    });
    app.run(|cx| {
        startup::mark("app running");
        init(cx);
        startup::mark("components");
        load_fonts(cx);
        startup::mark("fonts");
        let settings = AppSettings::load();
        settings.apply();
        startup::mark("settings");
        // Before any text is drawn: the renderer reads this once.
        set_font_smoothing(settings.font_smoothing);
        set_dock_icon();
        startup::mark("dock icon");
        cx.set_global(settings);
        cx.on_action(quit);
        startup::mark("keys");
        desktop_notify::install(cx);
        menubar::install(cx);
        startup::mark("notifications, menu bar");
        cx.set_menus(app_menus());
        startup::mark("menus");
        v2::open_window(cx);
        startup::mark("window opened");
    });
}

/// Quit from the shortcut or the menu. With the menu bar item on, quitting hides
/// Squad Code there instead and the crew keeps working.
fn quit(_: &Quit, cx: &mut App) {
    if menubar::keeps_running(cx) {
        menubar::to_background(cx);
        return;
    }
    cx.quit();
}

/// The menu bar: the app menu with Quit.
fn app_menus() -> Vec<Menu> {
    vec![Menu::new("Squad Code").items([MenuItem::action("Quit Squad Code", Quit)])]
}
