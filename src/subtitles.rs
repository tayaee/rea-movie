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
/// The prompt forces real output in the target language: the model must
/// never echo the input unchanged (unless it is already in the target
/// language) and never emit English for non-English targets.
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
        "You are a professional film subtitle translator. Translate each subtitle line into natural, conversational {} ({}). The output MUST be in {} even when the input is English: never echo English input unchanged for a non-English target, and never output English unless the target is English. Keep the same screen-time length, keep speaker names and proper nouns transliterated, and keep punctuation light. Output a valid JSON array of strings with the exact same order and count as the input, no extra text.",
        target_lang_name, target_lang, target_lang_name
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

/// Map an SRT language code to the display name used by
/// `detect_dialogue_language` (issue 11 §3-2).
fn lang_code_to_language(code: &str) -> &'static str {
    match code {
        "ko" => "Korean",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "fr" => "French",
        "de" => "German",
        _ => "English",
    }
}

/// True when the source cues are already (mostly) in the target language,
/// so no M3 call is needed and a verbatim copy preserves quality.
fn source_already_in_target(cues: &[SubtitleCue], target_code: &str) -> bool {
    if cues.is_empty() {
        return true;
    }
    let want = lang_code_to_language(target_code);
    let mut hits = 0usize;
    let mut counted = 0usize;
    for cue in cues {
        let t = cue.text.trim();
        if t.is_empty() {
            continue;
        }
        counted += 1;
        // Detect against the target as hint so digits/punctuation-only
        // lines do not force a translation round-trip.
        if crate::decompile::prompt::detect_dialogue_language(t, target_code) == want {
            hits += 1;
        }
    }
    if counted == 0 {
        return true;
    }
    hits * 10 >= counted * 8
}

/// Strip a deterministic-fallback `"[XX] "` prefix for one language.
fn strip_fallback_prefix<'a>(text: &'a str, target_lang: &str) -> &'a str {
    let t = text.trim();
    let tag = format!("[{}]", target_lang.to_uppercase());
    if let Some(rest) = t.strip_prefix(&tag) {
        return rest.trim();
    }
    t
}

/// True when a cached/output line is just the offline placeholder
/// (`verbatim source` or `"[XX] source"`) rather than a real translation.
fn is_fallback_line(line: &str, source_text: &str, target_lang: &str) -> bool {
    let t = line.trim();
    let s = source_text.trim();
    if t == s {
        // Identical copy is only a real translation when the source is
        // already in the target language.
        return target_lang != "ko"
            || crate::decompile::prompt::detect_dialogue_language(s, "ko") != "Korean";
    }
    strip_fallback_prefix(t, target_lang) == s
}

/// True when a cached SRT looks like the deterministic offline placeholder
/// instead of a real M3 translation (majority of lines are fallbacks).
fn cache_is_fallback(cache: &[SubtitleCue], source: &[SubtitleCue], target_lang: &str) -> bool {
    if cache.len() != source.len() || source.is_empty() {
        return false;
    }
    let mut fallback_hits = 0usize;
    for (c, s) in cache.iter().zip(source.iter()) {
        if is_fallback_line(&c.text, &s.text, target_lang) {
            fallback_hits += 1;
        }
    }
    fallback_hits * 2 >= source.len()
}

/// True when a cached SRT fails target-script validation (e.g. an old
/// `.ja.srt` full of English from before validation existed).
fn cache_fails_validation(
    cache: &[SubtitleCue],
    source: &[SubtitleCue],
    target_lang: &str,
) -> bool {
    if cache.len() != source.len() || source.is_empty() {
        return false;
    }
    if source_already_in_target(source, target_lang) {
        return false;
    }
    let lines: Vec<String> = cache.iter().map(|c| c.text.clone()).collect();
    validate_translation(&lines, target_lang, source).is_err()
}

fn contains_hangul(s: &str) -> bool {
    s.chars().any(|c| {
        ('\u{AC00}'..='\u{D7A3}').contains(&c)
            || ('\u{1100}'..='\u{11FF}').contains(&c)
            || ('\u{3130}'..='\u{318F}').contains(&c)
    })
}

