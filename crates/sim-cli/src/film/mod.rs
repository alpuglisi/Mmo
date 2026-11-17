//! `evo-sim film`: run a world for years headless and turn it into
//! marketing material: map time-lapse frames in the client's art style (9:16
//! and 1:1, with a date/season overlay and population sparklines), an MP4 and
//! GIF of each (when ffmpeg is installed), `timeline.json`, ranked
//! `stories.json`, and devlog charts. See tools/marketing/README.md.

mod art;
mod charts;
mod raster;
mod stories;
mod timeline;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use serde::Serialize;

use evo_sim::World;
use raster::{text_width, Canvas, Rgb};
use timeline::Timeline;

const HELP: &str = "\
evo-sim film — headless time-lapse, story mining and charts

USAGE:
    evo-sim film [--years N] [--out DIR] [--frame-days N] [--formats 9x16,1x1] [--fps N]
                 [--title TEXT] [--clips N] [--no-frames] [--no-video] [--keep-frames]
                 [world options: --seed N --size N --meat --food --catastrophes --catastrophe KIND@DAY
                  --migrants --niches --founding SPEC --design SPEC --set NAME=V ..]
    evo-sim film --from DIR/timeline.json [--out DIR]     (re-mine stories and redraw charts only)

WRITES (into --out, default film-out/):
    frames-9x16/00001.png ..   1080×1920 frames (title band, map, sparkline panel)
    frames-1x1/00001.png ..    1080×1080 frames (map, date pill, sparkline band)
    film-9x16.mp4 film-1x1.mp4 H.264 (yuv420p), and film-*.gif (480 px wide), if ffmpeg is on PATH
    clips/<story id>-9x16.mp4  a 6–30 s clip of each of the top --clips stories (needs ffmpeg)
    stories.json               notable events, ranked, in the publisher's format (tools/marketing/PUBLISHING.md)
    manifest.json              every video, GIF, clip and chart with its size, length, story and alt text
    timeline.json              per-day population and gene means per lineage, species, catastrophes
    population.svg/.png        population by lineage over time (catastrophes shaded)
    genes.svg/.png             gene drift: lineage means of ten genes over time

