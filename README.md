<div align="center">

<img src="docs/images/icon.png" width="132" alt="Squad Code app icon: Peek, the crew lead, peeking over a terminal, in graphite and lime" />

<h1>Squad Code</h1>

<h3>Slack for your coding bots. Talk to the crew in threads; they do the work.</h3>

<p>
Each project is a workspace, each piece of work is a <b>#thread</b>, and the bots are your teammates.<br />
Say what you want in a sentence. <b>Peek</b>, the crew lead, answers or plans it and hands each job to the right bot.<br />
Bots work on the coding agent you already use (Claude Code, Codex, Gemini and nine more), each job on its own branch,<br />
and you approve, send it back or throw it away right in the thread.
</p>

<p>
<a href="https://github.com/cosmospan/squadcode/releases"><img src="https://img.shields.io/badge/Download-macOS_·_Windows_·_Linux-000000?style=for-the-badge&logo=github&logoColor=white" alt="Download for macOS, Windows and Linux" /></a>
&nbsp;
<a href="#run-from-source"><img src="https://img.shields.io/badge/Build_from_source-b7410e?style=for-the-badge&logo=rust&logoColor=white" alt="Build from source" /></a>
&nbsp;
<a href="#run-the-crew-on-a-server"><img src="https://img.shields.io/badge/Run_on_a_server-5b5bd6?style=for-the-badge&logo=linux&logoColor=white" alt="Run the crew on a server" /></a>
</p>

<p>
<a href="https://github.com/cosmospan/squadcode/actions/workflows/ci.yml"><img src="https://github.com/cosmospan/squadcode/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
<a href="https://github.com/cosmospan/squadcode/releases"><img src="https://img.shields.io/github/v/release/cosmospan/squadcode?include_prereleases&label=release&color=5b5bd6" alt="Latest release" /></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
<img src="https://img.shields.io/badge/built%20with-pure%20Rust-b7410e?logo=rust&logoColor=white" alt="Pure Rust" />
<img src="https://img.shields.io/badge/no-Electron-47848f?logo=electron&logoColor=white" alt="No Electron" />
<img src="https://img.shields.io/badge/engines-12_supported-2ea44f" alt="12 engines supported" />
<img src="https://img.shields.io/badge/status-alpha-orange" alt="Status: alpha" />
</p>

<p>
<a href="#how-it-works"><b>How it works</b></a> ·
<a href="#a-tour"><b>Tour</b></a> ·
<a href="#run-your-app-from-the-sidebar"><b>Run app</b></a> ·
<a href="#a-terminal-in-every-project"><b>Terminal</b></a> ·
<a href="#the-crew"><b>The crew</b></a> ·
<a href="#peek-squads-own-intelligence"><b>Peek</b></a> ·
<a href="#make-it-yours"><b>Themes</b></a> ·
<a href="#squad-in-a-terminal"><b>squad CLI</b></a> ·
<a href="#run-the-crew-on-a-server"><b>Server</b></a> ·
<a href="#getting-started"><b>Get started</b></a>
</p>

<br />

<img src="docs/images/thread.png" alt="Squad Code's main window: the icon rail on the far left, the project switcher and Run app at the head of the sidebar with the project's threads and bots under them, and the #checkout thread with a race of three tries, Peek's four-step plan card ready to start and a job queued to run later" width="920" />

</div>

<br />

