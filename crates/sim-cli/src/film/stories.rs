//! Story mining: scan a [`Timeline`] for notable events and rank them by how
//! interesting they are. Writes the `stories.json` that the publishing tool
//! reads (tools/marketing/PUBLISHING.md, "Inputs"): each story has a stable
//! `id`, a `kind`, a one-line `title`, a short `summary`, flat `facts` (the
//! only numbers copy may cite), `media` ids from `manifest.json` and
//! `players`, plus its time range and the series behind it.

use serde::Serialize;

use super::timeline::Timeline;

#[derive(Serialize, Clone, Debug)]
pub struct Story {
    /// Stable for a seed: `s{seed}-{kind}-{subject}-d{start_day}`.
    pub id: String,
    /// speciation, species_extinction, extinction, catastrophe,
    /// predator_boom, population_boom, comeback, trait_shift.
    pub kind: &'static str,
    /// One-line headline, true as stated.
    pub title: String,
    /// One to three sentences.
    pub summary: String,
    /// `{ name: value }`, scalars only: every number the copy may cite.
    pub facts: serde_json::Map<String, serde_json::Value>,
    /// Ids of this story's items in `manifest.json` (filled in by `film`).
    pub media: Vec<String>,
    /// Never anyone: sample lineages belong to no real player.
    pub players: Vec<serde_json::Value>,
    /// Higher is more interesting; roughly 0..1.
    pub score: f64,
    pub start_day: u64,
    pub end_day: u64,
    /// "year 2, spring (day 151) – year 3, winter (day 350)".
    pub when: String,
    pub lineages: Vec<String>,
    /// The series behind it (`[day, value]`, thinned), for charts.
    pub series: serde_json::Value,
}

/// The `stories.json` file.
#[derive(Serialize)]
pub struct StoriesFile<'a> {
    pub generated_at: String,
    pub world: String,
    pub seed: u64,
    pub stories: &'a [Story],
}

/// Every story found, best first.
pub fn mine(t: &Timeline) -> Vec<Story> {
    let mut out = Vec::new();
    species(t, &mut out);
    extinctions(t, &mut out);
    catastrophes(t, &mut out);
    for l in 0..t.lineages.len() {
        let series = smooth(&population(t, l), 5);
        boom_crash(t, l, &series, &mut out);
        comeback(t, l, &series, &mut out);
        trait_shifts(t, l, &mut out);
    }
    let dpy = t.days_per_year.max(4) as u64;
    for s in &mut out {
        s.when = if s.start_day == s.end_day {
            t.date(s.start_day)
        } else {
            format!("{} – {}", t.date(s.start_day), t.date(s.end_day))
        };
        let subject = s
            .lineages
            .first()
            .cloned()
            .or_else(|| s.facts.get("catastrophe").and_then(|v| v.as_str()).map(String::from))
            .unwrap_or_else(|| s.kind.to_string());
        s.id = format!("s{}-{}-{}-d{}", t.seed, s.kind, slug(&subject), s.start_day);
        let f = &mut s.facts;
        f.entry("year").or_insert((s.start_day / dpy + 1).into());
        f.entry("season").or_insert(season(t, s.start_day).into());
        f.entry("start_day").or_insert(s.start_day.into());
        f.entry("end_day").or_insert(s.end_day.into());
    }
    // Variety: each further story of a kind already ranked counts a little
    // less, so one busy kind doesn't fill the top of the list.
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.start_day.cmp(&b.start_day)));
    let mut seen: std::collections::HashMap<&str, i32> = std::collections::HashMap::new();
    for s in &mut out {
        let k = seen.entry(s.kind).or_insert(0);
        s.score = ((s.score * 0.92f64.powi(*k)) * 1000.0).round() / 1000.0;
        *k += 1;
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.start_day.cmp(&b.start_day)));
    // Ids must be unique (the publisher dedupes on them).
    let mut ids: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for s in &mut out {
        let n = ids.entry(s.id.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            s.id = format!("{}-{n}", s.id);
        }
    }
    out
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn season(t: &Timeline, day: u64) -> &'static str {
    let dpy = t.days_per_year.max(4) as u64;
    ["spring", "summer", "autumn", "winter"][((day % dpy) * 4 / dpy) as usize]
}

