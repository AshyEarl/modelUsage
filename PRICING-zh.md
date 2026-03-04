# 价格维护说明

这份文档说明 `modelUsage` 的价格来源，以及以后出现新模型时应该怎么补价格。

英文版见：

- [PRICING.md](/home/ashyearl/workspace/rust/modelUsage/PRICING.md)

项目总览文档：

- [README.md](/home/ashyearl/workspace/rust/modelUsage/README.md)
- [README-zh.md](/home/ashyearl/workspace/rust/modelUsage/README-zh.md)

## 结论

`modelUsage` 不再依赖 LiteLLM 获取价格。

当前价格来源：

1. 项目内维护的官方价格文件  
   [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)
2. 本地运行时价格缓存  
   `~/.cache/modelUsage/pricing.json`

默认行为：

- 先读项目内维护的官方价格文件
- 需要时写入本地缓存
- 后续优先使用较新的本地缓存

## 为什么去掉 LiteLLM

之前的问题很直接：

- 新模型收录可能滞后
- 网络拉取失败会导致价格不完整
- token 有统计，但金额可能变成 `0` 或 partial

对这个项目来说，本地维护一份价格表更可控，也更稳定。

## 官方来源

截至 **2026-03-04**，OpenAI 价格是人工从官方页面核对后写入的：

- OpenAI Pricing  
  https://developers.openai.com/api/docs/pricing
- OpenAI Models  
  https://developers.openai.com/api/docs/models

补充说明：

- OpenAI 有模型 API，也有 usage API
- 但我没有查到稳定公开的官方价格 API
- 所以这里采用“人工核对官方页面后维护本地价格表”的方式

Anthropic / Claude 的价格也按官方公开价格维护。

## 当前已覆盖模型

### Claude

- `haiku-4-5`
- `sonnet-4-5`
- `sonnet-4-6`
- `opus-4-5`
- `opus-4-6`

### OpenAI / Codex

- `gpt-5`
- `gpt-5-codex`
- `gpt-5.1-codex`
- `gpt-5.1-codex-max`
- `gpt-5.2`
- `gpt-5.2-codex`
- `gpt-5.3-codex`

## 文件格式

价格文件是普通 JSON，以后如果你要托管到公网，这份文件可以直接发布。

结构如下：

```json
{
  "version": 1,
  "updated_at": "2026-03-04T00:00:00Z",
  "models": {
    "gpt-5.2-codex": {
      "input_cost_per_mtoken": 1.75,
      "output_cost_per_mtoken": 14.0,
      "cache_write_5m_cost_per_mtoken": null,
      "cache_write_1h_cost_per_mtoken": null,
      "cache_read_cost_per_mtoken": 0.175
    }
  }
}
```

所有 `*_cost_per_mtoken` 字段的单位都是“每百万 token 的美元价格”。

## 更新流程

以后遇到未知模型时，按这个流程处理：

1. 从 `partial cost; unpriced models: ...` 里拿到模型名
2. 去官方页面确认价格
3. 如果官方已经公开：
   - 更新 `pricing/official-pricing.json`
   - 需要时同步更新本文档
4. 如果官方还没公开：
   - 先保持 `N/A`
   - 不要猜价格，也不要偷偷映射成其他模型

## 重要规则

### 不合并 Codex 小版本

当前规则：

- 可以去 provider 前缀
- 但 `gpt-5.2`、`gpt-5.3-codex`、`gpt-5.1-codex-max` 这类小版本要保留

原因：

- 你明确要求保留真实版本
- 这样后续核价更清晰

### 未知价格不能显示成 0

如果价格缺失：

- 显示 `N/A`
- 需要时把报表标记成 partial

不要把未知价格伪装成 `$0.00`。

## 如果以后要自己托管价格文件

当前这份 JSON 已经可以直接放到一个公网 URL。

以后如果要支持“从你自己的 URL 拉价格”，建议逻辑是：

1. 保留项目内置价格文件
2. 启动时可选拉取你自己的在线 JSON
3. 拉取成功就覆盖本地缓存
4. 拉取失败继续回退到内置价格

这样你既有：

- 本地离线兜底
- 自己可控的在线更新

## 维护建议

- 每次确认价格后都更新 `updated_at`
- 只信官方价格页面
- 未确认的新模型宁可先 `N/A`
- 不要把不同版本模型偷偷合并到一起
