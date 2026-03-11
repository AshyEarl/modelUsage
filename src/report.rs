use crate::model::PricingCache;
use crate::model::{
    DailyReport, DailyRow, FileCacheEntry, ReportGrouping, ReportTotals, UsageTotals,
};
use crate::pricing::{compute_cost, known_unpriced_models};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Daily(chrono::NaiveDate),
    Project(String),
    DailyProject(chrono::NaiveDate, String),
    ProjectDaily(String, chrono::NaiveDate),
}

pub fn build_report(
    entries: impl Iterator<Item = FileCacheEntry>,
    prices: &PricingCache,
    grouping: ReportGrouping,
) -> DailyReport {
    let mut rows_by_key: BTreeMap<GroupKey, DailyRow> = BTreeMap::new();

    for entry in entries {
        for row in entry.daily_rows {
            let key = match grouping {
                ReportGrouping::Daily => GroupKey::Daily(row.date),
                ReportGrouping::Project => GroupKey::Project(row.project.clone()),
                ReportGrouping::DailyProject => {
                    GroupKey::DailyProject(row.date, row.project.clone())
                }
                ReportGrouping::ProjectDaily => {
                    GroupKey::ProjectDaily(row.project.clone(), row.date)
                }
            };
            // Merge file-level cached rows into the final per-day report here.
            // 文件级缓存里的结果在这里二次汇总成真正的日报。
            let day = rows_by_key.entry(key).or_insert_with(|| DailyRow {
                date: match grouping {
                    ReportGrouping::Project => None,
                    _ => Some(row.date),
                },
                project: match grouping {
                    ReportGrouping::Daily => None,
                    _ => Some(row.project.clone()),
                },
                models: BTreeSet::new(),
                usage: UsageTotals::default(),
                cost_usd: Some(0.0),
                partial_cost: false,
                unpriced_models: BTreeSet::new(),
            });
            day.models.insert(row.model.clone());
            day.usage.add_assign(&row.usage);
            match compute_cost(entry.source, &row.model, &row.usage, prices) {
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

    for row in rows_by_key.values_mut() {
        if row.partial_cost && row.cost_usd == Some(0.0) {
            row.cost_usd = None;
        }
        if row.unpriced_models.is_empty() {
            row.unpriced_models =
                known_unpriced_models(row.models.iter().map(String::as_str), prices);
        }
    }

    let mut total_usage = UsageTotals::default();
    let mut total_cost = 0.0;
    let mut any_priced = false;
    let mut any_partial = false;
    let mut unpriced_models = BTreeSet::new();
    let mut rows: Vec<DailyRow> = rows_by_key.into_values().collect();
    if grouping == ReportGrouping::Project {
        // Project-only view should rank by cost so the most expensive project is immediately visible.
        // 仅按项目分组时按花费降序排列，最贵的项目优先展示。
        rows.sort_by(sort_rows_by_direct_cost_desc);
    } else if grouping == ReportGrouping::ProjectDaily {
        // When users choose --project --daily, they expect project blocks to follow spending rank.
        // 用户使用 --project --daily 时，项目分块应按总花费排序，再展示分块内日期。
        let mut project_costs: BTreeMap<String, Option<f64>> = BTreeMap::new();
        for row in &rows {
            let project = row.project.as_deref().unwrap_or_default().to_string();
            let entry = project_costs.entry(project).or_insert(Some(0.0));
            if let Some(cost) = row.cost_usd {
                if let Some(sum) = entry.as_mut() {
                    *sum += cost;
                }
            } else {
                *entry = None;
            }
        }
        rows.sort_by(|a, b| {
            let a_project = a.project.as_deref().unwrap_or_default();
            let b_project = b.project.as_deref().unwrap_or_default();
            let project_order = compare_optional_cost_desc(
                project_costs.get(a_project).copied().flatten(),
                project_costs.get(b_project).copied().flatten(),
            )
            .then_with(|| a_project.cmp(b_project));
            if project_order != Ordering::Equal {
                return project_order;
            }
            a.date.cmp(&b.date)
        });
    }

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
        grouping,
        rows,
        totals: ReportTotals {
            usage: total_usage,
            cost_usd: if any_priced { Some(total_cost) } else { None },
            partial_cost: any_partial,
            unpriced_models,
        },
        warnings: Vec::new(),
    }
}

fn sort_rows_by_direct_cost_desc(a: &DailyRow, b: &DailyRow) -> Ordering {
    compare_optional_cost_desc(a.cost_usd, b.cost_usd).then_with(|| {
        let a_project = a.project.as_deref().unwrap_or_default();
        let b_project = b.project.as_deref().unwrap_or_default();
        a_project.cmp(b_project)
    })
}

fn compare_optional_cost_desc(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a_cost), Some(b_cost)) => b_cost.total_cmp(&a_cost),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::build_report;
    use crate::model::{
        FileCacheEntry, FileDailyRow, ModelPrice, PricingCache, ReportGrouping, SourceKind,
        UsageTotals,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn default_daily_report_merges_projects_of_same_day() {
        let report = build_report(
            sample_entries().into_iter(),
            &empty_prices(),
            ReportGrouping::Daily,
        );
        assert_eq!(report.rows.len(), 1);
        assert!(report.rows[0].project.is_none());
        assert_eq!(
            report.rows[0].date,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap())
        );
        assert_eq!(report.rows[0].usage.input, 3);
    }

    #[test]
    fn project_only_report_merges_dates() {
        let report = build_report(
            sample_entries().into_iter(),
            &empty_prices(),
            ReportGrouping::Project,
        );
        assert_eq!(report.rows.len(), 2);
        assert!(report.rows.iter().all(|row| row.project.is_some()));
        assert!(report.rows.iter().all(|row| row.date.is_none()));
        assert_eq!(report.totals.usage.input, 3);
    }

    #[test]
    fn project_only_report_sorts_by_cost_desc() {
        let report = build_report(
            sample_entries().into_iter(),
            &priced_sonnet(),
            ReportGrouping::Project,
        );
        assert_eq!(
            report.rows[0].project.as_deref(),
            Some("/repo/b"),
            "higher-cost project should come first"
        );
        assert_eq!(report.rows[1].project.as_deref(), Some("/repo/a"));
    }

    #[test]
    fn project_daily_report_keeps_dimension_order() {
        let report = build_report(
            sample_entries().into_iter(),
            &empty_prices(),
            ReportGrouping::ProjectDaily,
        );
        assert_eq!(report.rows.len(), 2);
        assert!(report.rows.iter().all(|row| row.project.is_some()));
        assert!(report.rows.iter().all(|row| row.date.is_some()));
        assert_eq!(report.totals.usage.input, 3);
    }

    #[test]
    fn project_daily_report_sorts_project_blocks_by_cost_desc() {
        let report = build_report(
            sample_entries_with_project_daily_rows().into_iter(),
            &priced_sonnet(),
            ReportGrouping::ProjectDaily,
        );
        let projects: Vec<&str> = report
            .rows
            .iter()
            .map(|row| row.project.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(
            projects,
            vec!["/repo/b", "/repo/b", "/repo/a"],
            "project blocks should follow total project cost"
        );
        let repo_b_dates: Vec<chrono::NaiveDate> = report
            .rows
            .iter()
            .filter(|row| row.project.as_deref() == Some("/repo/b"))
            .filter_map(|row| row.date)
            .collect();
        assert_eq!(
            repo_b_dates,
            vec![
                chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap()
            ]
        );
    }

    fn sample_entries() -> Vec<FileCacheEntry> {
        vec![
            FileCacheEntry {
                source: SourceKind::Claude,
                parser_version: 2,
                path: PathBuf::from("/tmp/a.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![FileDailyRow {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                    project: "/repo/a".to_string(),
                    model: "sonnet-4-6".to_string(),
                    usage: UsageTotals {
                        input: 1,
                        ..UsageTotals::default()
                    },
                }],
            },
            FileCacheEntry {
                source: SourceKind::Claude,
                parser_version: 2,
                path: PathBuf::from("/tmp/b.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![FileDailyRow {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                    project: "/repo/b".to_string(),
                    model: "sonnet-4-6".to_string(),
                    usage: UsageTotals {
                        input: 2,
                        ..UsageTotals::default()
                    },
                }],
            },
        ]
    }

    fn sample_entries_with_project_daily_rows() -> Vec<FileCacheEntry> {
        vec![
            FileCacheEntry {
                source: SourceKind::Claude,
                parser_version: 2,
                path: PathBuf::from("/tmp/c.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![
                    FileDailyRow {
                        date: chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
                        project: "/repo/b".to_string(),
                        model: "sonnet-4-6".to_string(),
                        usage: UsageTotals {
                            input: 4,
                            ..UsageTotals::default()
                        },
                    },
                    FileDailyRow {
                        date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                        project: "/repo/b".to_string(),
                        model: "sonnet-4-6".to_string(),
                        usage: UsageTotals {
                            input: 3,
                            ..UsageTotals::default()
                        },
                    },
                ],
            },
            FileCacheEntry {
                source: SourceKind::Claude,
                parser_version: 2,
                path: PathBuf::from("/tmp/d.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![FileDailyRow {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                    project: "/repo/a".to_string(),
                    model: "sonnet-4-6".to_string(),
                    usage: UsageTotals {
                        input: 1,
                        ..UsageTotals::default()
                    },
                }],
            },
        ]
    }

    fn empty_prices() -> PricingCache {
        PricingCache {
            version: 1,
            updated_at: Utc::now(),
            models: BTreeMap::new(),
        }
    }

    fn priced_sonnet() -> PricingCache {
        let mut models = BTreeMap::new();
        models.insert(
            "sonnet-4-6".to_string(),
            ModelPrice {
                input_cost_per_mtoken: 1.0,
                output_cost_per_mtoken: 1.0,
                cache_write_5m_cost_per_mtoken: None,
                cache_write_1h_cost_per_mtoken: None,
                cache_read_cost_per_mtoken: None,
            },
        );
        PricingCache {
            version: 1,
            updated_at: Utc::now(),
            models,
        }
    }
}