/// "year 3 (winter)".
fn at(t: &Timeline, day: u64) -> String {
    format!("year {} ({})", day / t.days_per_year.max(4) as u64 + 1, season(t, day))
}

fn facts(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn story(
    kind: &'static str,
    score: f64,
    (start_day, end_day): (u64, u64),
    title: String,
    summary: String,
    lineages: Vec<String>,
    f: serde_json::Value,
    series: serde_json::Value,
) -> Story {
    Story {
        id: String::new(),
        kind,
        title,
        summary,
        facts: facts(f),
        media: Vec::new(),
        players: Vec::new(),
        score,
        start_day,
        end_day,
        when: String::new(),
        lineages,
        series,
    }
}

/// Day-indexed population of lineage `l` (one value per sample).
fn population(t: &Timeline, l: usize) -> Vec<(u64, f64)> {
    t.days.iter().map(|d| (d.day, d.population.get(l).copied().unwrap_or(0) as f64)).collect()
}

/// Centered moving average over `2*r+1` samples.
fn smooth(s: &[(u64, f64)], r: usize) -> Vec<(u64, f64)> {
    (0..s.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(r), (i + r + 1).min(s.len()));
            (s[i].0, s[a..b].iter().map(|p| p.1).sum::<f64>() / (b - a) as f64)
        })
        .collect()
}

/// At most `n` evenly spaced `[day, value]` points of a series.
fn thin(s: &[(u64, f64)], n: usize) -> serde_json::Value {
    let step = s.len().div_ceil(n.max(1)).max(1);
    let pts: Vec<_> = s
        .iter()
        .enumerate()
        .filter(|(i, _)| i % step == 0 || *i + 1 == s.len())
        .map(|(_, &(d, v))| serde_json::json!([d, (v * 100.0).round() / 100.0]))
        .collect();
    serde_json::Value::Array(pts)
}

fn total_at(t: &Timeline, day: u64) -> u32 {
    t.days.iter().find(|d| d.day >= day).or(t.days.last()).map_or(0, |d| d.population.iter().sum())
}

fn round(v: f64) -> u64 {
    v.max(0.0).round() as u64
}

/// Species splits that held at least `MIN_SPECIES_DAYS` (shorter ones are
/// the clusterer flickering at a boundary, not a story), and the end of any
/// species that held for a season or more.
const MIN_SPECIES_DAYS: u64 = 20;

