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

As of **2026-04-25**, OpenAI pricing was manually verified from:

- OpenAI Pricing  
  https://developers.openai.com/api/docs/pricing
- OpenAI Models  
  https://developers.openai.com/api/docs/models

Notes:

- OpenAI exposes model APIs and usage APIs
- I did not find a stable public official pricing API
- pricing is therefore maintained by manually checking the official pages

As of **2026-06-10**, Anthropic / Claude pricing was manually verified from:

- Claude API pricing  
  https://platform.claude.com/docs/en/about-claude/pricing
- Claude plans & pricing  
  https://claude.com/pricing

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

- `fable-5` (1h prompt-cache write price is `N/A` until Anthropic publishes it)
- `haiku-4-5`
- `sonnet-4-5`
- `sonnet-4-6`
- `sonnet-5` (introductory pricing through Aug 31, 2026; standard $3/$15 thereafter)
- `opus-4-5`
- `opus-4-6`
- `opus-4-7`
- `opus-4-8`

### OpenAI / Codex

- `codex-mini-latest` (deprecated alias, still priced for historical usage)
- `gpt-5`
- `gpt-5-codex`
- `gpt-5.1-codex`
- `gpt-5.1-codex-mini`
- `gpt-5.1-codex-max`
- `gpt-5.2`
- `gpt-5.2-codex`
- `gpt-5.3-codex`
- `gpt-5.4`
- `gpt-5.4-pro`
- `gpt-5.5`
- `gpt-5.6` (alias for `gpt-5.6-sol`)
- `gpt-5.6-sol`
- `gpt-5.6-terra`
- `gpt-5.6-luna`

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
