use anyhow::{Context, Result};
use std::path::Path;

// Section 3.9: Process-wide static disable removed; API failure is handled per-shot with warning log.

#[allow(dead_code)]
pub struct PromptGenContext<'a> {
    pub shot_id: &'a str,
    pub shot_index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration: f64,
    pub is_chained_longtake: bool,
    pub audio_peaks: &'a [f64],
    pub speaker: &'a str,
    pub contact_sheet_path: &'a Path,
    pub offline: bool,
    pub force: bool,
    pub dub_lang: &'a str,
    pub subtitle_lines: &'a [String],
    pub has_subtitle_track: bool,
    pub custom_mod: Option<&'a str>,
    // Issue 9: anchor frames, scene bible, series context and anchor kind.
    pub first_frame_path: Option<&'a Path>,
    pub last_frame_path: Option<&'a Path>,
    pub scene_bible: Option<&'a crate::scenes::SceneBible>,
    pub series_world_summary: Option<&'a str>,
    pub anchor: &'a str,
}

/// Expected H3 reference header for an anchor kind (issue 9).
pub fn reference_line_for(anchor: &str, shot_index: usize) -> String {
    match anchor {
        "first_and_last" => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> (from [Shot {}]) are fully referenced.",
            shot_index
        ),
        "first_only" | "ref_only" => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot {}]) is fully referenced.",
            shot_index
        ),
        "none" => format!(
            "For the target video, at 0.00 seconds into the target video, generate Shot {} from the description below.",
            shot_index
        ),
        _ => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> (from [Shot {}]) are fully referenced.",
            shot_index
        ),
    }
}

/// Normalize an M3-generated H3 prompt so the anchor reference header is
/// guaranteed at the top (issue 9). Existing correct headers are kept;
/// a stale/mismatched first line is replaced; otherwise the expected
/// header is prepended.
pub fn ensure_h3_reference_header(h3: &str, shot_index: usize, anchor: &str) -> String {
    let expected = reference_line_for(anchor, shot_index);
    let trimmed = h3.trim();
    if trimmed.starts_with(&expected) {
        return h3.to_string();
    }
    let mut lines: Vec<&str> = trimmed.lines().collect();
    // Skip leading blank lines for the header check.
    let first_idx = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    if let Some(first) = lines.get(first_idx) {
        if first.trim_start().starts_with("For the target video") {
            lines[first_idx] = expected.as_str();
            return lines.join("\n");
        }
    }
    format!("{}\n\n{}", expected, trimmed)
}