fn species(t: &Timeline, out: &mut Vec<Story>) {
    let end = t.days.last().map_or(0, |d| d.day);
    for s in &t.species {
        let lived = s.extinct_day.unwrap_or(end).saturating_sub(s.founded_day);
        let peak = round(s.peak_adults);
        let lineages: Vec<String> = s.main_lineage.iter().cloned().collect();
        if s.origin == "split" && lived >= MIN_SPECIES_DAYS {
            let parent = s.parent.clone().unwrap_or_else(|| "its ancestors".into());
            let fate = match s.extinct_day {
                Some(_) => format!("It lasted {lived} days, peaking at {peak} adults."),
                None => format!("It is still alive, and peaked at {peak} adults."),
            };
            let score = 0.6
                + 0.2 * (lived as f64 / 120.0).min(1.0)
                + 0.1 * (s.peak_adults / 200.0).min(1.0)
                + if s.extinct_day.is_none() { 0.05 } else { 0.0 };
            out.push(story(
                "speciation",
                score,
                (s.founded_day, s.extinct_day.unwrap_or(s.founded_day)),
                format!("{}: a new species branched off the {parent}", s.name),
                format!(
                    "In {}, part of the {parent} became a species of its own: {}. {fate}",
                    at(t, s.founded_day),
                    s.name
                ),
                lineages.clone(),
                serde_json::json!({ "species": s.name, "parent_species": parent, "peak_adults": peak,
                                    "days_lasted": lived, "still_alive": s.extinct_day.is_none() }),
                serde_json::Value::Null,
            ));
        }
        if let Some(gone) = s.extinct_day {
            if lived < 30 || s.peak_adults < 20.0 {
                continue;
            }
            let heirs: Vec<&str> = t
                .species
                .iter()
                .filter(|c| c.parent.as_deref() == Some(&s.name) && c.extinct_day.is_none_or(|d| d > gone))
                .map(|c| c.name.as_str())
                .collect();
            let tail = match heirs.as_slice() {
                [] => "No descendant species survive it.".to_string(),
                [one] => format!("Its line lives on as {one}."),
                many => format!("Its line lives on as {}.", many.join(", ")),
            };
            out.push(story(
                "species_extinction",
                0.45 + 0.2 * (s.peak_adults / 300.0).min(1.0) + if heirs.is_empty() { 0.1 } else { 0.0 },
                // Its last season, for the clip; the facts give its whole span.
                (gone.saturating_sub(t.days_per_year as u64 / 2).max(s.founded_day), gone),
                format!("The {} species is gone", s.name),
                format!(
                    "The {} died out in {} after {lived} days, having peaked at {peak} adults. {tail}",
                    s.name,
                    at(t, gone)
                ),
                lineages,
                serde_json::json!({ "species": s.name, "peak_adults": peak, "days_lasted": lived,
                                    "extinct_year": gone / t.days_per_year.max(4) as u64 + 1,
                                    "descendants": heirs.join(", ") }),
                serde_json::Value::Null,
            ));
        }
    }
}

fn extinctions(t: &Timeline, out: &mut Vec<Story>) {
    for (l, info) in t.lineages.iter().enumerate() {
        let series = population(t, l);
        let peak = series.iter().map(|p| p.1).fold(0.0, f64::max);
        // Last day alive, followed by zeros to the end.
        let Some(last_alive) = series.iter().rposition(|p| p.1 > 0.0) else { continue };
        if last_alive + 1 >= series.len() || peak < 5.0 {
            continue;
        }
        let day = series[last_alive + 1].0;
        let peak_day = series.iter().find(|p| p.1 == peak).map_or(0, |p| p.0);
        out.push(story(
            "extinction",
            0.45 + 0.3 * (peak / 100.0).min(1.0),
            (peak_day, day),
            format!("The last of the {} died out", info.name),
            format!("The {} peaked at {} in {} and were gone by {}.", info.name, round(peak), at(t, peak_day), at(t, day)),
            vec![info.name.clone()],
            serde_json::json!({ "lineage": info.name, "peak": round(peak), "extinct_year": day / t.days_per_year.max(4) as u64 + 1 }),
            thin(&series, 40),
        ));
    }
}

fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "An"
    } else {
        "A"
    }
}

fn catastrophes(t: &Timeline, out: &mut Vec<Story>) {
    for c in &t.catastrophes {
        let before = c.population_before.max(1) as f64;
        let after = c.population_after.map(|a| a as f64).unwrap_or_else(|| total_at(t, c.end_day) as f64);
        let drop = (1.0 - after / before).clamp(0.0, 1.0);
        let pct = round(drop * 100.0);
        let (title, outcome) = if drop >= 0.05 {
            (
                format!("{} {} wiped out {pct}% of all life", article(&c.kind), c.kind),
                format!("The living population fell from {} to {}.", c.population_before, round(after)),
            )
        } else {
            (
                format!("{} {} struck, and life held on", article(&c.kind), c.kind),
                format!("The living population went from {} to {}.", c.population_before, round(after)),
            )
        };
        out.push(story(
            "catastrophe",
            0.5 + 0.4 * drop + 0.1 * c.severity.clamp(0.0, 1.0) as f64,
            (c.start_day, c.end_day),
            title,
            format!(
                "{} {} began in {} and lasted {} days. {outcome}",
                article(&c.kind),
                c.kind,
                at(t, c.start_day),
                c.end_day.saturating_sub(c.start_day)
            ),
            Vec::new(),
            serde_json::json!({ "catastrophe": c.kind, "severity": (c.severity * 100.0).round() / 100.0,
                                "population_before": c.population_before, "population_after": round(after),
                                "lost_pct": pct, "days_lasted": c.end_day.saturating_sub(c.start_day) }),
            serde_json::Value::Null,
        ));
    }
}

