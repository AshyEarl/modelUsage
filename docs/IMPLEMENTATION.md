# Implementation Notes

This document keeps technical details out of the main README.

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

The source of truth is:

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

Runtime behavior:

1. load bundled official pricing
2. write it into local pricing cache when needed
3. prefer the newer local cached pricing file

## Output semantics

- By default, only the latest 30 days are shown, based on the latest day present in the logs.
- `--all` is the explicit switch for full history.

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

- Claude logs often have no stable reasoning field; the column is hidden when all values are zero.
- Codex-only reports hide `Cache Write` because Codex logs have no stable cache-write field.
- Codex `Input` is non-cached input (aligned with `ccusage-codex`).
- Codex `Total Tokens` is `Input + Output + Cache Read`.
