use crate::cache::{load_pricing_cache, save_pricing_cache};
use crate::model::{PricingCache, SourceKind, UsageTotals};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

const OFFICIAL_PRICING_JSON: &str = include_str!("../pricing/official-pricing.json");
pub const OPENAI_LONG_CONTEXT_THRESHOLD: u64 = 272_000;

pub fn load_prices() -> Result<PricingCache> {
    // Pricing now comes from the project-maintained official price list instead of LiteLLM.
    // This keeps results stable and makes it easier to publish or maintain a custom public JSON later.
    // 价格来源改为项目内维护的官方价格表，不再依赖 LiteLLM。
    // 这样结果更稳定，也更便于自己维护和发布一份公网 JSON。
    let bundled = load_bundled_prices()?;
    let cache = match load_pricing_cache()? {
        Some(existing)
            if existing.updated_at >= bundled.updated_at && !existing.models.is_empty() =>
        {
            existing
        }
        _ => {
            let _ = save_pricing_cache(&bundled);
            bundled
        }
    };
    Ok(cache)
}

fn load_bundled_prices() -> Result<PricingCache> {
    let parsed: PricingCache = serde_json::from_str(OFFICIAL_PRICING_JSON)
        .context("failed to parse bundled official pricing file")?;
    Ok(parsed)
}

pub fn compute_cost(
    source: SourceKind,
    model: &str,
    usage: &UsageTotals,
    prices: &PricingCache,
    long_context: bool,
) -> Option<f64> {
    let price = prices.models.get(model)?;
    let (input, cache_read, cache_write_5m, cache_write_1h) = if source == SourceKind::Codex {
        // Codex input includes both cache reads and writes; charge each subset exactly once.
        // Codex 的 input 包含缓存读取和写入；各个子集只按对应价格计费一次。
        let cached = usage.cache_read.min(usage.input);
        let remaining = usage.input - cached;
        let written_5m = usage.cache_write_5m.min(remaining);
        let written_1h = usage.cache_write_1h.min(remaining - written_5m);
        (
            remaining - written_5m - written_1h,
            cached,
            written_5m,
            written_1h,
        )
    } else {
        (
            usage.input,
            usage.cache_read,
            usage.cache_write_5m,
            usage.cache_write_1h,
        )
    };
    let context_price = price.long_context.as_ref().filter(|_| long_context);
    let input_multiplier = context_price.map_or(1.0, |tier| tier.input_multiplier);
    let output_multiplier = context_price.map_or(1.0, |tier| tier.output_multiplier);

    Some(
        (mtok(input, price.input_cost_per_mtoken)
            + mtok(
                cache_write_5m,
                price
                    .cache_write_5m_cost_per_mtoken
                    .or(price.cache_write_1h_cost_per_mtoken)
                    .unwrap_or(price.input_cost_per_mtoken),
            )
            + mtok(
                cache_write_1h,
                price
                    .cache_write_1h_cost_per_mtoken
                    .or(price.cache_write_5m_cost_per_mtoken)
                    .unwrap_or(price.input_cost_per_mtoken),
            )
            + mtok(
                cache_read,
                price
                    .cache_read_cost_per_mtoken
                    .unwrap_or(price.input_cost_per_mtoken),
            ))
            * input_multiplier
            + mtok(usage.output, price.output_cost_per_mtoken) * output_multiplier,
    )
}

fn mtok(tokens: u64, price_per_million: f64) -> f64 {
    (tokens as f64 / 1_000_000.0) * price_per_million
}