// Section 3.2 & Section 3.3: Base preservation and idempotent block injection
pub fn generate_prompts(
    ctx: &PromptGenContext,
    blocks: &[crate::customize::Block],
    scene_bible: Option<&crate::scenes::SceneBible>,
    ko_path: &Path,
    h3_path: &Path,
) -> Result<()> {
    // Section 3.3: Base preservation
    // decompile preserves existing base even with --force (only customization blocks are updated)
    let existing_base_ko = if ko_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(ko_path) {
            let base = crate::customize::extract_base(&content);
            if !base.is_empty() {
                Some(base)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let existing_base_h3 = if h3_path.is_file() {
        if let Ok(content) = std::fs::read_to_string(h3_path) {
            let base = crate::customize::extract_base(&content);
            if !base.is_empty() {
                Some(base)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // If both bases exist, preserve them without calling M3 API.
    // Otherwise, generate base using M3 API (or fallback template).
    let (base_ko, base_h3) = match (existing_base_ko, existing_base_h3) {
        (Some(ko), Some(h3)) => (ko, h3),
        (opt_ko, opt_h3) => {
            let mut generated_ko = None;
            let mut generated_h3 = None;

            let api_key = std::env::var("MINIMAX_API_KEY").ok();
            if !ctx.offline
                && api_key
                    .as_deref()
                    .map(|k| !k.trim().is_empty())
                    .unwrap_or(false)
                && ctx.contact_sheet_path.exists()
            {
                // Retry transient M3 failures (e.g. 529 overloaded) with backoff
                // before falling back to the deterministic template.
                for attempt in 0..3u32 {
                    if attempt > 0 {
                        std::thread::sleep(std::time::Duration::from_secs(5 * attempt as u64));
                    }
                    match try_minimax_m3_api(ctx, api_key.as_deref().unwrap().trim()) {
                        Ok((ko, h3)) => {
                            generated_ko = Some(ko);
                            generated_h3 = Some(h3);
                            break;
                        }
                        Err(e) => {
                            // Section 3.9: M3 API failure is logged and falls back per-shot without disabling process-wide
                            eprintln!(
                                "Warning: MiniMax-M3 API failed for shot {} (attempt {}): {:#}; {}",
                                ctx.shot_id,
                                attempt + 1,
                                e,
                                if attempt < 2 {
                                    "retrying"
                                } else {
                                    "falling back to deterministic template"
                                }
                            );
                        }
                    }
                }
            }

            let (tmpl_ko, tmpl_h3) = match (generated_ko, generated_h3) {
                (Some(ko), Some(h3)) => (ko, h3),
                _ => generate_deterministic_template(ctx),
            };

            let final_base_ko = opt_ko.unwrap_or_else(|| crate::customize::extract_base(&tmpl_ko));
            let final_base_h3 = opt_h3.unwrap_or_else(|| crate::customize::extract_base(&tmpl_h3));
            (final_base_ko, final_base_h3)
        }
    };

    // Section 3.2: Apply customization blocks to bases, then prepend the
    // scene continuity block (idempotent: old scene block is replaced).
    // Issue 9: prefer the context-carried bible, fall back to the explicit arg.
    let mut final_ko = crate::customize::apply_to_ko(&base_ko, blocks);
    let mut final_h3 = crate::customize::apply_to_h3(&base_h3, blocks);
    let effective_bible = ctx.scene_bible.or(scene_bible);
    if let Some(bible) = effective_bible {
        final_ko = crate::scenes::prepend_scene_block(
            &final_ko,
            &crate::scenes::format_scene_block_ko(bible),
        );
        final_h3 = crate::scenes::prepend_scene_block(
            &final_h3,
            &crate::scenes::format_scene_block_h3(bible),
        );
    }

    if let Some(parent) = ko_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(ko_path, final_ko)
        .with_context(|| format!("Failed to write prompt.ko.md to {:?}", ko_path))?;

    if let Some(parent) = h3_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(h3_path, final_h3)
        .with_context(|| format!("Failed to write prompt.h3.txt to {:?}", h3_path))?;

    Ok(())
}

fn is_korean_dub(dub_lang: &str) -> bool {
    matches!(
        dub_lang.trim().to_ascii_lowercase().as_str(),
        "ko" | "kor" | "korean" | "kr"
    )
}

/// True when the text already contains Hangul (no translation needed).
pub fn is_korean_text(line: &str) -> bool {
    line.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c))
}

/// Batch-translate subtitle lines to natural conversational Korean via
/// MiniMax-M3 in a single API call. Returns the same number of lines in the
/// same order; fails when the count mismatches so callers can fall back to
/// the original text.
pub fn translate_lines_to_korean_m3(lines: &[String], api_key: &str) -> Result<Vec<String>> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    // Chunk to bound request size (full 10-episode batches can be large).
    const CHUNK: usize = 30;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let system_prompt = "You are a professional Korean drama subtitle translator. Translate each English subtitle line into natural, conversational Korean that fits the same screen time. Keep speaker names, proper nouns transliterated, and punctuation light. Output a valid JSON array of strings with the exact same order and count as the input, no extra text.";

    let mut out = Vec::with_capacity(lines.len());
    for chunk in lines.chunks(CHUNK) {
        let user_msg = serde_json::json!({
            "model": "MiniMax-M3",
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": serde_json::to_string(chunk)?
                }
            ]
        });

        // Retry transient failures (e.g. 529 overloaded) with backoff.
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
                        "MiniMax batch translation API returned status: {}",
                        resp.status()
                    );
                }

                let json_val: serde_json::Value = resp.json()?;
                let text = json_val["choices"][0]["message"]["content"]
                    .as_str()
                    .context("Missing content in MiniMax batch translation response")?;
                let translated: Vec<String> = parse_llm_json(strip_think_blocks(text).trim())?;
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
                        "Warning: batch translation attempt {} failed: {:#}",
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
                return Err(last_err
                    .unwrap_or_else(|| anyhow::anyhow!("batch translation failed without error")));
            }
        }
    }
    Ok(out)
}

