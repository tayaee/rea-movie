//! Issue 11 (§3-4, §3-3): multilingual external subtitles + proof subtitles.
//!
//! Generates six external `.srt` files (ko/zh/ja/en/fr/de) next to the
//! assembled movie, a speaker-tagged `.proof.srt`, and an optional proof
//! video with burned-in subtitles.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::decompile::story::SubtitleCue;

/// Language codes + display names for the six external subtitle files.
pub const SUPPORTED_LANGS: &[(&str, &str)] = &[
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
    ("en", "English"),
    ("fr", "French"),
    ("de", "German"),
];

pub fn format_timestamp(sec: f64) -> String {
    let total_ms = (sec.max(0.0) * 1000.0).round() as u64;
    let h = total_ms / 3_600_000;
    let m = (total_ms % 3_600_000) / 60_000;
    let s = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, ms)
}

pub fn format_srt(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_timestamp(cue.start_sec),
            format_timestamp(cue.end_sec),
            cue.text
        ));
    }
    out
}

fn write_srt(path: &Path, cues: &[SubtitleCue]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {:?}", parent))?;
    }
    std::fs::write(path, format_srt(cues))
        .with_context(|| format!("Failed to write {:?}", path))?;
    Ok(())
}

/// Load the original subtitle source: `story/transcript.srt` first,
/// falling back to an empty cue list when absent.
pub fn load_source_cues(proj_dir: &Path) -> Vec<SubtitleCue> {
    let transcript = proj_dir.join("story").join("transcript.srt");
    if transcript.is_file() {
        return crate::decompile::story::load_subtitle_cues_from(&transcript);
    }
    Vec::new()
}

/// Deterministic offline fallback translation (no API).
/// `ko` keeps the original verbatim; other languages get a tagged copy so
/// every file stays valid SRT with the same cue count/timings.
pub fn fallback_translate_line(text: &str, target_lang: &str) -> String {
    if target_lang == "ko" {
        return text.to_string();
    }
    format!("[{}] {}", target_lang.to_uppercase(), text)
}

/// Batch-translate cue texts into `target_lang` via MiniMax-M3.
/// Returns the same number of lines in the same order.
pub fn translate_cues_to_lang_m3(
    cues: &[SubtitleCue],
    target_lang: &str,
    target_lang_name: &str,
    api_key: &str,
) -> Result<Vec<String>> {
    if cues.is_empty() {
        return Ok(Vec::new());
    }
    const CHUNK: usize = 30;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let system_prompt = format!(
        "You are a professional film subtitle translator. Translate each subtitle line into natural, conversational {} ({}). Keep the same screen-time length, keep speaker names and proper nouns transliterated, and keep punctuation light. Output a valid JSON array of strings with the exact same order and count as the input, no extra text.",
        target_lang_name, target_lang
    );
    let mut out = Vec::with_capacity(cues.len());
    for chunk in cues.chunks(CHUNK) {
        let texts: Vec<String> = chunk.iter().map(|c| c.text.clone()).collect();
        let user_msg = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": serde_json::to_string(&texts)? }
            ]
        });
        let mut last_err: Option<anyhow::Error> = None;
        let mut translated_chunk: Option<Vec<String>> = None;
        for attempt in 0..3u32 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(5 * attempt as u64));
            }
            let attempt_result: Result<Vec<String>> = (|| {
                let resp = client
                    .post("https://api.minimax.io/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .json(&user_msg)
                    .send()?;
                if !resp.status().is_success() {
                    anyhow::bail!(
                        "MiniMax subtitle translation API returned status: {}",
                        resp.status()
                    );
                }
                let json_val: serde_json::Value = resp.json()?;
                let text = json_val["choices"][0]["message"]["content"]
                    .as_str()
                    .context("Missing content in subtitle translation response")?;
                let body = crate::decompile::prompt::strip_think_blocks(text);
                let translated: Vec<String> =
                    crate::decompile::prompt::parse_llm_json(body.trim())?;
                if translated.len() != chunk.len() {
                    anyhow::bail!(
                        "Translation count mismatch: got {} lines for {} inputs",
                        translated.len(),
                        chunk.len()
                    );
                }
                Ok(translated)
            })();
            match attempt_result {
                Ok(lines) => {
                    translated_chunk = Some(lines);
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "Warning: subtitle translation (lang={}) attempt {} failed: {:#}",
                        target_lang,
                        attempt + 1,
                        e
                    );
                    last_err = Some(e);
                }
            }
        }
        match translated_chunk {
            Some(lines) => out.extend(lines),
            None => {
                return Err(
                    last_err.unwrap_or_else(|| anyhow::anyhow!("subtitle translation failed"))
                );
            }
        }
    }
    Ok(out)
}

