# modelUsage

`modelUsage` is a small Rust CLI for summarizing local usage logs from:

- `Claude Code`
- `Codex`

It is intentionally narrower than `ccusage`. The goal is to solve the practical local-reporting workflow:

- show a daily report by default
- avoid rescanning every historical JSONL file on each run
- keep pricing stable even when external services are unavailable
- stay simple enough to maintain by hand

For the Chinese version, see:

- [README-zh.md](/home/ashyearl/workspace/rust/modelUsage/README-zh.md)

Pricing maintenance documentation:

- [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)

## Features

- show the latest 30 days by default
- support `--all` for full history
- support `--claude`
- support `--codex`
- support `--json`
- support `--refresh`
- persist file-level stats cache
- persist pricing cache

Not included on purpose:

- monthly reports
- TUI
- many compatibility flags for `ccusage`

## Platform support

Current support target:

- Linux x86_64
- Linux arm64
- macOS arm64

Notes:

- Claude/Codex home-directory layouts are expected to be the same on these platforms
- the release workflow builds and uploads prebuilt archives for all three targets
- installation examples remain shell-oriented and should work on Linux and modern macOS terminals

## Install

Run directly in the project:

```bash
cd /home/ashyearl/workspace/rust/modelUsage
cargo run -- --claude
```

Install into `~/.cargo/bin`:

```bash
cargo install --path /home/ashyearl/workspace/rust/modelUsage --force
```

After installation:

```bash
modelUsage --claude
modelUsage --codex
modelUsage --update
```

Download a prebuilt binary from GitHub Releases:

1. open the repository Releases page
2. download the archive that matches your platform:
   - `modelUsage-linux-x86_64.tar.gz`
   - `modelUsage-linux-aarch64.tar.gz`
   - `modelUsage-macos-aarch64.tar.gz`
3. extract it
4. move `modelUsage` into a directory on your `PATH`, for example `~/.local/bin`

Example:

```bash
tar -xzf modelUsage-linux-x86_64.tar.gz
install -m 755 modelUsage ~/.local/bin/modelUsage
```

You can also download the latest CI artifact from a tagged release workflow run if you do not want to build locally.

Auto-update notes:

- automatic update checks run only in interactive terminals and only after the main report finishes
- checks are throttled to once every 24 hours
- `--json` and non-TTY runs never contact GitHub for update checks
- `modelUsage --update` downloads the latest GitHub release and replaces the current binary in place
- the updater uses built-in Rust HTTP requests and selects the matching archive for Linux x86_64, Linux arm64, or macOS arm64
- the updater currently expects `tar` to be available on the host system

## Versioning

Current crate version:

- `0.1.3`

Versioning rule:

- `Cargo.toml` is the source of truth for the crate version
- Git tags should use the `vX.Y.Z` format
- the release workflow builds and uploads Linux x86_64, Linux arm64, and macOS arm64 binaries when a `v*` tag is pushed

Typical release flow:

```bash
git tag v0.1.3
git push github v0.1.3
```

## Usage

```bash
# Show the latest month for both Claude and Codex
modelUsage

# Claude only
modelUsage --claude

# Codex only
modelUsage --codex

# Show full history
modelUsage --all

# Output JSON
modelUsage --json

# Rebuild the stats cache
modelUsage --refresh

# Download and install the latest release
modelUsage --update
```

## Example output

### Claude-only

```text
Daily Token Usage Report

┌────────────┬─────────────────────────────────┬────────┬─────────┬─────────────┬────────────┬──────────────┬────────────┐
│ Date       ┆ Models                          ┆ Input  ┆ Output  ┆ Cache Write ┆ Cache Read ┆ Total Tokens ┆ Cost (USD) │
╞════════════╪═════════════════════════════════╪════════╪═════════╪═════════════╪════════════╪══════════════╪════════════╡
│ 2026-02-03 ┆ haiku-4-5, opus-4-5            ┆    384 ┆     254 ┆     265,854 ┆  3,350,740 ┆    3,617,232 ┆ $2.07      │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┤
│ 2026-02-04 ┆ haiku-4-5, sonnet-4-6          ┆  1,248 ┆     912 ┆     120,440 ┆  1,852,103 ┆    1,974,703 ┆ $1.08      │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┤
│ Total      ┆                                 ┆  1,632 ┆   1,166 ┆     386,294 ┆  5,202,843 ┆    5,590,935 ┆ $3.15      │
└────────────┴─────────────────────────────────┴────────┴─────────┴─────────────┴────────────┴──────────────┴────────────┘

Total: 2 days, 5,590,935 tokens, $3.15
```

### Codex-only

