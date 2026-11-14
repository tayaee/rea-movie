use anyhow::{Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
}

pub fn find_sibling_srt(video_path: &str) -> Option<PathBuf> {
    let path = Path::new(video_path);
    let parent = path.parent()?;
    let stem = path.file_stem()?.to_str()?;

    let candidates = [
        parent.join(format!("{}.ko.srt", stem)),
        parent.join(format!("{}.srt", stem)),
        parent.join(format!("{}.en.srt", stem)),
        parent.join(format!("{}.diarized.srt", stem)),
    ];
    for c in &candidates {
        if c.is_file() {
            return Some(c.clone());
        }
    }

    // Also check parent directory if input is inside a subfolder like `원본mkv/`
    if let Some(grandparent) = parent.parent() {
        let gp_candidates = [
            grandparent.join(format!("{}.ko.srt", stem)),
            grandparent.join(format!("{}.srt", stem)),
            grandparent.join(format!("{}.en.srt", stem)),
            grandparent.join(format!("{}.diarized.srt", stem)),
        ];
        for c in &gp_candidates {
            if c.is_file() {
                return Some(c.clone());
            }
        }
    }

    None
}

pub fn load_subtitle_cues(video_path: &str) -> Vec<SubtitleCue> {
    let Some(srt_path) = find_sibling_srt(video_path) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&srt_path) else {
        return Vec::new();
    };
    parse_srt(&content)
}

pub fn parse_srt(content: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    let time_re = Regex::new(
        r"(\d{2}):(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2})[,.](\d{3})",
    )
    .unwrap();
    let tag_re = Regex::new(r"<[^>]+>").unwrap();

    for block in content.replace("\r\n", "\n").split("\n\n") {
        let lines: Vec<&str> = block.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.len() < 2 {
            continue;
        }
        let mut time_idx = None;
        let mut start_sec = 0.0;
        let mut end_sec = 0.0;
        for (i, line) in lines.iter().enumerate() {
            if let Some(cap) = time_re.captures(line) {
                let h1: f64 = cap[1].parse().unwrap_or(0.0);
                let m1: f64 = cap[2].parse().unwrap_or(0.0);
                let s1: f64 = cap[3].parse().unwrap_or(0.0);
                let ms1: f64 = cap[4].parse().unwrap_or(0.0);
                let h2: f64 = cap[5].parse().unwrap_or(0.0);
                let m2: f64 = cap[6].parse().unwrap_or(0.0);
                let s2: f64 = cap[7].parse().unwrap_or(0.0);
                let ms2: f64 = cap[8].parse().unwrap_or(0.0);
                start_sec = h1 * 3600.0 + m1 * 60.0 + s1 + ms1 / 1000.0;
                end_sec = h2 * 3600.0 + m2 * 60.0 + s2 + ms2 / 1000.0;
                time_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = time_idx {
            let text_lines: Vec<String> = lines[(idx + 1)..]
                .iter()
                .map(|l| tag_re.replace_all(l, "").trim().to_string())
                .filter(|l| !l.is_empty() && !l.to_lowercase().contains("tvsubtitles"))
                .collect();
            if !text_lines.is_empty() {
                cues.push(SubtitleCue {
                    start_sec,
                    end_sec,
                    text: text_lines.join(" "),
                });
            }
        }
    }
    cues
}

pub fn cues_for_span(cues: &[SubtitleCue], start_sec: f64, end_sec: f64) -> Vec<String> {
    cues.iter()
        .filter(|c| c.end_sec > start_sec + 0.15 && c.start_sec < end_sec - 0.15)
        .map(|c| c.text.clone())
        .collect()
}

pub fn generate_story_assets(
    proj_dir: &Path,
    shot_count: usize,
    total_duration: f64,
    input_video_path: Option<&str>,
) -> Result<()> {
    let story_dir = proj_dir.join("story");
    std::fs::create_dir_all(&story_dir)?;

    let chars_path = story_dir.join("characters.toml");
    if !chars_path.exists() {
        let chars_content = r#"[characters.S1]
name = "Speaker_1"
description = "Primary protagonist and lead speaker"
voice_fingerprint = "deep resonant male voice with steady cadence, calm and authoritative"

[characters.S2]
name = "Speaker_2"
description = "Secondary interlocutor or antagonist"
voice_fingerprint = "gritty tense male voice with sharp delivery"
"#;
        std::fs::write(&chars_path, chars_content)
            .with_context(|| format!("Failed to write {:?}", chars_path))?;
    }

    let screenplay_path = story_dir.join("screenplay.md");
    if !screenplay_path.exists() {
        let screenplay_content = format!(
r#"# Reverse-Engineered Screenplay

## Scene 1 - Intro & Sequence ({:.2}s total)

This screenplay was decompiled from the source movie by rea-movie.
It contains {} sequential camera shots.

### Characters
- S1 (Speaker_1): Primary subject
- S2 (Speaker_2): Secondary subject
"#,
            total_duration, shot_count
        );
        std::fs::write(&screenplay_path, screenplay_content)
            .with_context(|| format!("Failed to write {:?}", screenplay_path))?;
    }

    let transcript_path = story_dir.join("transcript.srt");
    if !transcript_path.exists() {
        if let Some(vid) = input_video_path {
            if let Some(src_srt) = find_sibling_srt(vid) {
                let _ = std::fs::copy(&src_srt, &transcript_path);
            }
        }
        if !transcript_path.exists() {
            let srt_content = r#"1
00:00:00,000 --> 00:00:04,000
[S1] Cinematic scene in progress.

2
00:00:04,000 --> 00:00:08,000
[S2] Scene transition observed.
"#;
            std::fs::write(&transcript_path, srt_content)
                .with_context(|| format!("Failed to write {:?}", transcript_path))?;
        }
    }

    // assets/characters/S1_narrator/face.jpg, voice.wav
    let char_s1_dir = proj_dir.join("assets").join("characters").join("S1_narrator");
    std::fs::create_dir_all(&char_s1_dir)?;

    let face_path = char_s1_dir.join("face.jpg");
    if !face_path.exists() {
        let img = image::ImageBuffer::from_pixel(256, 256, image::Rgb([80u8, 120u8, 160u8]));
        let _ = img.save(&face_path);
    }

    let voice_path = char_s1_dir.join("voice.wav");
    if !voice_path.exists() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        if let Ok(mut writer) = hound::WavWriter::create(&voice_path, spec) {
            for t in 0..32000 {
                let sample = (t as f32 * 440.0 * 2.0 * std::f32::consts::PI / 32000.0).sin();
                let amplitude = (sample * 0.2 * i16::MAX as f32) as i16;
                let _ = writer.write_sample(amplitude);
            }
            let _ = writer.finalize();
        }
    }

    Ok(())
}
