use anyhow::{Context, Result};
use std::path::Path;

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
}

pub fn generate_prompts(
    ctx: &PromptGenContext,
    ko_path: &Path,
    h3_path: &Path,
) -> Result<()> {
    // Skip-by-default check
    let need_ko = ctx.force || !ko_path.exists();
    let need_h3 = ctx.force || !h3_path.exists();

    if !need_ko && !need_h3 {
        return Ok(());
    }

    let mut generated_ko = None;
    let mut generated_h3 = None;

    // Check if MINIMAX_API_KEY is available and offline is false
    let api_key = std::env::var("MINIMAX_API_KEY").ok();
    if !ctx.offline && api_key.is_some() && ctx.contact_sheet_path.exists() {
        if let Ok((ko, h3)) = try_minimax_m3_api(ctx, api_key.unwrap().trim()) {
            generated_ko = Some(ko);
            generated_h3 = Some(h3);
        }
    }

    let (ko_content, h3_content) = match (generated_ko, generated_h3) {
        (Some(ko), Some(h3)) => (ko, h3),
        _ => generate_deterministic_template(ctx),
    };

    if need_ko {
        if let Some(parent) = ko_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(ko_path, ko_content)
            .with_context(|| format!("Failed to write prompt.ko.md to {:?}", ko_path))?;
    }

    if need_h3 {
        if let Some(parent) = h3_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(h3_path, h3_content)
            .with_context(|| format!("Failed to write prompt.h3.txt to {:?}", h3_path))?;
    }

    Ok(())
}

fn try_minimax_m3_api(ctx: &PromptGenContext, api_key: &str) -> Result<(String, String)> {
    let img_bytes = std::fs::read(ctx.contact_sheet_path)?;
    let base64_img = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img_bytes);
    let data_uri = format!("data:image/jpeg;base64,{}", base64_img);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let system_prompt = "You are a professional film reverse-engineering director and prompt engineer for MiniMax-H3 video generation. Analyze the provided contact sheet image (1-second intervals with burned-in timestamps). Output valid JSON with two fields: 'prompt_ko' and 'prompt_h3'. prompt_h3 must adhere to the MiniMax-H3 schema with 'integrated_multimodal_description:', 'overall_soundscape:', and 'non_diegetic_music:'.";

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
                        "text": format!(
                            "Analyze shot '{}' ({:.2}s to {:.2}s, duration: {:.2}s). Speaker: {}. Audio peaks at: {:?}.",
                            ctx.shot_id, ctx.start_sec, ctx.end_sec, ctx.duration, ctx.speaker, ctx.audio_peaks
                        )
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
        anyhow::bail!("MiniMax API returned status: {}", resp.status());
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"].as_str()
        .context("Missing content in MiniMax M3 response")?;

    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let json_slice = match (cleaned.find('{'), cleaned.rfind('}')) {
        (Some(s), Some(e)) if e > s => &cleaned[s..=e],
        _ => cleaned,
    };

    let parsed: serde_json::Value = serde_json::from_str(json_slice)?;
    let ko = parsed["prompt_ko"].as_str().context("Missing prompt_ko")?.to_string();
    let h3 = parsed["prompt_h3"].as_str().context("Missing prompt_h3")?.to_string();

    Ok((ko, h3))
}

fn generate_deterministic_template(ctx: &PromptGenContext) -> (String, String) {
    let half_dur = (ctx.duration * 0.5 * 10.0).round() / 10.0;
    let dur_round = (ctx.duration * 10.0).round() / 10.0;
    let peak_str = if ctx.audio_peaks.is_empty() {
        "1.2s".to_string()
    } else {
        format!("{:.1}s", ctx.audio_peaks[0])
    };

    let ko = format!(
r#"# 샷 연출 지시서: {} ({:.2}s ~ {:.2}s)

## 1. 화면 및 카메라 연출
- 카메라: 정면 안정 구도 (Static Eye-Level Shot)
- 조명 및 색감: 자연스러운 조명과 균형 잡힌 대비
- 인물 구도: 화면 중앙에 인물이 안정적으로 배치됨

## 2. 초 단위 동작 궤적
- 0.0s ~ {:.1}s: 인물이 화면 중앙에서 시선을 유지하며 안정된 자세를 취한다.
- {:.1}s ~ {:.1}s: 미세하게 표정을 변화시키며 다음 앵커 프레임으로 자연스럽게 호흡한다.

## 3. 대사 및 보이스
- {} (Narrator): <d>[English] Cinematic scene in progress.</d>

## 4. 음향 및 공간음 (Soundscape)
- 공간음: 조용한 실내 배경 앰비언스 (Room tone)
- 피크 사운드: {} 효과음 발생

## 5. 배경음악 (BGM)
- 은은한 신디사이저 저음 패드
"#,
        ctx.shot_id, ctx.start_sec, ctx.end_sec,
        half_dur,
        half_dur, dur_round,
        ctx.speaker,
        peak_str
    );

    let h3 = format!(
r#"For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot {}]) is fully referenced.

integrated_multimodal_description:
From 0.0s to {:.1}s, the subject remains stationary in the center of the frame under clean lighting. {} speaks: <d>[English] Cinematic scene in progress.</d>. At {}, a subtle acoustic peak is heard.
From {:.1}s to {:.1}s, subtle head movement and facial expression transition smoothly toward the target composition.

overall_soundscape:
Quiet studio ambiance with low-level room resonance and clean room tone.

non_diegetic_music:
Minimalistic cinematic ambient synth pad maintaining steady emotional tension.
"#,
        ctx.shot_index,
        half_dur,
        ctx.speaker,
        peak_str,
        half_dur, dur_round
    );

    (ko, h3)
}
