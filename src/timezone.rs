use anyhow::{Result, anyhow};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone)]
pub enum AggregationTz {
    Local,
    Iana(Tz),
    Offset(FixedOffset),
}

impl AggregationTz {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        let Some(raw) = raw else {
            return Ok(Self::Local);
        };
        let tz = raw.trim();
        if tz.is_empty() {
            return Err(anyhow!("timezone cannot be empty"));
        }
        if tz.eq_ignore_ascii_case("local") {
            return Ok(Self::Local);
        }
        if tz.eq_ignore_ascii_case("utc") {
            return Ok(Self::Offset(FixedOffset::east_opt(0).expect("zero offset")));
        }
        if strip_ascii_prefix(tz, "utc").is_some() {
            let suffix = strip_ascii_prefix(tz, "utc").unwrap().trim();
            if suffix.is_empty() {
                return Ok(Self::Offset(FixedOffset::east_opt(0).expect("zero offset")));
            }
            let offset =
                parse_offset(suffix).ok_or_else(|| anyhow!("invalid timezone offset: {tz}"))?;
            return Ok(Self::Offset(offset));
        }
        if let Some(offset) = parse_offset(tz) {
            return Ok(Self::Offset(offset));
        }
        match tz.parse::<Tz>() {
            Ok(iana) => Ok(Self::Iana(iana)),
            Err(_) => Err(anyhow!("unsupported timezone: {tz}")),
        }
    }

    pub fn cache_key(&self) -> String {
        match self {
            Self::Local => "local".to_string(),
            Self::Iana(tz) => format!("iana:{tz}"),
            Self::Offset(offset) => format_offset_cache_key(offset),
        }
    }

    pub fn date_for(&self, timestamp: DateTime<Utc>) -> NaiveDate {
        match self {
            Self::Local => timestamp.with_timezone(&Local).date_naive(),
            Self::Iana(tz) => timestamp.with_timezone(tz).date_naive(),
            Self::Offset(offset) => timestamp.with_timezone(offset).date_naive(),
        }
    }
}

fn strip_ascii_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    if input.len() < prefix.len() {
        return None;
    }
    let (head, tail) = input.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

fn parse_offset(input: &str) -> Option<FixedOffset> {
    let trimmed = input.trim();
    let sign = match trimmed.as_bytes().first().copied() {
        Some(b'+') => 1_i32,
        Some(b'-') => -1_i32,
        _ => return None,
    };
    let digits = trimmed[1..].trim();
    if digits.is_empty() {
        return None;
    }

    // Support both colon and compact formats so users can type UTC+8 / +0800 / -3:30.
    // 同时支持冒号与紧凑格式，便于直接输入 UTC+8 / +0800 / -3:30 等快捷写法。
    let (hours_str, minutes_str) = if let Some((h, m)) = digits.split_once(':') {
        (h, m)
    } else if digits.len() >= 3 {
        if !digits.chars().all(|c| c.is_ascii_digit()) || digits.len() > 4 {
            return None;
        }
        digits.split_at(digits.len() - 2)
    } else {
        (digits, "0")
    };

    if !hours_str.chars().all(|c| c.is_ascii_digit())
        || !minutes_str.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let hours: i32 = hours_str.parse().ok()?;
    let minutes: i32 = minutes_str.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total_seconds = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(total_seconds)
}

fn format_offset_cache_key(offset: &FixedOffset) -> String {
    let seconds = offset.local_minus_utc();
    let sign = if seconds >= 0 { '+' } else { '-' };
    let abs = seconds.abs();
    let hours = abs / 3600;
    let minutes = (abs % 3600) / 60;
    format!("offset:{sign}{hours:02}:{minutes:02}")
}

#[cfg(test)]
mod tests {
    use super::AggregationTz;
    use chrono::TimeZone;

    #[test]
    fn parses_iana_timezone() {
        let tz = AggregationTz::parse(Some("Asia/Shanghai")).unwrap();
        assert_eq!(tz.cache_key(), "iana:Asia/Shanghai");
    }

    #[test]
    fn parses_utc_offsets_in_shortcuts() {
        let tz1 = AggregationTz::parse(Some("UTC+8")).unwrap();
        let tz2 = AggregationTz::parse(Some("utc+08:00")).unwrap();
        let tz3 = AggregationTz::parse(Some("+0800")).unwrap();
        assert_eq!(tz1.cache_key(), "offset:+08:00");
        assert_eq!(tz2.cache_key(), "offset:+08:00");
        assert_eq!(tz3.cache_key(), "offset:+08:00");
    }

    #[test]
    fn parses_negative_offset_with_minutes() {
        let tz = AggregationTz::parse(Some("UTC-3:30")).unwrap();
        assert_eq!(tz.cache_key(), "offset:-03:30");
    }

    #[test]
    fn maps_utc_timestamp_to_target_day() {
        let utc = chrono::Utc
            .with_ymd_and_hms(2026, 3, 10, 16, 30, 0)
            .single()
            .unwrap();
        let tz = AggregationTz::parse(Some("UTC+8")).unwrap();
        assert_eq!(tz.date_for(utc).to_string(), "2026-03-11");
    }

    #[test]
    fn rejects_invalid_offset_or_timezone() {
        assert!(AggregationTz::parse(Some("UTC+24")).is_err());
        assert!(AggregationTz::parse(Some("UTC+8:99")).is_err());
        assert!(AggregationTz::parse(Some("Mars/Olympus")).is_err());
    }
}