/// A population that at least doubled from where it was to a peak, then fell
/// to at most 40% of it, within about a year each way. Only the biggest boom
/// per lineage; carnivores (mean carnivory over 0.5) are predator booms.
fn boom_crash(t: &Timeline, l: usize, s: &[(u64, f64)], out: &mut Vec<Story>) {
    let window = t.days_per_year as u64;
    let mut best: Option<(f64, usize, usize, usize)> = None; // (ratio, base, peak, crash)
    for p in 0..s.len() {
        let peak = s[p].1;
        if peak < 20.0 {
            continue;
        }
        let base = (0..p).rev().take_while(|&i| s[p].0 - s[i].0 <= window).min_by(|&a, &b| s[a].1.total_cmp(&s[b].1));
        let crash =
            (p + 1..s.len()).take_while(|&i| s[i].0 - s[p].0 <= window).min_by(|&a, &b| s[a].1.total_cmp(&s[b].1));
        let (Some(b), Some(c)) = (base, crash) else { continue };
        if s[b].1 * 2.0 > peak || s[c].1 > 0.4 * peak {
            continue;
        }
        let ratio = peak / s[b].1.max(1.0) * (1.0 - s[c].1 / peak);
        if best.is_none_or(|x| ratio > x.0) {
            best = Some((ratio, b, p, c));
        }
    }
    let Some((ratio, b, p, c)) = best else { return };
    let info = &t.lineages[l];
    let predator = mean_gene(t, l, "carnivory", s[b].0, s[c].0).is_some_and(|m| m > 0.5);
    let (base, peak, crash) = (round(s[b].1), round(s[p].1), round(s[c].1));
    let (rise, fall) = (s[p].0 - s[b].0, s[c].0 - s[p].0);
    out.push(story(
        if predator { "predator_boom" } else { "population_boom" },
        0.55 + 0.25 * (ratio.ln() / 8f64.ln()).clamp(0.0, 1.0) + if predator { 0.1 } else { 0.0 },
        (s[b].0, s[c].0),
        format!("{} boom and bust: the {}", if predator { "Predator" } else { "Population" }, info.name),
        format!(
            "The {} went from {base} to {peak} in {rise} days, then crashed to {crash} within {fall} days.",
            info.name
        ),
        vec![info.name.clone()],
        serde_json::json!({ "lineage": info.name, "predator": predator, "from": base, "peak": peak, "crash": crash,
                            "rise_days": rise, "fall_days": fall }),
        thin(&s[b..=c], 40),
    ));
}

/// Down to at most 15% of an earlier peak (and 30 or fewer), then back to
/// at least half that peak and 3× the low.
fn comeback(t: &Timeline, l: usize, s: &[(u64, f64)], out: &mut Vec<Story>) {
    let mut peak = (0usize, 0.0f64);
    let mut low: Option<usize> = None;
    let mut best: Option<(f64, usize, usize, usize)> = None;
    for i in 0..s.len() {
        let v = s[i].1;
        match low {
            None => {
                if v > peak.1 {
                    peak = (i, v);
                } else if peak.1 >= 20.0 && v <= 0.15 * peak.1 && v <= 30.0 && v > 0.0 {
                    low = Some(i);
                }
            }
            Some(lo) => {
                if v < s[lo].1 && v > 0.0 {
                    low = Some(i);
                } else if v == 0.0 {
                    low = None;
                    peak = (i, 0.0);
                } else if v >= 0.5 * peak.1 && v >= 3.0 * s[lo].1 {
                    let depth = 1.0 - s[lo].1 / peak.1;
                    if best.is_none_or(|b| depth > b.0) {
                        best = Some((depth, peak.0, lo, i));
                    }
                    peak = (i, v);
                    low = None;
                }
            }
        }
    }
    let Some((depth, p, lo, r)) = best else { return };
    let info = &t.lineages[l];
    let (peak, low, back) = (round(s[p].1), round(s[lo].1), round(s[r].1));
    out.push(story(
        "comeback",
        0.7 + 0.25 * depth,
        (s[p].0, s[r].0),
        format!("Back from the brink: the {}", info.name),
        format!(
            "The {} fell from {peak} to just {low} in {}, then recovered to {back} by {}.",
            info.name,
            at(t, s[lo].0),
            at(t, s[r].0)
        ),
        vec![info.name.clone()],
        serde_json::json!({ "lineage": info.name, "peak": peak, "low": low, "recovered": back }),
        thin(&s[p..=r], 40),
    ));
}

