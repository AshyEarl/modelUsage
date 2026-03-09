# 实现说明

本文件用于存放技术实现细节，避免主 README 过重。

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

价格来源是：

- [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)

运行时逻辑：

1. 先读项目内置的官方价格文件
2. 需要时写入本地价格缓存
3. 优先使用较新的本地价格缓存

## 输出语义

- 默认只显示“日志里最新一天往前 30 天”的数据。
- `--all` 是查看全量历史的显式开关。

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

- Claude 本地日志通常没有稳定的 reasoning 字段，所以整份报表都是 0 时会自动隐藏该列。
- Codex-only 报表会隐藏 `Cache Write`，因为 Codex 本地日志没有稳定可统计的 cache write 字段。
- Codex 的 `Input` 显示的是“非缓存输入”，和 `ccusage-codex` 对齐。
- Codex 的 `Total Tokens` 显示的是 `Input + Output + Cache Read`。
