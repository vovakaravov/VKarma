use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct AudioMetadata {
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub bitrate_kbps: u32,
    pub codec: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    pub meta: AudioMetadata,
    pub peaks: Vec<f32>,
}

pub fn probe_audio_metadata(path: &Path) -> Result<AudioMetadata> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .context("Failed to execute `ffprobe`")?;

    if !out.status.success() {
        anyhow::bail!("ffprobe failed on {}", path.display());
    }

    let val: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("Invalid ffprobe JSON")?;

    let stream = val["streams"]
        .as_array()
        .and_then(|arr| arr.iter().find(|s| s["codec_type"] == "audio"))
        .or_else(|| val["streams"].as_array().and_then(|a| a.first()));

    let fmt = &val["format"];
    let duration_secs = fmt["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            stream
                .and_then(|s| s["duration"].as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    let sample_rate = stream
        .and_then(|s| s["sample_rate"].as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(44100);
    let channels = stream
        .and_then(|s| s["channels"].as_u64())
        .unwrap_or(2) as u32;
    let bitrate_kbps = fmt["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|b| (b / 1000) as u32)
        .unwrap_or(0);
    let codec = stream
        .and_then(|s| s["codec_name"].as_str())
        .or_else(|| fmt["format_name"].as_str())
        .unwrap_or("audio")
        .to_string();

    let tags = &fmt["tags"];
    let get_tag = |k: &str| -> Option<String> {
        tags[k]
            .as_str()
            .or_else(|| tags[k.to_ascii_uppercase().as_str()].as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    Ok(AudioMetadata {
        duration_secs,
        sample_rate,
        channels,
        bitrate_kbps,
        codec,
        title: get_tag("title"),
        artist: get_tag("artist"),
        album: get_tag("album"),
    })
}

pub fn compute_waveform_peaks(path: &Path, bars: usize) -> Result<Vec<f32>> {
    let bars = bars.max(32);
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-ac", "1", "-ar", "2000", "-f", "f32le", "-"])
        .output()
        .context("Failed to execute `ffmpeg` for waveform extraction")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!("ffmpeg waveform error: {stderr}");
    }

    let sample_count = out.stdout.len() / 4;
    if sample_count == 0 {
        return Ok(vec![0.05; bars]);
    }

    let mut samples = Vec::with_capacity(sample_count);
    for chunk in out.stdout.chunks_exact(4) {
        let s = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        samples.push(s.abs());
    }

    let mut peaks = vec![0.0f32; bars];
    let step = (samples.len() as f32) / (bars as f32);
    let mut max_peak = 0.001f32;
    for (i, peak_slot) in peaks.iter_mut().enumerate() {
        let s_idx = ((i as f32) * step).floor() as usize;
        let e_idx = (((i + 1) as f32) * step).ceil() as usize;
        let slice = &samples[s_idx.min(samples.len())..e_idx.clamp(s_idx + 1, samples.len())];
        let p = slice.iter().copied().fold(0.0f32, f32::max);
        *peak_slot = p;
        max_peak = max_peak.max(p);
    }
    for p in &mut peaks {
        *p = (*p / max_peak).clamp(0.04, 1.0);
    }
    Ok(peaks)
}

pub fn analyze_audio(path: &Path) -> Result<AudioAnalysis> {
    let meta = probe_audio_metadata(path)?;
    let peaks = compute_waveform_peaks(path, 260)?;
    Ok(AudioAnalysis { meta, peaks })
}

pub fn format_time_mmss(secs: f64) -> String {
    let tot = secs.max(0.0).round() as u64;
    let h = tot / 3600;
    let m = (tot % 3600) / 60;
    let s = tot % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Convert a normalized `0.0..=1.0` volume slider value into PulseAudio's `0..=65536` scale.
fn pa_volume_from_f32(volume: f32) -> u32 {
    (volume.clamp(0.0, 1.0) * 65536.0).round() as u32
}

fn spawn_ffmpeg(path: &Path, start_secs: f64) -> Result<Child> {
    let seek = format!("{:.3}", start_secs.max(0.0));
    Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-ss", &seek, "-i"])
        .arg(path)
        .args(["-f", "s16le", "-ac", "2", "-ar", "48000", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start `ffmpeg` decoder")
}

fn check_child_failure(slot: &mut Option<Child>, label: &str) -> Option<String> {
    let child = slot.as_mut()?;
    let status = child.try_wait().ok().flatten()?;
    if status.success() {
        *slot = None;
        return None;
    }
    let mut stderr_msg = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut stderr_msg);
    }
    let trimmed = stderr_msg.trim();
    if trimmed.is_empty() {
        Some(format!("{label} exited with {status}"))
    } else {
        Some(trimmed.to_string())
    }
}

pub struct AudioPlayer {
    ffmpeg: Option<Child>,
    sink: Option<Child>,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            ffmpeg: None,
            sink: None,
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.sink.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut child) = self.ffmpeg.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Poll whether either playback child exited with an error; stops both on failure.
    pub fn poll_failure(&mut self) -> Option<String> {
        // Check the sink before ffmpeg so a broken pipe in ffmpeg never masks the real sink error.
        if let Some(err) = check_child_failure(&mut self.sink, "audio playback sink") {
            self.stop();
            return Some(err);
        }
        if let Some(err) = check_child_failure(&mut self.ffmpeg, "ffmpeg decoder") {
            self.stop();
            return Some(err);
        }
        None
    }

    pub fn start(&mut self, path: &Path, start_secs: f64, volume: f32) -> Result<()> {
        self.stop();

        let mut ff = spawn_ffmpeg(path, start_secs)?;
        let ff_out = ff
            .stdout
            .take()
            .context("Missing piped stdout from ffmpeg")?;

        let clamped_vol = volume.clamp(0.0, 1.0);
        let vol = format!("{clamped_vol:.2}");
        let sink = Command::new("pw-play")
            .args([
                "--raw",
                "--format=s16",
                "--rate=48000",
                "--channels=2",
                &format!("--volume={vol}"),
                "-",
            ])
            .stdin(Stdio::from(ff_out))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();

        match sink {
            Ok(child) => {
                self.ffmpeg = Some(ff);
                self.sink = Some(child);
                Ok(())
            }
            Err(_) => {
                // Re-spawn ffmpeg cleanly if pw-play consumed stdout before failing to exec.
                let _ = ff.kill();
                let _ = ff.wait();
                let mut ff2 = spawn_ffmpeg(path, start_secs)?;
                let ff2_out = ff2.stdout.take().context("Missing piped stdout")?;
                let pa_vol = pa_volume_from_f32(clamped_vol);
                let pa = Command::new("paplay")
                    .args([
                        "--raw",
                        "--format=s16le",
                        "--rate=48000",
                        "--channels=2",
                        &format!("--volume={pa_vol}"),
                        "--client-name=VKarma",
                    ])
                    .stdin(Stdio::from(ff2_out))
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .context("Failed to start `pw-play` or `paplay` audio playback")?;
                self.ffmpeg = Some(ff2);
                self.sink = Some(pa);
                Ok(())
            }
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    fn write_test_wav(wav_path: &Path) {
        // Synthesize 0.5s 22050Hz mono 16-bit PCM WAV
        let sr = 22050u32;
        let num_samples = sr / 2;
        let data_bytes = num_samples * 2;
        let mut wav = Vec::with_capacity(44 + data_bytes as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_samples {
            let t = i as f32 / sr as f32;
            let val = ((t * 440.0 * std::f32::consts::TAU).sin() * 22000.0) as i16;
            wav.extend_from_slice(&val.to_le_bytes());
        }
        fs::write(wav_path, &wav).unwrap();
    }

    #[test]
    fn analyzes_synthesized_wav_duration_metadata_and_waveform_peaks() {
        let tmp = tempfile::tempdir().unwrap();
        let wav_path = tmp.path().join("tone.wav");
        write_test_wav(&wav_path);

        let analysis = analyze_audio(&wav_path).unwrap();
        assert!((analysis.meta.duration_secs - 0.5).abs() < 0.05);
        assert_eq!(analysis.meta.sample_rate, 22050);
        assert_eq!(analysis.meta.channels, 1);
        assert_eq!(analysis.peaks.len(), 260);
        assert!(analysis.peaks.iter().any(|&p| p > 0.8));
        assert_eq!(format_time_mmss(65.0), "01:05");
    }

    #[test]
    fn maps_volume_for_pulseaudio_and_starts_silent_playback_without_sink_failure() {
        assert_eq!(pa_volume_from_f32(-0.5), 0);
        assert_eq!(pa_volume_from_f32(0.0), 0);
        assert_eq!(pa_volume_from_f32(0.5), 32768);
        assert_eq!(pa_volume_from_f32(1.0), 65536);
        assert_eq!(pa_volume_from_f32(1.5), 65536);

        let tmp = tempfile::tempdir().unwrap();
        let wav_path = tmp.path().join("silent_test.wav");
        write_test_wav(&wav_path);

        let mut player = AudioPlayer::new();
        player.start(&wav_path, 0.0, 0.0).unwrap();
        thread::sleep(Duration::from_millis(120));
        assert_eq!(player.poll_failure(), None);
        player.stop();
    }
}
