use anyhow::{Context, Result};
use std::path::Path;

pub fn generate_story_assets(proj_dir: &Path, shot_count: usize, total_duration: f64) -> Result<()> {
    let story_dir = proj_dir.join("story");
    std::fs::create_dir_all(&story_dir)?;

    let chars_path = story_dir.join("characters.toml");
    if !chars_path.exists() {
        let chars_content = r#"[characters.S1]
name = "Narrator"
description = "Main protagonist and narrative voice"
voice_fingerprint = "deep resonant male voice with steady cadence, calm and authoritative"

[characters.S2]
name = "Partner"
description = "Secondary protagonist or supporting voice"
voice_fingerprint = "clear articulate female voice with calm delivery and professional tone"
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
- S1 (Narrator): Primary subject
- S2 (Partner): Secondary subject
"#,
            total_duration, shot_count
        );
        std::fs::write(&screenplay_path, screenplay_content)
            .with_context(|| format!("Failed to write {:?}", screenplay_path))?;
    }

    let transcript_path = story_dir.join("transcript.srt");
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

    // assets/characters/S1_narrator/face.jpg, voice.wav
    let char_s1_dir = proj_dir.join("assets").join("characters").join("S1_narrator");
    std::fs::create_dir_all(&char_s1_dir)?;

    let face_path = char_s1_dir.join("face.jpg");
    if !face_path.exists() {
        // Create simple 256x256 placeholder image
        let img = image::ImageBuffer::from_pixel(256, 256, image::Rgb([80u8, 120u8, 160u8]));
        let _ = img.save(&face_path);
    }

    let voice_path = char_s1_dir.join("voice.wav");
    if !voice_path.exists() {
        // Create 1-second sine wave at 440Hz, 32kHz
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
