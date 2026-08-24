//! ffmpeg-backed export of the selected timeline range.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Smallest clip we will ask ffmpeg to write.
pub const MIN_SELECTION_US: i64 = 100_000;

pub fn format_timestamp(us: i64) -> String {
    let total = us.max(0);
    let hours = total / 3_600_000_000;
    let mins = (total % 3_600_000_000) / 60_000_000;
    let secs = (total % 60_000_000) / 1_000_000;
    let ms = (total % 1_000_000) / 1_000;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}.{ms:03}")
    } else {
        format!("{mins:02}:{secs:02}.{ms:03}")
    }
}

pub fn us_to_secs(us: i64) -> f64 {
    us.max(0) as f64 / 1_000_000.0
}

pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Coded frame size from ffprobe.
#[derive(Clone, Copy, Debug, Default)]
pub struct VideoSize {
    pub width: i32,
    pub height: i32,
}

/// Probe duration in microseconds via `ffprobe`. `None` if ffprobe is missing or fails.
pub fn probe_duration_us(path: &Path) -> Option<i64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let secs: f64 = text.trim().parse().ok()?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    Some((secs * 1_000_000.0).round() as i64)
}

/// Probe coded width/height of the first video stream.
pub fn probe_size(path: &Path) -> Option<VideoSize> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut width = 0_i32;
    let mut height = 0_i32;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("width=") {
            width = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("height=") {
            height = v.trim().parse().unwrap_or(0);
        }
    }
    if width > 0 && height > 0 {
        Some(VideoSize { width, height })
    } else {
        None
    }
}

/// Even-align a crop rectangle for yuv420.
pub fn even_crop(x: i32, y: i32, w: i32, h: i32, vw: i32, vh: i32) -> (i32, i32, i32, i32) {
    let even = |n: i32| n & !1;
    let vw = vw.max(2);
    let vh = vh.max(2);
    let mut x = even(x.max(0));
    let mut y = even(y.max(0));
    let mut w = even(w.max(2));
    let mut h = even(h.max(2));
    if x + w > vw {
        w = even((vw - x).max(2));
    }
    if y + h > vh {
        h = even((vh - y).max(2));
    }
    if x + w > vw {
        x = even((vw - w).max(0));
    }
    if y + h > vh {
        y = even((vh - h).max(0));
    }
    (x, y, w.max(2), h.max(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_crop_aligns_and_stays_inside() {
        assert_eq!(even_crop(1, 1, 101, 51, 1920, 1080), (0, 0, 100, 50));
        let (x, y, w, h) = even_crop(1900, 1060, 40, 40, 1920, 1080);
        assert_eq!(x % 2, 0);
        assert_eq!(y % 2, 0);
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
        assert!(x + w <= 1920);
        assert!(y + h <= 1080);
    }
}

pub struct ExportJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub start_us: i64,
    pub end_us: i64,
    pub accurate: bool,
    /// Clockwise 90° steps (0..=3) applied after crop.
    pub rotate_q: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    /// Crop in source pixels `(x, y, w, h)`. `None` = full frame.
    pub crop: Option<(i32, i32, i32, i32)>,
    pub src_width: i32,
    pub src_height: i32,
}

impl ExportJob {
    pub fn needs_filters(&self) -> bool {
        self.rotate_q % 4 != 0
            || self.flip_h
            || self.flip_v
            || self.crop.is_some()
    }
}

fn video_filter(job: &ExportJob) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some((x, y, w, h)) = job.crop {
        let (x, y, w, h) = even_crop(x, y, w, h, job.src_width, job.src_height);
        if w < job.src_width || h < job.src_height || x > 0 || y > 0 {
            parts.push(format!("crop={w}:{h}:{x}:{y}"));
        }
    }
    match job.rotate_q % 4 {
        1 => parts.push("transpose=1".into()),
        2 => parts.push("transpose=1,transpose=1".into()),
        3 => parts.push("transpose=2".into()),
        _ => {}
    }
    if job.flip_h {
        parts.push("hflip".into());
    }
    if job.flip_v {
        parts.push("vflip".into());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Run ffmpeg, calling `progress` with 0.0–1.0 as the encode advances.
pub fn run_export(job: &ExportJob, mut progress: impl FnMut(f64)) -> Result<(), String> {
    if !ffmpeg_available() {
        return Err(
            "ffmpeg is not installed. Install it with: sudo apt install ffmpeg".into(),
        );
    }
    let duration_us = (job.end_us - job.start_us).max(MIN_SELECTION_US);
    let start = format!("{:.3}", us_to_secs(job.start_us));
    let dur = format!("{:.3}", us_to_secs(duration_us));

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner")
        .arg("-nostdin")
        .arg("-y")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats");

    let vf = video_filter(job);
    let reencode = job.accurate || job.needs_filters();

    if reencode {
        cmd.arg("-i")
            .arg(&job.input)
            .arg("-ss")
            .arg(&start)
            .arg("-t")
            .arg(&dur);
        if let Some(vf) = vf {
            cmd.arg("-vf").arg(vf);
        }
        encode_args(&mut cmd, &job.output);
    } else {
        cmd.arg("-ss")
            .arg(&start)
            .arg("-i")
            .arg(&job.input)
            .arg("-t")
            .arg(&dur)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("0:a:0?")
            .arg("-c")
            .arg("copy")
            .arg("-avoid_negative_ts")
            .arg("make_zero");
    }

    cmd.arg(&job.output);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start ffmpeg: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg stdout missing".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ffmpeg stderr missing".to_string())?;

    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            buf.push_str(&line);
            buf.push('\n');
            // Keep the tail so a long encode does not balloon memory.
            if buf.len() > 16_384 {
                buf = buf[buf.len() - 8_192..].to_string();
            }
        }
        buf
    });

    let reader = BufReader::new(stdout);
    for line in reader.lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("out_time_us=") {
            if let Ok(out_us) = rest.trim().parse::<i64>() {
                if duration_us > 0 {
                    let frac = (out_us as f64 / duration_us as f64).clamp(0.0, 1.0);
                    progress(frac);
                }
            }
        } else if line.trim() == "progress=end" {
            progress(1.0);
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("ffmpeg wait failed: {e}"))?;
    let stderr_text = err_handle.join().unwrap_or_default();

    if !status.success() {
        let tail = stderr_text
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if tail.is_empty() {
            format!("ffmpeg exited with {status}")
        } else {
            format!("ffmpeg failed:\n{tail}")
        });
    }

    if !job.output.is_file() {
        return Err("ffmpeg finished but the output file was not created".into());
    }
    Ok(())
}

fn encode_args(cmd: &mut Command, output: &Path) {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    cmd.arg("-map").arg("0:v:0").arg("-map").arg("0:a:0?");

    match ext.as_str() {
        "webm" => {
            cmd.args([
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-c:a",
                "libopus",
                "-b:a",
                "128k",
            ]);
        }
        _ => {
            cmd.args([
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ]);
        }
    }
}

pub fn suggested_export_name(input: &Path) -> String {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("clip");
    format!("{stem}-clip.mp4")
}

pub fn is_supported_video(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "mp4"
            | "m4v"
            | "mkv"
            | "webm"
            | "mov"
            | "avi"
            | "mpeg"
            | "mpg"
            | "m2ts"
            | "mts"
            | "ts"
            | "wmv"
            | "flv"
            | "ogv"
            | "ogg"
            | "3gp"
            | "3g2"
    )
}