fn gene_series(t: &Timeline, l: usize, g: usize) -> Vec<(u64, f64, u32)> {
    t.days
        .iter()
        .filter_map(|d| {
            let v = d.genes.get(l)?.get(g).copied().flatten()?;
            Some((d.day, v as f64, d.population[l]))
        })
        .collect()
}

fn mean_gene(t: &Timeline, l: usize, gene: &str, from: u64, to: u64) -> Option<f64> {
    let g = t.genes.iter().position(|n| n == gene)?;
    let s: Vec<f64> = gene_series(t, l, g).into_iter().filter(|p| p.0 >= from && p.0 <= to).map(|p| p.1).collect();
    (!s.is_empty()).then(|| s.iter().sum::<f64>() / s.len() as f64)
}

/// The biggest move of each gene's (30-day smoothed) mean within two years,
/// in a lineage of 10 or more. One story per lineage and gene.
fn trait_shifts(t: &Timeline, l: usize, out: &mut Vec<Story>) {
    let window = 2 * t.days_per_year as u64;
    let info = &t.lineages[l];
    for (g, gene) in t.genes.iter().enumerate() {
        let raw: Vec<(u64, f64)> = gene_series(t, l, g).into_iter().filter(|p| p.2 >= 10).map(|p| (p.0, p.1)).collect();
        if raw.len() < 30 {
            continue;
        }
        let s = smooth(&raw, 15);
        let mut best: Option<(f64, usize, usize)> = None;
        let mut a = 0;
        for b in 0..s.len() {
            while s[b].0 - s[a].0 > window {
                a += 1;
            }
            for i in a..b {
                let d = s[b].1 - s[i].1;
                if d.abs() >= 0.15 && best.is_none_or(|x| d.abs() > x.0.abs()) {
                    best = Some((d, i, b));
                }
            }
        }
        let Some((d, i, b)) = best else { continue };
        let (from, to) = ((s[i].1 * 100.0).round() / 100.0, (s[b].1 * 100.0).round() / 100.0);
        let days = s[b].0 - s[i].0;
        out.push(story(
            "trait_shift",
            0.4 + 0.5 * (d.abs() / 0.5).min(1.0) + if gene == "pack instinct" || gene == "carnivory" { 0.05 } else { 0.0 },
            (s[i].0, s[b].0),
            format!("The {} {}", info.name, trait_phrase(gene, to > from)),
            format!(
                "Over {days} days from {}, the {}' average {gene} moved from {from:.2} to {to:.2} (on a 0 to 1 scale), by natural selection and drift alone.",
                at(t, s[i].0),
                info.name
            ),
            vec![info.name.clone()],
            serde_json::json!({ "lineage": info.name, "gene": gene, "from": from, "to": to, "days": days }),
            thin(&s[i..=b], 40),
        ));
    }
}

