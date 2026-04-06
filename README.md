# modelUsage

`modelUsage` is a Rust CLI for local token/cost reports from `Claude Code`, `Codex`, and `GitHub Copilot CLI`.

## Example output

### `modelUsage --claude`

```text
modelUsage v0.1.7
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

### `modelUsage --codex`

```text
modelUsage v0.1.7
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

## Install

Run in this repository:

```bash
cd /home/ashyearl/workspace/rust/modelUsage
cargo run -- --claude
```

Install to `~/.cargo/bin`:

```bash
cargo install --path /home/ashyearl/workspace/rust/modelUsage --force
```

Download prebuilt archives from GitHub Releases:

- `modelUsage-linux-x86_64.tar.gz`
- `modelUsage-linux-aarch64.tar.gz`
- `modelUsage-macos-aarch64.tar.gz`
- `modelUsage-windows-x86_64.tar.gz`
- `modelUsage-windows-aarch64.tar.gz`

Linux/macOS example:

```bash
tar -xzf modelUsage-linux-x86_64.tar.gz
install -m 755 modelUsage ~/.local/bin/modelUsage
```

## Usage

```bash
modelUsage                 # latest 30 days, Claude + Codex + Copilot
modelUsage --claude        # Claude only
modelUsage --codex         # Codex only
modelUsage --copilot       # Copilot CLI only
modelUsage --project       # project summary (grouped by cwd only)
modelUsage --daily --project    # date -> project
modelUsage --project --daily    # project -> date
modelUsage --all           # full history
modelUsage --tz Asia/Shanghai   # aggregate by IANA timezone
modelUsage --tz UTC+8           # aggregate by UTC offset shortcut
modelUsage --json          # JSON output
modelUsage --refresh       # rebuild stats cache
modelUsage --update        # download and replace current binary
```

Timezone accepts:

- IANA names, e.g. `Asia/Shanghai`
- offset shortcuts, e.g. `UTC+8`, `utc+8`, `+08:00`, `-3:30`
- `local` (default)
- Codex source roots include both `~/.codex/sessions` and `~/.codex/archived_sessions` when present.
- Copilot CLI data is read from `~/.copilot/session-state/*/events.jsonl` (requires Copilot CLI v0.0.422+).

## Update behavior

- Auto-check runs only on interactive terminals after the main report.
- Checks are throttled to once per 24 hours.
- `--json` and non-TTY runs skip update checks.
- On Windows, `--update` currently requires manual binary replacement after download.
- When Claude data is present, a warning is emitted because upstream local `input/output` usage can be undercounted.

## Platform support

- Linux x86_64
- Linux arm64
- macOS arm64
- Windows x86_64
- Windows arm64

## Docs

- Chinese README: [README-zh.md](/home/ashyearl/workspace/rust/modelUsage/README-zh.md)
- Pricing rules: [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- Pricing rules (Chinese): [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)
- Implementation details: [docs/IMPLEMENTATION.md](/home/ashyearl/workspace/rust/modelUsage/docs/IMPLEMENTATION.md)

## Versioning

- Current version: `0.1.10`
- Source of truth: `Cargo.toml`
- Tag format: `vX.Y.Z`
