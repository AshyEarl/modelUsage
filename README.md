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
