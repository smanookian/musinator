//! Runs the `ffmpeg` CLI: encode PCM from memory and measure loudness (EBU R128).

use crate::formats::Fmt;
use crate::synth::Stereo;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// PCM sent to ffmpeg on stdin, always as two channels.
pub enum Pcm<'a> {
    F32(&'a Stereo),
    /// The same 16-bit samples on both channels.
    S16Mono {
        sr: u32,
        samples: &'a [i16],
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Loudness {
    /// Integrated loudness, LUFS.
    pub lufs: f64,
    /// Sample peak, dBFS.
    pub peak_db: f64,
}

impl Loudness {
    pub fn peak_linear(&self) -> f64 {
        10f64.powf(self.peak_db / 20.0)
    }
}

/// Fail early with a clear message if ffmpeg or an encoder is missing.
pub fn check() -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .output()
        .context("ffmpeg was not found. Install it first (Arch: sudo pacman -S ffmpeg).")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let has = |name: &str| text.lines().any(|l| l.split_whitespace().nth(1) == Some(name));
    let mut missing: Vec<&str> = Fmt::ALL.iter().map(|f| f.encoder()).filter(|e| !has(e)).collect();
    missing.dedup();
    if !missing.is_empty() {
        bail!("ffmpeg has no encoder for: {}. Install an ffmpeg build with these encoders.", missing.join(", "));
    }
    let out = Command::new("ffmpeg").args(["-hide_banner", "-filters"]).stdin(Stdio::null()).output()?;
    if !String::from_utf8_lossy(&out.stdout).lines().any(|l| l.split_whitespace().nth(1) == Some("ebur128")) {
        bail!("ffmpeg has no ebur128 filter. Install a full ffmpeg build.");
    }
    Ok(())
}

fn part_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Encode `pcm` into every output in one ffmpeg run. With `measure`, also return its loudness.
/// Outputs are written to `*.part` first and renamed on success.
pub fn encode(pcm: &Pcm, outputs: &[(Fmt, &Path)], measure: bool) -> Result<Option<Loudness>> {
    let (input_fmt, sr) = match pcm {
        Pcm::F32(s) => ("f32le", s.sr),
        Pcm::S16Mono { sr, .. } => ("s16le", *sr),
    };
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-nostats", "-loglevel", if measure { "info" } else { "error" }, "-y"]).args([
        "-f",
        input_fmt,
        "-ar",
        &sr.to_string(),
        "-ac",
        "2",
        "-i",
        "pipe:0",
    ]);
    for (fmt, path) in outputs {
        cmd.args(["-map", "0:a", "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact"])
            .args(["-threads", "1"])
            .args(fmt.ffmpeg_args())
            .arg(part_path(path));
    }
    if measure {
        cmd.args(["-map", "0:a", "-af", "ebur128=peak=sample:framelog=verbose", "-f", "null", "-"]);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().context("could not start ffmpeg")?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let (written, output) = std::thread::scope(|s| {
        let writer = s.spawn(move || write_pcm(&mut stdin, pcm));
        let output = child.wait_with_output();
        (writer.join().expect("pcm writer panicked"), output)
    });
    let output = output.context("ffmpeg did not finish")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let names: Vec<_> = outputs.iter().map(|(_, p)| p.display().to_string()).collect();
        bail!("ffmpeg failed for {}:\n{}", names.join(", "), stderr.trim());
    }
    written.context("could not send audio to ffmpeg")?;
    for (_, path) in outputs {
        std::fs::rename(part_path(path), path).with_context(|| format!("rename {}", path.display()))?;
    }
    if measure { parse_loudness(&stderr).map(Some) } else { Ok(None) }
}

fn write_pcm(w: &mut impl Write, pcm: &Pcm) -> std::io::Result<()> {
    const CHUNK: usize = 16 * 1024;
    let mut buf = Vec::with_capacity(CHUNK * 8);
    match pcm {
        Pcm::F32(s) => {
            for start in (0..s.frames()).step_by(CHUNK) {
                buf.clear();
                let end = (start + CHUNK).min(s.frames());
                for i in start..end {
                    buf.extend_from_slice(&s.l[i].to_le_bytes());
                    buf.extend_from_slice(&s.r[i].to_le_bytes());
                }
                w.write_all(&buf)?;
            }
        }
        Pcm::S16Mono { samples, .. } => {
            for chunk in samples.chunks(CHUNK) {
                buf.clear();
                for v in chunk {
                    buf.extend_from_slice(&v.to_le_bytes());
                    buf.extend_from_slice(&v.to_le_bytes());
                }
                w.write_all(&buf)?;
            }
        }
    }
    Ok(())
}

/// Measure an encoded file with the ebur128 filter.
pub fn measure_file(path: &Path) -> Result<Loudness> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(path)
        .args(["-map", "0:a", "-af", "ebur128=peak=sample:framelog=verbose", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output()
        .context("could not start ffmpeg")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!("ffmpeg could not measure {}:\n{}", path.display(), stderr.trim());
    }
    parse_loudness(&stderr)
}

fn parse_loudness(log: &str) -> Result<Loudness> {
    let summary = log.rfind("Summary:").map(|i| &log[i..]).context("no ebur128 summary in ffmpeg output")?;
    let value = |key: &str| -> Result<f64> {
        let line = summary.lines().map(str::trim).find(|l| l.starts_with(key)).with_context(|| format!("no '{key}' in ebur128 summary"))?;
        let token = line[key.len()..].split_whitespace().next().unwrap_or("");
        Ok(if token.ends_with("inf") { -200.0 } else { token.parse()? })
    };
    Ok(Loudness { lufs: value("I:")?, peak_db: value("Peak:")? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ebur128_summary() {
        let log = "[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    I:         -14.2 LUFS\n    \
                   Threshold: -24.4 LUFS\n\n  Sample peak:\n    Peak:       -1.5 dBFS\n";
        let l = parse_loudness(log).unwrap();
        assert_eq!((l.lufs, l.peak_db), (-14.2, -1.5));
    }
}
