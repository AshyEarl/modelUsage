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

截至 **2026-10-12**，OpenAI 文本／代码模型价格是人工从官方页面核对后写入的：

- OpenAI Pricing  
  https://developers.openai.com/api/docs/pricing
- OpenAI Models  
  https://developers.openai.com/api/docs/models

补充说明：

- OpenAI 有模型 API，也有 usage API
- 但我没有查到稳定公开的官方价格 API
- 所以这里采用“人工核对官方页面后维护本地价格表”的方式

截至 **2026-10-12**，补齐 45 个缺失的 OpenAI 文本／代码模型 ID，包括 GPT-6 Sol、GPT-6.1 Sol、GPT-6 Luna、GPT-5 mini/nano/pro 与 chat 变体、GPT-4.x，以及官方列出的历史快照。同时刷新已有 GPT-5.6 系列价格，并补充已公开的 OpenAI 缓存写入价格。

| 模型 | 输入 | 输出 | 缓存读取 | 缓存写入 |
| --- | ---: | ---: | ---: | ---: |
| `gpt-6-astra` | $10 | $50 | $1 | $12.50 |
| `gpt-6-sol` | $2 | $10 | $0.20 | $2.50 |
| `gpt-6.1-sol` | $2 | $10 | $0.10 | $2.50 |
| `gpt-6-luna` | $0.10 | $0.50 | $0.01 | $0.125 |
| `gpt-5.6` / `gpt-5.6-sol` | $4 | $20 | $0.40 | $5 |
| `gpt-5.6-terra` | $2 | $12 | $0.20 | $2.50 |
| `gpt-5.6-luna` | $0.20 | $1.20 | $0.02 | $0.25 |

以上为 Standard 短上下文档位的每百万 token 美元价格。官方公布 GPT-5.6 Sol 当前促销价至少持续到 2026 年 11 月 21 日。本地价格表也会将当前核实的价格用于历史用量，不会按用量日期选择历史价格。

