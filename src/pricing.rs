use crate::cache::{load_pricing_cache, save_pricing_cache};
use crate::model::{PricingCache, SourceKind, UsageTotals};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

const OFFICIAL_PRICING_JSON: &str = include_str!("../pricing/official-pricing.json");

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
) -> Option<f64> {
    let price = prices.models.get(model)?;
    if source == SourceKind::Codex {
        // Cached input for Codex must use the cheaper cache-read price instead of the regular input price.
        // Codex 的 cached input 要按更低的 cache read 单价计费，不能和普通 input 混算。
        let cached = usage.cache_read.min(usage.input);
        let non_cached = usage.input.saturating_sub(cached);
        return Some(
            mtok(non_cached, price.input_cost_per_mtoken)
                + mtok(
                    cached,
                    price
                        .cache_read_cost_per_mtoken
                        .unwrap_or(price.input_cost_per_mtoken),
                )
                + mtok(usage.output, price.output_cost_per_mtoken),
        );
    }

    Some(
        mtok(usage.input, price.input_cost_per_mtoken)
            + mtok(usage.output, price.output_cost_per_mtoken)
            + mtok(
                usage.cache_write_5m,
                price
                    .cache_write_5m_cost_per_mtoken
                    .or(price.cache_write_1h_cost_per_mtoken)
                    .unwrap_or(price.input_cost_per_mtoken),
            )
            + mtok(
                usage.cache_write_1h,
                price
                    .cache_write_1h_cost_per_mtoken
                    .or(price.cache_write_5m_cost_per_mtoken)
                    .unwrap_or(price.input_cost_per_mtoken),
            )
            + mtok(
                usage.cache_read,
                price
                    .cache_read_cost_per_mtoken
                    .unwrap_or(price.input_cost_per_mtoken),
            ),
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
    use super::{compute_cost, load_bundled_prices};
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
        let cost = compute_cost(SourceKind::Codex, "gpt-5-codex", &usage, &prices).unwrap();
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
        let cost = compute_cost(SourceKind::Codex, "gpt-5.4", &usage, &prices).unwrap();
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
        assert_eq!(sol.input_cost_per_mtoken, 5.0);
        assert_eq!(sol.output_cost_per_mtoken, 30.0);
        assert_eq!(sol.cache_read_cost_per_mtoken, Some(0.5));

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

        let terra = prices
            .models
            .get("gpt-5.6-terra")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6-terra"));
        assert_eq!(terra.input_cost_per_mtoken, 2.5);
        assert_eq!(terra.output_cost_per_mtoken, 15.0);
        assert_eq!(terra.cache_read_cost_per_mtoken, Some(0.25));

        let luna = prices
            .models
            .get("gpt-5.6-luna")
            .unwrap_or_else(|| panic!("missing model: gpt-5.6-luna"));
        assert_eq!(luna.input_cost_per_mtoken, 1.0);
        assert_eq!(luna.output_cost_per_mtoken, 6.0);
        assert_eq!(luna.cache_read_cost_per_mtoken, Some(0.1));
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
            ("opus-4-5", 10.0),
            ("opus-4-6", 10.0),
            ("opus-4-7", 10.0),
            ("opus-4-8", 10.0),
            ("opus-5", 10.0),
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
