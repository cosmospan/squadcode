# Peek's eval

A fixed set of moments, each with the decisions that are right for it. Peek's
real engine reads every case a few times, and each run is scored. Run it
before and after any change to Peek's rules, so a fix for one case can't
quietly break another.

| File | What it tests |
| --- | --- |
| `front.toml` | A message the person sends (`brain/think.rs`): which bot gets the work, when to ask back, when to make a bot, analysis to Guard and not Builder, and the whole ask in a hand-off. |
| `steer.toml` | One bot's message to another (`brain/talk.rs`): pass it on, give it to a better bot, drop it, or stop the talk for the person. |
| `baseline-<engine>-<model>.json` | How many runs of each case were right, from the last accepted run. |

The runner is `src/v2/brain/eval.rs`.

## Running it

On a computer with Claude Code installed and signed in:

```sh
cargo test -p squadcode peek_eval -- --ignored --nocapture
```

It prints the score, a table by area, and every case that wasn't right every
time with what Peek did instead. `target/peek-eval/report.md` and
`results.json` keep the same.

| Setting | Default | |
| --- | --- | --- |
| `ENGINE` | `claude` | Any engine Squad Code can ask (`codex`, `grok`…), run the way the app runs it. |
| `MODEL` | `haiku` | The model Peek reads with. |
| `RUNS` | `3` | Runs per case. |
| `JOBS` | `6` | Runs at once. |
| `ONLY` | | Only the cases whose id holds this, like `analysis` or `steer.adversarial`. |
| `PEEK_EVAL_OUT` | `target/peek-eval` | Where the report goes. |
| `UPDATE_BASELINE` | | `1` writes this run's scores as the new baseline. |

The run fails when a case that was right at least 80% of the time in the
baseline is right less than half the time now, or when more than one run in
ten fails in the engine.

On GitHub, the **Peek eval** workflow runs it on pull requests that change
Peek's rules or these cases, and by hand from any branch (Actions, Peek eval,
Run workflow). It needs the `ANTHROPIC_API_KEY` repository secret, and puts
the report on the run's summary page.

## Adding a case

Add a `[[case]]` to the file it belongs in. The comment at the top of each
file lists every field. A case needs an `id` (`area.what`, where the area
groups it in the report), where it came from, the moment, and at least one
right outcome in `ok`. Use `never` for a bot that must not get it whatever
else happens, and `gap` when the case is known to fail today and something
specific fixes it.

`cargo test -p squadcode eval::` checks the cases with no engine: every case
loads, each right outcome can be reached through Peek's own parsing, and wrong
ones score wrong. A case whose right outcome can't be reached (a guard rail
always turns it into a yes-or-no question, say) fails there.

When a change to Peek's rules is meant to change the scores, run it with
`UPDATE_BASELINE=1 RUNS=5` and commit the new baseline with the change.

## Where the cases came from

- The 30 bot-to-bot scenes from #132 (300 runs on haiku, 90 on sonnet).
- The discussion scenes from #131, including making a bot.
- The #optimisation replay from #140: "all togther" after Peek's scope
  question must reach Guard with the whole ask.
- New cases for each kind of decision that had none: bugs, tests, tidying,
  questions about the code, asking back, stop and run, the guard rails,
  the Do and Ask chips, and analysis asks.