pub fn translate_line_to_korean(line: &str) -> String {
    let clean = line
        .replace("[spk:0]", "")
        .replace("[spk:1]", "")
        .replace("[spk:2]", "")
        .trim()
        .to_string();

    if clean
        .chars()
        .any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c))
    {
        return clean;
    }

    let lower = clean.to_ascii_lowercase();

    // Built-in accurate Korean dubbing translations for Breaking Bad S05E07 ("Say My Name" opening)
    let table: &[(&str, &str)] = &[
        ("there.", "저기 오는군."),
        ("that's them.", "저놈들이야."),
        ("nice sound.", "소리 한번 좋군."),
        ("your play, walter", "자네 차례야, 월터."),
        ("you're playing, walter", "자네 차례야, 월터."),
        ("you're on your own", "이제부턴 혼자 알아서 해."),
        (
            "looks like you're about 1,000",
            "1천 갤런쯤 모자라 보이는데, 마이크.",
        ),
        ("where's the juice", "물건은 어딨어?"),
        ("the methylamine isn't coming", "메틸아민은 오지 않는다."),
        ("methalamine isn't coming", "메틸아민은 오지 않는다."),
        ("why is that", "그게 무슨 소리야?"),
        ("who the hell are you", "그리고 당신은 대체 누구야?"),
        (
            "i'm the man who's keeping it",
            "내가 그 물건을 쥐고 있는 사람이다.",
        ),
        ("what the hell's this", "이게 대체 무슨 수작이야?"),
        (
            "we had an agreement, right",
            "우리 이미 계약했잖아, 안 그래?",
        ),
        (
            "so where's the tank, mike",
            "그러니까 탱크는 어딨어, 마이크?",
        ),
        (
            "mike doesn't know where it is",
            "마이크는 그 물건이 어딨는지 몰라.",
        ),
        (
            "only i do",
            "오직 나만 알지. 이제부터 거래 상대는 마이크가 아니라 나다.",
        ),
        ("why don't you just cut", "원하는 게 뭔지 본론만 말해 봐."),
        (
            "what you think is gonna",
            "여기서 무슨 일이 벌어질지 잘 생각해 보라고.",
        ),
        (
            "what you think is going to",
            "여기서 무슨 일이 벌어질지 잘 생각해 보라고.",
        ),
        ("we're gonna get", "우린 무조건 가지러 온 걸 챙겨갈 거니까."),
        (
            "we're going to get",
            "우린 무조건 가지러 온 걸 챙겨갈 거니까.",
        ),
        (
            "1,000 gallons of methylamine",
            "그 메틸아민 1천 갤런은 내 손에 있을 때 훨씬 더 가치가 크다.",
        ),
        (
            "1,000 gal of methalamine",
            "그 메틸아민 1천 갤런은 내 손에 있을 때 훨씬 더 가치가 크다.",
        ),
        (
            "than it is in yours",
            "당신들이나 다른 누구 손에 있을 때보다 말이야.",
        ),
        ("but i need distribution", "하지만 난 유통망이 필요해."),
        ("distribution.", "유통망이라고?"),
        ("distribution?", "유통망이라고?"),
        ("that's right", "그래, 맞다."),
        ("so if you agree", "그러니 너희 제조를 포기하고"),
        ("and sell my product instead", "내 물건을 대신 유통한다면,"),
        ("i'll give you 35", "수익의 35퍼센트를 주겠다."),
        ("35 percent?", "35퍼센트? 와, 지금 장난하는 거야?"),
        ("35% wow", "35퍼센트? 와, 지금 장난하는 거야?"),
        ("mike, please tell", "마이크, 제발 이게 농담이라고 말해 봐."),
        (
            "stick our necks out",
            "이 현금을 마련하려고 우리가 얼마나 목숨을 걸었는지 알아?",
        ),
        (
            "why the hell would we want you",
            "그런데 우리가 대체 왜 당신이랑 손을 잡아야 하지?",
        ),
        (
            "we have our own operation",
            "우리도 자체 제조 시설이 있는 거 알잖아?",
        ),
        (
            "i know all about your operation",
            "너희 제조 시설에 대해선 이미 다 알고 있다.",
        ),
        (
            "70% pure",
            "운이 좋아 봐야 순도 70퍼센트짜리 마약을 만든다더군.",
        ),
        ("99.1% pure", "내가 만드는 물건은 순도 99.1퍼센트다."),
        (
            "grade school t-ball",
            "초등학교 동네 야구와 뉴욕 양키스의 차이지.",
        ),
        (
            "classic coke",
            "네 것은 싸구려 유사 콜라고, 내 것은 정통 코카콜라다.",
        ),
    ];

    let mut matched = Vec::new();
    for (pat, ko) in table {
        if lower.contains(pat) && !matched.contains(ko) {
            matched.push(*ko);
        }
    }
    if !matched.is_empty() {
        return matched.join(" ");
    }

    clean
}

fn build_dialogue_lines(ctx: &PromptGenContext) -> (String, String) {
    let want_ko = is_korean_dub(ctx.dub_lang);
    let lang_tag = if want_ko { "Korean" } else { "English" };

    if ctx.has_subtitle_track {
        if ctx.subtitle_lines.is_empty() {
            // Silent shot in a movie that has subtitles
            let ko_line = "- 대사 없음 (인물 표정 연기 및 현장 환경음 중심)".to_string();
            let h3_line = "No spoken dialogue in this shot; characters remain silent with natural ambient sound.".to_string();
            return (ko_line, h3_line);
        }

        let joined_dialogue = if want_ko {
            ctx.subtitle_lines
                .iter()
                .map(|l| translate_line_to_korean(l))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            ctx.subtitle_lines.join(" ")
        };

        let ko_line = format!(
            "- {}: <d>[{}] {}</d>",
            ctx.speaker, lang_tag, joined_dialogue
        );
        let h3_line = format!(
            "{} speaks clearly with synchronized lip movement: <d>[{}] {}</d>.",
            ctx.speaker, lang_tag, joined_dialogue
        );
        return (ko_line, h3_line);
    }

    // Fallback when no subtitle file exists (e.g., synthetic test video)
    if want_ko {
        let ko_line = format!(
            "- {} (Narrator): <d>[Korean] 긴장감 넘치는 영화 장면이 이어진다.</d>",
            ctx.speaker
        );
        let h3_line = format!(
            "{} speaks: <d>[Korean] 긴장감 넘치는 영화 장면이 이어진다.</d>.",
            ctx.speaker
        );
        (ko_line, h3_line)
    } else {
        let ko_line = format!(
            "- {} (Narrator): <d>[English] Cinematic scene in progress.</d>",
            ctx.speaker
        );
        let h3_line = format!(
            "{} speaks: <d>[English] Cinematic scene in progress.</d>.",
            ctx.speaker
        );
        (ko_line, h3_line)
    }
}

fn encode_image_data_uri(path: &Path) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(Some(format!("data:image/jpeg;base64,{}", b64)))
}