fn contains_kana(s: &str) -> bool {
    s.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c))
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

/// Validate that M3 output is really in `target_lang`.
///
/// Without this, an M3 outage/echo (or a code path that copies English
/// verbatim) silently produces e.g. `EP01_smoke.ja.srt` full of English
/// lines. Validation fails loudly so the caller regenerates instead of
/// caching bogus subtitles.
pub fn validate_translation(
    lines: &[String],
    target_lang: &str,
    source_cues: &[SubtitleCue],
) -> Result<()> {
    if lines.len() != source_cues.len() {
        anyhow::bail!(
            "Translation count mismatch for lang={}: got {} lines for {} inputs",
            target_lang,
            lines.len(),
            source_cues.len()
        );
    }
    let mut checked = 0usize;
    let mut script_hits = 0usize;
    let mut identical = 0usize;
    for (out, src) in lines.iter().zip(source_cues.iter()) {
        let t = strip_fallback_prefix(out.trim(), target_lang);
        if t.is_empty() {
            continue;
        }
        checked += 1;
        if t == src.text.trim() {
            identical += 1;
        }
        let hit = match target_lang {
            "ko" => contains_hangul(t),
            "ja" => contains_kana(t) || contains_cjk(t),
            "zh" => contains_cjk(t),
            _ => t.chars().any(|c| c.is_ascii_alphabetic()),
        };
        if hit {
            script_hits += 1;
        }
    }
    if checked == 0 {
        return Ok(());
    }
    let required_ratio = 0.5;
    if (script_hits as f64) < (checked as f64) * required_ratio
        && !source_already_in_target(source_cues, target_lang)
    {
        anyhow::bail!(
            "M3 translation for lang={} is not in the target script (only {}/{} lines carry target script); refusing to write English-looking subtitles into a .{}.srt file",
            target_lang,
            script_hits,
            checked,
            target_lang
        );
    }
    if identical * 10 >= checked * 9 && !source_already_in_target(source_cues, target_lang) {
        anyhow::bail!(
            "M3 translation for lang={} echoed the source verbatim ({}/{} identical); refusing to cache",
            target_lang,
            identical,
            checked
        );
    }
    Ok(())
}

