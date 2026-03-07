use crate::cache::{load_pricing_cache, save_pricing_cache};
use crate::model::{PricingCache, UsageTotals};
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

pub fn compute_cost(model: &str, usage: &UsageTotals, prices: &PricingCache) -> Option<f64> {
    let price = prices.models.get(model)?;
    if model.contains("codex") {
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
    use crate::model::UsageTotals;

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
        let cost = compute_cost("gpt-5-codex", &usage, &prices).unwrap();
        assert!((cost - 2.776117).abs() < 0.000001);
    }

    #[test]
    fn has_new_codex_models() {
        let prices = load_bundled_prices().unwrap();
        for model in [
            "gpt-5.1-codex-max",
            "gpt-5.2",
            "gpt-5.2-codex",
            "gpt-5.3-codex",
            "gpt-5.4",
            "gpt-5.4-pro",
        ] {
            assert!(prices.models.contains_key(model), "missing model: {model}");
        }
    }
}
