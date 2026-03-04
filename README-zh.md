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
