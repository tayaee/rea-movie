use anyhow::Result;
use std::path::Path;

pub fn analyze_audio_peaks<P: AsRef<Path>>(wav_path: P, duration: f64) -> Result<Vec<f64>> {
    let reader = hound::WavReader::open(wav_path.as_ref());
    let mut reader = match reader {
        Ok(r) => r,
        Err(_) => {
            // Fallback peak if file cannot be read
            return Ok(default_peaks(duration));
        }
    };

    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f64;
    let channels = spec.channels as usize;

    if sample_rate <= 0.0 || channels == 0 {
        return Ok(default_peaks(duration));
    }

    // Process in 0.5s windows
    let window_samples = (sample_rate * 0.5) as usize;
    let mut peaks = Vec::new();

    let mut current_window_max: i32 = 0;
    let mut current_sample_idx = 0usize;
    let mut max_overall: i32 = 0;

    let mut window_peaks: Vec<(f64, i32)> = Vec::new();

    for (i, sample) in reader.samples::<i32>().enumerate() {
        let val = sample.unwrap_or(0).abs();
        if val > current_window_max {
            current_window_max = val;
        }
        if val > max_overall {
            max_overall = val;
        }

        if (i / channels) % window_samples == 0 && i > 0 {
            let t = ((current_sample_idx / channels) as f64) / sample_rate;
            window_peaks.push((t, current_window_max));
            current_window_max = 0;
        }
        current_sample_idx = i;
    }

    // Filter significant peaks (>= 50% of max_overall, or top 3)
    let threshold = (max_overall as f64 * 0.5) as i32;
    for (t, amp) in &window_peaks {
        if *amp >= threshold && *t > 0.1 && *t < (duration - 0.1) {
            peaks.push((*t * 10.0).round() / 10.0);
        }
    }

    peaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    peaks.dedup_by(|a, b| (*a - *b).abs() < 0.2);

    if peaks.is_empty() {
        peaks = default_peaks(duration);
    }

    // Limit to at most 3 peaks
    if peaks.len() > 3 {
        peaks.truncate(3);
    }

    Ok(peaks)
}

fn default_peaks(duration: f64) -> Vec<f64> {
    if duration >= 4.0 {
        vec![1.2, ((duration * 0.7) * 10.0).round() / 10.0]
    } else if duration >= 1.5 {
        vec![((duration * 0.5) * 10.0).round() / 10.0]
    } else {
        vec![0.5]
    }
}
