use anyhow::{Context, Result};
use regex::Regex;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ShotSpan {
    pub index: usize,
    pub shot_id: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration: f64,
    pub is_chained_longtake: bool,
}

pub fn probe_video_duration(video_path: &str) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            video_path,
        ])
        .output()
        .with_context(|| format!("Failed to run ffprobe on '{}'", video_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed: {}", stderr);
    }

    let out_str = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = out_str
        .trim()
        .parse()
        .with_context(|| format!("Failed to parse duration: '{}'", out_str.trim()))?;
    Ok(duration)
}

pub fn parse_time_str(s: &str) -> Result<f64> {
    let s = s.trim();
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            2 => {
                let m: f64 = parts[0].parse()?;
                let sec: f64 = parts[1].parse()?;
                Ok(m * 60.0 + sec)
            }
            3 => {
                let h: f64 = parts[0].parse()?;
                let m: f64 = parts[1].parse()?;
                let sec: f64 = parts[2].parse()?;
                Ok(h * 3600.0 + m * 60.0 + sec)
            }
            _ => anyhow::bail!("Invalid time format: {}", s),
        }
    } else {
        let sec: f64 = s.parse()?;
        Ok(sec)
    }
}

pub fn parse_time_range(range_str: &str, total_duration: f64) -> Result<(f64, f64)> {
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid range format (expected START-END): {}", range_str);
    }
    let start = parse_time_str(parts[0])?;
    let end = parse_time_str(parts[1])?;
    let start = start.max(0.0).min(total_duration);
    let end = end.max(start).min(total_duration);
    Ok((start, end))
}

pub fn detect_scene_cuts(video_path: &str, start_bound: f64, end_bound: f64) -> Result<Vec<f64>> {
    let output = Command::new("ffmpeg")
        .args([
            "-ss",
            &format!("{:.3}", start_bound),
            "-to",
            &format!("{:.3}", end_bound),
            "-i",
            video_path,
            "-filter_complex",
            "select='gt(scene,0.14)',showinfo",
            "-f",
            "null",
            "-",
        ])
        .output()
        .with_context(|| format!("Failed to run ffmpeg scene detection on '{}'", video_path))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let re = Regex::new(r"pts_time:([0-9.]+)")?;
    let mut cuts = Vec::new();

    for cap in re.captures_iter(&stderr) {
        if let Some(m) = cap.get(1) {
            if let Ok(rel_time) = m.as_str().parse::<f64>() {
                let abs_time = start_bound + rel_time;
                if abs_time > start_bound + 0.1 && abs_time < end_bound - 0.1 {
                    cuts.push(abs_time);
                }
            }
        }
    }

    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup_by(|a, b| (*a - *b).abs() < 0.1);
    Ok(cuts)
}

#[allow(dead_code)]
pub fn build_shot_spans(
    video_path: &str,
    range: Option<&str>,
    max_shot_duration: f64,
    min_shot_duration: f64,
) -> Result<Vec<ShotSpan>> {
    let total_duration = probe_video_duration(video_path)?;
    build_shot_spans_with(
        total_duration,
        range,
        max_shot_duration,
        min_shot_duration,
        |s, e| detect_scene_cuts(video_path, s, e).unwrap_or_default(),
    )
}

/// Merge micro-cuts shorter than `min_shot_duration` into an adjacent span
/// whenever the merged span stays within `max_shot_duration` (issue 9).
/// Short spans that cannot merge without exceeding the max are kept as-is.
/// The result is renumbered sequentially (shot_0001, ...).
pub fn merge_short_spans(
    spans: Vec<ShotSpan>,
    min_shot_duration: f64,
    max_shot_duration: f64,
) -> Vec<ShotSpan> {
    if min_shot_duration <= 0.0 || spans.is_empty() {
        return spans;
    }
    let mut merged: Vec<ShotSpan> = Vec::with_capacity(spans.len());
    let mut pending: Option<ShotSpan> = None;
    for span in spans {
        if let Some(p) = pending.take() {
            let combined_dur = p.duration + span.duration;
            if combined_dur <= max_shot_duration {
                let combined = ShotSpan {
                    index: 0,
                    shot_id: String::new(),
                    start_sec: p.start_sec,
                    end_sec: span.end_sec,
                    duration: combined_dur,
                    is_chained_longtake: p.is_chained_longtake || span.is_chained_longtake,
                };
                if combined.duration < min_shot_duration {
                    pending = Some(combined);
                } else {
                    merged.push(combined);
                }
            } else {
                // Cannot merge without exceeding max: keep pending as-is.
                merged.push(p);
                if span.duration < min_shot_duration {
                    pending = Some(span);
                } else {
                    merged.push(span);
                }
            }
        } else if span.duration < min_shot_duration {
            pending = Some(span);
        } else {
            merged.push(span);
        }
    }
    if let Some(p) = pending {
        if let Some(last) = merged.last_mut() {
            if last.duration + p.duration <= max_shot_duration {
                last.end_sec = p.end_sec;
                last.duration += p.duration;
                last.is_chained_longtake = last.is_chained_longtake || p.is_chained_longtake;
            } else {
                merged.push(p);
            }
        } else {
            merged.push(p);
        }
    }
    for (i, s) in merged.iter_mut().enumerate() {
        s.index = i + 1;
        s.shot_id = format!("shot_{:04}", i + 1);
    }
    merged
}

