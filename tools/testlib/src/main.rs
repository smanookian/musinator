//! `testlib`: builds a deterministic test music library for Musinator.
//!
//! Short audio masters are synthesized and encoded once with the `ffmpeg` CLI (cached in
//! `<out>/.masters/`). Library tracks are copies of these masters with tags written by lofty.
//! Same seed and track count give the same library.

mod cover;
mod ffmpeg;
mod formats;
mod library;
mod masters;
mod names;
mod rng;
mod special;
mod synth;
mod tags;

use anyhow::{Context, Result, bail};
use library::Stats;
use rayon::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const MARKER: &str = ".musinator-testlib";
const MASTERS_DIR: &str = ".masters";
const USAGE: &str = "\
Usage: testlib [--out DIR] [--tracks N] [--seed S] [--jobs J] [--clean]

Builds a test music library for Musinator.

  --out DIR     where to write (default: ~/Music/Musinator Test Library)
  --tracks N    number of normal library tracks (default: 10000)
  --seed S      seed for names, formats and tags (default: 1)
  --jobs J      parallel jobs (default: number of CPUs)
  --clean       delete the library folder and exit
  -h, --help    show this help";

struct Args {
    out: PathBuf,
    tracks: usize,
    seed: u64,
    jobs: Option<usize>,
    clean: bool,
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        out: std::env::home_dir().context("no home folder")?.join("Music/Musinator Test Library"),
        tracks: 10_000,
        seed: 1,
        jobs: None,
        clean: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => args.out = PathBuf::from(value()?),
            "--tracks" => args.tracks = value()?.parse().context("--tracks needs a number")?,
            "--seed" => args.seed = value()?.parse().context("--seed needs a number")?,
            "--jobs" => args.jobs = Some(value()?.parse().context("--jobs needs a number")?),
            "--clean" => args.clean = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("unknown option '{other}'\n\n{USAGE}"),
        }
    }
    Ok(Some(args))
}

fn main() -> ExitCode {
    match parse_args().and_then(|a| a.map_or(Ok(()), run)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    if args.clean {
        return clean(&args.out);
    }
    if let Some(jobs) = args.jobs {
        rayon::ThreadPoolBuilder::new().num_threads(jobs.max(1)).build_global()?;
    }
    ffmpeg::check()?;
    let start = Instant::now();
    let out = &args.out;
    prepare(out)?;

    println!("Building audio masters (cached in {})...", out.join(MASTERS_DIR).display());
    let m = masters::build(&out.join(MASTERS_DIR))?;

    let durations: Vec<f64> = m.lib.iter().map(|x| x.dur).collect();
    let plan = library::plan(args.seed, args.tracks, &durations);
    println!("Writing {} tracks in {} albums...", args.tracks, plan.len());
    let mut lib = Stats::default();
    for s in plan.par_iter().map(|a| library::write_album(out, a, &m)).collect::<Result<Vec<_>>>()? {
        lib.merge(s);
    }

    println!("Writing demo, gapless and loudness albums...");
    let demo = special::write_demo(out, &m)?;
    let gapless = special::write_gapless(out, &m)?;
    let loudness = special::write_loudness(out, &m)?;
    let special_tracks = json!({
        "musinator_demo": demo.tracks(),
        "gapless_test": gapless.tracks(),
        "loudness_test": loudness.tracks(),
    });
    let mut total = Stats::default();
    for s in [lib, demo, gapless, loudness] {
        total.merge(s);
    }
    let artists = plan.iter().map(|a| a.album_artist.as_str()).collect::<BTreeSet<_>>().len() + 3;
    let bytes = dir_size(out)?;
    let elapsed = start.elapsed().as_secs_f64();

    let formats: serde_json::Map<String, serde_json::Value> =
        total.formats.iter().map(|(f, n)| (f.label().to_string(), json!(n))).collect();
    let manifest = json!({
        "generator": "musinator testlib",
        "seed": args.seed,
        "library_tracks": args.tracks,
        "tracks": total.tracks(),
        "albums": total.albums,
        "artists": artists,
        "total_bytes": bytes,
        "elapsed_seconds": (elapsed * 10.0).round() / 10.0,
        "formats": formats,
        "special_tracks": special_tracks,
        "covers": {
            "embedded_albums": total.cover_embedded,
            "folder_file_albums": total.cover_file,
            "no_cover_albums": total.cover_none,
        },
        "lyrics": { "lrc_files": total.lrc, "embedded": total.lyrics_embedded },
        "gain_tags": { "replaygain_albums": total.replaygain_albums, "r128_albums": total.r128_albums },
    });
    std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;

    println!("\nDone in {elapsed:.1} s: {} tracks, {} albums, {artists} artists, {}", total.tracks(), total.albums, human_size(bytes));
    for (f, n) in &total.formats {
        println!("  {:<18} {n:>6}", f.label());
    }
    println!("Covers: {} embedded, {} as files, {} none (albums)", total.cover_embedded, total.cover_file, total.cover_none);
    println!("Lyrics: {} .lrc files, {} embedded", total.lrc, total.lyrics_embedded);
    println!("Gain tags: {} ReplayGain albums, {} R128 albums", total.replaygain_albums, total.r128_albums);
    println!("Library: {}", out.display());
    Ok(())
}

/// Make `out` ready: create it, or empty a folder made by an earlier run (keeping the cache).
fn prepare(out: &Path) -> Result<()> {
    if out.exists() {
        if !out.join(MARKER).exists() {
            if std::fs::read_dir(out)?.next().is_some() {
                bail!("{} is not empty and was not made by testlib. Use another --out.", out.display());
            }
        } else {
            let old: Vec<PathBuf> = std::fs::read_dir(out)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<PathBuf>>>()?
                .into_iter()
                .filter(|p| p.file_name().is_some_and(|n| n != MASTERS_DIR && n != MARKER))
                .collect();
            old.par_iter().try_for_each(|p| {
                if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
                    .with_context(|| format!("remove {}", p.display()))
            })?;
        }
    }
    std::fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;
    std::fs::write(out.join(MARKER), "Made by Musinator testlib. 'testlib --clean' deletes this folder.\n")?;
    Ok(())
}

fn clean(out: &Path) -> Result<()> {
    if !out.exists() {
        println!("Nothing to clean: {} does not exist.", out.display());
        return Ok(());
    }
    if !out.join(MARKER).exists() {
        bail!("{} has no {MARKER} file, so it was not made by testlib. Not deleting it.", out.display());
    }
    std::fs::remove_dir_all(out).with_context(|| format!("delete {}", out.display()))?;
    println!("Deleted {}", out.display());
    Ok(())
}

/// Bytes in `dir`, without the master cache.
fn dir_size(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == MASTERS_DIR {
            continue;
        }
        let meta = entry.metadata()?;
        total += if meta.is_dir() { dir_size(&entry.path())? } else { meta.len() };
    }
    Ok(total)
}

fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 { format!("{:.2} GB", b / 1e9) } else { format!("{:.1} MB", b / 1e6) }
}