OPTIONS:
    --years N        Years to simulate (default 3; a year is 120 days)
    --frame-days N   A frame every N days (default 1)
    --fps N          Video frame rate (default 24)
    --formats LIST   Which frame shapes: 9x16, 1x1 or both (default both)
    --title TEXT     Title in the 9:16 band (default \"Evolution · seed N\")
    --clips N        Story clips to cut (default 6; 0 for none)
    --no-frames      Simulate and mine only (timeline, stories, charts)
    --no-video       Keep the PNG frames, skip ffmpeg
    --keep-frames    Keep the PNG frames after encoding (default: kept only without ffmpeg)
";

struct Opts {
    years: u32,
    out: PathBuf,
    frame_days: u32,
    fps: u32,
    tall: bool,
    square: bool,
    title: Option<String>,
    frames: bool,
    video: bool,
    keep_frames: bool,
    clips: usize,
    from: Option<PathBuf>,
}

pub fn main(argv: &[String]) -> ExitCode {
    let mut o = Opts {
        years: 3,
        out: PathBuf::from("film-out"),
        frame_days: 1,
        fps: 24,
        tall: true,
        square: true,
        title: None,
        frames: true,
        video: true,
        keep_frames: false,
        clips: 6,
        from: None,
    };
    let mut world_args = vec!["film".to_string()];
    let mut it = argv.iter();
    let parsed: Result<(), String> = (|| {
        while let Some(flag) = it.next() {
            let mut value = || it.next().cloned().ok_or(format!("{flag} needs a value"));
            let num = |v: String| v.parse::<u32>().map_err(|_| format!("{flag}: '{v}' is not a number"));
            match flag.as_str() {
                "--years" => o.years = num(value()?)?.max(1),
                "--out" => o.out = PathBuf::from(value()?),
                "--frame-days" => o.frame_days = num(value()?)?.max(1),
                "--fps" => o.fps = num(value()?)?.max(1),
                "--formats" => {
                    let v = value()?;
                    o.tall = v.split(',').any(|f| f.trim() == "9x16");
                    o.square = v.split(',').any(|f| f.trim() == "1x1");
                    if !o.tall && !o.square {
                        return Err(format!("--formats: expected 9x16 and/or 1x1, got '{v}'"));
                    }
                }
                "--title" => o.title = Some(value()?),
                "--no-frames" => o.frames = false,
                "--no-video" => o.video = false,
                "--keep-frames" => o.keep_frames = true,
                "--clips" => o.clips = num(value()?)? as usize,
                "--from" => o.from = Some(PathBuf::from(value()?)),
                "-h" | "--help" => return Err(String::new()),
                "--days" => return Err("film takes --years, not --days".into()),
                _ => world_args.push(flag.clone()),
            }
        }
        Ok(())
    })();
    match parsed {
        Err(e) if e.is_empty() => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            return ExitCode::FAILURE;
        }
        Ok(()) => {}
    }
    if let Err(e) = std::fs::create_dir_all(&o.out) {
        eprintln!("error: cannot create {}: {e}", o.out.display());
        return ExitCode::FAILURE;
    }
    let result = match &o.from {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|s| serde_json::from_str::<Timeline>(&s).map_err(|e| format!("{}: {e}", path.display())))
            .and_then(|t| outputs(&t, &o.out, &[])),
        None => crate::parse_args(&world_args).and_then(|args| {
            if let Some(n) = args.threads {
                rayon::ThreadPoolBuilder::new().num_threads(n).build_global().map_err(|e| e.to_string())?;
            }
            let size = if world_args.iter().any(|a| a == "--size") { args.size } else { 96 };
            let args = crate::Args { size, ..args };
            let world = crate::build_world(&args, 1);
            let meat = world_args.iter().any(|a| a == "--meat");
            film(world, meat, &o)
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One entry of `manifest.json` (tools/marketing/PUBLISHING.md, "Inputs").
#[derive(Serialize)]
struct Item {
    id: String,
    /// Relative to the manifest.
    path: String,
    /// video, image or chart.
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_s: Option<f64>,
    width: usize,
    height: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    story_id: Option<String>,
    alt: String,
}

#[derive(Serialize)]
struct Manifest<'a> {
    generated_at: String,
    world: String,
    items: &'a [Item],
}

fn world_name(t: &Timeline) -> String {
    format!("seed {} ({}x{} map)", t.seed, t.size, t.size)
}

/// Now as RFC 3339 UTC, e.g. "2026-11-17T12:00:00Z".
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, json + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}

/// A story clip cut from the frames: (story id, file name, seconds).
struct Clip {
    story: String,
    file: String,
    seconds: f64,
    format: &'static str,
}

/// stories.json, manifest.json and the charts, from a finished timeline
/// (and the film and clips, when they were made).
fn outputs(t: &Timeline, out: &Path, clips: &[Clip]) -> Result<(), String> {
    let mut stories = stories::mine(t);
    let charts = charts::write_all(t, out).map_err(|e| e.to_string())?;
    let mut items: Vec<Item> = Vec::new();
    let dpy = t.days_per_year.max(1);
    let years = t.days.last().map_or(0, |d| d.day) as f64 / dpy as f64;
    // The whole film, when there is one: a time-lapse story of its own.
    let film_seconds = (t.film.fps > 0).then(|| (t.film.frames as f64 / t.film.fps as f64 * 10.0).round() / 10.0);
    let mut film_ids = Vec::new();
    for (name, w, h) in [("9x16", 1080, 1920), ("1x1", 1080, 1080)] {
        let (mp4, gif) = (format!("film-{name}.mp4"), format!("film-{name}.gif"));
        let alt = format!(
            "Time-lapse of {years:.0} years of evolution on a {}x{} map: coloured dots are creatures of each lineage, with the date and population graphs overlaid",
            t.size, t.size
        );
        if out.join(&mp4).exists() {
            film_ids.push(format!("film-{name}"));
            items.push(Item {
                id: format!("film-{name}"),
                path: mp4,
                kind: "video",
                duration_s: film_seconds,
                width: w,
                height: h,
                story_id: None,
                alt: alt.clone(),
            });
        }
        if out.join(&gif).exists() {
            items.push(Item {
                id: format!("gif-{name}"),
                path: gif,
                kind: "image",
                duration_s: film_seconds,
                width: 480,
                height: 480 * h / w,
                story_id: None,
                alt,
            });
        }
    }
    for c in &charts {
        let alt = match c.name {
            "population" => "Line chart: population of each lineage over time, catastrophes shaded".to_string(),
            _ => "Small multiples: the average of ten genes in each lineage over time".to_string(),
        };
        let file = c.png.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        items.push(Item {
            id: format!("chart-{}", c.name),
            path: file,
            kind: "chart",
            duration_s: None,
            width: c.width,
            height: c.height,
            story_id: None,
            alt,
        });
        println!("Wrote {} and {}", c.svg.display(), c.png.display());
    }
    for c in clips {
        let Some(s) = stories.iter().find(|s| s.id == c.story) else { continue };
        let (w, h) = if c.format == "9x16" { (1080, 1920) } else { (1080, 1080) };
        let id = format!("clip-{}-{}", s.id, c.format);
        items.push(Item {
            id: id.clone(),
            path: format!("clips/{}", c.file),
            kind: "video",
            duration_s: Some(c.seconds),
            width: w,
            height: h,
            story_id: Some(s.id.clone()),
            alt: format!("Time-lapse of the map, {}: {}", s.when, s.title),
        });
    }
    for s in &mut stories {
        s.media = items.iter().filter(|i| i.story_id.as_deref() == Some(&s.id)).map(|i| i.id.clone()).collect();
        s.media.push(if s.kind == "trait_shift" { "chart-genes" } else { "chart-population" }.to_string());
    }
    if !film_ids.is_empty() {
        let end = t.days.last().map_or(0, |d| d.day);
        let first: u32 = t.days.first().map_or(0, |d| d.population.iter().sum());
        let last: u32 = t.days.last().map_or(0, |d| d.population.iter().sum());
        let peak: u32 = t.days.iter().map(|d| d.population.iter().sum::<u32>()).max().unwrap_or(0);
        let splits = t.species.iter().filter(|s| s.origin == "split").count();
        let mut facts = serde_json::Map::new();
        for (k, v) in [
            ("years", serde_json::json!(years.round() as u64)),
            ("video_seconds", serde_json::json!(film_seconds)),
            ("start_population", serde_json::json!(first)),
            ("end_population", serde_json::json!(last)),
            ("peak_population", serde_json::json!(peak)),
            ("lineages", serde_json::json!(t.lineages.len())),
            ("new_species", serde_json::json!(splits)),
            ("catastrophes", serde_json::json!(t.catastrophes.len())),
        ] {
            facts.insert(k.into(), v);
        }
        stories.push(stories::Story {
            id: format!("s{}-timelapse-d0", t.seed),
            kind: "timelapse",
            title: format!("{} years of evolution on one map", years.round() as u64),
            summary: format!(
                "One world from its first {first} creatures to {last}, with {splits} new species along the way. Every creature is simulated; nothing is scripted."
            ),
            facts,
            media: film_ids.iter().cloned().chain(["chart-population".to_string()]).collect(),
            players: Vec::new(),
            score: 0.5,
            start_day: 0,
            end_day: end,
            when: format!("{} – {}", t.date(0), t.date(end)),
            lineages: Vec::new(),
            series: serde_json::Value::Null,
        });
        stories.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.start_day.cmp(&b.start_day)));
    }
    let generated_at = now_rfc3339();
    let world = world_name(t);
    write_json(
        &out.join("stories.json"),
        &stories::StoriesFile {
            generated_at: generated_at.clone(),
            world: world.clone(),
            seed: t.seed,
            stories: &stories,
        },
    )?;
    for s in stories.iter().take(6) {
        println!("  {:.2}  {:<18} {}", s.score, s.kind, s.title);
    }
    write_json(&out.join("manifest.json"), &Manifest { generated_at, world, items: &items })
}