> [!NOTE]
> **Squad Code 2.0 is in alpha** (`v2.0.0-alpha.12`). Expect rough edges, and please [open an issue](https://github.com/cosmospan/squadcode/issues) when you hit one. See [what's missing](#known-gaps) before you download.

## How it works

If you've used Slack, you already know your way around:

| Slack | Squad Code |
| --- | --- |
| The workspace switcher | **Projects**: one per folder, each with its own emblem, switched from the top of the sidebar |
| Channels | **#threads**: one per piece of work, like `#checkout` or `#flaky-tests` |
| Teammates and DMs | **Bots**: Builder, Fixer, Guard and the rest, each one a message away |
| The person who runs the channel | **Peek**, the crew lead, in every thread |

```mermaid
flowchart LR
    You(["You, in a #thread"]) --> Peek["Peek<br/>answers, or plans and picks the bot"]
    Peek --> Bot["A bot<br/>Builder, Fixer, Guard…"]
    Bot -- "runs on" --> Engine["Your coding agent<br/>Claude Code, Codex, Gemini…"]
    Engine --> WT[("Its own branch<br/>and worktree")]
    WT --> Checker["Checker<br/>runs your checks"]
    Checker --> Review{"You review<br/>in the thread"}
    Review -- "Approve" --> Shipper["Shipper<br/>merges or opens the PR"]
    Review -- "Send back" --> Bot
    Review -- "Discard" --> Gone(["Thrown away"])
```

- **Just talk.** Type in a thread the way you'd message a teammate. Peek decides whether to answer, ask back, or turn it into jobs. `@` brings a bot in, `#` points at another thread, and `/` has the rest: `/plan`, `/race`, `/ask`, `/thread`, `/rename`, `/archive`.
- **One job per bot per thread.** Every job belongs to the bot you asked (or the one Peek picked when you didn't name one), and that bot does its own kind of work. Until you approve its job, whatever you say to that bot in that thread goes into the same job, on the same branch. After that, your next ask starts a new one. Each bot also keeps a short notebook of its recent jobs in each project.
- **Plans before work.** For anything bigger than one step, Peek posts a plan card: the steps and the bot for each. Nothing starts until you press its **Start** button.
- **Your checkout is never touched.** Every job that changes files gets its own branch (`squad/<slug>`) and worktree, so several jobs run side by side without stepping on each other.
- **Proven before you look.** When a job finishes, Checker runs the project's checks in its worktree, and the job card shows the result next to the diff.
- **Bring your own agents.** Squad Code drives the agent CLIs you already have installed and signed in. It never sells or proxies tokens, and every bot can run on a different engine and model.
- **Native and light.** Pure Rust on GPUI, with no Electron and no web view. The crew animates at 60 frames a second without getting in the way of the work.

## A tour

### Threads: where the work is talked about

<img src="docs/images/thread.png" alt="The #checkout thread: three tries of one job raced side by side with Peek's pick, then Peek's plan card with four steps for Builder, Checker and Guard and Discard, Edit plan and Start 4 jobs, and a receipt email job waiting until Builder is free" width="920" />

Each thread is a chat with the crew about one piece of work. Your asks and the bots' replies read like a conversation, grouped by who said them, with clock times and reactions. Every job arrives as a live card in the thread: its progress while it runs, then its branch, its change and its checks, with the buttons it needs right there. A job that just finished folds down to a single line once you've dealt with it.

The rail on the far left holds the places you go: **Inbox**, **Overview**, **Jobs** and **Crew**, with the night shift, today's usage, your machines and **Settings** at its foot. The sidebar starts with the [project switcher](#projects-each-with-its-own-emblem) and [Run app](#run-your-app-from-the-sidebar), then lists the project's threads (with unread counts) and a direct message with every bot. Close the right side and the conversation takes the whole width. The status bar along the bottom shows where work runs, the [terminal](#a-terminal-in-every-project), every engine in play today, how much of its plan is left and what the crew has spent.

### Projects, each with its own emblem

<table>
<tr>
<td width="50%"><img src="docs/images/switcher.png" alt="The project switcher open at the head of the sidebar: shop with 4 waiting, api and docs-site with their shortcuts, and Add a project" /></td>
<td width="50%"><img src="docs/images/emblem.png" alt="The emblem picker: a grid of shapes, a row of colours and Let Squad pick" /></td>
</tr>
</table>

The switcher at the head of the sidebar shows the project you're in: its emblem, name, folder, branch and state. Click it for every project, with what waits or runs in each, or jump straight there with ⌥⌘1–9 (Ctrl+Alt+1–9 on Windows and Linux). Every project gets its own shape and colour, chosen so no two look alike; right-click the switcher and choose **Change emblem…** to pick from 28 shapes and 8 colours, or let Squad pick. **Add a project…** sits at the end of the list.

### A job, with everything about it in one sidebar

<img src="docs/images/job.png" alt="Job #4, Stop double charges on retry by Fixer, waiting for review: the conversation with Fixer, the seatbelt holding it over a Stripe key left in pay.ts, a Clash ahead warning about job #9, and the panel beside it on Changes with the two files it touched and Discard, Send back and Approve" width="920" />

Open any job to see what the bot says and does as it happens, and keep talking to it in the box below. The panel beside it has **Changes** (the diff, where a comment on any line goes back to the bot), **Checks**, **Browser**, **Terminal** (a real shell in the job's own worktree) and **Files**, with **Discard**, **Send back** and **Approve** along the top. **Race it** tries the same job on other engines.

<table>
<tr>
<td width="50%" valign="top">

### Git, in the thread

<img src="docs/images/git.png" alt="The Git tab of a thread's right sidebar, in the dark theme: main with 2 commits to pull and 2 files changed, watch-outs for a clash between #9 and #4, uncommitted files, the crew's branches and recent commits on main" />

Every job card shows its branch, its commits and how far its base has moved. A thread's right sidebar has **Jobs**, **Git** and **Files**; the Git tab shows your folder against GitHub, what isn't committed yet, every branch the crew is working on, the latest commits on main, and the watch-outs to sort out before approving.

</td>
<td width="50%" valign="top">

### Seatbelt and clash ahead

<img src="docs/images/seatbelt.png" alt="The seatbelt card holding a job over a Stripe secret key and a leftover console.log, with Ask Fixer to remove it, above a Clash ahead card" />

Before Shipper pushes or a job is merged, the **seatbelt** checks the change for secrets, key files and leftover clash markers, and holds it with a card that says what and where. Every 45 seconds Squad test-merges the crew's work in the same project, and when two jobs change the same lines, Peek warns you on both, before either is approved.

</td>
</tr>
</table>

### Run your app from the sidebar

<img src="docs/images/run-app.png" alt="Run app turned into a pill under the project's name, showing python3 on port 8000 with an Open button, and its card open beside it: Running, localhost:8000 answering, the server's output, Copy output, Restart and Stop" width="620" />

**Run app** starts the project's app and becomes a small pill under the project's name: the command, its port, an **Open** button and a chevron. Its card (the output, **Restart** and **Stop**) opens beside the sidebar and tucks back into the pill when you go elsewhere or click outside it; a crashed app turns the pill red, and **Running now** at the bottom of the sidebar lists what's up. Before you press it, Run app says what it will run, from [`squad.toml`](#the-project-file) or from what it finds in the project, and when it can't tell, it opens **Setup** on the Overview so you can say.

### A terminal in every project

<img src="docs/images/terminal.png" alt="The terminal drawer under the #checkout thread with two shell tabs side by side: ls in one, a Python web server running in the other" width="920" />

**Ctrl+`** opens the terminal drawer: a real shell in the project's folder, or in a job's own worktree from its **Terminal** tab. **+** (or Cmd+T; Ctrl+Shift+T off a Mac) opens another shell, and each tab is named by what its shell is doing. **Side by side** (Cmd+D) shows up to three next to each other, with the one taking your keys marked in the accent, and **Fill the page** (Cmd+Shift+Enter) lets the terminal take over the page until you put it back.

<table>
<tr>
<td width="50%" valign="top">

### Run it later

<img src="docs/images/later.png" alt="The Run it later menu by the send button: Tonight at 11:00 PM, Tomorrow morning at 9:00 AM, In 2 hours, and When the plan resets, with Or type /later 2am or /later in 3h" />

The chevron by the send button runs a job later: tonight while you sleep, tomorrow morning, in two hours, or when your plan's limit resets so it runs on a full plan. Or type `/later 2am` or `/later in 3h`. Waiting jobs sit above the box until they start, and the moon on the rail is the night shift.

</td>
<td width="50%" valign="top">

### Usage at a glance

<img src="docs/images/usage.png" alt="The usage popover: $1.99 spent today, 2M tokens and 9 jobs, then Claude Code, Codex and Gemini with their spend and how much of each 5-hour and weekly limit is used" />

The thin bar at the foot of the rail is today's usage, amber at 75% of your plan and red at 90%. Click it for what each engine spent today and how much of its 5-hour and weekly limits is left.

</td>
</tr>
</table>

### A browser the bots can drive, live inside Squad

<img src="docs/images/browser.png" alt="Guard's job with its Browser tab open on a local checkout page at localhost:3000, live, with the address bar and Point at it" width="920" />

Each job gets its own headless browser, and its bot drives it with Squad's browser tools: open, look, click, type, screenshot, run JavaScript. Pages are read as a short numbered outline, and each step reports only what changed plus the page's errors, so testing a page costs a few hundred tokens instead of a screenshot every time. Open a job's **Browser** tab to watch the page live and click, scroll or type into it yourself, or **Point at it** to show the bot what you mean. `localhost` links in a job's messages open here too. Every engine gets it, through Squad's MCP server or the `squad browser` commands.

<table>
<tr>
<td width="50%" valign="top">

### Race it

<img src="docs/images/race.png" alt="A race: Add Apple Pay to checkout tried on Claude Code, Codex and Gemini side by side, each with its time, cost, change and checks, with Keep this one on the try whose checks all pass" />

Try one job on two or three engines at once (`/race`), each on its own branch, and compare them side by side: time, cost, size of change and checks. Keep the one you like; Shipper takes it from there and the others are thrown away.

</td>
<td width="50%" valign="top">

### Plans

<img src="docs/images/goal.png" alt="A plan, Let customers save their cart and come back to it, split into four jobs for Builder, Checker and Guard, each with Move up and Remove, and Discard and Start 4 jobs" />

Something too big for one job? Peek splits it into two to six jobs, each for the right bot. Remove a step, move it up or hand it to another bot, then start it: the jobs run one after another, each building on the last.

</td>
</tr>
<tr>
<td width="50%" valign="top">

### Inbox

<img src="docs/images/needs.png" alt="The Inbox: three jobs ready for review and one that stopped on the left, and on the right Builder's gift card job with a clash warning, its checks and changes, and Discard, Send back and Approve" />

Everything waiting on you, in one two-pane list you can work through from the keyboard: finished work to review, bots waiting on your yes, and jobs that stopped. A job that ends badly says what went wrong in plain words, with the one or two things that get it going again.

</td>
<td width="50%" valign="top">

### Direct messages

<img src="docs/images/dm.png" alt="A direct message with Peek, with three starter questions: What does this project do in plain words, Which bots would help most here, and What should we work on first" />

Every bot is a DM away. Ask about the code in plain words: the bot reads the folder and answers, and nothing changes unless you start a job. Each bot keeps its own conversation.

</td>
</tr>
</table>

## The crew

<img src="docs/images/crew.png" alt="The Crew page: every bot down the left with what it's doing now, and Builder's profile with its engine, its jobs, what waits for review and what shipped, and Edit, Message and Give a job" width="920" />

Every bot is a line-drawn face in its own colour, and its face follows what it's doing: listening, thinking, working, done, stuck or waiting on you.

| Bot | What it does | Kind of job |
| --- | --- | --- |
| **Peek** | Leads every crew. Answers questions, plans bigger work, hands each job to the right bot, and offers a bot your crew is missing. Always there. | Ask, plan |
| **Builder** | Writes the code: plans briefly, makes the change with tests, keeps to the project's style. | Build |
| **Fixer** | Hunts the bug: paste an error or describe it, and it finds the cause and fixes it. | Fix |
| **Guard** | Keeps your rules: reviews changes for risks to login, payments and data before they land. | Review |
| **Checker** | Tries it for real: runs the tests or drives the app in its browser, and runs the checks on every finished job. | Test |
| **Keeper** | Tidies up: dependencies, flaky tests, dead code, the chores nobody gets to. | Tidy |
| **Explainer** | Reads the code and answers in plain words. | Ask |
| **Shipper** | Owns git: squashes a job's commits, opens the pull request, watches its checks and merges when you've asked it to. | Ships what the others made |

**Make your own** with **Make a bot**: give it a name, a role, its skills and tools, and an engine. Each bot's profile has its **Brain** (the engine and model it runs on), **Skills**, **Memory** of what it learned in each project, and **Activity**.

### Engines: any bot on any agent

<img src="docs/images/engines.png" alt="Settings, Engines: the agents on this computer (Claude Code, Codex, OpenCode, Gemini and Grok) with Update and Make default, one to install, and Bot brains, which picks each bot's engine and model" width="920" />

An **engine** is one of the coding agents Squad Code drives and the model it runs on. Each bot can have its own (Builder on Claude Code, Guard on Codex, say) and the rest follow the default. Squad Code detects which agent CLIs are installed and reads their models from the CLI itself, so new releases show up without an update.

| Engine | Command | | Engine | Command |
| --- | --- | --- | --- | --- |
| Claude Code | `claude` | | Copilot | `copilot` |
| Codex | `codex` | | Amp | `amp` |
| Gemini | `gemini` | | Droid | `droid` |
| OpenCode | `opencode` | | Qwen Code | `qwen` |
| Cursor | `cursor-agent` | | Grok | `grok` |
| Antigravity | `agy` | | Pi | `pi` |

See [provider setup](docs/providers.md) for installing and signing in to each one.

## Peek: Squad's own intelligence

Peek reads every message you send with your own engine (Claude Code, Codex, Grok and the others, on your own plan) and decides: reply, ask back, plan, stop what's running, run the app, answer a question about the project, or hand the work to the bot whose role fits. When your engine can't be reached, Peek says so and starts nothing. Claude Code is kept warm in the background, so a message reads in about two seconds. When you come back to a thread, Peek posts a short catch-up of only what you missed.

Squad Code still decides some things for itself: whether a job runs here or on your server (and why, in the job's feed), when to suggest a bot you keep needing, and what Peek says. **Smart Peek**, if you turn it on, writes card titles and recaps with a small model on your computer. Turn it on in **Settings → Engines**, or with `squad brain install`.

## Make it yours

<table>
<tr>
<td width="33%"><img src="docs/images/theme-graphite-dark.png" alt="The #checkout thread in Graphite and lime, dark" /></td>
<td width="33%"><img src="docs/images/theme-harbour.png" alt="The #checkout thread in Harbour, light" /></td>
<td width="33%"><img src="docs/images/theme-orchid-dark.png" alt="The #checkout thread in Orchid, dark" /></td>
</tr>
</table>

**Settings → Appearance** has three looks: **Graphite and lime** (the default: warm paper and graphite ink, with lime for the one thing to press), **Harbour** (cool slate and navy, with periwinkle) and **Orchid** (warm sand and brown, with orchid pink). Each comes in light and dark, or follows your system as it changes. A **see-through window** lets your desktop show through, blurred, in any theme, with its own sliders for the window and for popovers, and the page can carry a background pattern or image.

<img src="docs/images/appearance.png" alt="Settings, Appearance: the Graphite and lime, Harbour and Orchid themes, Light, Dark and Match the system, the see-through window and popover switches, and the background patterns" width="920" />

**Settings → Appearance → Mascot** sets how the crew is drawn: line weight, line colour (crew colour, pastel, two-tone or rainbow) and eyes. Click any bot to dress it with a role, a prop and its own colour.

<img src="docs/images/mascot.png" alt="Settings, Mascot: the crew in a row, Guard dressed for security with a role, prop and colour, and the crew look's line weight, line colour and eyes" width="920" />

## The project file

Put a `squad.toml` at the top of a repository (`squad project init` writes one from what Squad finds) and every job sets up the same way. Every key is optional.

```toml
setup = ["pnpm install"]            # run in each job's fresh worktree first
copy = [".env.local"]               # ignored files each worktree gets a copy of
run = "pnpm dev --port $PORT"       # how to start the app
url = "http://localhost:$PORT"
port = 3100                         # each job's app gets its own port from here
checks = ["pnpm lint", "pnpm test"] # what Checker runs
```

**Run app** starts what `run` says. With no `squad.toml`, Squad works it out from the project: a `dev`, `start`, `serve` or `preview` script in `package.json` (with pnpm, yarn or bun when their lockfile is there), `deno task`, `cargo run`, Django's `manage.py`, a Python `app.py` or `main.py` (FastAPI under uvicorn, Streamlit, Flask; through uv, Poetry or the project's `.venv` when it has one), `go run .`, Flutter, Rails, Laravel, Phoenix, Rack, Spring Boot with Maven or Gradle, `dotnet run`, Hugo, Jekyll, a Procfile's `web:` line, a `dev`, `run`, `serve` or `start` target in a Makefile or justfile, Docker Compose, a Dockerfile, PHP, and last a plain `index.html` (at the top or in `public/`, `docs/` or `dist/`) served as files. When it can't tell, Run app opens Setup on the Overview so you can say.

## Squad in a terminal

Every job you can start in the window you can start from a terminal, including the one built into each job. `squad` is installed for you in `~/.squadcode/bin`.

```bash
squad do make the checkout remember the cart   # Peek picks the bot
squad fix the login redirect loops on Safari   # a Fix job, for Fixer
squad new --bot Guard look for leaked keys     # a job for one bot
squad ls                                       # jobs, newest first
squad show 12                                  # where #12 stands and what it changed
squad approve 12                               # or: squad discard 12, squad stop 12
squad brain why tidy up the old feature flags  # where Peek would send a job, and why
```

<details>
<summary><b>Every command</b></summary>
<br />

| Command | What it does |
| --- | --- |
| `squad ls` | Jobs, newest first |
| `squad show <id>` | One job: where it stands and what it changed |
| `squad new <words…>` | A Build job in this folder |
| `squad fix\|test\|tidy\|review <words…>` | A job of that kind |
| `squad new --bot <name> <words…>` | A job for one bot |
| `squad do <words…>` | Your engine picks the bot |
| `squad approve <id>` / `discard <id>` / `stop <id>` | Approve a job waiting for review, throw its changes away, or stop it |
| `squad later [<when> <words…>\|cancel <id>]` | Start a job later (tonight, 2am, in 2h, tomorrow at 7am), or list and cancel waiting ones |
| `squad rewind <id> [n]` | Put a job back to before your message n |
| `squad issue <ref>` / `squad linear key <key>` | Read a GitHub or Linear issue, or save a Linear key |
| `squad lessons [forget <bot> <n>]` | What the bots learned from your reviews here |
| `squad project [init]` | Show this project's `squad.toml`, or write a starting one |
| `squad serve [--listen <address>\|tailscale]` | Run the crew here with no window ([below](#run-the-crew-on-a-server)) |
| `squad pair [name]` / `devices` / `unpair <name>` | Pair a device with the server, list them, or cut one off |
| `squad brain why <words…>` | Where Peek would send a job, and why |
| `squad brain ideas` | A bot Peek thinks you could use |
| `squad brain install [file]` / `remove` | Turn Smart Peek on with Squad's own model, or off and delete it |

Jobs run in the Squad Code app while it's open, or in `squad serve`.

</details>

## Run the crew on a server

`squad serve` runs the same crew with no window, on a VPS or any machine that stays on, so jobs keep going while your laptop is shut.

```bash
squad serve                      # listens on 127.0.0.1:7640 and prints a pairing code
squad serve --listen tailscale   # or listen on this machine's Tailscale address
squad pair laptop                # a one-time code for another device
```

- **Nothing is opened to the internet.** It listens on `127.0.0.1:7640` unless told otherwise; reach it over an SSH tunnel or Tailscale.
- **Every device pairs.** `squad pair` prints a one-time code (8 characters, 10 minutes, 5 tries) that a device trades for its own token, and `squad unpair` revokes one.
- **One API for every client.** A small HTTP API under `/v1/` covers state, jobs and their actions, chats, goals, folders and devices, and `GET /v1/events` streams every change as server-sent events, so a job started from one device shows up live on all of them. `squad new` and the other commands on the server feed the same crew.
- **Peek knows where work belongs.** When you have a server, Peek decides per job whether it runs here or there, by where the project lives, where the agent is signed in and how long the work is, and says why in the job's feed.

Connecting the desktop app and the phone to a server is coming next; today you drive a server with `squad` on it or through the API.

## Getting started

### Download

Builds are on the [Releases](https://github.com/cosmospan/squadcode/releases) page:

| Platform | File |
| --- | --- |
| Mac (Apple silicon) | `SquadCode-<version>-macos-arm64.dmg` or `.zip` |
| Mac (Intel) | `SquadCode-<version>-macos-x86_64.dmg` or `.zip` |
| Linux x86_64 and arm64 | `.deb` or `.tar.gz` |
| Windows x86_64 | `.zip` with `SquadCode.exe` |

On a Mac, open the disk image and drag **Squad Code** to Applications. On Linux, install the `.deb` or unpack the tarball; on Windows, unzip and run `SquadCode.exe`. Squad Code checks for newer versions and offers to download them; nothing installs on its own.

### Known gaps

> [!WARNING]
> - **The Mac app is signed ad hoc, not notarized.** The first time, right-click **Squad Code** in Applications and choose **Open**.
> - **The Windows build is unsigned.** SmartScreen warns the first time; choose **More info → Run anyway**.
> - **Smart Peek's model has no public download yet.** The built-in classifier and rules work without it; to try Smart Peek now, point `squad brain install <file.gguf>` at a model you have, or use one you serve with llama-server or Ollama.
> - **The desktop app can't connect to `squad serve` yet.** See [Run the crew on a server](#run-the-crew-on-a-server).

### First run

<img src="docs/images/welcome.png" alt="Welcome to Squad Code: step one of five, Pick a look, with the Graphite and lime, Harbour and Orchid themes, Light, Dark and Match the system, and Continue" width="920" />

A short welcome walks you through five steps: pick a look, check which coding agents are installed, choose a folder, meet the crew, and say how work runs (how far bots go before asking you, where jobs run and how they ship). Every step can be skipped. The folder becomes your first project, with a `#general` thread and Peek ready to talk. Until there's work, the page shows what to do first.

### Requirements

- macOS on Apple silicon or Intel (Linux and Windows builds are made too, but macOS is the primary platform)
- `git`
- At least one agent CLI from [the table above](#engines-any-bot-on-any-agent), installed and signed in
- Optional: the [GitHub CLI](https://cli.github.com) (`gh`), for Shipper's pull requests
- To build from source: a recent stable [Rust toolchain](https://rustup.rs) (pinned in `rust-toolchain.toml`)

### Run from source

```bash
git clone https://github.com/cosmospan/squadcode
cd squadcode
cargo run -p squadcode --release
```

Set `SQUADCODE_DEMO=1` to open the app on a made-up crew with threads and jobs in every state, without running anything or saving anything. That's how the screenshots in this README were taken; `SQUADCODE_PLACE` opens it on a given screen (`crew`, `inbox`, `race`, `switcher`, `later-open` and more).

### Build the app

To build `Squad Code.app` and a disk image with its icon, use [cargo-bundle](https://github.com/burtonageo/cargo-bundle):

```bash
cargo install cargo-bundle
cd native/squad-app && cargo bundle --release
```

On Linux the same command makes a `.deb`, and on Windows the icon is built into `SquadCode.exe`. To publish a release, see [docs/releasing.md](docs/releasing.md).

### Where Squad Code keeps its data

Everything lives in `~/.squadcode`: the crew's folders, jobs and chats in `v2.json`, settings in `settings.json`, logs and crash reports in `logs/`, and the `squad` command in `bin/`. The first time Squad Code opens it copies what Runo had saved in `~/.runo`. Set `SQUADCODE_HOME` to move it all.

## Learn more

- 🔌 **[Provider setup](docs/providers.md)**: installing and signing in to each engine.
- 🧠 **[Training Squad's own model](training/README.md)**: the test sets, data and scripts behind Smart Peek.
- 📝 **[Changelog](CHANGELOG.md)**: everything that changed, as it lands.
- 🚀 **[Releasing](docs/releasing.md)**: how a release is built and signed.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) to get set up, and [SECURITY.md](SECURITY.md) to report a vulnerability privately.

## License

[MIT](LICENSE)

<br />

<div align="center">
<img src="docs/images/icon.png" width="40" alt="" />
<br />
<sub>Built in Rust for people who'd rather hand out the jobs than juggle the agents.</sub>
</div>