```text
Daily Token Usage Report

┌────────────┬───────────────┬────────────┬───────────┬───────────┬─────────────┬──────────────┬────────────┐
│ Date       ┆ Models        ┆ Input      ┆ Output    ┆ Reasoning ┆ Cache Read  ┆ Total Tokens ┆ Cost (USD) │
╞════════════╪═══════════════╪════════════╪═══════════╪═══════════╪═════════════╪══════════════╪════════════╡
│ 2026-02-03 ┆ gpt-5.2       ┆    601,961 ┆   151,564 ┆   134,331 ┆   4,275,328 ┆    5,028,853 ┆ $11.41     │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┤
│ 2026-02-10 ┆ gpt-5.3-codex ┆  2,344,719 ┆   458,072 ┆    88,263 ┆ 112,996,096 ┆  115,798,887 ┆ $30.29     │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┤
│ 2026-03-03 ┆ gpt-5.3-codex ┆ 11,671,017 ┆   495,864 ┆   115,215 ┆ 137,844,096 ┆  150,010,977 ┆ $51.49     │
├╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌━┼╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌╌╌━━┼╌╌╌╌╌╌╌╌╌╌━━┤
│ Total      ┆               ┆ 14,617,697 ┆ 1,105,500 ┆   337,809 ┆ 255,115,520 ┆  270,838,717 ┆ $93.19     │
└────────────┴───────────────┴────────────┴───────────┴───────────┴─────────────┴──────────────┴────────────┘

Total: 3 days, 270,838,717 tokens, $93.19
```

## Data sources

Claude logs:

```text
~/.claude/projects/**/*.jsonl
```

Codex logs:

```text
~/.codex/sessions/**/*.jsonl
```

## Cache files

Stats cache:

```text
~/.cache/modelUsage/stats.json
```

Pricing cache:

```text
~/.cache/modelUsage/pricing.json
```

## How it works

## File-level incremental cache

The tool stores per-file daily aggregates in `stats.json`.

On each run:

1. scan the current JSONL file list
2. compare `size` and `mtime`
3. reuse unchanged file results
4. fully reparse changed files
5. rebuild the final daily report

This is intentionally file-level, not line-level, to keep the implementation easy to reason about.

## Pricing strategy

Pricing no longer comes from LiteLLM.

The current source of truth is:

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

Runtime behavior:

1. load bundled official pricing
2. write it into local pricing cache when needed
3. prefer the newer local cached pricing file

This keeps the output deterministic and easier to maintain.

## Output semantics

By default, only the latest 30 days are shown, based on the latest day present in the logs.

Use:

```bash
modelUsage --all
```

to show all historical dates.

### Claude / mixed reports

Columns:

- `Date`
- `Models`
- `Input`
- `Output`
- `Reasoning`
- `Cache Write`
- `Cache Read`
- `Total Tokens`
- `Cost (USD)`

### Codex-only reports

Columns:

- `Date`
- `Models`
- `Input`
- `Output`
- `Reasoning`
- `Cache Read`
- `Total Tokens`
- `Cost (USD)`

Notes:

- Claude reasoning is usually unavailable in local logs, so the reasoning column is hidden if it is all zero.
- Codex-only reports hide `Cache Write` because Codex logs do not expose a stable cache-write field.
- Codex `Input` is displayed as non-cached input, matching `ccusage-codex`.
- Codex `Total Tokens` is displayed as `Input + Output + Cache Read`.

## Parsing notes

### Claude

Claude JSONL can contain repeated intermediate assistant usage records for the same logical response.

Current behavior:

- parse `message.usage`
- deduplicate by `message.id` or `uuid`
- keep the first usage entry to match `ccusage`

Model normalization:

- strip date suffixes only
- keep real model versions such as `4-5` and `4-6`

### Codex

Current behavior:

- read the current model from `turn_context.payload.model`
- prefer `last_token_usage`
- fall back to `total_token_usage - previous_total` only when `last_token_usage` is missing

This matches `ccusage-codex` more closely.

Model normalization:

- strip provider prefixes only
- do not merge minor versions such as `gpt-5.2` or `gpt-5.3-codex`

## Cost calculation

### Claude

Claude uses separate pricing buckets for:

- input
- output
- cache write
- cache read

### Codex

Codex uses:

- non-cached input
- cached input
- output

Conceptually:

```text
non_cached_input = input - cache_read
cost = non_cached_input + cached_input + output
```

Reasoning is shown for visibility but not charged separately.

## Project layout

```text
src/
  main.rs
  cli.rs
  app.rs
  cache.rs
  claude.rs
  codex.rs
  pricing.rs
  report.rs
  table.rs
  model.rs
```

## Main files for future maintainers

Recommended reading order:

1. `src/main.rs`
2. `src/app.rs`
3. `src/claude.rs`
4. `src/codex.rs`
5. `src/pricing.rs`
6. `src/report.rs`

## Common issues

## Why can results differ from other tools?

Possible reasons:

- different Claude deduplication logic
- different Codex usage source (`last_token_usage` vs total deltas)
- missing explicit pricing for new models
- different display semantics for input and total tokens

## Why can a model still show `N/A`?

Because usage exists, but pricing is not yet present in the maintained pricing file.

Unknown pricing is intentionally shown as `N/A`, not `$0.00`.

## Where should I add new model prices?

Update:

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

And document the change in:

- [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)