fn trait_phrase(gene: &str, up: bool) -> String {
    match (gene, up) {
        ("carnivory", true) => "turned to hunting".into(),
        ("carnivory", false) => "gave up meat for plants".into(),
        ("pack instinct", true) => "evolved pack hunting".into(),
        ("pack instinct", false) => "abandoned pack hunting".into(),
        ("body size", true) => "grew bigger".into(),
        ("body size", false) => "shrank".into(),
        ("group tolerance", true) => "became more social".into(),
        ("group tolerance", false) => "turned solitary".into(),
        ("aggression", true) => "turned aggressive".into(),
        ("aggression", false) => "mellowed out".into(),
        ("boldness", true) => "grew bolder".into(),
        ("boldness", false) => "grew cautious".into(),
        ("litter size", true) => "started having bigger litters".into(),
        ("litter size", false) => "started having fewer young".into(),
        ("fat stores", true) => "put on fat for hard times".into(),
        ("fat stores", false) => "slimmed down".into(),
        ("pickiness", true) => "became picky eaters".into(),
        ("pickiness", false) => "started eating anything".into(),
        (g, true) => format!("evolved a stronger {g}"),
        (g, false) => format!("lost their {g}"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::timeline::{CatastropheEvent, DaySample, LineageInfo, SpeciesEvent, Timeline};
    use super::*;

    fn timeline(pop: impl Fn(u64) -> u32, gene: impl Fn(u64) -> f32) -> Timeline {
        Timeline {
            seed: 1,
            size: 64,
            days_per_year: 120,
            meat: false,
            genes: vec!["carnivory".into(), "pack instinct".into()],
            lineages: vec![LineageInfo { name: "Foxes".into(), owner: 1, color: [200, 80, 60] }],
            days: (0..480)
                .map(|d| DaySample { day: d, population: vec![pop(d)], genes: vec![vec![Some(0.8), Some(gene(d))]] })
                .collect(),
            species: vec![SpeciesEvent {
                id: 2,
                name: "Red foxes".into(),
                origin: "split".into(),
                parent: Some("Foxes".into()),
                founded_day: 200,
                extinct_day: None,
                peak_adults: 30.0,
                main_lineage: Some("Foxes".into()),
            }],
            film: Default::default(),
            catastrophes: vec![CatastropheEvent {
                kind: "severe winter".into(),
                severity: 0.8,
                start_day: 300,
                end_day: 340,
                population_before: 100,
                population_after: Some(40),
            }],
        }
    }

    #[test]
    fn finds_the_planted_stories() {
        // Boom 20 → 200 by day 100, crash to 10 by day 160 (a comeback
        // from 10 back to 150 by day 260), then extinct at day 400; pack
        // instinct 0.2 → 0.7 over days 50..250.
        let pop = |d: u64| match d {
            0..=40 => 20,
            41..=100 => 20 + (d - 40) as u32 * 3,
            101..=160 => 200 - (d - 100) as u32 * 19 / 6,
            161..=260 => 10 + (d - 160) as u32 * 14 / 10,
            261..=399 => 150,
            _ => 0,
        };
        let gene = |d: u64| (0.2 + 0.5 * ((d as f32 - 50.0) / 200.0).clamp(0.0, 1.0)).min(0.7);
        let stories = mine(&timeline(pop, gene));
        let kinds: Vec<_> = stories.iter().map(|s| s.kind).collect();
        for k in ["speciation", "catastrophe", "predator_boom", "comeback", "trait_shift", "extinction"] {
            assert!(kinds.contains(&k), "no {k} in {kinds:?}");
        }
        assert!(stories.windows(2).all(|w| w[0].score >= w[1].score));
        let pack = stories.iter().find(|s| s.kind == "trait_shift").unwrap();
        assert!(pack.title.contains("evolved pack hunting"), "{}", pack.title);
        // The publisher's contract: unique ids, a title, scalar facts.
        let mut ids: Vec<_> = stories.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), stories.len());
        for s in &stories {
            assert!(!s.title.is_empty() && !s.summary.is_empty());
            assert!(s.facts.values().all(|v| !v.is_object() && !v.is_array()), "{}: {:?}", s.id, s.facts);
            assert!(s.facts.contains_key("year"));
        }
    }

    #[test]
    fn a_flat_world_has_no_population_stories() {
        let mut t = timeline(|_| 50, |_| 0.3);
        t.species.clear();
        t.catastrophes.clear();
        assert!(mine(&t).is_empty());
    }
}