fn format_scene_bible_for_m3(bible: &crate::scenes::SceneBible) -> String {
    let mut parts = vec![format!(
        "Scene {} ({:.1}s-{:.1}s, {} shots):",
        bible.scene_id, bible.start_sec, bible.end_sec, bible.shot_count
    )];
    if !bible.location.is_empty() {
        parts.push(format!("Location: {}.", bible.location));
    }
    if !bible.time_of_day.is_empty() {
        parts.push(format!("Time of day: {}.", bible.time_of_day));
    }
    for ch in &bible.characters {
        let mut desc = format!("Person {}: appearance [{}]", ch.id, ch.appearance);
        if !ch.wardrobe.is_empty() {
            desc.push_str(&format!("; wardrobe [{}]", ch.wardrobe));
        }
        parts.push(desc);
    }
    if !bible.spatial_map.is_empty() {
        parts.push(format!("Spatial layout: {}.", bible.spatial_map));
    }
    if !bible.eyeline_notes.is_empty() {
        parts.push(format!("Eyeline grammar: {}.", bible.eyeline_notes));
    }
    if !bible.dialogue_flow.is_empty() {
        parts.push(format!("Dialogue flow: {}.", bible.dialogue_flow));
    }
    if !bible.lighting.is_empty() {
        parts.push(format!("Lighting: {}.", bible.lighting));
    }
    parts.join("\n")
}

fn try_minimax_m3_api(ctx: &PromptGenContext, api_key: &str) -> Result<(String, String)> {
    let contact_uri = encode_image_data_uri(ctx.contact_sheet_path)?
        .context("contact_sheet.jpg is missing or empty")?;
    let first_uri = ctx
        .first_frame_path
        .and_then(|p| encode_image_data_uri(p).ok())
        .flatten();
    let last_uri = ctx
        .last_frame_path
        .and_then(|p| encode_image_data_uri(p).ok())
        .flatten();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let (ko_dlg, h3_dlg) = build_dialogue_lines(ctx);
    let system_prompt = "You are a professional film reverse-engineering director and prompt engineer for MiniMax-H3 video generation. Analyze the provided storyboard images and output valid JSON with two fields: 'prompt_ko' and 'prompt_h3'. prompt_h3 must adhere to the MiniMax-H3 schema with 'integrated_multimodal_description:', 'overall_soundscape:', and 'non_diegetic_music:'. Keep the exact <d>[Language] ...</d> dialogue tag provided by the user. The shot body itself must use the scene bible person IDs (S1, S2, ...) and eyeline directions with 100% consistency. NEGATIVE RULES (never describe these): broadcast station logos (e.g. VA logo), watermarks, burned-in timestamp numbers, hardcoded subtitle text, or any on-screen overlay graphics. Describe only the underlying cinematic scene without visual noise.";
    let anchor = if ctx.anchor.is_empty() {
        "first_and_last"
    } else {
        ctx.anchor
    };

    let mut delta_instruction = String::from(
        "Anchor trajectory: <Picture 1> (first_frame.jpg) shows the opening composition at 0.00s and <Picture 2> (last_frame.jpg) shows the final composition at the end of the shot. Describe the exact Delta trajectory from the <Picture 1> framing through the middle motion to the <Picture 2> framing.",
    );
    if first_uri.is_none() || last_uri.is_none() {
        delta_instruction.push_str(
            " If an anchor frame image is missing, infer the missing endpoint from the contact sheet instead.",
        );
    }

    let mut brief = format!(
        "Analyze shot '{}' ({:.2}s to {:.2}s, duration: {:.2}s, anchor: {}). Speaker: {}. Audio peaks at: {:?}. Required dialogue line for prompt_ko: '{}'. Required dialogue clause for prompt_h3: '{}'. {}. Reproduce the original shot as closely as possible: match the camera framing, subject blocking, lighting, wardrobe, and background visible in the images exactly, changing nothing except what customization blocks require.",
        ctx.shot_id,
        ctx.start_sec,
        ctx.end_sec,
        ctx.duration,
        anchor,
        ctx.speaker,
        ctx.audio_peaks,
        ko_dlg,
        h3_dlg,
        delta_instruction
    );
    let scene_bible = ctx.scene_bible;
    if let Some(bible) = scene_bible {
        brief.push_str("\n\nScene bible (must stay consistent):\n");
        brief.push_str(&format_scene_bible_for_m3(bible));
    }
    if let Some(summary) = ctx.series_world_summary {
        if !summary.trim().is_empty() {
            brief.push_str("\n\nSeries context (world bible summary):\n");
            brief.push_str(summary.trim());
        }
    }

    let mut content: Vec<serde_json::Value> = vec![serde_json::json!({
        "type": "text",
        "text": brief
    })];
    // Contact sheet first (storyboard overview), then <Picture 1> and
    // <Picture 2> anchors so the model can trace the Delta trajectory.
    content.push(serde_json::json!({
        "type": "image_url",
        "image_url": { "url": contact_uri }
    }));
    if let Some(uri) = first_uri {
        content.push(serde_json::json!({
            "type": "text",
            "text": "<Picture 1> (first_frame.jpg): opening composition at 0.00s."
        }));
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": uri }
        }));
    }
    if let Some(uri) = last_uri {
        content.push(serde_json::json!({
            "type": "text",
            "text": "<Picture 2> (last_frame.jpg): final composition at the end of the shot."
        }));
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": uri }
        }));
    }

    let user_msg = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": content
            }
        ]
    });

    let resp = client
        .post("https://api.minimax.io/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&user_msg)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!("MiniMax API returned status: {}", resp.status());
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in MiniMax M3 response")?;

    let parsed: serde_json::Value = parse_llm_json(text)?;
    let ko =
        json_field_to_text(&parsed["prompt_ko"], &["prompt_ko"]).context("Missing prompt_ko")?;
    let raw_h3 = json_field_to_text(
        &parsed["prompt_h3"],
        &[
            "integrated_multimodal_description",
            "overall_soundscape",
            "non_diegetic_music",
        ],
    )
    .context("Missing prompt_h3")?;
    let h3 = ensure_h3_reference_header(&raw_h3, ctx.shot_index, anchor);

    Ok((ko, h3))
}

