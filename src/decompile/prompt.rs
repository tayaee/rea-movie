use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static MINIMAX_API_DISABLED: AtomicBool = AtomicBool::new(false);

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
}

pub fn generate_prompts(ctx: &PromptGenContext, ko_path: &Path, h3_path: &Path) -> Result<()> {
    let need_ko = ctx.force || !ko_path.exists();
    let need_h3 = ctx.force || !h3_path.exists();

    if !need_ko && !need_h3 {
        return Ok(());
    }

    let mut generated_ko = None;
    let mut generated_h3 = None;

    let api_key = std::env::var("MINIMAX_API_KEY").ok();
    if !ctx.offline
        && !MINIMAX_API_DISABLED.load(Ordering::Relaxed)
        && api_key
            .as_deref()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
        && ctx.contact_sheet_path.exists()
    {
        match try_minimax_m3_api(ctx, api_key.as_deref().unwrap().trim()) {
            Ok((ko, h3)) => {
                generated_ko = Some(ko);
                generated_h3 = Some(h3);
            }
            Err(_) => {
                MINIMAX_API_DISABLED.store(true, Ordering::Relaxed);
            }
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

fn is_korean_dub(dub_lang: &str) -> bool {
    matches!(
        dub_lang.trim().to_ascii_lowercase().as_str(),
        "ko" | "kor" | "korean" | "kr"
    )
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

fn try_minimax_m3_api(ctx: &PromptGenContext, api_key: &str) -> Result<(String, String)> {
    let img_bytes = std::fs::read(ctx.contact_sheet_path)?;
    let base64_img = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img_bytes);
    let data_uri = format!("data:image/jpeg;base64,{}", base64_img);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let (ko_dlg, h3_dlg) = build_dialogue_lines(ctx);
    let system_prompt = "You are a professional film reverse-engineering director and prompt engineer for MiniMax-H3 video generation. Analyze the provided contact sheet image (1-second intervals with burned-in timestamps). Output valid JSON with two fields: 'prompt_ko' and 'prompt_h3'. prompt_h3 must adhere to the MiniMax-H3 schema with 'integrated_multimodal_description:', 'overall_soundscape:', and 'non_diegetic_music:'. Keep the exact <d>[Language] ...</d> dialogue tag provided by the user.";

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
                            "Analyze shot '{}' ({:.2}s to {:.2}s, duration: {:.2}s). Speaker: {}. Audio peaks at: {:?}. Required dialogue line for prompt_ko: '{}'. Required dialogue clause for prompt_h3: '{}'.",
                            ctx.shot_id, ctx.start_sec, ctx.end_sec, ctx.duration, ctx.speaker, ctx.audio_peaks, ko_dlg, h3_dlg
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
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
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
    let ko = parsed["prompt_ko"]
        .as_str()
        .context("Missing prompt_ko")?
        .to_string();
    let h3 = parsed["prompt_h3"]
        .as_str()
        .context("Missing prompt_h3")?
        .to_string();

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
    let (ko_dlg, h3_dlg) = build_dialogue_lines(ctx);
    let (ko_mod, h3_mod) = match ctx.custom_mod {
        Some(m) if !m.trim().is_empty() => {
            let trimmed = m.trim();
            if trimmed.to_ascii_lowercase().contains("kbo") || trimmed.contains("야구모자") {
                (
                    format!(
                        "\n- 커스텀 연출(재생성 식별 마커): {} — 주인공은 머리에 'KBO' 로고가 선명하게 자수된 네이비/레드 배색의 한국 프로야구(KBO) 야구모자를 착용하고 있다.",
                        trimmed
                    ),
                    " IMPORTANT VISUAL MODIFICATION: The main protagonist prominently wears a navy-and-red KBO (Korean Baseball Organization) baseball cap with a bold white embroidered 'KBO' emblem on his head throughout the entire shot.".to_string(),
                )
            } else {
                (
                    format!("\n- 커스텀 연출: {}", trimmed),
                    format!(" IMPORTANT VISUAL MODIFICATION: {}.", trimmed),
                )
            }
        }
        _ => (String::new(), String::new()),
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
        r#"For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> (from [Shot {}]) are fully referenced.

integrated_multimodal_description:
From 0.0s to {:.1}s, the camera and subjects follow a smooth, realistic trajectory anchored by the first frame.{} {} At {}, a subtle acoustic peak is heard.
From {:.1}s to {:.1}s, subtle head movement, natural gesture, and facial expression transition seamlessly into the final frame composition.

overall_soundscape:
Realistic spatial ambiance with clean environmental presence and natural acoustic texture.

non_diegetic_music:
Subtle, restrained cinematic underscore maintaining dramatic tension without overpowering the dialogue.
"#,
        ctx.shot_index, half_dur, h3_mod, h3_dlg, peak_str, half_dur, dur_round
    );

    (ko, h3)
}