pub fn known_unpriced_models<'a>(
    models: impl Iterator<Item = &'a str>,
    prices: &PricingCache,
) -> BTreeSet<String> {
    models
        .filter(|model| !prices.models.contains_key(*model))
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{compute_cost, known_unpriced_models, load_bundled_prices};
    use crate::codex::normalize_codex_model;
    use crate::model::{SourceKind, UsageTotals};

    #[test]
    fn computes_codex_cost() {
        let prices = load_bundled_prices().unwrap();
        let usage = UsageTotals {
            input: 6_253_428,
            output: 105_730,
            reasoning: 77_504,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 5_420_416,
            total: 6_359_158,
        };
        let cost = compute_cost(SourceKind::Codex, "gpt-5-codex", &usage, &prices, false).unwrap();
        assert!((cost - 2.776117).abs() < 0.000001);
    }

    #[test]
    fn computes_codex_source_cost_for_plain_gpt_model() {
        let prices = load_bundled_prices().unwrap();
        let usage = UsageTotals {
            input: 1_915_287,
            output: 24_844,
            reasoning: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 1_456_128,
            total: 1_940_131,
        };
        let cost = compute_cost(SourceKind::Codex, "gpt-5.4", &usage, &prices, false).unwrap();
        assert!((cost - 1.8845895).abs() < 0.000001);
    }

    #[test]
    fn has_new_codex_models() {
        let prices = load_bundled_prices().unwrap();
        for model in [
            "codex-mini-latest",
            "gpt-5.1-codex-mini",
            "gpt-5.1-codex-max",
            "gpt-5.2",
            "gpt-5.2-codex",
            "gpt-5.3-codex",
            "gpt-5.4",
            "gpt-5.4-pro",
            "gpt-5.5",
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
        ] {
            assert!(prices.models.contains_key(model), "missing model: {model}");
        }
    }

    #[test]
    fn has_gpt_5_5_official_short_context_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("gpt-5.5")
            .unwrap_or_else(|| panic!("missing model: gpt-5.5"));
        assert_eq!(price.input_cost_per_mtoken, 5.0);
        assert_eq!(price.output_cost_per_mtoken, 30.0);
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.5));
    }

    #[test]
    fn has_gpt_5_6_official_short_context_prices() {
        let prices = load_bundled_prices().unwrap();
        let sol = prices
            .models
            .get("gpt-5.6-sol")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6-sol"));
        assert_eq!(sol.input_cost_per_mtoken, 4.0);
        assert_eq!(sol.output_cost_per_mtoken, 20.0);
        assert_eq!(sol.cache_read_cost_per_mtoken, Some(0.4));
        assert_eq!(sol.cache_write_5m_cost_per_mtoken, Some(5.0));

        let alias = prices
            .models
            .get("gpt-5.6")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6"));
        assert_eq!(alias.input_cost_per_mtoken, sol.input_cost_per_mtoken);
        assert_eq!(alias.output_cost_per_mtoken, sol.output_cost_per_mtoken);
        assert_eq!(
            alias.cache_read_cost_per_mtoken,
            sol.cache_read_cost_per_mtoken
        );
        assert_eq!(
            alias.cache_write_5m_cost_per_mtoken,
            sol.cache_write_5m_cost_per_mtoken
        );

        let terra = prices
            .models
            .get("gpt-5.6-terra")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6-terra"));
        assert_eq!(terra.input_cost_per_mtoken, 2.0);
        assert_eq!(terra.output_cost_per_mtoken, 12.0);
        assert_eq!(terra.cache_read_cost_per_mtoken, Some(0.2));
        assert_eq!(terra.cache_write_5m_cost_per_mtoken, Some(2.5));

        let luna = prices
            .models
            .get("gpt-5.6-luna")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6-luna"));
        assert_eq!(luna.input_cost_per_mtoken, 0.2);
        assert_eq!(luna.output_cost_per_mtoken, 1.2);
        assert_eq!(luna.cache_read_cost_per_mtoken, Some(0.02));
        assert_eq!(luna.cache_write_5m_cost_per_mtoken, Some(0.25));
    }

    #[test]
    fn has_gpt_6_astra_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("gpt-6-astra")
            .unwrap_or_else(|| panic!("missing model: gpt-6-astra"));
        assert_eq!(price.input_cost_per_mtoken, 10.0);
        assert_eq!(price.output_cost_per_mtoken, 50.0);
        assert_eq!(price.cache_read_cost_per_mtoken, Some(1.0));
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(12.5));
    }

    #[test]
    fn computes_cost_for_missing_gpt_text_models_without_merging_versions() {
        let prices = load_bundled_prices().unwrap();
        let usage = UsageTotals {
            input: 2_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            reasoning: 500_000,
            total: 3_000_000,
            ..UsageTotals::default()
        };
        for (model, expected) in [
            ("gpt-6-sol", 12.2),
            ("gpt-6.1-sol", 12.1),
            ("gpt-6-luna", 0.61),
            ("gpt-5.1", 11.375),
            ("gpt-5-mini", 2.275),
            ("gpt-5-nano", 0.455),
            ("gpt-5-pro", 150.0),
            ("gpt-5.2-pro", 210.0),
            ("gpt-5.4-nano", 1.47),
            ("gpt-5.5-pro", 240.0),
            ("gpt-5.3-chat-latest", 15.925),
            ("gpt-5.6-cyber", 88.75),
            ("gpt-rosalind-research", 30.5),
            ("gpt-4.1", 10.5),
            ("gpt-4o", 13.75),
            ("gpt-4o-2024-05-13", 25.0),
            ("gpt-4o-mini", 0.825),
            ("gpt-4-turbo", 50.0),
            ("gpt-3.5-turbo", 2.5),
        ] {
            let normalized = normalize_codex_model(&format!("openrouter/openai/{model}"));
            assert_eq!(normalized, model);
            let cost = compute_cost(SourceKind::Codex, &normalized, &usage, &prices, false)
                .unwrap_or_else(|| panic!("missing price: {model}"));
            assert!((cost - expected).abs() < 0.000001, "{model}: {cost}");
        }
    }

    #[test]
    fn computes_openai_cache_writes_for_other_sources() {
        let prices = load_bundled_prices().unwrap();
        let usage = UsageTotals {
            input: 1_000_000,
            output: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            cache_read: 1_000_000,
            total: 5_000_000,
            ..UsageTotals::default()
        };
        for source in [
            SourceKind::Claude,
            SourceKind::Copilot,
            SourceKind::Opencode,
        ] {
            let cost = compute_cost(source, "gpt-6.1-sol", &usage, &prices, false).unwrap();
            assert!((cost - 17.1).abs() < 0.000001, "{source:?}: {cost}");
        }
    }

    #[test]
    fn keeps_unverified_gpt_models_unpriced() {
        let prices = load_bundled_prices().unwrap();
        let models = [
            "gpt-6-sol",
            "gpt-6.1-sol",
            "gpt-6-luna",
            "gpt-99-unverified",
        ];
        let unpriced = known_unpriced_models(models.into_iter(), &prices);
        assert_eq!(
            unpriced.into_iter().collect::<Vec<_>>(),
            ["gpt-99-unverified"]
        );
        assert_eq!(
            compute_cost(
                SourceKind::Codex,
                "gpt-99-unverified",
                &UsageTotals::default(),
                &prices,
                false,
            ),
            None
        );
    }

    #[test]
    fn has_new_oss_models_from_claude_usage() {
        let prices = load_bundled_prices().unwrap();
        for (model, input, output, cache_read) in [
            ("kimi-k2.6", 0.95, 4.0, 0.16),
            ("minimax-m3", 0.309059, 1.236234, 0.061812),
            ("glm-5", 0.588683, 2.649074, 0.147171),
            // glm-5.1 uses the long-context [32K+) tier (8/28/2 CNY), same as glm-5.2.
            // glm-5.1 采用长上下文 [32K+) 档（8/28/2 元），与 glm-5.2 相同。
            ("glm-5.1", 1.177366, 4.120781, 0.294342),
            ("glm-5.2", 1.177366, 4.120781, 0.294342),
        ] {
            let price = prices
                .models
                .get(model)
                .unwrap_or_else(|| panic!("missing model: {model}"));
            assert_eq!(price.input_cost_per_mtoken, input);
            assert_eq!(price.output_cost_per_mtoken, output);
            assert_eq!(price.cache_read_cost_per_mtoken, Some(cache_read));
            assert_eq!(price.cache_write_5m_cost_per_mtoken, None);
            assert_eq!(price.cache_write_1h_cost_per_mtoken, None);
        }
    }

    #[test]
    fn has_glm_5_2_official_price() {
        // GLM-5.2 (new) lists a single tier: input 8 CNY / output 28 CNY / cache read 2 CNY per 1M,
        // cache write is a limited-time free promotion. Converted to USD at USD/CNY 6.794828.
        // GLM-5.2（新品）单一档位：输入 8 元 / 输出 28 元 / 缓存读取 2 元（每百万 token），
        // 缓存写入限时免费。按 USD/CNY 6.794828 折算成美元。
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("glm-5.2")
            .unwrap_or_else(|| panic!("missing model: glm-5.2"));
        assert_eq!(price.input_cost_per_mtoken, 1.177366);
        assert_eq!(price.output_cost_per_mtoken, 4.120781);
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.294342));
    }

    #[test]
    fn has_sonnet_5_5_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("sonnet-5-5")
            .unwrap_or_else(|| panic!("missing model: sonnet-5-5"));
        assert_eq!(price.input_cost_per_mtoken, 2.0);
        assert_eq!(price.output_cost_per_mtoken, 10.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(2.5));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(4.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.1));
    }

    #[test]
    fn has_opus_5_5_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("opus-5-5")
            .unwrap_or_else(|| panic!("missing model: opus-5-5"));
        assert_eq!(price.input_cost_per_mtoken, 4.0);
        assert_eq!(price.output_cost_per_mtoken, 20.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(5.0));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(8.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.2));
    }

    #[test]
    fn has_fable_5_2_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("fable-5-2")
            .unwrap_or_else(|| panic!("missing model: fable-5-2"));
        assert_eq!(price.input_cost_per_mtoken, 8.0);
        assert_eq!(price.output_cost_per_mtoken, 40.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(10.0));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(16.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.2));
    }

    #[test]
    fn has_fable_5_1_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("fable-5-1")
            .unwrap_or_else(|| panic!("missing model: fable-5-1"));
        assert_eq!(price.input_cost_per_mtoken, 10.0);
        assert_eq!(price.output_cost_per_mtoken, 50.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(12.5));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(20.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.25));
    }

    #[test]
    fn has_fable_5_official_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("fable-5")
            .unwrap_or_else(|| panic!("missing model: fable-5"));
        assert_eq!(price.input_cost_per_mtoken, 10.0);
        assert_eq!(price.output_cost_per_mtoken, 50.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(12.5));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(20.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(1.0));
    }

    #[test]
    fn has_current_claude_4x_models_and_cache_1h_prices() {
        let prices = load_bundled_prices().unwrap();
        for (model, expected_1h) in [
            ("haiku-4-5", 2.0),
            ("sonnet-4-5", 6.0),
            ("sonnet-4-6", 6.0),
            ("sonnet-5-5", 4.0),
            ("opus-4-5", 10.0),
            ("opus-4-6", 10.0),
            ("opus-4-7", 10.0),
            ("opus-4-8", 10.0),
            ("opus-5", 10.0),
            ("opus-5-5", 8.0),
            ("fable-5-2", 16.0),
            ("fable-5-1", 20.0),
        ] {
            let price = prices
                .models
                .get(model)
                .unwrap_or_else(|| panic!("missing model: {model}"));
            assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(expected_1h));
        }
    }

    #[test]
    fn has_sonnet_5_standard_prices() {
        let prices = load_bundled_prices().unwrap();
        let price = prices
            .models
            .get("sonnet-5")
            .unwrap_or_else(|| panic!("missing model: sonnet-5"));
        assert_eq!(price.input_cost_per_mtoken, 3.0);
        assert_eq!(price.output_cost_per_mtoken, 15.0);
        assert_eq!(price.cache_write_5m_cost_per_mtoken, Some(3.75));
        assert_eq!(price.cache_write_1h_cost_per_mtoken, Some(6.0));
        assert_eq!(price.cache_read_cost_per_mtoken, Some(0.3));
    }
}
