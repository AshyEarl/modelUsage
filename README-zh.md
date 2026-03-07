# modelUsage

`modelUsage` 是一个轻量的 Rust CLI，用来统计本地的：

- `Claude Code`
- `Codex`

它不是为了完整复刻 `ccusage`，而是为了把本地统计这件事做得更稳、更简单：

- 默认直接出日报
- 不要每次都全量扫描历史 JSONL
- 价格不要依赖不稳定的在线服务
- 后续自己维护也容易

英文版说明见：

- [README.md](/home/ashyearl/workspace/rust/modelUsage/README.md)

价格维护文档：

- [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)

## 功能

- 默认显示最近 30 天
- 支持 `--all` 查看全部历史
- 支持 `--claude`
- 支持 `--codex`
- 支持 `--json`
- 支持 `--refresh`
- 统计缓存持久化
- 价格缓存持久化

刻意不做的内容：

- 月报
- TUI
- 一堆兼容 `ccusage` 的参数

## 平台支持

当前支持目标：

- Linux x86_64
- Linux arm64
- macOS arm64

说明：

- 假设 Claude / Codex 在这些平台上的 home 目录布局保持一致
- release workflow 会为这三个目标构建并上传预编译压缩包
- 文档中的 shell 安装示例可用于 Linux 和现代 macOS 终端

## 安装

直接在项目里运行：

```bash
cd /home/ashyearl/workspace/rust/modelUsage
cargo run -- --claude
```

安装到 `~/.cargo/bin`：

```bash
cargo install --path /home/ashyearl/workspace/rust/modelUsage --force
```

安装后可直接使用：

```bash
modelUsage --claude
modelUsage --codex
modelUsage --update
```

也可以从 GitHub Releases 下载预编译二进制：

1. 打开仓库的 Releases 页面
2. 下载与你平台匹配的压缩包：
   - `modelUsage-linux-x86_64.tar.gz`
   - `modelUsage-linux-aarch64.tar.gz`
   - `modelUsage-macos-aarch64.tar.gz`
3. 解压
4. 把 `modelUsage` 放到你的 `PATH` 目录里，比如 `~/.local/bin`

示例：

```bash
tar -xzf modelUsage-linux-x86_64.tar.gz
install -m 755 modelUsage ~/.local/bin/modelUsage
```

如果你不想本地编译，也可以直接使用打 tag 后的 GitHub Actions 产物。

自动更新说明：

- 自动检查只会在交互式终端里执行，并且放在主报表输出之后
- 更新检查默认 24 小时最多执行一次
- `--json` 和非 TTY 场景不会联网检查更新
- `modelUsage --update` 会下载最新 GitHub Release，并原地替换当前二进制
- 当前更新流程使用 Rust 内置 HTTP 请求，并会为 Linux x86_64、Linux arm64、macOS arm64 自动选择匹配的压缩包
- 当前更新流程依赖系统可用的 `tar`

## 版本说明

当前 crate 版本：

- `0.1.3`

版本规则：

- `Cargo.toml` 是版本号的唯一来源
- Git tag 使用 `vX.Y.Z` 格式
- 当推送 `v*` tag 时，release workflow 会自动构建并上传 Linux x86_64、Linux arm64、macOS arm64 二进制

一次典型发版流程：

```bash
git tag v0.1.3
git push github v0.1.3
```

## 使用方式

```bash
# 默认同时看 Claude 和 Codex，只显示最近一个月
modelUsage

# 只看 Claude
modelUsage --claude

# 只看 Codex
modelUsage --codex

# 看全部历史
modelUsage --all

# 输出 JSON
modelUsage --json

# 重建统计缓存
modelUsage --refresh

# 下载并安装最新 release
modelUsage --update
```

## 示例输出

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

## 数据来源

Claude 日志：

```text
~/.claude/projects/**/*.jsonl
```

Codex 日志：

```text
~/.codex/sessions/**/*.jsonl
```

