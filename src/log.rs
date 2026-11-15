//! Timestamped operation log helper.
//!
//! Every user-facing log line is prefixed with an ISO-8601 timestamp in the
//! local node's timezone at second precision so per-operation durations can be
//! read directly from the log.

use chrono::Local;

/// Current time as ISO-8601 in the local timezone, second precision
/// (e.g. `2026-11-15T09:12:33+09:00`).
pub fn stamp() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Total seconds as `MM:SS` (e.g. `02:31`).
pub fn mmss(total_secs: f64) -> String {
    let total = total_secs.max(0.0).round() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Elapsed wall time as `#m##s` (e.g. `4m07s`).
pub fn took(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{}m{:02}s", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_formats_minutes_and_seconds() {
        assert_eq!(mmss(0.0), "00:00");
        assert_eq!(mmss(5.566), "00:06");
        assert_eq!(mmss(151.0), "02:31");
    }

    #[test]
    fn took_formats_minutes_and_seconds() {
        assert_eq!(took(std::time::Duration::from_secs(0)), "0m00s");
        assert_eq!(took(std::time::Duration::from_secs(252)), "4m12s");
    }

    #[test]
    fn stamp_is_iso8601_second_precision_in_local_zone() {
        let s = stamp();
        let local_offset = Local::now().format("%:z").to_string();
        assert!(
            s.ends_with(&local_offset),
            "stamp must carry local offset {local_offset}, got {s}"
        );
        assert_eq!(s.len(), "2026-11-15T09:12:33+09:00".len(), "got {s}");
        assert_eq!(&s[10..11], "T", "got {s}");
    }
}
