//! Story mining: scan a [`Timeline`] for notable events and rank them by how
//! interesting they are, for captions and posts. Every story carries its
//! time range, a one-line hook and the numbers behind it.

use serde::Serialize;

use super::timeline::Timeline;

#[derive(Serialize, Clone, Debug)]
pub struct Story {
    /// speciation, species_end, extinction, catastrophe, boom_crash, comeback,
    /// trait_shift.
    pub kind: &'static str,
    /// Higher is more interesting; roughly 0..1.
    pub score: f64,
    pub start_day: u64,
    pub end_day: u64,
    /// "year 2, spring (day 151) – year 3, winter (day 350)".
    pub when: String,
    pub hook: String,
    pub lineages: Vec<String>,
    /// The numbers behind it (kind-specific; series are `[day, value]`).
    pub data: serde_json::Value,
}

/// Every story found, best first.
pub fn mine(t: &Timeline) -> Vec<Story> {
    let mut out = Vec::new();
    speciation(t, &mut out);
    extinctions(t, &mut out);
    catastrophes(t, &mut out);
    for l in 0..t.lineages.len() {
        let series = smooth(&population(t, l), 5);
        boom_crash(t, l, &series, &mut out);
        comeback(t, l, &series, &mut out);
        trait_shifts(t, l, &mut out);
    }
    for s in &mut out {
        s.when = if s.start_day == s.end_day {
            t.date(s.start_day)
        } else {
            format!("{} – {}", t.date(s.start_day), t.date(s.end_day))
        };
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
    out
}

fn story(
    kind: &'static str,
    score: f64,
    start: u64,
    end: u64,
    hook: String,
    lineages: Vec<String>,
    data: serde_json::Value,
) -> Story {
    Story { kind, score, start_day: start, end_day: end, when: String::new(), hook, lineages, data }
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

/// At most `n` evenly spaced `[day, value]` points of a series (for the JSON).
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

/// Species splits that held at least `MIN_SPECIES_DAYS` (shorter ones are
/// the clusterer flickering at a boundary, not a story), and the end of any
/// species that held for a season or more.
const MIN_SPECIES_DAYS: u64 = 20;

fn speciation(t: &Timeline, out: &mut Vec<Story>) {
    let end = t.days.last().map_or(0, |d| d.day);
    for s in &t.species {
        let lived = s.extinct_day.unwrap_or(end).saturating_sub(s.founded_day);
        if s.origin == "split" && lived >= MIN_SPECIES_DAYS {
            let parent = s.parent.clone().unwrap_or_else(|| "its ancestors".into());
            let tail = match s.extinct_day {
                Some(_) => format!(" It lasted {lived} days."),
                None => format!(" It peaked at {} adults.", s.peak_adults.round()),
            };
            let score = 0.6
                + 0.2 * (lived as f64 / 120.0).min(1.0)
                + 0.1 * (s.peak_adults / 200.0).min(1.0)
                + if s.extinct_day.is_none() { 0.05 } else { 0.0 };
            out.push(story(
                "speciation",
                score,
                s.founded_day,
                s.extinct_day.unwrap_or(s.founded_day),
                format!("A new species branched off the {parent}: {}.{tail}", s.name),
                s.main_lineage.iter().cloned().collect(),
                serde_json::json!({ "species": s.name, "parent": s.parent, "peak_adults": s.peak_adults,
                                    "founded_day": s.founded_day, "extinct_day": s.extinct_day, "days": lived }),
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
                [] => " No descendants survive.".to_string(),
                [one] => format!(" Its line lives on as {one}."),
                many => format!(" Its line lives on as {}.", many.join(", ")),
            };
            out.push(story(
                "species_end",
                0.45 + 0.2 * (s.peak_adults / 300.0).min(1.0) + if heirs.is_empty() { 0.1 } else { 0.0 },
                s.founded_day,
                gone,
                format!("The {} species is gone after {lived} days.{tail}", s.name),
                s.main_lineage.iter().cloned().collect(),
                serde_json::json!({ "species": s.name, "peak_adults": s.peak_adults, "extinct_day": gone, "heirs": heirs }),
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
            peak_day,
            day,
            format!("The last of the {} died out — {} of them at their peak.", info.name, peak as u64),
            vec![info.name.clone()],
            serde_json::json!({ "peak": peak, "peak_day": peak_day, "extinct_day": day, "population": thin(&series, 40) }),
        ));
    }
}

fn catastrophes(t: &Timeline, out: &mut Vec<Story>) {
    for c in &t.catastrophes {
        let before = c.population_before.max(1) as f64;
        let after = c.population_after.map(|a| a as f64).unwrap_or_else(|| total_at(t, c.end_day) as f64);
        let drop = (1.0 - after / before).clamp(0.0, 1.0);
        let hook = if drop >= 0.05 {
            format!(
                "A {} struck and wiped out {:.0}% of all life ({} → {}).",
                c.kind,
                drop * 100.0,
                c.population_before,
                after as u64
            )
        } else {
            format!(
                "{} {} struck — and life held on ({} → {}).",
                article(&c.kind),
                c.kind,
                c.population_before,
                after as u64
            )
        };
        out.push(story(
            "catastrophe",
            0.5 + 0.4 * drop + 0.1 * c.severity.clamp(0.0, 1.0) as f64,
            c.start_day,
            c.end_day,
            hook,
            Vec::new(),
            serde_json::json!({ "catastrophe": c.kind, "severity": c.severity, "population_before": c.population_before,
                                "population_after": after, "drop": drop }),
        ));
    }
}

/// A population that at least doubled from where it was to a peak, then fell
/// to at most 40% of it, within about a year each way. Only the biggest boom
/// per lineage; carnivores (predators) get the "predator" wording.
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
    let what = if predator { "Predator boom and bust" } else { "Boom and bust" };
    out.push(story(
        "boom_crash",
        0.55 + 0.25 * (ratio.ln() / 8f64.ln()).clamp(0.0, 1.0) + if predator { 0.1 } else { 0.0 },
        s[b].0,
        s[c].0,
        format!(
            "{what}: the {} went from {} to {} in {} days — then crashed to {}.",
            info.name,
            s[b].1.round(),
            s[p].1.round(),
            s[p].0 - s[b].0,
            s[c].1.round()
        ),
        vec![info.name.clone()],
        serde_json::json!({ "predator": predator, "base": s[b].1, "peak": s[p].1, "crash": s[c].1,
                            "peak_day": s[p].0, "population": thin(&s[b..=c], 40) }),
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
    out.push(story(
        "comeback",
        0.7 + 0.25 * depth,
        s[p].0,
        s[r].0,
        format!(
            "Back from the brink: the {} fell from {} to just {} — and recovered to {}.",
            info.name,
            s[p].1.round(),
            s[lo].1.round(),
            s[r].1.round()
        ),
        vec![info.name.clone()],
        serde_json::json!({ "peak": s[p].1, "low": s[lo].1, "low_day": s[lo].0, "recovered": s[r].1,
                            "population": thin(&s[p..=r], 40) }),
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
            // The extreme of [a, b) against b.
            for i in a..b {
                let d = s[b].1 - s[i].1;
                if d.abs() >= 0.15 && best.is_none_or(|x| d.abs() > x.0.abs()) {
                    best = Some((d, i, b));
                }
            }
        }
        let Some((d, i, b)) = best else { continue };
        let (from, to) = (s[i].1, s[b].1);
        out.push(story(
            "trait_shift",
            0.4 + 0.5 * (d.abs() / 0.5).min(1.0)
                + if gene == "pack instinct" || gene == "carnivory" { 0.05 } else { 0.0 },
            s[i].0,
            s[b].0,
            trait_hook(&info.name, gene, from, to, s[b].0 - s[i].0),
            vec![info.name.clone()],
            serde_json::json!({ "gene": gene, "from": from, "to": to, "change": d, "mean": thin(&s[i..=b], 40) }),
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

fn trait_hook(name: &str, gene: &str, from: f64, to: f64, days: u64) -> String {
    let up = to > from;
    let phrase = match (gene, up) {
        ("carnivory", true) => "turned to hunting".to_string(),
        ("carnivory", false) => "gave up meat for plants".to_string(),
        ("pack instinct", true) => "evolved pack hunting".to_string(),
        ("pack instinct", false) => "abandoned pack hunting".to_string(),
        ("body size", true) => "grew bigger".to_string(),
        ("body size", false) => "shrank".to_string(),
        ("group tolerance", true) => "became more social".to_string(),
        ("group tolerance", false) => "turned solitary".to_string(),
        ("aggression", true) => "turned aggressive".to_string(),
        ("aggression", false) => "mellowed out".to_string(),
        ("boldness", true) => "grew bolder".to_string(),
        ("boldness", false) => "grew cautious".to_string(),
        ("litter size", true) => "started having bigger litters".to_string(),
        ("litter size", false) => "started having fewer young".to_string(),
        ("fat stores", true) => "put on fat for hard times".to_string(),
        ("fat stores", false) => "slimmed down".to_string(),
        ("pickiness", true) => "became picky eaters".to_string(),
        ("pickiness", false) => "started eating anything".to_string(),
        (g, true) => format!("evolved a stronger {g}"),
        (g, false) => format!("lost their {g}"),
    };
    format!("The {name} {phrase} in {days} days ({gene} {from:.2} → {to:.2}).")
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
        for k in ["speciation", "catastrophe", "boom_crash", "comeback", "trait_shift", "extinction"] {
            assert!(kinds.contains(&k), "no {k} in {kinds:?}");
        }
        assert!(stories.windows(2).all(|w| w[0].score >= w[1].score));
        let pack = stories.iter().find(|s| s.kind == "trait_shift").unwrap();
        assert!(pack.hook.contains("evolved pack hunting"), "{}", pack.hook);
        let boom = stories.iter().find(|s| s.kind == "boom_crash").unwrap();
        assert!(boom.hook.starts_with("Predator boom"), "{}", boom.hook);
    }

    #[test]
    fn a_flat_world_has_no_population_stories() {
        let mut t = timeline(|_| 50, |_| 0.3);
        t.species.clear();
        t.catastrophes.clear();
        assert!(mine(&t).is_empty());
    }
}