/// Build shot spans from a known total duration and a scene-cut provider
/// (issue 8: the provider may serve cuts from `<root>/input-details/<id>/scene_cuts.json`).
pub fn build_shot_spans_with<F>(
    total_duration: f64,
    range: Option<&str>,
    max_shot_duration: f64,
    min_shot_duration: f64,
    detect: F,
) -> Result<Vec<ShotSpan>>
where
    F: FnOnce(f64, f64) -> Vec<f64>,
{
    let (start_time, end_time) = match range {
        Some(r) => parse_time_range(r, total_duration)?,
        None => (0.0, total_duration),
    };

    if end_time <= start_time {
        anyhow::bail!(
            "Effective range duration must be > 0 (start: {}, end: {})",
            start_time,
            end_time
        );
    }

    let detected_cuts = detect(start_time, end_time);

    let mut boundary_points = Vec::new();
    boundary_points.push(start_time);
    for cut in detected_cuts {
        if cut > start_time + 0.05 && cut < end_time - 0.05 {
            boundary_points.push(cut);
        }
    }
    boundary_points.push(end_time);

    let mut spans = Vec::new();
    let mut shot_counter = 1;

    for i in 0..(boundary_points.len() - 1) {
        let seg_start = boundary_points[i];
        let seg_end = boundary_points[i + 1];
        let seg_duration = seg_end - seg_start;

        if seg_duration <= 0.05 {
            continue;
        }

        if seg_duration <= max_shot_duration {
            let shot_id = format!("shot_{:04}", shot_counter);
            spans.push(ShotSpan {
                index: shot_counter,
                shot_id,
                start_sec: seg_start,
                end_sec: seg_end,
                duration: seg_duration,
                is_chained_longtake: false,
            });
            shot_counter += 1;
        } else {
            // Split long take into pieces <= max_shot_duration
            let num_pieces = (seg_duration / max_shot_duration).ceil() as usize;
            let sub_dur = seg_duration / (num_pieces as f64);
            for p in 0..num_pieces {
                let sub_start = seg_start + (p as f64) * sub_dur;
                let sub_end = if p == num_pieces - 1 {
                    seg_end
                } else {
                    sub_start + sub_dur
                };
                let shot_id = format!("shot_{:04}", shot_counter);
                spans.push(ShotSpan {
                    index: shot_counter,
                    shot_id,
                    start_sec: sub_start,
                    end_sec: sub_end,
                    duration: sub_end - sub_start,
                    is_chained_longtake: true,
                });
                shot_counter += 1;
            }
        }
    }

    Ok(merge_short_spans(
        spans,
        min_shot_duration,
        max_shot_duration,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_str() {
        assert_eq!(parse_time_str("10").unwrap(), 10.0);
        assert_eq!(parse_time_str("01:30").unwrap(), 90.0);
        assert_eq!(parse_time_str("01:02:03").unwrap(), 3723.0);
    }

    #[test]
    fn test_parse_time_range() {
        let (s, e) = parse_time_range("00:01:00-00:02:30", 300.0).unwrap();
        assert_eq!(s, 60.0);
        assert_eq!(e, 150.0);

        let (s2, e2) = parse_time_range("10-25", 100.0).unwrap();
        assert_eq!(s2, 10.0);
        assert_eq!(e2, 25.0);
    }

    fn span(start: f64, end: f64) -> ShotSpan {
        ShotSpan {
            index: 0,
            shot_id: String::new(),
            start_sec: start,
            end_sec: end,
            duration: end - start,
            is_chained_longtake: false,
        }
    }

    #[test]
    fn test_merge_short_spans_noop_when_min_zero() {
        let spans = vec![span(0.0, 0.3), span(0.3, 2.0)];
        let merged = merge_short_spans(spans, 0.0, 8.0);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_short_spans_combines_micro_cut() {
        let spans = vec![span(0.0, 2.0), span(2.0, 2.4), span(2.4, 5.0)];
        let merged = merge_short_spans(spans, 1.0, 8.0);
        assert_eq!(merged.len(), 2);
        // Micro-cut merges forward with the next span (total duration kept).
        assert!((merged[0].duration - 2.0).abs() < 1e-6);
        assert!((merged[1].duration - 3.0).abs() < 1e-6);
        assert_eq!(merged[0].shot_id, "shot_0001");
        assert_eq!(merged[1].shot_id, "shot_0002");
    }

    #[test]
    fn test_merge_short_spans_respects_max() {
        // Merging would exceed max, so the micro-cut is kept as-is.
        let spans = vec![span(0.0, 7.9), span(7.9, 8.2)];
        let merged = merge_short_spans(spans, 1.0, 8.0);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_build_shot_spans_with_min_merges() {
        // Cuts at 2.0 and 2.4 create a 0.4s micro-cut; min=1.0 merges it.
        let spans = build_shot_spans_with(6.0, None, 8.0, 1.0, |_, _| vec![2.0, 2.4]).unwrap();
        assert!(spans.iter().all(|s| s.duration >= 1.0 - 1e-6));
        assert!((spans.iter().map(|s| s.duration).sum::<f64>() - 6.0).abs() < 1e-6);
        let plain = build_shot_spans_with(6.0, None, 8.0, 0.0, |_, _| vec![2.0, 2.4]).unwrap();
        assert_eq!(plain.len(), 3);
    }
}
