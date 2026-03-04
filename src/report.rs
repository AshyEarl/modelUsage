use crate::model::{DailyReport, DailyRow, FileCacheEntry, ReportTotals, UsageTotals};
use crate::pricing::{compute_cost, known_unpriced_models};
use crate::model::PricingCache;
use std::collections::{BTreeMap, BTreeSet};

pub fn build_daily_report(entries: impl Iterator<Item = FileCacheEntry>, prices: &PricingCache) -> DailyReport {
    let mut rows_by_date: BTreeMap<chrono::NaiveDate, DailyRow> = BTreeMap::new();

    for entry in entries {
        for row in entry.daily_rows {
            // Merge file-level cached rows into the final per-day report here.
            // 文件级缓存里的结果在这里二次汇总成真正的日报。
            let day = rows_by_date.entry(row.date).or_insert_with(|| DailyRow {
                date: row.date,
                models: BTreeSet::new(),
                usage: UsageTotals::default(),
                cost_usd: Some(0.0),
                partial_cost: false,
                unpriced_models: BTreeSet::new(),
            });
            day.models.insert(row.model.clone());
            day.usage.add_assign(&row.usage);
            match compute_cost(&row.model, &row.usage, prices) {
                Some(cost) => {
                    let current = day.cost_usd.get_or_insert(0.0);
                    *current += cost;
                }
                None => {
                    // Unknown pricing should never masquerade as zero cost; mark it as partial/N/A instead.
                    // 未知价格不伪装成 0，而是标记成 partial/N/A。
                    day.partial_cost = true;
                    day.unpriced_models.insert(row.model.clone());
                }
            }
        }
    }

    for row in rows_by_date.values_mut() {
        if row.partial_cost && row.cost_usd == Some(0.0) {
            row.cost_usd = None;
        }
        if row.unpriced_models.is_empty() {
            row.unpriced_models = known_unpriced_models(row.models.iter().map(String::as_str), prices);
        }
    }

    let mut total_usage = UsageTotals::default();
    let mut total_cost = 0.0;
    let mut any_priced = false;
    let mut any_partial = false;
    let mut unpriced_models = BTreeSet::new();
    let rows: Vec<DailyRow> = rows_by_date.into_values().collect();
    for row in &rows {
        total_usage.add_assign(&row.usage);
        if let Some(cost) = row.cost_usd {
            total_cost += cost;
            any_priced = true;
        }
        if row.partial_cost {
            any_partial = true;
        }
        unpriced_models.extend(row.unpriced_models.iter().cloned());
    }

    DailyReport {
        rows,
        totals: ReportTotals {
            usage: total_usage,
            cost_usd: if any_priced { Some(total_cost) } else { None },
            partial_cost: any_partial,
            unpriced_models,
        },
    }
}