/// Generate the six external subtitle files plus the always-on `.proof.srt`.
/// Returns the list of written subtitle paths.
pub fn generate_multilang_subtitles(
    proj_dir: &Path,
    output_dir: &Path,
    movie_name: &str,
    profile: &str,
    force: bool,
    offline: bool,
) -> Result<Vec<PathBuf>> {
    let source_cues = load_source_cues(proj_dir);
    let cache_dir = proj_dir.join("story").join("subtitles_cache");
    std::fs::create_dir_all(&cache_dir).ok();
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

    let api_key = std::env::var("MINIMAX_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    let online = !offline
        && api_key
            .as_deref()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false);

    let mut written = Vec::new();
    for (code, name) in SUPPORTED_LANGS {
        let cache_path = cache_dir.join(format!("{}.srt", code));
        let out_path = output_dir.join(format!("{}_{}.{}.srt", movie_name, profile, code));
        let mut cues = source_cues.clone();

        if cache_path.is_file() && !force {
            // Reuse cache verbatim.
            std::fs::copy(&cache_path, &out_path)
                .with_context(|| format!("Failed to copy {:?} to {:?}", cache_path, out_path))?;
            written.push(out_path);
            continue;
        }

        // Translate (online M3) or deterministic fallback (offline/missing/failed).
        let translated: Option<Vec<String>> = if online && !cues.is_empty() {
            match translate_cues_to_lang_m3(&cues, code, name, api_key.as_deref().unwrap().trim()) {
                Ok(lines) => Some(lines),
                Err(e) => {
                    eprintln!(
                        "Warning: M3 subtitle translation failed for lang={} ({:#}); using deterministic fallback",
                        code, e
                    );
                    None
                }
            }
        } else {
            None
        };
        match translated {
            Some(lines) => {
                for (cue, line) in cues.iter_mut().zip(lines.into_iter()) {
                    cue.text = line;
                }
            }
            None => {
                for cue in cues.iter_mut() {
                    let src = cue.text.clone();
                    cue.text = fallback_translate_line(&src, code);
                }
            }
        }
        write_srt(&cache_path, &cues)?;
        write_srt(&out_path, &cues)?;
        written.push(out_path);
    }

    // Always-on proof subtitle: `[Speaker] line` per cue.
    let proof_path = output_dir.join(format!("{}_{}.proof.srt", movie_name, profile));
    let proof_cues = build_proof_cues(proj_dir, &source_cues);
    write_srt(&proof_path, &proof_cues)?;
    written.push(proof_path);

    Ok(written)
}