OpenAI 对这些模型只公布统一的缓存写入价，没有 Claude 的 5 分钟／1 小时区分。这里将其存入 `cache_write_5m_cost_per_mtoken`，`cache_write_1h_cost_per_mtoken` 保持 `null`，计算逻辑会为任一写入桶回退到该官方价格。Codex 的 `cache_write_input_tokens`（也接受 `cache_write_tokens`）归入 5 分钟写入桶。缓存读取和写入都是 Codex 输入的子集：普通输入为 `input - cache_read - cache_write`，各桶分别按对应单价计费。累计写入计数缺失或重置时，新请求会在可用时采用单次写入计数；重复快照仍会去重。如果两种写入计数都不可用，则写入仍为零，无法仅凭 input 重建。Reasoning 已包含在 output 中，不会重复计费。参见[官方缓存计费规则](https://developers.openai.com/api/docs/guides/prompt-caching)。

截至 **2026-10-13**，已核对[官方 Standard 价格表](https://developers.openai.com/api/docs/pricing)中的长上下文价格。本地 `long_context` 配置为 GPT-5.4/Pro、GPT-5.5/Pro、GPT-5.6/Sol/Terra/Luna、GPT-6 Astra/Sol/Luna 和 GPT-6.1 Sol 记录输入／缓存 ×2、输出 ×1.5 的倍率。只有带此配置的模型才会加价。Codex 按每次请求的 `last_token_usage.input_tokens > 272000`（包含缓存输入）判断；缺失单次 usage 时回退到累计计数差分。短、长请求在日报汇总和 fork 去重中保持分开，因此不会把一天的累计输入误当成单次长上下文。若只有覆盖多个请求的累计快照，则无法重建每次请求的上下文长度。解析器版本 4 会自动使旧 Codex 文件缓存失效；同时推进价格时间戳，使新配置替换之前内置价格的缓存。

其他来源仍按短上下文价格估算。当前未建模 Batch/Flex/Fast/Ultrafast 档位、区域溢价及工具调用费。搜索模型仅计算 token 成本。音频、图像生成及按分钟计费模型不在本次范围，因为现有用量字段无法区分其计费单位。

Chat 与历史型号价格另核对了以下官方模型页：

- [GPT-5 Chat](https://developers.openai.com/api/docs/models/gpt-5-chat-latest)、[GPT-5.1 Chat](https://developers.openai.com/api/docs/models/gpt-5.1-chat-latest)、[GPT-5.2 Chat](https://developers.openai.com/api/docs/models/gpt-5.2-chat-latest)、[GPT-5.3 Chat](https://developers.openai.com/api/docs/models/gpt-5.3-chat-latest)
- [ChatGPT-4o](https://developers.openai.com/api/docs/models/chatgpt-4o-latest)、[GPT-4](https://developers.openai.com/api/docs/models/gpt-4)、[GPT-4 Turbo](https://developers.openai.com/api/docs/models/gpt-4-turbo)、[GPT-4 Turbo Preview](https://developers.openai.com/api/docs/models/gpt-4-turbo-preview)、[GPT-4.5 Preview](https://developers.openai.com/api/docs/models/gpt-4.5-preview)
- [GPT-4o Search Preview](https://developers.openai.com/api/docs/models/gpt-4o-search-preview)、[GPT-4o Mini Search Preview](https://developers.openai.com/api/docs/models/gpt-4o-mini-search-preview)

截至 **2026-06-10**，Anthropic / Claude 价格是人工从官方页面核对后写入的：

- Claude API pricing  
  https://platform.claude.com/docs/en/about-claude/pricing
- Claude plans & pricing  
  https://claude.com/pricing

截至 **2026-07-27**，新增 `opus-5`，来源为 Claude API 官方价格页。其每百万 token 单价与 `opus-4-x` 系列一致：输入 $5、输出 $25、5 分钟缓存写入 $6.25、1 小时缓存写入 $10、缓存读取 $0.50。

截至 **2026-08-02**，修正两处 Claude 价格。`sonnet-5` 更新为标准价（输入 $3、输出 $15、5 分钟缓存写入 $3.75、1 小时缓存写入 $6、缓存读取 $0.30），因为实测 Claude Code `/cost` 在 9 月 1 日前已按标准价计费。`fable-5` 1 小时缓存写入按官方价格页补齐为 $20（此前为 `N/A`）。

截至 **2026-09-23**，新增 `opus-5-5`（Claude Opus 5.5），来源为 Claude API 官方价格。其单价低于 `opus-5`：输入 $4、输出 $20、5 分钟缓存写入 $5、1 小时缓存写入 $8、缓存读取 $0.20（每百万 token）。Fast mode（$8/$40）未单独建模。

截至 **2026-10-09**，新增 `sonnet-5-5`（Claude Sonnet 5.5），来源为 Claude API 官方价格：输入 $2、输出 $10、5 分钟缓存写入 $2.50、1 小时缓存写入 $4、缓存读取 $0.10（每百万 token；该模型缓存命中为基础输入价的 0.05 倍）。

截至 **2026-11-09**，新增 `fable-5-2`（Claude Fable 5.2），来源为 Claude 官方模型页与 API 价格：输入 $8、输出 $40、5 分钟缓存写入 $10、1 小时缓存写入 $16、缓存读取 $0.20（每百万 token；该模型缓存命中为基础输入价的 0.025 倍），较 `fable-5-1` 约降价 20%。Fast mode（$16/$80）未单独建模。Claude Mythos 5.2 与其同价，但因仅邀请制可用，未单独列出。

截至 **2026-06-09**，Kimi、MiniMax 和 GLM 价格是人工从官方页面核对后写入的：

- Kimi K2.6 pricing  
  https://platform.kimi.ai/docs/pricing/chat-k26
- MiniMax M3 pricing / pay-as-you-go pricing  
  https://www.minimax.io/models/text/m3  
  https://platform.minimaxi.com/docs/guides/pricing-paygo
- BigModel GLM pricing  
  https://bigmodel.cn/pricing

MiniMax 和 GLM 官方价格以人民币发布。这里按 **2026-06-09 00:02:31 UTC** 从 ExchangeRate-API 取得的 USD/CNY **6.794828** 换算成美元。

截至 **2026-07-09**，新增 `glm-5.2`（新品），来源为 BigModel 官方价格页。其单一档位为：输入 8 元、输出 28 元、缓存读取 2 元（每百万 token），缓存写入限时免费（与 `glm-5` / `glm-5.1` 保持一致，记为 `N/A`）。同样按 USD/CNY **6.794828** 折算。

`glm-5.1` 官方按输入长度分两档（`[0, 32K)` 为 6/24/1.3 元，`[32K+)` 为 8/28/2 元）；价格文件里存的是长上下文 `[32K+)` 档，因为实际重度用量都是长上下文。注意 `glm-5.2` 的单一档位与 `glm-5.1` 的 `[32K+)` 档相同。

## 当前已覆盖模型

### Claude

- `fable-5`
- `fable-5-1`
- `fable-5-2`
- `haiku-4-5`
- `sonnet-4-5`
- `sonnet-4-6`
- `sonnet-5`（标准价：$3/$15/$3.75/$6/$0.3）
- `sonnet-5-5`
- `opus-4-5`
- `opus-4-6`
- `opus-4-7`
- `opus-4-8`
- `opus-5`
- `opus-5-5`

### OpenAI / Codex

已废弃的 ID（包括 `codex-mini-latest`）仍保留价格，用于历史用量。`gpt-5.6` 按官方别名 `gpt-5.6-sol` 的价格计费，但报表行仍保持独立。

- `chat-latest`
- `chatgpt-4o-latest`
- `codex-mini-latest`
- `gpt-3.5-turbo`
- `gpt-3.5-turbo-0125`
- `gpt-3.5-turbo-1106`
- `gpt-3.5-turbo-instruct`
- `gpt-4`
- `gpt-4-0125-preview`
- `gpt-4-0314`
- `gpt-4-0613`
- `gpt-4-1106-vision-preview`
- `gpt-4-turbo`
- `gpt-4-turbo-2024-04-09`
- `gpt-4-turbo-preview`
- `gpt-4.1`
- `gpt-4.1-mini`
- `gpt-4.1-nano`
- `gpt-4.5-preview`
- `gpt-4.5-preview-2025-02-27`
- `gpt-4o`
- `gpt-4o-2024-05-13`
- `gpt-4o-mini`
- `gpt-4o-mini-search-preview`
- `gpt-4o-mini-search-preview-2025-03-11`
- `gpt-4o-search-preview`
- `gpt-4o-search-preview-2025-03-11`
- `gpt-5`
- `gpt-5-chat-latest`
- `gpt-5-codex`
- `gpt-5-mini`
- `gpt-5-nano`
- `gpt-5-pro`
- `gpt-5-search-api`
- `gpt-5.1`
- `gpt-5.1-chat-latest`
- `gpt-5.1-codex`
- `gpt-5.1-codex-max`
- `gpt-5.1-codex-mini`
- `gpt-5.2`
- `gpt-5.2-chat-latest`
- `gpt-5.2-codex`
- `gpt-5.2-pro`
- `gpt-5.3-chat-latest`
- `gpt-5.3-codex`
- `gpt-5.4`
- `gpt-5.4-mini`
- `gpt-5.4-nano`
- `gpt-5.4-pro`
- `gpt-5.5`
- `gpt-5.5-cyber`
- `gpt-5.5-pro`
- `gpt-5.6`
- `gpt-5.6-cyber`
- `gpt-5.6-luna`
- `gpt-5.6-sol`
- `gpt-5.6-terra`
- `gpt-6-astra`
- `gpt-6-luna`
- `gpt-6-sol`
- `gpt-6.1-sol`
- `gpt-rosalind-discovery`
- `gpt-rosalind-research`

### Kimi / MiniMax / GLM

- `kimi-k2.6`
- `minimax-m3`
- `glm-5`
- `glm-5.1`
- `glm-5.2`

## 文件格式

价格文件是普通 JSON，以后如果你要托管到公网，这份文件可以直接发布。

结构如下：

```json
{
  "version": 1,
  "updated_at": "2026-04-17T00:00:00Z",
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

对已核实官方长上下文档位的 OpenAI 模型，在 token 价格旁添加 `"long_context": { "input_multiplier": 2.0, "output_multiplier": 1.5 }`。输入倍率同时适用于普通输入、缓存读取和缓存写入。没有官方加价规则的模型应省略该字段。当前支持的阈值为单次输入严格大于 272,000 token。

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