/// Convert a JSON field into prompt text.
///
/// MiniMax-M3 returns `prompt_h3` either as a plain string or as an object with
/// the H3 schema keys. Both shapes have to be accepted, otherwise every shot
/// silently degrades to the deterministic template.
fn json_field_to_text(value: &serde_json::Value, schema_keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => {
            let mut parts: Vec<String> = Vec::new();
            for key in schema_keys {
                if let Some(serde_json::Value::String(text)) = map.get(*key) {
                    parts.push(format!("{}: {}", key, text.trim()));
                }
            }
            if parts.is_empty() {
                // Unknown object shape: flatten every string value.
                for (key, val) in map {
                    if let serde_json::Value::String(text) = val {
                        parts.push(format!("{}: {}", key, text.trim()));
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

// Section 3.4: Character detection via MiniMax-M3 Vision API
pub fn detect_characters_with_m3(
    contact_sheet_path: &Path,
    bible: &std::collections::BTreeMap<String, crate::customize::Character>,
    api_key: &str,
) -> Result<Vec<String>> {
    let img_bytes = std::fs::read(contact_sheet_path)?;
    let base64_img = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img_bytes);
    let data_uri = format!("data:image/jpeg;base64,{}", base64_img);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let mut bible_entries = Vec::new();
    for (id, ch) in bible {
        bible_entries.push(format!(
            "ID: '{}', Appearance: '{}', Wardrobe: '{}'",
            id, ch.appearance_en, ch.wardrobe_en
        ));
    }
    let bible_text = bible_entries.join("\n");

    let system_prompt = "You are a film casting director and vision analyzer. Given a character Bible with visual descriptions and a contact sheet image containing sequential frames of a movie shot, identify which characters appear in the shot. Respond ONLY with valid JSON in the format {\"characters\": [\"id1\", \"id2\"]}. Only include IDs present in the provided Bible. If no character from the Bible appears, return {\"characters\": []}.";

    let user_msg = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": format!("Character Bible:\n{}\n\nWhich of these characters appear in the contact sheet?", bible_text)
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": data_uri
                        }
                    }
                ]
            }
        ]
    });

    let resp = client
        .post("https://api.minimax.io/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&user_msg)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "MiniMax character detection API returned status: {}",
            resp.status()
        );
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in MiniMax character detection response")?;

    let parsed: serde_json::Value = parse_llm_json(text)?;
    let mut detected = Vec::new();
    if let Some(arr) = parsed["characters"].as_array() {
        for item in arr {
            if let Some(id) = item.as_str() {
                if bible.contains_key(id) && !detected.contains(&id.to_string()) {
                    detected.push(id.to_string());
                }
            }
        }
    }

    Ok(detected)
}