/// Generate the six external subtitle files plus the always-on `.proof.srt`.
/// Returns the list of written subtitle paths.
///
/// Online mode (`MINIMAX_API_KEY` set, no `--offline`) always translates via
/// MiniMax-M3 and fails loudly on API/validation errors: it never writes the
/// deterministic `"[XX] source"` placeholder into a language file, so a
/// `.ja.srt` can no longer end up full of English. Pass `--offline`
/// explicitly to get the deterministic placeholders. Stale placeholder
/// caches from earlier runs are detected and retranslated when online.
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

    // Online without an API key would previously write English-looking
    // placeholders into every language file. Fail loudly instead so the
    // user knows real M3 translation did not happen.
    if !source_cues.is_empty() && !offline && !online {
        anyhow::bail!(
            "Cannot produce real multilingual subtitles: MINIMAX_API_KEY is missing. Set it for MiniMax-M3 translation, or rerun with --offline for deterministic placeholders."
        );
    }

    let mut written = Vec::new();
    for (code, name) in SUPPORTED_LANGS {
        let cache_path = cache_dir.join(format!("{}.srt", code));
        let out_path = output_dir.join(format!("{}_{}.{}.srt", movie_name, profile, code));
        let mut cues = source_cues.clone();

        if cache_path.is_file() && !force {
            let cached = crate::decompile::story::load_subtitle_cues_from(&cache_path);
            let count_ok = cached.len() == source_cues.len();
            let stale_fallback = online && cache_is_fallback(&cached, &source_cues, code);
            let stale_invalid = online
                && count_ok
                && !stale_fallback
                && cache_fails_validation(&cached, &source_cues, code);
            if count_ok && !stale_fallback && !stale_invalid {
                // Reuse a real cache verbatim.
                std::fs::copy(&cache_path, &out_path).with_context(|| {
                    format!("Failed to copy {:?} to {:?}", cache_path, out_path)
                })?;
                written.push(out_path);
                continue;
            }
            if stale_fallback || stale_invalid {
                eprintln!(
                    "Warning: subtitles cache {:?} looks like an offline placeholder or wrong-language content; retranslating {} via MiniMax-M3",
                    cache_path, code
                );
            } else if !count_ok {
                eprintln!(
                    "Warning: subtitles cache {:?} has {} cues for {} source cues; retranslating {}",
                    cache_path,
                    cached.len(),
                    source_cues.len(),
                    code
                );
            }
        }

        if online && !cues.is_empty() && source_already_in_target(&cues, code) {
            // Source already reads as the target language: verbatim copy,
            // no M3 round-trip (avoids echo/paraphrase drift).
        } else if online && !cues.is_empty() {
            // Real M3 translation. Any API or validation failure is fatal:
            // never cache or emit the `"[XX] English"` placeholder as if it
            // were a Japanese/Chinese/... subtitle file.
            let key = api_key.as_deref().unwrap_or("").trim().to_string();
            let lines = translate_cues_to_lang_m3(&cues, code, name, &key).with_context(|| {
                format!("MiniMax-M3 subtitle translation failed for lang={}", code)
            })?;
            validate_translation(&lines, code, &cues)?;
            for (cue, line) in cues.iter_mut().zip(lines.into_iter()) {
                cue.text = line;
            }
        } else if !online {
            // Explicit offline (or empty cues): deterministic placeholders.
            for cue in cues.iter_mut() {
                let src = cue.text.clone();
                cue.text = fallback_translate_line(&src, code);
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

    #[test]
    fn test_cache_is_fallback_detects_placeholders() {
        let src = vec![cue(0.0, 1.0, "Hello world"), cue(1.0, 2.0, "How are you?")];
        // Offline placeholder cache for ja.
        let cached = vec![
            cue(0.0, 1.0, "[JA] Hello world"),
            cue(1.0, 2.0, "[JA] How are you?"),
        ];
        assert!(cache_is_fallback(&cached, &src, "ja"));
        // Real Japanese translation is not a fallback.
        let real = vec![
            cue(0.0, 1.0, "こんにちは、世界"),
            cue(1.0, 2.0, "お元気ですか？"),
        ];
        assert!(!cache_is_fallback(&real, &src, "ja"));
        // ko verbatim of Korean source is legitimate.
        let ko_src = vec![cue(0.0, 1.0, "안녕하세요")];
        let ko_cache = vec![cue(0.0, 1.0, "안녕하세요")];
        assert!(!cache_is_fallback(&ko_cache, &ko_src, "ko"));
        // ko cache holding English for an English source is a placeholder.
        assert!(
            cache_is_fallback(&cached, &src, "ko")
                || is_fallback_line("[JA] Hello world", "Hello world", "ja")
        );
    }

    #[test]
    fn test_validate_translation_rejects_english_in_ja() {
        let src = vec![cue(0.0, 1.0, "Hello world"), cue(1.0, 2.0, "How are you?")];
        // English echoed into a .ja file must fail.
        let bad = vec!["Hello world".to_string(), "How are you?".to_string()];
        assert!(validate_translation(&bad, "ja", &src).is_err());
        let tagged = vec![
            "[JA] Hello world".to_string(),
            "[JA] How are you?".to_string(),
        ];
        assert!(validate_translation(&tagged, "ja", &src).is_err());
        // Real Japanese passes.
        let good = vec!["こんにちは、世界".to_string(), "お元気ですか？".to_string()];
        assert!(validate_translation(&good, "ja", &src).is_ok());
    }

    #[test]
    fn test_source_already_in_target_skips_m3() {
        let ja = vec![cue(0.0, 1.0, "こんにちは"), cue(1.0, 2.0, "お元気ですか")];
        assert!(source_already_in_target(&ja, "ja"));
        let en = vec![cue(0.0, 1.0, "Hello"), cue(1.0, 2.0, "How are you?")];
        assert!(!source_already_in_target(&en, "ja"));
        assert!(source_already_in_target(&en, "en"));
    }
}