## 缓存文件

统计缓存：

```text
~/.cache/modelUsage/stats.json
```

价格缓存：

```text
~/.cache/modelUsage/pricing.json
```

## 工作方式

## 文件级增量缓存

工具会把“每个文件按天聚合后的结果”保存到 `stats.json`。

每次运行时：

1. 扫描当前 JSONL 文件列表
2. 比较 `size` 和 `mtime`
3. 没变的文件直接复用旧结果
4. 变过的文件整文件重算
5. 最后汇总成日报

这里故意只做文件级增量，不做逐行增量，主要是为了让实现更稳、更容易排查。

## 价格策略

现在不再依赖 LiteLLM。

当前价格来源是：

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

运行时逻辑：

1. 先读项目内置的官方价格文件
2. 需要时写入本地价格缓存
3. 优先使用较新的本地价格缓存

这样结果更稳定，也更容易自己维护。

## 输出语义

默认只显示“日志里最新一天往前 30 天”的数据。

如果要看全部历史：

```bash
modelUsage --all
```

### Claude / 混合报表

列如下：

- `Date`
- `Models`
- `Input`
- `Output`
- `Reasoning`
- `Cache Write`
- `Cache Read`
- `Total Tokens`
- `Cost (USD)`

### Codex-only 报表

列如下：

- `Date`
- `Models`
- `Input`
- `Output`
- `Reasoning`
- `Cache Read`
- `Total Tokens`
- `Cost (USD)`

说明：

- Claude 本地日志通常没有稳定的 reasoning 字段，所以如果整份报表都是 0，会自动隐藏该列
- Codex-only 报表会隐藏 `Cache Write`，因为 Codex 本地日志没有稳定可统计的 cache write 字段
- Codex 的 `Input` 显示的是“非缓存输入”，和 `ccusage-codex` 对齐
- Codex 的 `Total Tokens` 显示的是 `Input + Output + Cache Read`

## 解析说明

### Claude

Claude 的 JSONL 里，同一条逻辑响应可能出现多条中间态 usage 记录。

当前处理方式：

- 读取 `message.usage`
- 按 `message.id` 或 `uuid` 去重
- 保留第一次 usage，尽量和 `ccusage` 对齐

模型归一化：

- 只去日期后缀
- 保留真实版本，比如 `4-5`、`4-6`

### Codex

当前处理方式：

- 从 `turn_context.payload.model` 读取当前模型
- 优先用 `last_token_usage`
- 只有缺少 `last_token_usage` 时，才退回到 `total_token_usage - previous_total`

这是为了尽量和 `ccusage-codex` 保持同一口径。

模型归一化：

- 只去 provider 前缀
- 不合并 `gpt-5.2`、`gpt-5.3-codex` 这类小版本

## 成本计算

### Claude

Claude 按这些部分分别计费：

- input
- output
- cache write
- cache read

### Codex

Codex 按这些部分计费：

- 非缓存输入
- 缓存输入
- 输出

逻辑上可以近似理解为：

```text
non_cached_input = input - cache_read
cost = non_cached_input + cached_input + output
```

`Reasoning` 只展示，不单独重复收费。

## 项目结构

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

## 后续维护时建议先看

推荐顺序：

1. `src/main.rs`
2. `src/app.rs`
3. `src/claude.rs`
4. `src/codex.rs`
5. `src/pricing.rs`
6. `src/report.rs`

## 常见问题

## 为什么结果可能和别的工具不同？

常见原因：

- Claude 去重逻辑不同
- Codex 使用的 usage 字段不同
- 新模型还没补价格
- Input / Total Tokens 的展示口径不同

## 为什么有的模型还是 `N/A`？

因为 token 统计有了，但维护的价格表里还没有这个模型的价格。

这里故意显示 `N/A`，而不是 `$0.00`。

## 新模型价格该改哪里？

直接修改：

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

并同步更新：

- [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)
- [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)