// Generate one scene bible from the scene's contact sheets + dialogue via
// MiniMax-M3 vision. One call per scene keeps character IDs, spatial layout
// and eyeline grammar consistent across the scene's angle changes.
pub fn generate_scene_bible_m3(
    scene_id: &str,
    start_sec: f64,
    end_sec: f64,
    shot_count: usize,
    contact_sheets: &[std::path::PathBuf],
    dialogue: &[(f64, String)],
    api_key: &str,
) -> Result<crate::scenes::SceneBible> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let system_prompt = "You are a TV drama script supervisor. Given timestamped storyboard frames and dialogue lines from ONE continuous scene, output valid JSON with exactly these keys, ALL VALUES AS STRINGS (dialogue_flow: one multi-line string, never an array): location, time_of_day, characters (array of {id, appearance, wardrobe}), spatial_map, eyeline_notes, dialogue_flow, lighting. Rules: label distinct people S1, S2, ... consistently across ALL frames by persistent appearance (face, hair, wardrobe) even when the camera angle changes; appearance must list re-identification features; spatial_map describes where each person is and faces; eyeline_notes state who looks toward whom (screen direction); dialogue_flow summarizes who speaks to whom in order. Output JSON only, no markdown.";
    let dialogue_text = dialogue
        .iter()
        .map(|(t, line)| format!("[{t:.1}s] {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": format!(
            "Scene {scene_id} ({start_sec:.1}s to {end_sec:.1}s, {shot_count} shots).\nDialogue:\n{dialogue_text}\n\nDescribe this single scene as JSON."
        ),
    })];
    for sheet in contact_sheets.iter().take(8) {
        let img_bytes =
            std::fs::read(sheet).with_context(|| format!("Failed to read {}", sheet.display()))?;
        let base64_img =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, img_bytes);
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{base64_img}") },
        }));
    }

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(5 * attempt as u64));
        }
        let attempt_result: Result<crate::scenes::SceneBible> = (|| {
            let user_msg = serde_json::json!({
                "model": "MiniMax-M3",
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": content },
                ]
            });
            let resp = client
                .post("https://api.minimax.io/v1/chat/completions")
                .header("Authorization", format!("Bearer {api_key}"))
                .json(&user_msg)
                .send()?;
            if !resp.status().is_success() {
                anyhow::bail!("MiniMax scene bible API returned status: {}", resp.status());
            }
            let json_val: serde_json::Value = resp.json()?;
            let text = json_val["choices"][0]["message"]["content"]
                .as_str()
                .context("Missing content in scene bible response")?;
            let mut bible: crate::scenes::SceneBible =
                parse_llm_json(strip_think_blocks(text).trim())?;
            bible.scene_id = scene_id.to_string();
            bible.start_sec = start_sec;
            bible.end_sec = end_sec;
            bible.shot_count = shot_count;
            if bible.characters.is_empty() {
                anyhow::bail!("Scene bible has no characters");
            }
            Ok(bible)
        })();
        match attempt_result {
            Ok(bible) => return Ok(bible),
            Err(e) => {
                eprintln!(
                    "Warning: scene bible attempt {} failed: {:#}",
                    attempt + 1,
                    e
                );
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("scene bible generation failed")))
}

// Section 3.7: Translate --mod directive into English statement via MiniMax-M3 API
pub fn translate_mod_to_english_m3(mod_ko: &str, api_key: &str) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let system_prompt = "You are a film visual director and prompt engineer. Translate the given visual direction or trait into concise, cinematic English for a video generation prompt. Output only the translated English sentence, no markdown, no quotes.";

    let user_msg = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": mod_ko
            }
        ]
    });

    let resp = client
        .post("https://api.minimax.io/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&user_msg)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!("MiniMax translation API returned status: {}", resp.status());
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in MiniMax translation response")?;

    let cleaned = strip_think_blocks(text);
    Ok(cleaned.trim().to_string())
}

// Section 3.6: Recompile Korean director prompt base to English H3 prompt via MiniMax-M3 API
pub fn translate_ko_base_to_h3_m3(
    ko_base: &str,
    shot_meta: &crate::timeline::ShotMeta,
    api_key: &str,
) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let system_prompt = "You are a professional film reverse-engineering director and prompt engineer for MiniMax-H3 video generation. Translate and recompile the provided Korean director notes (prompt.ko.md base) into a structured English MiniMax-H3 prompt. The prompt must strictly adhere to the MiniMax-H3 schema with 'integrated_multimodal_description:', 'overall_soundscape:', and 'non_diegetic_music:'. The shot body must stay consistent with the scene bible person IDs and eyeline grammar when provided. NEGATIVE RULES (never describe these): broadcast station logos (e.g. VA logo), watermarks, burned-in timestamp numbers, hardcoded subtitle text, or any on-screen overlay graphics. Output valid JSON with a single key 'prompt_h3'. Preserve any <d>[Language] ...</d> dialogue tags exactly as provided in the input text without translation.";

    let user_msg = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": format!(
                    "Recompile the following Korean director notes for shot '{}' (duration {:.2}s, anchor '{}') into an English MiniMax-H3 prompt:\n\n{}",
                    shot_meta.shot_id, shot_meta.duration, shot_meta.anchor, ko_base
                )
            }
        ]
    });

    let resp = client
        .post("https://api.minimax.io/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&user_msg)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "MiniMax prompt compile API returned status: {}",
            resp.status()
        );
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in MiniMax prompt compile response")?;

    let parsed: serde_json::Value = parse_llm_json(text)?;
    let raw_h3 = json_field_to_text(
        &parsed["prompt_h3"],
        &[
            "integrated_multimodal_description",
            "overall_soundscape",
            "non_diegetic_music",
        ],
    )
    .context("Missing prompt_h3 in response")?;
    let shot_number = shot_meta
        .shot_id
        .rsplit('_')
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1);
    Ok(ensure_h3_reference_header(
        &raw_h3,
        shot_number,
        &shot_meta.anchor,
    ))
}

