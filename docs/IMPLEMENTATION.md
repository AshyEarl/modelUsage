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

Codex multi-agent v2 rollouts replay the parent's token-count history when a child thread is
forked, and the replayed records use the child spawn time. The parser keeps session relationship
metadata and compact token fingerprints, removes only the verified common parent/child prefix,
and caches the child branch delta. v1 sub-agent logs without `forked_from_id`, and forked logs
whose parent is unavailable, are left intact.

OpenCode logs (single SQLite database, opened read-only):

```text
~/.local/share/opencode/opencode.db
```

OpenCode keeps all sessions in one SQLite file instead of per-session JSONL. Each assistant row in the `message` table already carries its own `modelID`, full token totals (`tokens.{input,output,reasoning,cache.{read,write},total}`), and the working directory (`path.cwd`), so usage is attributed per assistant message and naturally handles a session that switches models mid-conversation. The message-level totals reconcile exactly with the `session.*` token columns. The db is cached as a single "file" entry (size/mtime/parser_version).

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
- OpenCode reports are Claude-style (not codex-like): `cache_read` is additive to `input`, and `Total Tokens` is the reported `tokens.total`. OpenCode exposes a single `cache.write` with no 5m/1h split, bucketed into the 5m column. Cost comes from local pricing, so plan-profile models without a token price show `N/A`.