/// Cut a clip per top story out of the frames in `dir`: the story's days
/// plus 15 on each side, slowed or sped to last 6 to 30 seconds.
fn cut_clips(t: &Timeline, dir: &Path, format: &'static str, out: &Path, n: usize) -> Result<Vec<Clip>, String> {
    let fd = t.film.frame_days.max(1) as u64;
    let last = t.days.last().map_or(0, |d| d.day);
    let clips_dir = out.join("clips");
    std::fs::create_dir_all(&clips_dir).map_err(|e| e.to_string())?;
    let mut clips = Vec::new();
    for s in stories::mine(t).iter().filter(|s| s.end_day > s.start_day || s.kind == "catastrophe").take(n) {
        let (a, b) = (s.start_day.saturating_sub(15), (s.end_day + 15).min(last));
        let (f0, f1) = (a / fd + 1, (b / fd + 1).min(t.film.frames as u64));
        if f1 <= f0 + 1 {
            continue;
        }
        let count = f1 - f0 + 1;
        let real = count as f64 / t.film.fps.max(1) as f64;
        let seconds = real.clamp(6.0, 30.0);
        let rate = count as f64 / seconds;
        let file = format!("{}-{format}.mp4", s.id);
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-framerate",
                &format!("{rate:.4}"),
                "-start_number",
                &f0.to_string(),
                "-i",
            ])
            .arg(dir.join("%05d.png"))
            .args(["-frames:v", &count.to_string(), "-r", &t.film.fps.to_string()])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart"])
            .arg(clips_dir.join(&file))
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("ffmpeg failed cutting {file}"));
        }
        println!("Wrote {}", clips_dir.join(&file).display());
        clips.push(Clip { story: s.id.clone(), file, seconds: (seconds * 10.0).round() / 10.0, format });
    }
    Ok(clips)
}