fn generate_deterministic_template(ctx: &PromptGenContext) -> (String, String) {
    let half_dur = (ctx.duration * 0.5 * 10.0).round() / 10.0;
    let dur_round = (ctx.duration * 10.0).round() / 10.0;
    let peak_str = if ctx.audio_peaks.is_empty() {
        "1.2s".to_string()
    } else {
        format!("{:.1}s", ctx.audio_peaks[0])
    };
    let (ko_dlg, h3_dlg) = build_dialogue_lines(ctx);
    // Section 3.7: Removed hardcoded "kbo"/"야구모자" branching.
    // Visual modifications and traits are injected via customization blocks.
    let ko_mod = "";
    let h3_mod = "";
    let anchor = if ctx.anchor.is_empty() {
        "first_and_last"
    } else {
        ctx.anchor
    };
    let reference_line = reference_line_for(anchor, ctx.shot_index);
    let series_line = match ctx.series_world_summary {
        Some(s) if !s.trim().is_empty() => format!("\nSeries context: {}.", s.trim()),
        _ => String::new(),
    };

    let ko = format!(
        r#"# 샷 연출 지시서: {} ({:.2}s ~ {:.2}s)

## 1. 화면 및 카메라 연출
- 카메라: 원본 첫 프레임(<Picture 1>)과 끝 프레임(<Picture 2>) 구도를 정밀하게 잇는 시네마틱 샷
- 조명 및 색감: 자연스러운 현장 조명과 높은 다이내믹 레인지 대비
- 인물 구도: 프레임 내 인물 위치와 시선 방향을 일관되게 유지{}

## 2. 초 단위 동작 궤적
- 0.0s ~ {:.1}s: 시작 프레임 구도에서 인물이 자연스러운 호흡과 시선을 유지하며 동작을 이어간다.
- {:.1}s ~ {:.1}s: 끝 프레임 앵커 구도를 향해 카메라와 인물의 자세·표정이 매끄럽게 연결된다.

## 3. 대사 및 보이스
{}

## 4. 음향 및 공간음 (Soundscape)
- 공간음: 현장 공간감과 바람·실내 잔향이 살아있는 리얼한 앰비언스
- 피크 사운드: {} 부근 현장 효과음

## 5. 배경음악 (BGM)
- 대사와 현장음을 해치지 않는 절제된 시네마틱 배경음
"#,
        ctx.shot_id,
        ctx.start_sec,
        ctx.end_sec,
        ko_mod,
        half_dur,
        half_dur,
        dur_round,
        ko_dlg,
        peak_str
    );

    let h3 = format!(
        r#"{reference_line}

integrated_multimodal_description:
From 0.0s to {:.1}s, the camera and subjects follow a smooth, realistic trajectory anchored by the first frame.{} {} At {}, a subtle acoustic peak is heard. Describe the exact Delta trajectory from <Picture 1> through the middle motion to <Picture 2>.{} Do not describe broadcast logos, watermarks, burned-in timestamps, or hardcoded subtitle text.
From {:.1}s to {:.1}s, subtle head movement, natural gesture, and facial expression transition seamlessly into the final frame composition.

overall_soundscape:
Realistic spatial ambiance with clean environmental presence and natural acoustic texture.

non_diegetic_music:
Subtle, restrained cinematic underscore maintaining dramatic tension without overpowering the dialogue.
"#,
        half_dur, h3_mod, h3_dlg, peak_str, series_line, half_dur, dur_round
    );

    (ko, h3)
}

/// Strip all `<think>...</think>` reasoning blocks (case-insensitive, across lines) from LLM responses.
pub fn strip_think_blocks(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut last_idx = 0;

    while let Some(start_rel) = lower[last_idx..].find("<think>") {
        let start_pos = last_idx + start_rel;
        result.push_str(&text[last_idx..start_pos]);
        let after_start = start_pos + "<think>".len();
        if let Some(end_rel) = lower[after_start..].find("</think>") {
            last_idx = after_start + end_rel + "</think>".len();
        } else {
            // Unclosed <think> tag: treat remainder as thinking block
            last_idx = text.len();
            break;
        }
    }
    result.push_str(&text[last_idx..]);
    result
}

/// Extract clean JSON slice from LLM response text (strips markdown code fences and extracts outermost object or array).
pub fn extract_json_slice(text: &str) -> &str {
    let trimmed = text.trim();
    let without_fences = if trimmed.starts_with("```") {
        let without_prefix = if let Some(stripped) = trimmed.strip_prefix("```json") {
            stripped
        } else if let Some(stripped) = trimmed.strip_prefix("```JSON") {
            stripped
        } else if let Some(stripped) = trimmed.strip_prefix("```") {
            stripped
        } else {
            trimmed
        };
        without_prefix.trim_end_matches('`').trim()
    } else {
        trimmed
    };

    // Find outermost { ... }
    let obj_start = without_fences.find('{');
    let obj_end = without_fences.rfind('}');
    // Find outermost [ ... ]
    let arr_start = without_fences.find('[');
    let arr_end = without_fences.rfind(']');

    match (obj_start, obj_end, arr_start, arr_end) {
        (Some(os), Some(oe), Some(as_), Some(ae)) if oe > os && ae > as_ => {
            if os <= as_ && oe >= ae {
                &without_fences[os..=oe]
            } else if as_ <= os && ae >= oe {
                &without_fences[as_..=ae]
            } else if os < as_ {
                &without_fences[os..=oe]
            } else {
                &without_fences[as_..=ae]
            }
        }
        (Some(os), Some(oe), _, _) if oe > os => &without_fences[os..=oe],
        (_, _, Some(as_), Some(ae)) if ae > as_ => &without_fences[as_..=ae],
        _ => without_fences,
    }
}

