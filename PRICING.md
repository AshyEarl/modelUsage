# Pricing maintenance

This document explains where `modelUsage` pricing comes from and how to update it when new models appear.

For the Chinese version, see:

- [PRICING-zh.md](/home/ashyearl/workspace/rust/modelUsage/PRICING-zh.md)

Project overview documents:

- [README.md](/home/ashyearl/workspace/rust/modelUsage/README.md)
- [README-zh.md](/home/ashyearl/workspace/rust/modelUsage/README-zh.md)

## Summary

`modelUsage` no longer depends on LiteLLM for pricing.

Current pricing sources:

1. bundled official pricing file  
   [pricing/official-pricing.json](/home/ashyearl/workspace/rust/modelUsage/pricing/official-pricing.json)
2. local runtime pricing cache  
   `~/.cache/modelUsage/pricing.json`

Default behavior:

- load bundled official pricing
- write it into the local cache when appropriate
- reuse the newer local cache on later runs

## Why LiteLLM was removed

The previous problems were straightforward:

- some new models were missing or delayed upstream
- external fetch failures made pricing incomplete
- usage could exist while cost became `0` or partial

For this project, a maintained local pricing file is more predictable.

## Official sources

As of **2026-10-12**, OpenAI text/code pricing was manually verified from:

- OpenAI Pricing  
  https://developers.openai.com/api/docs/pricing
- OpenAI Models  
  https://developers.openai.com/api/docs/models

Notes:

- OpenAI exposes model APIs and usage APIs
- I did not find a stable public official pricing API
- pricing is therefore maintained by manually checking the official pages

As of **2026-10-12**, 45 missing OpenAI text/code model IDs were added, including GPT-6 Sol, GPT-6.1 Sol, GPT-6 Luna, GPT-5 mini/nano/pro and chat variants, GPT-4.x, and officially listed legacy snapshots. Existing GPT-5.6 rates were refreshed, and published OpenAI cache-write rates were filled in.

| Model | Input | Output | Cache read | Cache write |
| --- | ---: | ---: | ---: | ---: |
| `gpt-6-astra` | $10 | $50 | $1 | $12.50 |
| `gpt-6-sol` | $2 | $10 | $0.20 | $2.50 |
| `gpt-6.1-sol` | $2 | $10 | $0.10 | $2.50 |
| `gpt-6-luna` | $0.10 | $0.50 | $0.01 | $0.125 |
| `gpt-5.6` / `gpt-5.6-sol` | $4 | $20 | $0.40 | $5 |
| `gpt-5.6-terra` | $2 | $12 | $0.20 | $2.50 |
| `gpt-5.6-luna` | $0.20 | $1.20 | $0.02 | $0.25 |

These are USD per million tokens at Standard short-context rates. GPT-5.6 Sol's current promotional pricing is published as available at least through November 21, 2026. The local price table applies the currently verified rates to historical usage as well; it does not select prices by usage date.

