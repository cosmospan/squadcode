//! A made-up store for looking at every screen without running anything:
//! `RUNO_V2_DEMO=1`. Jobs in every state, a race, a pull request and a crew
//! bigger than the sidebar shows. It is never saved.

use super::data::{Made, Member, Mode};
use super::goal::{Goal, State, Step};
use super::store::{now, Changed, Folder, Item, Job, Said, Status, Store};
use crate::proof::CheckRun;

fn tool(label: &str, kind: &str, state: u8) -> Item {
    Item::Tool {
        id: label.into(),
        label: label.into(),
        kind: kind.into(),
        state,
    }
}

fn change(path: &str, status: &str, added: usize, removed: usize) -> Changed {
    Changed {
        path: path.into(),
        status: status.into(),
        added,
        removed,
    }
}

fn check(command: &str, ok: bool, secs: u64) -> CheckRun {
    CheckRun {
        command: command.into(),
        ok,
        secs,
        tail: if ok {
            String::new()
        } else {
            "test checkout::apple_pay_total ... FAILED\nexpected 1999, got 2000".into()
        },
        ..Default::default()
    }
}

pub fn seed() -> Store {
    let t = now();
    let folder = Folder {
        name: "shop".into(),
        path: std::env::temp_dir().to_string_lossy().into_owned(),
        ship: String::new(),
    };
    let base = |id: u64, bot: Member, mode: Mode, title: &str, ago: u64, secs: u64| Job {
        id,
        bot,
        mode,
        title: title.into(),
        folder: folder.clone(),
        engine: "Claude Code \u{b7} Sonnet 4.5".into(),
        agent: "claude".into(),
        started: t - ago,
        ended: Some(t - ago + secs),
        branch: format!("squad/demo-{id}"),
        base: "main".into(),
        seen: true,
        summary: format!("{secs}s \u{b7} 9 turns \u{b7} $0.{:02}", 10 + id * 3),
        ..Default::default()
    };
    let words = "Added Apple Pay as a payment option on the checkout page. It shows only on devices that support it, and the total matches the card form. Try it in Safari on a Mac with a card in Wallet.";

    let mut jobs = Vec::new();

    // Shipped a while ago.
    let mut j = base(
        1,
        Member::Fixer,
        Mode::Fix,
        "Fix the coupon field on mobile",
        86_000,
        140,
    );
    j.status = Status::Shipped;
    j.shipped = "Merged into main as 3f2a1c9.".into();
    j.changes = vec![change("src/coupon.tsx", "M", 12, 4)];
    j.feed = vec![Item::Text(
        "The coupon field was hidden by the keyboard. It now scrolls into view.".into(),
    )];
    jobs.push(j);

    // Stopped with a problem.
    let mut j = base(
        2,
        Member::Keeper,
        Mode::Tidy,
        "Update the payment SDK",
        7_200,
        35,
    );
    j.status = Status::Failed;
    j.seen = false;
    j.feed = vec![
        tool("Read package.json", "read", 1),
        tool("Run npm install", "bash", 2),
        Item::Error("You've hit your usage limit for Claude Code. It resets at 4pm.".into()),
    ];
    jobs.push(j);

    // A pull request Shipper is watching.
    let mut j = base(
        3,
        Member::Builder,
        Mode::Build,
        "Add order history page",
        5_400,
        610,
    );
    j.status = Status::Shipping;
    j.pr = "https://github.com/hyperhale/shop/pull/482".into();
    j.pr_number = 482;
    j.pr_state = "OPEN".into();
    j.pr_line = "Checks running: 3 of 5 done".into();
    j.changes = vec![
        change("src/orders/page.tsx", "A", 148, 0),
        change("src/orders/api.ts", "A", 61, 0),
        change("src/nav.tsx", "M", 3, 1),
    ];
    j.checks = vec![check("npm test", true, 41), check("npm run lint", true, 9)];
    j.feed = vec![
        Item::Text("I added an order history page under Account.".into()),
        Item::By(Member::Shipper, "Approved. Opening a pull request.".into()),
        Item::By(Member::Shipper, "Squashed 4 commits into one".into()),
        Item::By(
            Member::Shipper,
            "Pushed squad/add-order-history-page".into(),
        ),
    ];
    jobs.push(j);

    // Waiting for review.
    let mut j = base(
        4,
        Member::Fixer,
        Mode::Fix,
        "Stop double charges on retry",
        1_800,
        260,
    );
    j.status = Status::Review;
    j.seen = false;
    j.changes = vec![
        change("src/checkout/pay.ts", "M", 22, 9),
        change("src/checkout/pay.test.ts", "M", 34, 0),
    ];
    j.checks = vec![check("npm test", true, 44), check("npm run lint", true, 8)];
    j.plan = vec![
        ("Find where retries charge again".into(), 2),
        ("Send an idempotency key".into(), 2),
        ("Add a test".into(), 2),
    ];
    j.feed = vec![
        Item::Note("Working on its own branch, squad/stop-double-charges-retry, so your checkout stays as it is.".into()),
        tool("Read src/checkout/pay.ts", "read", 1),
        tool("Edit src/checkout/pay.ts", "edit", 1),
        tool("Run npm test", "bash", 1),
        Item::Text("A retry after a timeout sent the charge again. Each charge now carries a key, so the payment service ignores the repeat. I added a test that retries twice and checks there's one charge.".into()),
    ];
    jobs.push(j);

    // A race on three engines.
    let engines = [
        (
            "claude",
            "Claude Code \u{b7} Sonnet 4.5",
            240,
            Status::Review,
            true,
            "0.42",
        ),
        (
            "codex",
            "Codex \u{b7} gpt-5",
            180,
            Status::Review,
            false,
            "0.19",
        ),
        ("gemini", "Gemini CLI", 0, Status::Running, true, "0"),
    ];
    for (k, (agent, engine, secs, status, ok, cost)) in engines.into_iter().enumerate() {
        let id = 5 + k as u64;
        let mut j = base(
            id,
            Member::Builder,
            Mode::Build,
            "Add Apple Pay to checkout",
            900,
            secs,
        );
        j.race = 5;
        j.pick = agent.into();
        j.agent = agent.into();
        j.engine = engine.into();
        j.status = status;
        j.seen = status != Status::Review;
        if status == Status::Running {
            j.ended = None;
            j.summary.clear();
            j.plan = vec![
                ("Read the checkout form".into(), 2),
                ("Add the Apple Pay button".into(), 1),
                ("Wire up the payment sheet".into(), 0),
                ("Test it".into(), 0),
            ];
            j.feed = vec![
                tool("Read src/checkout/form.tsx", "read", 1),
                tool("Edit src/checkout/form.tsx", "edit", 0),
            ];
        } else {
            j.summary = format!("{secs}s \u{b7} 12 turns \u{b7} ${cost}");
            j.changes = if k == 0 {
                vec![
                    change("src/checkout/form.tsx", "M", 64, 8),
                    change("src/checkout/applePay.ts", "A", 92, 0),
                    change("src/checkout/applePay.test.ts", "A", 40, 0),
                ]
            } else {
                vec![
                    change("src/checkout/form.tsx", "M", 51, 3),
                    change("src/checkout/applePay.ts", "A", 70, 0),
                ]
            };
            j.checks = vec![check("npm test", ok, 46), check("npm run lint", true, 9)];
            j.feed = vec![Item::Text(words.into())];
        }
        jobs.push(j);
    }

    // Its base moved and two files clash.
    let mut j = base(
        9,
        Member::Builder,
        Mode::Build,
        "Add gift cards",
        3_000,
        420,
    );
    j.status = Status::Review;
    j.seen = false;
    j.handoff = "This is a build job. Onboarding Builder: it's new to this project, so it'll look around the code first.".into();
    j.short = "Gift cards at checkout".into();
    j.summary = "7m · 12 turns · $0.41 · 62% from cache".into();
    j.changes = vec![
        change("src/checkout/total.ts", "M", 18, 6),
        change("src/cart/summary.tsx", "M", 25, 2),
    ];
    j.checks = vec![check("npm test", true, 40)];
    j.conflict = vec![
        "src/checkout/total.ts".into(),
        "src/cart/summary.tsx".into(),
    ];
    j.feed = vec![
        tool("Read src/checkout/total.ts", "read", 1),
        tool("Ran cat > src/cart/gift.ts <<'EOF'", "shell", 1),
        Item::Text("I'll create the files with the Write tool instead.".into()),
        tool("Ran npm test", "shell", 1),
        Item::Text("Gift cards can be added at checkout and come off the total.\n\nTo try it, run `npm run dev` and open `http://localhost:3000/checkout`. The new code is in `src/cart/gift.ts`.".into()),
        Item::Note("main moved on and src/checkout/total.ts, src/cart/summary.tsx now clash, so nothing was merged.".into()),
    ];
    jobs.push(j);

    // Stopped by you halfway.
    let mut j = base(
        10,
        Member::Keeper,
        Mode::Tidy,
        "Remove the old analytics code",
        4_000,
        95,
    );
    j.status = Status::Stopped;
    j.plan = vec![
        ("Find the old tracker".into(), 2),
        ("Remove its calls".into(), 1),
        ("Run the tests".into(), 0),
    ];
    j.feed = vec![
        tool("Search for track(", "search", 1),
        tool("Edit src/app.tsx", "edit", 1),
    ];
    jobs.push(j);

    // Working now: the last job, so RUNO_V2_PLACE=live opens it.
    let mut j = base(
        8,
        Member::Guard,
        Mode::Review,
        "Look for leaked keys before launch",
        120,
        0,
    );
    j.status = Status::Running;
    j.ended = None;
    j.summary.clear();
    j.branch.clear();
    j.plan = vec![
        ("Search the code for keys".into(), 2),
        ("Check the git history".into(), 1),
        ("Write up what to rotate".into(), 0),
    ];
    j.feed = vec![
        tool("Search for sk_live_", "search", 1),
        tool("Read .env.example", "read", 1),
        Item::Text("No live keys in the code so far. Checking the history next.".into()),
        tool("Run git log -S sk_live_", "bash", 0),
    ];
    jobs.push(j);

    let made = vec![
        Made {
            id: 1,
            name: "Pixel".into(),
            role: "Polishes the UI".into(),
            like: Member::Builder,
            look: [3, 0, 0, 0],
            outfit: "beret.none.pencil.heart".into(),
            skills: Vec::new(),
            rules: Vec::new(),
        },
        Made {
            id: 2,
            name: "Scribe".into(),
            role: "Keeps the docs in step with the code".into(),
            like: Member::Explainer,
            look: [4, 0, 0, 0],
            outfit: "headset.clipboard.pencil.tag".into(),
            skills: vec!["docs".into(), "readme".into(), "changelog".into()],
            rules: vec![
                "Run the docs build before you stop.".into(),
                "Never change code, only docs.".into(),
            ],
        },
        Made {
            id: 3,
            name: "Atlas".into(),
            role: "Keeps the database tidy".into(),
            like: Member::Keeper,
            look: [6, 0, 0, 0],
            outfit: "bandana.none.broom.gear".into(),
            skills: Vec::new(),
            rules: Vec::new(),
        },
    ];
    super::data::set_made(&made);
    // Scribe's setup, as the engine drafts one, for looking at Crew.
    let mut setups = std::collections::HashMap::new();
    setups.insert(
        "made-2".to_string(),
        super::setup::Setup {
            instructions: "# Scribe\n\nYou keep README.md, docs/ and CHANGELOG.md in step with the code.\nRead the change first, then update only what it touched.".into(),
            skill_files: vec![super::setup::SkillRef {
                name: "docs-sync".into(),
                description: "Find docs a change made stale and fix them".into(),
                path: "/demo/.squadcode/bots/made-2/skills/docs-sync/SKILL.md".into(),
            }],
            mcp: vec!["Claude Code/github".into(), "Cursor/linear".into()],
            servers: Vec::new(),
            perms: super::setup::Perms {
                install: false,
                push: false,
                ..Default::default()
            },
            reads: vec!["docs/".into(), "CONTRIBUTING.md".into()],
            memory: vec!["The docs site builds with pnpm docs:build".into()],
            drafted: true,
        },
    );
    super::setup::set_all(&setups);
    let mut chats = std::collections::HashMap::new();
    chats.insert(
        Member::Explainer,
        vec![
            Said::mine("How does checkout talk to the payment service?".into()),
            Said::note("Explainer is reading the code."),
        ],
    );
    for j in jobs.iter_mut().filter(|j| j.id == 1 || j.id == 3) {
        j.goal = 21;
        j.thread = 2;
    }
    // The rest of the work is on checkout, apart from a couple of loose asks.
    for j in jobs.iter_mut().filter(|j| j.thread == 0 && j.id % 4 != 0) {
        j.thread = 1;
    }
    let thread = |id: u64, name: &str, bots: Vec<Member>| super::store::Thread {
        id,
        name: name.into(),
        folder: folder.path.clone(),
        bots,
        archived: false,
        made: t - 100_000,
    };
    let other = |name: &str| Folder {
        name: name.into(),
        path: format!("/demo/{name}"),
        ship: String::new(),
    };
    Store {
        setups,
        threads: vec![
            thread(
                1,
                "checkout",
                vec![
                    Member::Builder,
                    Member::Guard,
                    Member::Fixer,
                    Member::Shipper,
                ],
            ),
            thread(2, "order-history", vec![Member::Builder, Member::Checker]),
            thread(3, "flaky-tests", vec![Member::Fixer]),
        ],
        thread_now: 1,
        // #order-history has news you haven't looked at; the rest are read.
        seen_at: [(0, t + 10), (1, t + 10), (2, 1), (3, t + 10)]
            .into_iter()
            .map(|(id, at)| (format!("{}#{id}", folder.path), at))
            .collect(),
        talks: vec![super::store::Talk {
            id: 1,
            folder: folder.path.clone(),
            thread: 1,
            bot: Member::Explainer,
            question: "Where does the tax get added to the total?".into(),
            answer: Some(super::store::Said::bot(
                "In src/checkout/total.ts: totalWithTax() adds it after discounts, \
                 so gift cards and coupons come off before tax is worked out.\n\n\
                 ```ts\nconst subtotal = items.sum() - discounts;\nreturn subtotal + tax(subtotal);\n```"
                    .into(),
                "Claude Code \u{b7} 9s".into(),
            )),
            at: t - 1_500,
            choices: Vec::new(),
        }],
        folders: vec![folder.clone(), other("api"), other("docs-site")],
        current: 0,
        jobs,
        chats,
        next_id: 11,
        made,
        removed: Vec::new(),
        set_up: true,
        steps: Vec::new(),
        guide_hidden: true,
        try_hidden: false,
        reactions: Default::default(),
        offered: Vec::new(),
        pinned: vec![Member::Made(2)],
        goals: vec![
            Goal {
                id: 20,
                thread: 1,
                text: "Let customers save their cart and come back to it".into(),
                folder: folder.clone(),
                state: State::Ready,
                started: t - 60,
                steps: vec![
                    Step {
                        mode: Mode::Build,
                        title: "Store carts for signed-in customers".into(),
                        ..Default::default()
                    },
                    Step {
                        mode: Mode::Build,
                        title: "Restore the saved cart on sign-in".into(),
                        ..Default::default()
                    },
                    Step {
                        mode: Mode::Test,
                        title: "Cover saving and restoring carts with tests".into(),
                        ..Default::default()
                    },
                    Step {
                        mode: Mode::Review,
                        title: "Check carts can't be read by other customers".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            Goal {
                id: 21,
                thread: 2,
                text: "Order history for customers".into(),
                folder: folder.clone(),
                state: State::Running,
                started: t - 90_000,
                steps: vec![
                    Step {
                        mode: Mode::Fix,
                        title: "Fix the coupon field on mobile".into(),
                        job: 1,
                        ..Default::default()
                    },
                    Step {
                        mode: Mode::Build,
                        title: "Add order history page".into(),
                        job: 3,
                        ..Default::default()
                    },
                    Step {
                        mode: Mode::Test,
                        title: "Test the order history page".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ],
        demo: true,
    }
}

/// Tokens for the made-up jobs, and plan windows for the engines that report
/// them, so the status bar and its popover have something to show.
pub(super) fn usage(store: &mut Store) {
    for j in &mut store.jobs {
        j.tokens = 38_000 + j.id * 61_700 % 420_000;
    }
    let now = crate::usage::now_secs();
    let window = |kind: &str, used: f32, resets: i64| crate::planlimit::PlanLimit {
        status: crate::planlimit::Status::Allowed,
        kind: kind.into(),
        resets_at: Some(now + resets),
        used: Some(used),
    };
    crate::usage::note_live("claude", &window("five_hour", 42., 2 * 3600 + 900), now);
    crate::usage::note_live("claude", &window("seven_day", 18., 4 * 86_400), now);
    crate::usage::note_live("codex", &window("five_hour", 86., 47 * 60), now);
}