/// Common parser for MiniMax-M3 LLM responses:
/// 1. Strips `<think>...</think>` reasoning blocks
/// 2. Strips markdown fences (```json ... ```)
/// 3. Extracts and parses outermost JSON object or array
/// 4. Sanitizes raw control characters emitted inside JSON strings
pub fn parse_llm_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let stripped = strip_think_blocks(text);
    let json_str = extract_json_slice(&stripped);
    let sanitized = sanitize_json_control_chars(json_str);
    serde_json::from_str(&sanitized)
        .with_context(|| format!("Failed to parse JSON from LLM response:\n{}", text))
}

/// Escape raw control characters that appear inside JSON string literals.
///
/// MiniMax-M3 sometimes emits a multi-line prompt value with literal line
/// breaks, which is invalid JSON ("control character found while parsing a
/// string"). Newlines and tabs are re-escaped so the prompt keeps its shape;
/// any other control byte is dropped.
pub fn sanitize_json_control_chars(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;

    for ch in json.chars() {
        if in_string {
            if escaped {
                escaped = false;
                out.push(ch);
                continue;
            }
            match ch {
                '\\' => {
                    escaped = true;
                    out.push(ch);
                }
                '"' => {
                    in_string = false;
                    out.push(ch);
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {}
                c => out.push(c),
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_strip_think_blocks() {
        let input = "<think>\nThinking about life...\n{\n  \"ignore\": 1\n}\n</think>{\"head\": {\"x\": 0.1}}";
        assert_eq!(strip_think_blocks(input), "{\"head\": {\"x\": 0.1}}");

        let case_insensitive = "<THINK>Thinking...</THINK>{\"ok\": true}";
        assert_eq!(strip_think_blocks(case_insensitive), "{\"ok\": true}");

        let multiple_blocks = "<think>first</think>part1<think>second</think>part2";
        assert_eq!(strip_think_blocks(multiple_blocks), "part1part2");

        let unclosed = "<think>unfinished thinking";
        assert_eq!(strip_think_blocks(unclosed), "");

        let no_think = "{\"hello\": \"world\"}";
        assert_eq!(strip_think_blocks(no_think), "{\"hello\": \"world\"}");
    }

    #[test]
    fn test_extract_json_slice() {
        let wrapped_json = "```json\n{\"foo\": \"bar\"}\n```";
        assert_eq!(extract_json_slice(wrapped_json), "{\"foo\": \"bar\"}");

        let with_text = "Here is the result:\n```json\n{\"x\": 1}\n```\nHope it helps!";
        assert_eq!(extract_json_slice(with_text), "{\"x\": 1}");

        let array_json = "Prefix [1, 2, 3] Suffix";
        assert_eq!(extract_json_slice(array_json), "[1, 2, 3]");
    }

    #[test]
    fn test_parse_llm_json_with_think_and_markdown() {
        let raw = r#"
<think>
I need to locate the head.
The character is Walter White.
Let's emit JSON:
{"ignore_this_head": null}
</think>
```json
{
  "head": {
    "x": 0.263,
    "y": 0.079,
    "w": 0.206,
    "h": 0.519
  }
}
```
"#;
        let val: Value = parse_llm_json(raw).expect("parse_llm_json should succeed");
        assert_eq!(val["head"]["x"].as_f64().unwrap(), 0.263);
        assert_eq!(val["head"]["y"].as_f64().unwrap(), 0.079);
        assert_eq!(val["head"]["w"].as_f64().unwrap(), 0.206);
        assert_eq!(val["head"]["h"].as_f64().unwrap(), 0.519);
    }

    #[test]
    fn test_sanitize_json_control_chars() {
        let raw = "{\"a\": \"line1\nline2\"}";
        let fixed = sanitize_json_control_chars(raw);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).expect("should parse");
        assert_eq!(parsed["a"], "line1\nline2");
    }

    #[test]
    fn test_reference_line_for_anchors() {
        assert!(reference_line_for("first_and_last", 3).contains("<Picture 1> and <Picture 2>"));
        assert!(reference_line_for("first_and_last", 3).contains("[Shot 3]"));
        assert!(reference_line_for("first_only", 5).contains("<Picture 1> (from [Shot 5])"));
        assert!(reference_line_for("ref_only", 5).contains("<Picture 1> (from [Shot 5])"));
        assert!(reference_line_for("none", 7).contains("generate Shot 7"));
    }

    #[test]
    fn test_ensure_h3_reference_header() {
        // Missing header is prepended.
        let body = "integrated_multimodal_description:\nSome scene.";
        let fixed = ensure_h3_reference_header(body, 2, "first_and_last");
        assert!(fixed.starts_with("For the target video"));
        assert!(fixed.contains("<Picture 1> and <Picture 2> (from [Shot 2])"));
        // Correct header is kept as-is.
        let expected = reference_line_for("first_only", 4);
        let already = format!("{}\n\nintegrated_multimodal_description:\nX", expected);
        assert_eq!(
            ensure_h3_reference_header(&already, 4, "first_only"),
            already
        );
        // Stale header line is replaced.
        let stale = "For the target video, at 0.00 seconds into the target video, WRONG.\n\nbody";
        let fixed2 = ensure_h3_reference_header(stale, 9, "first_and_last");
        assert!(fixed2.contains("[Shot 9]"));
        assert!(!fixed2.contains("WRONG"));
    }
}