OpenAI publishes a single cache-write rate for these models, without Claude's 5-minute/1-hour distinction. It is stored in `cache_write_5m_cost_per_mtoken`; `cache_write_1h_cost_per_mtoken` stays `null`, and the calculation falls back to the published rate for either write bucket. Codex's `cache_write_input_tokens` (also accepted as `cache_write_tokens`) is stored in the 5-minute bucket. Both cache reads and cache writes are subsets of Codex input: ordinary input is `input - cache_read - cache_write`, and each bucket is charged at its own rate. When cumulative write counters are missing or reset, new requests use their last-request write counter if available; repeated snapshots remain deduplicated. If neither counter is available, writes remain zero and cannot be reconstructed from input alone. Reasoning is already included in output and is not charged again. See the [official prompt-caching accounting](https://developers.openai.com/api/docs/guides/prompt-caching).

As of **2026-10-13**, the [official Standard pricing table](https://developers.openai.com/api/docs/pricing) was checked for long-context rates. The local `long_context` configuration stores input/cache ×2 and output ×1.5 for GPT-5.4/Pro, GPT-5.5/Pro, GPT-5.6/Sol/Terra/Luna, GPT-6 Astra/Sol/Luna, and GPT-6.1 Sol. Only models with this configuration receive an uplift. Codex classifies each request using `last_token_usage.input_tokens > 272000` (including cached input), falling back to the cumulative-counter delta when the last-request usage is unavailable. Short and long requests remain separate through daily aggregation and fork reconciliation, so a day's input total never triggers an uplift. A cumulative-only snapshot covering multiple requests cannot reconstruct their individual context lengths. Parser version 4 automatically invalidates older Codex file caches; the pricing timestamp also advances so the new configuration replaces the previous bundled-price cache.

Other sources still use short-context rates. Batch/Flex/Fast/Ultrafast tiers, regional premiums, and tool-call fees are not modeled. Search models cover token costs only. Audio, image generation, and per-minute models are excluded because the current usage totals cannot distinguish their billing units.

Additional official model pages used for chat and legacy prices:

- [GPT-5 Chat](https://developers.openai.com/api/docs/models/gpt-5-chat-latest), [GPT-5.1 Chat](https://developers.openai.com/api/docs/models/gpt-5.1-chat-latest), [GPT-5.2 Chat](https://developers.openai.com/api/docs/models/gpt-5.2-chat-latest), [GPT-5.3 Chat](https://developers.openai.com/api/docs/models/gpt-5.3-chat-latest)
- [ChatGPT-4o](https://developers.openai.com/api/docs/models/chatgpt-4o-latest), [GPT-4](https://developers.openai.com/api/docs/models/gpt-4), [GPT-4 Turbo](https://developers.openai.com/api/docs/models/gpt-4-turbo), [GPT-4 Turbo Preview](https://developers.openai.com/api/docs/models/gpt-4-turbo-preview), [GPT-4.5 Preview](https://developers.openai.com/api/docs/models/gpt-4.5-preview)
- [GPT-4o Search Preview](https://developers.openai.com/api/docs/models/gpt-4o-search-preview), [GPT-4o Mini Search Preview](https://developers.openai.com/api/docs/models/gpt-4o-mini-search-preview)

As of **2026-06-10**, Anthropic / Claude pricing was manually verified from:

- Claude API pricing  
  https://platform.claude.com/docs/en/about-claude/pricing
- Claude plans & pricing  
  https://claude.com/pricing

As of **2026-07-27**, `opus-5` was added from the official Claude API pricing page. It shares the same per-MTok rates as the `opus-4-x` family: input $5, output $25, 5m cache write $6.25, 1h cache write $10, cache read $0.50.

As of **2026-08-02**, two Claude price corrections were applied. `sonnet-5` was updated to its standard rates (input $3, output $15, 5m cache write $3.75, 1h cache write $6, cache read $0.30) because Claude Code's `/cost` was observed billing at the standard rates even before September 1. `fable-5` 1h cache write was filled in at $20 per the official pricing page (previously `N/A`).

As of **2026-09-23**, `opus-5-5` (Claude Opus 5.5) was added from the official Claude API pricing. It is priced below `opus-5`: input $4, output $20, 5m cache write $5, 1h cache write $8, cache read $0.20 per MTok. Fast mode ($8/$40) is not modeled separately.

As of **2026-10-09**, `sonnet-5-5` (Claude Sonnet 5.5) was added from the official Claude API pricing: input $2, output $10, 5m cache write $2.50, 1h cache write $4, cache read $0.10 per MTok (cache hits are 0.05x base input on this model).

As of **2026-11-09**, `fable-5-2` (Claude Fable 5.2) was added from the official Claude model page and API pricing: input $8, output $40, 5m cache write $10, 1h cache write $16, cache read $0.20 per MTok (cache hits are 0.025x base input). This is roughly a 20% cut versus `fable-5-1`. Fast mode ($16/$80) is not modeled separately. Claude Mythos 5.2 shares the same rates but is not listed separately (invitation-only).

As of **2026-06-09**, Kimi, MiniMax, and GLM pricing was manually verified from:

- Kimi K2.6 pricing  
  https://platform.kimi.ai/docs/pricing/chat-k26
- MiniMax M3 pricing / pay-as-you-go pricing  
  https://www.minimax.io/models/text/m3  
  https://platform.minimaxi.com/docs/guides/pricing-paygo
- BigModel GLM pricing  
  https://bigmodel.cn/pricing

MiniMax and GLM prices are published in CNY. They were converted to USD with USD/CNY **6.794828** from ExchangeRate-API on **2026-06-09 00:02:31 UTC**.

As of **2026-07-09**, `glm-5.2` (new) was added from the official BigModel pricing page. It lists a single tier: input 8 CNY, output 28 CNY, cache read 2 CNY per 1M tokens (cache write is a limited-time free promotion, kept as `N/A` for consistency with `glm-5` / `glm-5.1`). Converted with the same USD/CNY **6.794828**.

`glm-5.1` has two context-length tiers on the official page (`[0, 32K)` at 6/24/1.3 CNY and `[32K+)` at 8/28/2 CNY); the file stores the long-context `[32K+)` tier, since real heavy usage is long-context. Note `glm-5.2`'s single tier equals `glm-5.1`'s `[32K+)` tier.

## Models currently covered

### Claude

- `fable-5`
- `fable-5-1`
- `fable-5-2`
- `haiku-4-5`
- `sonnet-4-5`
- `sonnet-4-6`
- `sonnet-5` (standard pricing: $3/$15/$3.75/$6/$0.3)
- `sonnet-5-5`
- `opus-4-5`
- `opus-4-6`
- `opus-4-7`
- `opus-4-8`
- `opus-5`
- `opus-5-5`

### OpenAI / Codex

Deprecated IDs remain priced for historical usage, including `codex-mini-latest`. `gpt-5.6` is priced as the official alias of `gpt-5.6-sol` while keeping its own report row.

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

## File format

The pricing file is plain JSON and can also be hosted on your own public URL later.

Structure:

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

All `*_cost_per_mtoken` fields are expressed in USD per million tokens.

For verified OpenAI models with a long-context tier, add `"long_context": { "input_multiplier": 2.0, "output_multiplier": 1.5 }` beside the token prices. Input multipliers apply to ordinary input, cache reads, and cache writes. Omit this field for models without an officially published uplift. The currently supported threshold is strictly more than 272,000 input tokens per request.

## Update workflow

When a new unknown model appears:

1. read the model name from `partial cost; unpriced models: ...`
2. verify the official published price
3. if officially priced:
   - update `pricing/official-pricing.json`
   - update this document if needed
4. if not officially priced:
   - keep it as `N/A`
   - do not guess or silently map it to another model

## Important rules

### Do not merge Codex minor versions

Current rule:

- provider prefixes may be stripped
- minor versions such as `gpt-5.2`, `gpt-5.3-codex`, and `gpt-5.1-codex-max` should remain distinct

Reason:

- you explicitly wanted real model versions preserved
- this keeps future pricing verification cleaner

### Unknown price must not become zero

If pricing is missing:

- show `N/A`
- mark the report as partial when appropriate

Do not convert unknown pricing into `$0.00`.

## If you want to host the pricing file yourself

The current JSON is already suitable for hosting on a public URL.

A future loading strategy can be:

1. keep the bundled local file
2. optionally fetch your own hosted JSON
3. overwrite local cache on success
4. fall back to bundled pricing on failure

That gives you:

- local offline fallback
- your own controlled online updates

## Maintenance suggestions

- update `updated_at` whenever you confirm prices
- only trust official pricing pages
- keep unknown models as `N/A` until verified
- do not secretly map different model versions together
