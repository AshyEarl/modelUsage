# modelUsage

`modelUsage` 是一个 Rust CLI，用来统计本地 `Claude Code`、`Codex` 和 `GitHub Copilot CLI` 的 token / 成本使用情况。

## 示例输出

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

## 安装

在仓库内直接运行：

```bash
cd /home/ashyearl/workspace/rust/modelUsage
cargo run -- --claude
```

安装到 `~/.cargo/bin`：

```bash
cargo install --path /home/ashyearl/workspace/rust/modelUsage --force
```

也可以从 GitHub Releases 下载预编译压缩包：

- `modelUsage-linux-x86_64.tar.gz`
- `modelUsage-linux-aarch64.tar.gz`
- `modelUsage-macos-aarch64.tar.gz`
- `modelUsage-windows-x86_64.tar.gz`
- `modelUsage-windows-aarch64.tar.gz`

Linux/macOS 示例：

```bash
tar -xzf modelUsage-linux-x86_64.tar.gz
install -m 755 modelUsage ~/.local/bin/modelUsage
```

## 使用方式

```bash
modelUsage                 # 最近 30 天，Claude + Codex + Copilot
modelUsage --claude        # 只看 Claude
modelUsage --codex         # 只看 Codex
modelUsage --copilot       # 只看 Copilot CLI
modelUsage --project       # 仅按项目（cwd）汇总
modelUsage --daily --project    # 日期 -> 项目
modelUsage --project --daily    # 项目 -> 日期
modelUsage --all           # 全量历史
modelUsage --tz Asia/Shanghai   # 按 IANA 时区聚合
modelUsage --tz UTC+8           # 按 UTC 偏移快捷写法聚合
modelUsage --json          # JSON 输出
modelUsage --refresh       # 重建统计缓存
modelUsage --update        # 下载并替换当前二进制
```

`--tz` 支持：

- IANA 名称，例如 `Asia/Shanghai`
- 偏移快捷写法，例如 `UTC+8`、`utc+8`、`+08:00`、`-3:30`
- `local`（默认）
- Codex 数据源会同时包含 `~/.codex/sessions` 与 `~/.codex/archived_sessions`（目录存在时）。
- Copilot CLI 数据来自 `~/.copilot/session-state/*/events.jsonl`（需要 Copilot CLI v0.0.422+）。

## 更新行为

- 自动检查仅在交互终端中执行，且在主报表输出后执行。
- 更新检查默认 24 小时最多一次。
- `--json` 和非 TTY 场景不会执行联网检查。
- Windows 上 `--update` 当前仍需下载后手动替换二进制。
- 当报表包含 Claude 数据时，会输出 warning：上游本地日志的 `input/output` 可能低估。

## 平台支持

- Linux x86_64
- Linux arm64
- macOS arm64
- Windows x86_64
- Windows arm64

## 文档

- 英文 README：[README.md](/home/ashyearl/workspace/rust/modelUsage/README.md)
- 价格规则：[PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- 价格规则（中文）：[PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)
- 实现细节：[docs/IMPLEMENTATION-zh.md](/home/ashyearl/workspace/rust/modelUsage/docs/IMPLEMENTATION-zh.md)

## 版本说明

- 当前版本：`0.1.10`
- 版本号来源：`Cargo.toml`
- Tag 格式：`vX.Y.Z`
