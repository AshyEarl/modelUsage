use crate::model::{FileDailyRow, UsageTotals};
use crate::profile;
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Resolve the OpenCode session database path.
/// OpenCode stores all sessions in a single SQLite file under the data dir.
/// OpenCode 把全部会话存在 data 目录下的单个 SQLite 文件里。
pub fn default_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("opencode").join("opencode.db"))
}

/// Parse the OpenCode SQLite database into per-day-per-model rows.
///
/// Approach B reads the `message` table at assistant-message granularity: each
/// assistant message already carries its own `modelID`, full token totals, and
/// the working directory. This reconciles exactly with the session-level columns
/// while still attributing usage to the correct model when a session switches
/// models mid-conversation.
///
/// 方案 B 直接读 `message` 表到助手消息粒度：每条助手消息自带 `modelID`、
/// 完整 token 统计和工作目录。这样既能与会话级汇总完全对账，又能在会话内
/// 切模型时把用量归到正确的模型上。
pub fn parse_db(path: &Path, aggregation_tz: &AggregationTz) -> Result<Vec<FileDailyRow>> {
    let profile_enabled = profile::enabled();
    let started = Instant::now();
    // Open read-only so we never block OpenCode's own writes to its database.
    // 以只读方式打开，避免阻塞 OpenCode 自身对数据库的写入。
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open OpenCode db {}", path.display()))?;

    let mut stmt = conn
        .prepare("SELECT data, time_created FROM message")
        .context("failed to prepare OpenCode message query")?;

    let mut rows = stmt.query([])?;
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    let mut scanned: u64 = 0;
    let mut assistant_rows: u64 = 0;
    let mut emitted: u64 = 0;
    let mut skipped_zero: u64 = 0;

    while let Some(row) = rows.next()? {
        scanned += 1;
        let data_raw: String = match row.get::<_, Option<String>>(0) {
            Ok(Some(v)) => v,
            _ => continue,
        };
        let time_ms: i64 = row.get::<_, Option<i64>>(1).unwrap_or_default().unwrap_or(0);

        let value: Value = match serde_json::from_str(&data_raw) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        assistant_rows += 1;

        let Some(model_raw) = value.get("modelID").and_then(Value::as_str) else {
            continue;
        };
        let model = normalize_opencode_model(model_raw);
        if model.is_empty() {
            continue;
        }

        let tokens = value.get("tokens");
        let input = tokens
            .and_then(|t| t.get("input"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let output = tokens
            .and_then(|t| t.get("output"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let reasoning = tokens
            .and_then(|t| t.get("reasoning"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache = tokens.and_then(|t| t.get("cache"));
        let cache_read = cache
            .and_then(|c| c.get("read"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // OpenCode exposes a single cache-write counter with no 5m/1h split; bucket it into 5m
        // so the total stays intact, mirroring how Claude's unsplittable cache creation is handled.
        // OpenCode 只给一个 cache write 总数，不拆 5m/1h；归到 5m 桶以保证总数不丢，与 Claude 不拆分时的处理一致。
        let cache_write = cache
            .and_then(|c| c.get("write"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total = tokens
            .and_then(|t| t.get("total"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        if input == 0
            && output == 0
            && reasoning == 0
            && cache_read == 0
            && cache_write == 0
            && total == 0
        {
            skipped_zero += 1;
            continue;
        }

        let Some(timestamp) = DateTime::<Utc>::from_timestamp_millis(time_ms) else {
            continue;
        };

        let project = value
            .get("path")
            .and_then(|p| p.get("cwd"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "<unknown-project>".to_string());

        let computed_total = if total > 0 {
            total
        } else {
            input + output + reasoning + cache_write + cache_read
        };

        emitted += 1;
        let day = aggregation_tz.date_for(timestamp);
        let key = (day, project, model);
        daily.entry(key).or_default().add_assign(&UsageTotals {
            input,
            output,
            reasoning,
            cache_write_5m: cache_write,
            cache_write_1h: 0,
            cache_read,
            total: computed_total,
        });
    }

    if profile_enabled {
        profile::log(format!(
            "opencode parse db={} scanned={} assistant_rows={} emitted={} skipped_zero={} daily_rows={} elapsed_ms={}",
            path.display(),
            scanned,
            assistant_rows,
            emitted,
            skipped_zero,
            daily.len(),
            started.elapsed().as_millis()
        ));
    }

    Ok(daily
        .into_iter()
        .map(|((date, project, model), usage)| FileDailyRow {
            date,
            project,
            model,
            usage,
        })
        .collect())
}

/// Normalize an OpenCode model id.
/// OpenCode stores the bare model id (e.g. `glm-5.2`) separately from the
/// provider id, so we only trim and strip a provider prefix if one is present.
/// Per the repo rules we do not merge minor versions.
/// OpenCode 的 model id 已经是裸模型名（如 `glm-5.2`），provider id 另存；
/// 这里只做 trim 和 provider 前缀去除，按仓库规则不合并小版本。
pub fn normalize_opencode_model(raw: &str) -> String {
    let mut model = raw.trim();
    if let Some(idx) = model.find('/') {
        model = model[idx + 1..].trim();
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::{normalize_opencode_model, parse_db};
    use crate::timezone::AggregationTz;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn trims_and_strips_provider_prefix() {
        assert_eq!(normalize_opencode_model("glm-5.2"), "glm-5.2");
        assert_eq!(normalize_opencode_model(" glm-5.1 "), "glm-5.1");
        assert_eq!(normalize_opencode_model("volcengine-plan/glm-5.2"), "glm-5.2");
    }

    #[test]
    fn aggregates_assistant_messages_by_day_and_model() {
        let path = write_temp_db(&[
            assistant_row(1780970000000, "glm-5.1", "/repo/a", 100, 20, 0, 0, 0, 120),
            // Same day, different model -> separate row.
            assistant_row(1780971000000, "glm-5.2", "/repo/a", 50, 10, 5, 30, 0, 95),
            // Non-assistant row must be ignored.
            user_row(1780972000000, "/repo/a"),
        ]);
        let tz = AggregationTz::parse(Some("UTC")).unwrap();
        let rows = parse_db(&path, &tz).unwrap();
        let _ = std::fs::remove_file(&path);

        let glm51 = rows.iter().find(|r| r.model == "glm-5.1").unwrap();
        assert_eq!(glm51.usage.input, 100);
        assert_eq!(glm51.usage.output, 20);
        assert_eq!(glm51.usage.total, 120);
        assert_eq!(glm51.project, "/repo/a");

        let glm52 = rows.iter().find(|r| r.model == "glm-5.2").unwrap();
        assert_eq!(glm52.usage.input, 50);
        assert_eq!(glm52.usage.reasoning, 5);
        assert_eq!(glm52.usage.cache_read, 30);
        assert_eq!(glm52.usage.total, 95);
    }

    #[test]
    fn honors_aggregation_timezone_for_day_bucket() {
        // 2026-07-08T20:30:00Z is 2026-07-09 04:30 in UTC+8.
        let path = write_temp_db(&[assistant_row(
            1783542600000, "glm-5.2", "/repo/a", 10, 4, 0, 0, 0, 14,
        )]);
        let tz_utc = AggregationTz::parse(Some("UTC")).unwrap();
        let tz_plus8 = AggregationTz::parse(Some("UTC+8")).unwrap();
        let rows_utc = parse_db(&path, &tz_utc).unwrap();
        let rows_plus8 = parse_db(&path, &tz_plus8).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(rows_utc[0].date.to_string(), "2026-07-08");
        assert_eq!(rows_plus8[0].date.to_string(), "2026-07-09");
    }

    #[test]
    fn skips_zero_token_messages() {
        let path = write_temp_db(&[
            assistant_row(1780970000000, "glm-5.2", "/repo/a", 0, 0, 0, 0, 0, 0),
            assistant_row(1780970000000, "glm-5.2", "/repo/a", 10, 4, 0, 0, 0, 14),
        ]);
        let tz = AggregationTz::parse(Some("UTC")).unwrap();
        let rows = parse_db(&path, &tz).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.input, 10);
    }

    fn write_temp_db(rows: &[(&str, i64)]) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("modelusage-opencode-test-{nanos}.db"));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
        )
        .unwrap();
        for (idx, (data, time_created)) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, 's', ?2, ?2, ?3)",
                rusqlite::params![format!("m{idx}"), time_created, data],
            )
            .unwrap();
        }
        drop(conn);
        path
    }

    #[allow(clippy::too_many_arguments)]
    fn assistant_row(
        time_created_ms: i64,
        model: &str,
        cwd: &str,
        input: u64,
        output: u64,
        reasoning: u64,
        cache_read: u64,
        cache_write: u64,
        total: u64,
    ) -> (&'static str, i64) {
        let data = format!(
            "{{\"role\":\"assistant\",\"modelID\":\"{model}\",\"path\":{{\"cwd\":\"{cwd}\"}},\"tokens\":{{\"input\":{input},\"output\":{output},\"reasoning\":{reasoning},\"cache\":{{\"read\":{cache_read},\"write\":{cache_write}}},\"total\":{total}}}}}"
        );
        let leaked: &'static str = Box::leak(data.into_boxed_str());
        (leaked, time_created_ms)
    }

    fn user_row(time_created_ms: i64, cwd: &str) -> (&'static str, i64) {
        let data = format!(
            "{{\"role\":\"user\",\"path\":{{\"cwd\":\"{cwd}\"}}}}"
        );
        let leaked: &'static str = Box::leak(data.into_boxed_str());
        (leaked, time_created_ms)
    }
}