fn film(mut world: World, meat: bool, o: &Opts) -> Result<(), String> {
    let days = o.years * world.config.days_per_year;
    let mut t = Timeline::new(&world, meat);
    t.sample(&world);
    let dirs: Vec<(&str, PathBuf)> = [("9x16", o.tall), ("1x1", o.square)]
        .into_iter()
        .filter(|&(_, on)| on && o.frames)
        .map(|(name, _)| (name, o.out.join(format!("frames-{name}"))))
        .collect();
    for (_, d) in &dirs {
        // Fresh frames only: leftovers from a longer run would end up in the video.
        if d.exists() {
            for e in std::fs::read_dir(d).map_err(|e| e.to_string())?.flatten() {
                if e.path().extension().is_some_and(|x| x == "png") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let title = o.title.clone().unwrap_or_else(|| format!("Evolution · seed {}", world.config.seed));
    println!(
        "Filming seed {} on a {}×{} map for {} years ({} days), a frame every {} day(s)",
        world.config.seed, world.map.width, world.map.height, o.years, days, o.frame_days
    );
    let started = Instant::now();
    let mut frame = 0u32;
    let render = |world: &World, t: &Timeline, frame: u32| -> Result<(), String> {
        if dirs.is_empty() {
            return Ok(());
        }
        let mut map = Canvas::new(MAP, MAP, [20, 24, 28]);
        art::paint_map(&mut map, world, 0, 0, MAP);
        for (name, dir) in &dirs {
            let c = if *name == "9x16" {
                tall_frame(&map, world, t, days, &title)
            } else {
                square_frame(&map, world, t, days)
            };
            c.save_png(&dir.join(format!("{frame:05}.png"))).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    frame += 1;
    render(&world, &t, frame)?;
    for day in 1..=days {
        for _ in 0..world.config.ticks_per_day {
            world.step();
        }
        t.sample(&world);
        if day % o.frame_days == 0 {
            frame += 1;
            render(&world, &t, frame)?;
        }
        if day % world.config.days_per_year == 0 {
            println!(
                "  year {} done · {} alive · {:.0}s",
                day / world.config.days_per_year,
                world.alive_count(),
                started.elapsed().as_secs_f64()
            );
        }
        if world.population() == 0 {
            println!("All life ended on day {}.", world.day());
            break;
        }
    }
    t.finish(&world);
    if !dirs.is_empty() {
        t.film = timeline::FilmInfo { frames: frame, frame_days: o.frame_days, fps: o.fps };
    }
    let json = serde_json::to_string(&t).map_err(|e| e.to_string())?;
    std::fs::write(o.out.join("timeline.json"), json).map_err(|e| e.to_string())?;
    println!("Wrote {}", o.out.join("timeline.json").display());
    let mut clips = Vec::new();
    if !dirs.is_empty() {
        println!("Wrote {frame} frames per format");
        let ffmpeg = o.video && Command::new("ffmpeg").arg("-version").output().is_ok_and(|r| r.status.success());
        if o.video && !ffmpeg {
            println!("ffmpeg not found: kept the PNG frames (see tools/marketing/README.md to encode them)");
        }
        if ffmpeg {
            for (name, dir) in &dirs {
                encode(dir, &o.out, name, o.fps)?;
            }
            // Story clips from the vertical frames (or the square ones).
            if o.clips > 0 {
                let _ = std::fs::remove_dir_all(o.out.join("clips"));
                let (name, dir) = &dirs[0];
                clips = cut_clips(&t, dir, name, &o.out, o.clips)?;
            }
            if !o.keep_frames {
                for (_, dir) in &dirs {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
        }
    }
    outputs(&t, &o.out, &clips)
}

fn encode(frames: &Path, out: &Path, name: &str, fps: u32) -> Result<(), String> {
    let input = frames.join("%05d.png");
    let mp4 = out.join(format!("film-{name}.mp4"));
    let gif = out.join(format!("film-{name}.gif"));
    let run = |args: &[&str], target: &Path| -> Result<(), String> {
        let r = Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-framerate", &fps.to_string(), "-i"])
            .arg(&input)
            .args(args)
            .arg(target)
            .status()
            .map_err(|e| e.to_string())?;
        if !r.success() {
            return Err(format!("ffmpeg failed writing {}", target.display()));
        }
        println!("Wrote {}", target.display());
        Ok(())
    };
    run(&["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart"], &mp4)?;
    let gif_fps = fps.min(15);
    let filter = format!(
        "fps={gif_fps},scale=480:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=4"
    );
    run(&["-vf", &filter, "-loop", "0"], &gif)
}

/// Map side in pixels (both formats are 1080 wide).
const MAP: usize = 1080;
const PANEL: Rgb = [21, 24, 28];
const WHITE: Rgb = [240, 243, 246];
const DIM: Rgb = [150, 160, 170];

fn blit(dst: &mut Canvas, src: &Canvas, x0: usize, y0: usize) {
    for y in 0..src.h.min(dst.h.saturating_sub(y0)) {
        let w = src.w.min(dst.w - x0);
        let s = y * src.w * 3;
        let d = ((y0 + y) * dst.w + x0) * 3;
        dst.px[d..d + w * 3].copy_from_slice(&src.px[s..s + w * 3]);
    }
}

fn season_name(world: &World) -> &'static str {
    let dpy = world.config.days_per_year.max(4) as u64;
    ["spring", "summer", "autumn", "winter"][((world.day() % dpy) * 4 / dpy) as usize]
}

/// The biggest lineages right now (index, count), at most `n`.
fn leaders(t: &Timeline, n: usize) -> Vec<(usize, u32)> {
    let Some(last) = t.days.last() else { return Vec::new() };
    let mut v: Vec<(usize, u32)> = last.population.iter().copied().enumerate().filter(|p| p.1 > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// A sparkline of `values` (one per day so far) over a box whose width
/// stands for the whole run (`days`), so the line grows across the video.
#[allow(clippy::too_many_arguments)]
fn sparkline(c: &mut Canvas, values: &[f64], days: u32, max: f64, x: f32, y: f32, w: f32, h: f32, col: Rgb) {
    if values.len() < 2 || max <= 0.0 {
        return;
    }
    let pts: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| (x + w * i as f32 / days.max(1) as f32, y + h - h * (v / max).min(1.0) as f32))
        .collect();
    c.polyline(&pts, 2.5, col, 1.0);
    let &(ex, ey) = pts.last().unwrap();
    c.disc(ex, ey, 4.0, col, 1.0);
}

fn series(t: &Timeline, l: Option<usize>) -> Vec<f64> {
    t.days
        .iter()
        .map(|d| match l {
            Some(l) => d.population.get(l).copied().unwrap_or(0) as f64,
            None => d.population.iter().sum::<u32>() as f64,
        })
        .collect()
}

/// 1080×1920: title and date band, the map, then one sparkline row per big lineage.
fn tall_frame(map: &Canvas, world: &World, t: &Timeline, days: u32, title: &str) -> Canvas {
    let mut c = Canvas::new(1080, 1920, PANEL);
    let top = 300;
    blit(&mut c, map, 0, top);
    c.text(60, 70, title, 5, WHITE, 1.0);
    let date = format!("Year {} · {}", world.year() + 1, season_name(world));
    c.text(60, 150, &date, 7, WHITE, 1.0);
    c.text(60, 230, &format!("Day {} · {} alive", world.day(), world.alive_count()), 4, DIM, 1.0);
    // Season progress bar under the band.
    let frac = world.day_of_year() as f32 / world.config.days_per_year.max(1) as f32;
    c.fill_rect(0, top as i64 - 6, 1080, 6, [40, 45, 52], 1.0);
    c.fill_rect(0, top as i64 - 6, (1080.0 * frac) as i64, 6, season_color(world), 1.0);
    // Panel: total, then the top lineages.
    let y0 = (top + MAP) as f32 + 40.0;
    let total = series(t, None);
    let max_total = total.iter().copied().fold(1.0, f64::max);
    c.text(60, y0 as i64, "All life", 3, WHITE, 1.0);
    let n = format!("{}", total.last().copied().unwrap_or(0.0) as u64);
    c.text(1020 - text_width(&n, 3), y0 as i64, &n, 3, WHITE, 1.0);
    sparkline(&mut c, &total, days, max_total, 60.0, y0 + 30.0, 960.0, 90.0, WHITE);
    let rows = leaders(t, 4);
    let max_l = rows.iter().map(|&(l, _)| series(t, Some(l)).into_iter().fold(1.0, f64::max)).fold(1.0, f64::max);
    for (k, &(l, count)) in rows.iter().enumerate() {
        let y = y0 + 150.0 + k as f32 * 80.0;
        let col = t.lineages[l].color;
        c.disc(72.0, y + 10.0, 9.0, col, 1.0);
        let name: String = t.lineages[l].name.chars().take(17).collect();
        c.text(94, y as i64, &name, 3, WHITE, 1.0);
        let n = format!("{count}");
        c.text(1020 - text_width(&n, 3), y as i64, &n, 3, col, 1.0);
        sparkline(&mut c, &series(t, Some(l)), days, max_l, 440.0, y - 12.0, 470.0, 44.0, col);
    }
    c
}

/// 1080×1080: the map, a date pill top-left and a translucent sparkline band.
fn square_frame(map: &Canvas, world: &World, t: &Timeline, days: u32) -> Canvas {
    let mut c = Canvas::new(1080, 1080, PANEL);
    blit(&mut c, map, 0, 0);
    let date = format!("Year {} · {}", world.year() + 1, season_name(world));
    let w = text_width(&date, 5) + 56;
    c.fill_rect(30, 30, w, 78, PANEL, 0.72);
    c.fill_rect(30, 30, 8, 78, season_color(world), 1.0);
    c.text(58, 50, &date, 5, WHITE, 1.0);
    // Bottom band.
    let (bh, by) = (190i64, 1080 - 190);
    c.fill_rect(0, by, 1080, bh, PANEL, 0.66);
    let total = series(t, None);
    let max_total = total.iter().copied().fold(1.0, f64::max);
    let rows = leaders(t, 4);
    for &(l, _) in rows.iter().rev() {
        sparkline(
            &mut c,
            &series(t, Some(l)),
            days,
            max_total,
            40.0,
            by as f32 + 40.0,
            640.0,
            120.0,
            t.lineages[l].color,
        );
    }
    sparkline(&mut c, &total, days, max_total, 40.0, by as f32 + 40.0, 640.0, 120.0, WHITE);
    c.text(720, by + 26, &format!("{} alive", world.alive_count()), 3, WHITE, 1.0);
    for (k, &(l, count)) in rows.iter().enumerate() {
        let y = by + 70 + k as i64 * 28;
        let col = t.lineages[l].color;
        c.disc(728.0, y as f32 + 7.0, 6.0, col, 1.0);
        let name: String = t.lineages[l].name.chars().take(14).collect();
        c.text(744, y, &name, 2, WHITE, 1.0);
        let n = format!("{count}");
        c.text(1050 - text_width(&n, 2), y, &n, 2, col, 1.0);
    }
    c
}

fn season_color(world: &World) -> Rgb {
    match season_name(world) {
        "spring" => [110, 168, 82],
        "summer" => [150, 168, 76],
        "autumn" => [190, 132, 64],
        _ => [214, 232, 240],
    }
}