/// Build proof cues: each cue prefixed with its shot speaker (`[S1] text`).
fn build_proof_cues(proj_dir: &Path, source_cues: &[SubtitleCue]) -> Vec<SubtitleCue> {
    // Map each cue to the shot containing its start time (via timeline.json).
    let mut speakers: Vec<String> = Vec::new();
    let mut spans: Vec<(f64, f64)> = Vec::new();
    if let Ok(manifest) =
        crate::timeline::TimelineManifest::load_from_file(proj_dir.join("timeline.json"))
    {
        for item in &manifest.shots {
            spans.push((item.start_sec, item.end_sec));
            speakers.push(
                item.characters
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "S1".to_string()),
            );
        }
    }
    // Fallback: derive spans from shot.toml files when timeline.json is absent.
    if spans.is_empty() {
        let shots_dir = proj_dir.join("shots");
        if let Ok(entries) = std::fs::read_dir(&shots_dir) {
            let mut ids: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_str()?.to_string();
                    if n.starts_with("shot_") && e.path().is_dir() {
                        Some(n)
                    } else {
                        None
                    }
                })
                .collect();
            ids.sort();
            for (i, id) in ids.iter().enumerate() {
                let meta_path = shots_dir.join(id).join("shot.toml");
                if let Ok(meta) = crate::timeline::ShotMeta::load_from_file(&meta_path) {
                    spans.push((meta.start_time, meta.end_time));
                    speakers.push(
                        meta.characters
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "S1".to_string()),
                    );
                } else {
                    let _ = i;
                }
            }
        }
    }

    source_cues
        .iter()
        .map(|cue| {
            let speaker = speaker_for_cue(cue.start_sec, &spans, &speakers);
            let body = strip_existing_speaker_tag(&cue.text);
            SubtitleCue {
                start_sec: cue.start_sec,
                end_sec: cue.end_sec,
                text: format!("[{}] {}", speaker, body),
            }
        })
        .collect()
}

fn speaker_for_cue(start_sec: f64, spans: &[(f64, f64)], speakers: &[String]) -> String {
    const EPS: f64 = 0.15;
    for (i, (s, e)) in spans.iter().enumerate() {
        if start_sec >= s - EPS && start_sec < e - EPS {
            return speakers.get(i).cloned().unwrap_or_else(|| "S1".to_string());
        }
    }
    // Fallback: largest overlap.
    let mut best: Option<(usize, f64)> = None;
    for (i, (s, e)) in spans.iter().enumerate() {
        let overlap = (e.min(start_sec + 0.01) - s.max(start_sec)).max(0.0);
        if overlap > 0.0 && best.map(|(_, b)| overlap > b).unwrap_or(true) {
            best = Some((i, overlap));
        }
    }
    best.and_then(|(i, _)| speakers.get(i).cloned())
        .unwrap_or_else(|| "S1".to_string())
}

fn strip_existing_speaker_tag(text: &str) -> String {
    let t = text.trim();
    // Remove a leading `[Sx ...]` tag so the proof line has exactly one tag.
    if t.starts_with('[') {
        if let Some(close) = t.find(']') {
            let inner = t[1..close].trim().to_ascii_uppercase();
            let head: String = inner.split_whitespace().next().unwrap_or("").to_string();
            if head.len() >= 2
                && head.starts_with('S')
                && head[1..].chars().all(|c| c.is_ascii_digit())
            {
                return t[close + 1..].trim().to_string();
            }
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(s: f64, e: f64, t: &str) -> SubtitleCue {
        SubtitleCue {
            start_sec: s,
            end_sec: e,
            text: t.to_string(),
        }
    }

    #[test]
    fn test_format_srt_roundtrip() {
        let cues = vec![cue(0.5, 2.0, "Hello"), cue(2.5, 4.0, "World")];
        let srt = format_srt(&cues);
        assert!(srt.contains("00:00:00,500 --> 00:00:02,000"));
        assert!(srt.contains("00:00:02,500 --> 00:00:04,000"));
        let parsed = crate::decompile::story::parse_srt(&srt);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "Hello");
    }

    #[test]
    fn test_fallback_translate_keeps_ko_and_tags_others() {
        assert_eq!(fallback_translate_line("안녕", "ko"), "안녕");
        assert_eq!(fallback_translate_line("Hello", "en"), "[EN] Hello");
        assert_eq!(fallback_translate_line("Hello", "fr"), "[FR] Hello");
    }

    #[test]
    fn test_strip_existing_speaker_tag() {
        assert_eq!(
            strip_existing_speaker_tag("[S2] 이쪽으로 와."),
            "이쪽으로 와."
        );
        assert_eq!(strip_existing_speaker_tag("Hello"), "Hello");
    }
}
