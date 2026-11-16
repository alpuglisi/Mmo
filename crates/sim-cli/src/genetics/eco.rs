//! Ecology level: full worlds over many in-game years.
//!
//! Standard worlds (the wild ecosystem and the four sample player
//! lineages), default balance, run headless. Between steps the harness only
//! reads organisms: nothing it does changes a world. Each day it inspects
//! the day's newborns against their parents' genomes (mutations per segment,
//! origin tags, litter-mates' gene backgrounds); every `sample_days` it
//! records per lineage: census, generation, heterozygosity (infinite
//! alleles: allele identity by exact value), allele variance, morphology
//! allele and phenotype frequencies, hidden load, divergence from the
//! founders, divergence and fertility between the west and east halves of
//! the map, and between lineages that can hybridize.
//!
//! Real ecology breaks every idealisation of the genome level (selection,
//! overlapping generations, fluctuating census, spatial structure), so most
//! checks here are soft: direction and magnitude. Invariants (valid
//! morphology, mutation rates of births, tags) are strict.

use std::collections::HashMap;
use std::fmt::Write as _;

use evo_sim::genome::{Gene, Genome, PlayerId, IN_DISTANCE, SEASONAL_GENES};
use evo_sim::mating;
use evo_sim::morph::{MorphGene, MorphLocus};
use evo_sim::organism::OrgId;
use evo_sim::rng::rng_for;
use evo_sim::{presets, Balance, WorldConfig};
use rand::Rng;
use rayon::prelude::*;

use super::{num, prop_check, Check, Verdict};

const STREAM: u64 = 0x0065_636f_6c6f_6779;

/// Hybrid offspring from which a lineage pair counts as exchanging genes.
const HYBRID_FLOW: u64 = 10;

#[derive(Clone, Debug)]
pub struct EcoOpts {
    /// Simulated days per world (`years × days_per_year` from the CLI).
    pub days: u64,
    pub years: u32,
    pub size: usize,
    pub seeds: u64,
    pub sample_days: u32,
    pub seed: u64,
    /// The carnivore-diet rules on (`Balance::carnivore_diet`).
    pub meat: bool,
}

/// One lineage at one sample.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub world: u64,
    pub day: u64,
    pub lineage: usize,
    pub name: String,
    pub alive: usize,
    pub gen_mean: f64,
    pub gen_max: u32,
    /// Mean unbiased infinite-alleles heterozygosity over the distance genes.
    pub h_genes: f64,
    /// The same over this lineage's neutral loci (seasonal genes it does
    /// not express).
    pub h_neutral: f64,
    /// Mean within-lineage allele variance over the distance genes / the
    /// neutral loci.
    pub v_genes: f64,
    pub v_neutral: f64,
    /// Mean |lineage mean − founders' mean| of expressed values over the
    /// distance genes / neutral loci.
    pub drift_genes: f64,
    pub drift_neutral: f64,
    /// Mean genetic distance (Genome::distance) between random members.
    pub within: f64,
    /// Net divergence between the west and east halves: mean between-half
    /// distance − mean within-half distance; and mean fertility between.
    pub ew_net: f64,
    pub ew_fertility: f64,
    /// Non-default discrete morphology alleles per allele, and members
    /// expressing any non-default discrete value.
    pub morph_alleles: f64,
    pub morph_expressed: f64,
    pub harmful_per_org: f64,
    pub affected: f64,
}

/// A pair of lineages that could hybridize at founding.
#[derive(Clone, Debug, Default)]
pub struct PairRow {
    pub world: u64,
    pub a: usize,
    pub b: usize,
    pub distance: f64,
    pub fertility: f64,
    /// Hybrid offspring born so far (either way round).
    pub hybrids: u64,
}

#[derive(Clone, Debug, Default)]
struct Births {
    /// Per segment (genes, obedience, load, morph discrete, morph continuous):
    /// alleles examined and mutations seen.
    alleles: [u64; 5],
    mutated: [u64; 5],
    tag_errors: u64,
    invalid: u64,
    newborns: u64,
    litter_pairs: u64,
    identical_backgrounds: u64,
}

struct WorldResult {
    rows: Vec<Row>,
    pairs: Vec<PairRow>,
    births: Births,
    invalid_sampled: u64,
    names: Vec<String>,
    founders_h: Vec<f64>,
    seconds: f64,
}

pub struct EcoResults {
    pub checks: Vec<Check>,
    pub sections: Vec<(String, String)>,
    pub csv: String,
}

fn h_unbiased(vals: &mut [u32]) -> f64 {
    let n = vals.len() as f64;
    if n < 2.0 {
        return f64::NAN;
    }
    vals.sort_unstable();
    let mut hom = 0.0;
    let mut k = 0;
    while k < vals.len() {
        let j = vals[k..].iter().take_while(|&&x| x == vals[k]).count();
        hom += (j as f64 / n).powi(2);
        k += j;
    }
    n / (n - 1.0) * (1.0 - hom)
}

fn gene_stats(gs: &[&Genome], k: usize) -> (f64, f64, f64) {
    let mut keys: Vec<u32> = Vec::with_capacity(2 * gs.len());
    let (mut s, mut s2, mut e) = (0.0, 0.0, 0.0);
    for g in gs {
        let l = &g.loci()[k];
        keys.push(l.a.value.to_bits());
        keys.push(l.b.value.to_bits());
        s += (l.a.value + l.b.value) as f64;
        s2 += (l.a.value * l.a.value + l.b.value * l.b.value) as f64;
        e += g.expr(Gene::ALL[k]) as f64;
    }
    let n = 2.0 * gs.len() as f64;
    let m = s / n;
    (h_unbiased(&mut keys), s2 / n - m * m, e / gs.len() as f64)
}

fn invalid_morph(g: &Genome) -> u64 {
    let m = g.morph();
    MorphLocus::ALL
        .iter()
        .map(|&l| {
            let p = &m.discrete[l.index()];
            !l.is_valid(p.a.value) as u64 + !l.is_valid(p.b.value) as u64 + !l.is_valid(m.expressed(l)) as u64
        })
        .sum()
}

/// Mutations of one newborn against its parents (`a` from the mother, `b`
/// from the father), counted only where they are certain to be visible.
fn count_birth(c: &Genome, owner: PlayerId, m: &Genome, f: &Genome, b: &mut Births) {
    b.newborns += 1;
    for k in 0..Gene::COUNT {
        let seg = if k == Gene::Obedience as usize { 1 } else { 0 };
        let l = &c.loci()[k];
        for (a, p) in [(l.a, &m.loci()[k]), (l.b, &f.loci()[k])] {
            // A mutation clamped onto a parental bound value is invisible.
            if [p.a.value, p.b.value].iter().any(|&v| v == 0.0 || v == 1.0) {
                continue;
            }
            b.alleles[seg] += 1;
            let mutated = a.value != p.a.value && a.value != p.b.value;
            if mutated {
                b.mutated[seg] += 1;
                b.tag_errors += (a.origin != owner) as u64;
            }
        }
    }
    for k in 0..c.load().len() {
        let l = &c.load()[k];
        for (a, p) in [(l.a, &m.load()[k]), (l.b, &f.load()[k])] {
            if p.a.is_harmful() || p.b.is_harmful() {
                continue;
            }
            b.alleles[2] += 1;
            if a.is_harmful() {
                b.mutated[2] += 1;
                b.tag_errors += (a.origin != owner) as u64;
            }
        }
    }
    for l in MorphLocus::ALL {
        let p = &c.morph().discrete[l.index()];
        for (a, q) in [(p.a, &m.morph().discrete[l.index()]), (p.b, &f.morph().discrete[l.index()])] {
            if q.a.value != q.b.value {
                continue;
            }
            b.alleles[3] += 1;
            if a.value != q.a.value {
                b.mutated[3] += 1;
                b.tag_errors += (a.origin != owner) as u64;
            }
        }
    }
    for g in MorphGene::ALL {
        let p = &c.morph().continuous[g.index()];
        for (a, q) in [(p.a, &m.morph().continuous[g.index()]), (p.b, &f.morph().continuous[g.index()])] {
            if [q.a.value, q.b.value].iter().any(|&v| v == 0.0 || v == 1.0) {
                continue;
            }
            b.alleles[4] += 1;
            if a.value != q.a.value && a.value != q.b.value {
                b.mutated[4] += 1;
                b.tag_errors += (a.origin != owner) as u64;
            }
        }
    }
    b.invalid += invalid_morph(c);
}

fn run_world(opts: &EcoOpts, world: u64, log: bool) -> WorldResult {
    let started = std::time::Instant::now();
    let seed = opts.seed + world;
    let balance = if opts.meat { Balance::default().carnivore_diet() } else { Balance::default() };
    let config = WorldConfig { seed, width: opts.size, height: opts.size, balance, ..Default::default() };
    let mut w = presets::standard_world(config, 1);
    let tpd = w.config.ticks_per_day;
    let days = opts.days;
    let names: Vec<String> = w.lineages.iter().map(|l| l.name.clone()).collect();
    let nl = names.len();
    // Founders: mean expressed values, heterozygosity, neutral loci.
    let mut founder_mean = vec![[0.0f64; Gene::COUNT]; nl];
    let mut founders_h = vec![0.0; nl];
    let mut neutral: Vec<Vec<usize>> = vec![Vec::new(); nl];
    let dist_genes: Vec<usize> = (0..Gene::COUNT).filter(|&k| IN_DISTANCE[k]).collect();
    for (li, fm) in founder_mean.iter_mut().enumerate() {
        let gs: Vec<&Genome> = w.organisms.iter().filter(|o| o.lineage as usize == li).map(|o| &*o.genome).collect();
        let mut h = 0.0;
        for k in 0..Gene::COUNT {
            let (hk, _, e) = gene_stats(&gs, k);
            fm[k] = e;
            if IN_DISTANCE[k] {
                h += hk;
            }
        }
        founders_h[li] = h / dist_genes.len() as f64;
        // Seasonal genes far below the strategy threshold do nothing.
        neutral[li] = SEASONAL_GENES.iter().map(|g| g.index()).filter(|&k| fm[k] < 0.25).collect();
    }
    // Lineage pairs that could hybridize at founding.
    let mut hyb_pairs = Vec::new();
    for a in 0..nl {
        for b in a + 1..nl {
            let ga: Vec<&Genome> = w.organisms.iter().filter(|o| o.lineage as usize == a).map(|o| &*o.genome).collect();
            let gb: Vec<&Genome> = w.organisms.iter().filter(|o| o.lineage as usize == b).map(|o| &*o.genome).collect();
            if let (Some(x), Some(y)) = (ga.first(), gb.first()) {
                if mating::fertility(x, y) > 0.0 {
                    hyb_pairs.push((a, b));
                }
            }
        }
    }
    let mut rows = Vec::new();
    let mut pairs = Vec::new();
    let mut births = Births::default();
    let mut invalid_sampled = 0u64;
    for day in 0..=days {
        if day > 0 {
            for _ in 0..tpd {
                w.step();
            }
            // The day's newborns against their parents.
            let index: HashMap<OrgId, usize> = w.organisms.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
            let mut litters: HashMap<(OrgId, u32), Vec<usize>> = HashMap::new();
            for (i, o) in w.organisms.iter().enumerate() {
                if o.age_ticks >= tpd || o.generation == 0 {
                    continue;
                }
                let (Some(m), Some(f)) = (o.mother.and_then(|m| index.get(&m)), o.father.and_then(|f| index.get(&f)))
                else {
                    continue;
                };
                count_birth(&o.genome, o.owner, &w.organisms[*m].genome, &w.organisms[*f].genome, &mut births);
                litters.entry((o.mother.unwrap(), o.age_ticks)).or_default().push(i);
            }
            for kids in litters.values() {
                for x in 0..kids.len() {
                    for y in x + 1..kids.len() {
                        births.litter_pairs += 1;
                        let (gx, gy) = (&w.organisms[kids[x]].genome, &w.organisms[kids[y]].genome);
                        births.identical_backgrounds += (gx.loci() == gy.loci()) as u64;
                    }
                }
            }
        }
        if day % opts.sample_days as u64 != 0 {
            continue;
        }
        let mut rng = rng_for(seed, STREAM, day, 0);
        let half = opts.size as f32 / 2.0;
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); nl];
        for (i, o) in w.organisms.iter().enumerate() {
            if o.alive {
                members[o.lineage as usize].push(i);
                invalid_sampled += invalid_morph(&o.genome);
            }
        }
        for li in 0..nl {
            let idx = &members[li];
            if idx.len() < 4 {
                continue;
            }
            let gs: Vec<&Genome> = idx.iter().map(|&i| &*w.organisms[i].genome).collect();
            let mut r = Row {
                world,
                day,
                lineage: li,
                name: names[li].clone(),
                alive: idx.len(),
                gen_mean: idx.iter().map(|&i| w.organisms[i].generation as f64).sum::<f64>() / idx.len() as f64,
                gen_max: idx.iter().map(|&i| w.organisms[i].generation).max().unwrap_or(0),
                ..Default::default()
            };
            let (mut nn, mut nd) = (0.0, 0.0);
            for k in 0..Gene::COUNT {
                let is_n = neutral[li].contains(&k);
                if !is_n && !IN_DISTANCE[k] {
                    continue;
                }
                let (h, v, e) = gene_stats(&gs, k);
                let d = (e - founder_mean[li][k]).abs();
                if is_n {
                    r.h_neutral += h;
                    r.v_neutral += v;
                    r.drift_neutral += d;
                    nn += 1.0;
                } else {
                    r.h_genes += h;
                    r.v_genes += v;
                    r.drift_genes += d;
                    nd += 1.0;
                }
            }
            r.h_genes /= nd;
            r.v_genes /= nd;
            r.drift_genes /= nd;
            if nn > 0.0 {
                r.h_neutral /= nn;
                r.v_neutral /= nn;
                r.drift_neutral /= nn;
            } else {
                (r.h_neutral, r.v_neutral, r.drift_neutral) = (f64::NAN, f64::NAN, f64::NAN);
            }
            let pick = |rng: &mut evo_sim::rng::SimRng, v: &[usize]| v[rng.gen_range(0..v.len())];
            let pairs_n = 120;
            r.within = (0..pairs_n)
                .map(|_| {
                    let (a, b) = (pick(&mut rng, idx), pick(&mut rng, idx));
                    w.organisms[a].genome.distance(&w.organisms[b].genome) as f64
                })
                .sum::<f64>()
                / pairs_n as f64;
            let west: Vec<usize> = idx.iter().copied().filter(|&i| w.organisms[i].pos.x < half).collect();
            let east: Vec<usize> = idx.iter().copied().filter(|&i| w.organisms[i].pos.x >= half).collect();
            if west.len() >= 4 && east.len() >= 4 {
                let mean_d = |rng: &mut evo_sim::rng::SimRng, x: &[usize], y: &[usize]| {
                    let (mut d, mut f) = (0.0, 0.0);
                    for _ in 0..pairs_n {
                        let (a, b) = (pick(rng, x), pick(rng, y));
                        let (ga, gb) = (&w.organisms[a].genome, &w.organisms[b].genome);
                        d += ga.distance(gb) as f64;
                        f += mating::fertility(ga, gb) as f64;
                    }
                    (d / pairs_n as f64, f / pairs_n as f64)
                };
                let (between, fert) = mean_d(&mut rng, &west, &east);
                let (ww, _) = mean_d(&mut rng, &west, &west);
                let (ee, _) = mean_d(&mut rng, &east, &east);
                r.ew_net = between - 0.5 * (ww + ee);
                r.ew_fertility = fert;
            } else {
                (r.ew_net, r.ew_fertility) = (f64::NAN, f64::NAN);
            }
            let (mut var_alleles, mut expressing) = (0usize, 0usize);
            for g in &gs {
                let m = g.morph();
                let mut shows = false;
                for l in MorphLocus::ALL {
                    let p = &m.discrete[l.index()];
                    var_alleles +=
                        (p.a.value != l.default_value()) as usize + (p.b.value != l.default_value()) as usize;
                    shows |= m.expressed(l) != l.default_value();
                }
                expressing += shows as usize;
                r.harmful_per_org += g.harmful_alleles() as f64;
                r.affected += (g.homozygous_load() > 0) as u8 as f64;
            }
            r.morph_alleles = var_alleles as f64 / (gs.len() * 2 * MorphLocus::COUNT) as f64;
            r.morph_expressed = expressing as f64 / gs.len() as f64;
            r.harmful_per_org /= gs.len() as f64;
            r.affected /= gs.len() as f64;
            rows.push(r);
        }
        for &(a, b) in &hyb_pairs {
            if members[a].len() < 2 || members[b].len() < 2 {
                continue;
            }
            let n = 120;
            let (mut d, mut f) = (0.0, 0.0);
            for _ in 0..n {
                let x = &w.organisms[members[a][rng.gen_range(0..members[a].len())]].genome;
                let y = &w.organisms[members[b][rng.gen_range(0..members[b].len())]].genome;
                d += x.distance(y) as f64;
                f += mating::fertility(x, y) as f64;
            }
            let hy = |x: usize, y: usize| w.counters.hybrids.get(&(x as u32, y as u32)).copied().unwrap_or(0);
            pairs.push(PairRow {
                world,
                a,
                b,
                distance: d / n as f64,
                fertility: f / n as f64,
                hybrids: hy(a, b) + hy(b, a),
            });
        }
        if log && day % w.config.days_per_year as u64 == 0 {
            eprintln!(
                "genetics: world {world} (seed {seed}) year {} of {}: {} alive ({:.0} s)",
                day / w.config.days_per_year as u64,
                opts.years,
                w.alive_count(),
                started.elapsed().as_secs_f64()
            );
        }
    }
    WorldResult { rows, pairs, births, invalid_sampled, names, founders_h, seconds: started.elapsed().as_secs_f64() }
}

/// Mutation–drift balance (infinite alleles), 4Nμ/(1+4Nμ).
fn h_eq(ne: f64, mu: f64) -> f64 {
    let t = 4.0 * ne * mu;
    t / (1.0 + t)
}

pub fn run(opts: &EcoOpts, log: bool) -> EcoResults {
    let worlds: Vec<WorldResult> = (0..opts.seeds).into_par_iter().map(|k| run_world(opts, k, log)).collect();
    let mut checks = Vec::new();
    let mut sections = Vec::new();
    let mu = WorldConfig::default().mutation.rate as f64;
    let b = Balance::default();
    let eco =
        |group: &'static str, name: String, expected: String, observed: String, verdict: Verdict, detail: String| {
            Check { level: "ecology", group, name, expected, observed, ci: "-".into(), verdict, detail }
        };
    // --- invariants
    let mut births = Births::default();
    for w in &worlds {
        for i in 0..5 {
            births.alleles[i] += w.births.alleles[i];
            births.mutated[i] += w.births.mutated[i];
        }
        births.tag_errors += w.births.tag_errors;
        births.invalid += w.births.invalid;
        births.newborns += w.births.newborns;
        births.litter_pairs += w.births.litter_pairs;
        births.identical_backgrounds += w.births.identical_backgrounds;
    }
    let invalid: u64 = worlds.iter().map(|w| w.invalid_sampled).sum::<u64>() + births.invalid;
    let mut c = super::exact_check(
        "morphology",
        "impossible morphology values (every living organism every sample, every newborn)",
        0,
        invalid,
        "Odd legs, ears not in {0, 2, 4}, eyes outside 1–6, unknown shapes.",
    );
    c.level = "ecology";
    checks.push(c);
    let labels = ["genes", "obedience", "load", "morph discrete", "morph continuous"];
    let rates = [mu, mu, b.recessive_mutation as f64, b.morph_step_rate as f64, mu];
    for i in 0..5 {
        let mut c = prop_check(
            "births",
            format!("mutations per allele per birth in the world, {}", labels[i]),
            rates[i],
            births.mutated[i],
            births.alleles[i],
            0.0,
            format!(
                "{} newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).",
                births.newborns
            ),
        );
        c.level = "ecology";
        checks.push(c);
    }
    let mut c = super::exact_check(
        "births",
        "mutated alleles not tagged with the newborn's owner",
        0,
        births.tag_errors,
        "Every visible mutation in a newborn carries its owner's ID (the mother's owner).",
    );
    c.level = "ecology";
    checks.push(c);
    checks.push(eco(
        "hash-seeded streams",
        "litter-mates with identical gene backgrounds".into(),
        "≈ 0".into(),
        format!("{} of {} pairs", births.identical_backgrounds, births.litter_pairs),
        Verdict::Info,
        "Litter-mates sharing every gene allele would also share their obedience and morphology draws (see the genome level's hash-seeded streams row). Measured on every litter born.".into(),
    ));
    // --- per lineage summaries at the end
    let mut table = String::from(
        "| World | Lineage | Alive | Mean gen. (max) | H founders → end | H neutral end | Implied Ne (H) | Harm. mean N | Drift genes / neutral | Within dist. | E–W net div. start → end | E–W fertility end | Morph variant alleles / expressing | Harmful / org |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let mut gens_ok = 0;
    let mut gens_max = 0.0f64;
    let (mut h_decl, mut h_total) = (0, 0);
    let (mut div_up, mut div_total) = (0, 0);
    let (mut ew_up, mut ew_total, mut ew_mixed) = (0, 0, 0);
    let (mut neutral_moved, mut neutral_total) = (0, 0);
    let (mut morph_hidden, mut morph_seen) = (0, 0);
    let mut ne_ratios = Vec::new();
    for w in &worlds {
        for li in 0..w.names.len() {
            let rs: Vec<&Row> = w.rows.iter().filter(|r| r.lineage == li).collect();
            let (Some(first), Some(last)) = (rs.first(), rs.last()) else { continue };
            let survived = last.day == rs.iter().map(|r| r.day).max().unwrap_or(0)
                && last.day == w.rows.iter().map(|r| r.day).max().unwrap_or(0);
            if !survived || rs.len() < 4 {
                let _ = writeln!(
                    table,
                    "| {} | {} | extinct by day {} | | | | | | | | | | | |",
                    last.world, w.names[li], last.day
                );
                continue;
            }
            gens_max = gens_max.max(last.gen_mean);
            if last.gen_mean >= 20.0 {
                gens_ok += 1;
            }
            let harm = rs.len() as f64 / rs.iter().map(|r| 1.0 / r.alive as f64).sum::<f64>();
            h_total += 1;
            if last.h_genes < w.founders_h[li] {
                h_decl += 1;
            }
            let ne = last.h_genes / (1.0 - last.h_genes) / (4.0 * mu);
            if last.gen_mean >= 20.0 {
                ne_ratios.push(ne / harm);
            }
            div_total += 1;
            let quarter = rs[rs.len() / 4];
            if last.drift_genes > quarter.drift_genes {
                div_up += 1;
            }
            let ew: Vec<&&Row> = rs.iter().filter(|r| r.ew_net.is_finite()).collect();
            let ew_first = ew.first().map(|r| r.ew_net).unwrap_or(f64::NAN);
            let ew_last = ew.last().map(|r| r.ew_net).unwrap_or(f64::NAN);
            if ew.len() >= 4 {
                ew_total += 1;
                // Mean over the last quarter vs the first quarter of samples.
                let q = (ew.len() / 4).max(1);
                let early = ew[..q].iter().map(|r| r.ew_net).sum::<f64>() / q as f64;
                let late = ew[ew.len() - q..].iter().map(|r| r.ew_net).sum::<f64>() / q as f64;
                if late > early {
                    ew_up += 1;
                }
                if late.abs() < 0.01 && last.ew_fertility > 0.9 {
                    ew_mixed += 1;
                }
            }
            if last.drift_neutral.is_finite() {
                neutral_total += 1;
                if last.drift_neutral > first.drift_neutral {
                    neutral_moved += 1;
                }
            }
            if last.morph_alleles > 0.0 {
                morph_seen += 1;
                let carriers_bound = 2.0 * MorphLocus::COUNT as f64 * last.morph_alleles;
                if last.morph_expressed < carriers_bound {
                    morph_hidden += 1;
                }
            }
            let _ = writeln!(
                table,
                "| {} | {} | {} | {:.1} ({}) | {} → {} | {} | {} | {} | {} / {} | {} | {} → {} | {} | {} / {} | {} |",
                last.world,
                w.names[li],
                last.alive,
                last.gen_mean,
                last.gen_max,
                num(w.founders_h[li]),
                num(last.h_genes),
                num(last.h_neutral),
                num(ne),
                num(harm),
                num(last.drift_genes),
                num(last.drift_neutral),
                num(last.within),
                num(ew_first),
                num(ew_last),
                num(last.ew_fertility),
                num(last.morph_alleles),
                num(last.morph_expressed),
                num(last.harmful_per_org)
            );
        }
    }
    sections.push(("Ecology: lineages at the end of each world".into(), table));
    checks.push(eco(
        "generations",
        "lineages reaching a mean generation of 20".into(),
        "≥ 1 per world".into(),
        format!("{gens_ok} lineages (deepest mean generation {:.1})", gens_max),
        Verdict::soft(gens_ok as u64 >= opts.seeds),
        format!("{} worlds of {}×{}, {} years each.", opts.seeds, opts.size, opts.size, opts.years),
    ));
    checks.push(eco(
        "heterozygosity",
        "lineages whose heterozygosity fell from the founders' level".into(),
        "most (founders are all distinct; drift outpaces μ)".into(),
        format!("{h_decl} of {h_total}"),
        Verdict::soft(h_total > 0 && h_decl * 4 >= h_total * 3),
        "Infinite-alleles heterozygosity (allele identity by exact value) over the distance genes.".into(),
    ));
    let (ne_mean, _) = super::mean_se(&ne_ratios);
    checks.push(eco(
        "heterozygosity",
        "implied Ne / harmonic-mean census (lineages ≥ 20 generations)".into(),
        "0.01–2 (typically 0.1–0.5 in real populations)".into(),
        format!("mean {} over {} lineages", num(ne_mean), ne_ratios.len()),
        Verdict::soft(ne_ratios.is_empty() || (0.01..2.0).contains(&ne_mean)),
        format!(
            "Ne from H = 4Neμ/(1+4Neμ) (μ={mu}); assumes mutation–drift balance, which selection, sweeps and founder effects bend. For scale: H_eq(Ne=50) = {}, H_eq(Ne=500) = {}.",
            num(h_eq(50.0, mu)),
            num(h_eq(500.0, mu))
        ),
    ));
    checks.push(eco(
        "divergence",
        "lineages still diverging from their founders (last sample vs first quarter)".into(),
        "most".into(),
        format!("{div_up} of {div_total}"),
        Verdict::soft(div_total > 0 && div_up * 2 > div_total),
        "Mean |lineage mean − founders' mean| over the distance genes: drift and selection move it away; it keeps growing.".into(),
    ));
    checks.push(eco(
        "divergence",
        "within a lineage, west and east halves stay one gene pool".into(),
        "most: net divergence < 0.01, fertility > 0.9".into(),
        format!("{ew_mixed} of {ew_total} (net divergence grew in {ew_up})"),
        Verdict::soft(ew_total > 0 && ew_mixed * 4 >= ew_total * 3),
        "Net divergence = mean distance between members in different halves of the map − mean within halves (last quarter of samples). On a map this size members range across it in a few generations: gene flow (4Nm ≫ 1) keeps F_ST near 0, so the halves do not diverge; the founding placement's initial difference decays instead. Isolation by distance would need a map many home ranges wide.".into(),
    ));
    checks.push(eco(
        "drift",
        "neutral loci (unexpressed seasonal genes) drifting from the founders".into(),
        "most".into(),
        format!("{neutral_moved} of {neutral_total}"),
        Verdict::soft(neutral_total == 0 || neutral_moved * 2 > neutral_total),
        "Seasonal genes far below the strategy threshold do nothing and are outside the distance: pure mutation and drift (reflected at 0).".into(),
    ));
    checks.push(eco(
        "morphology",
        "lineages carrying new morphology variants, mostly hidden".into(),
        "variants appear by step mutation; carriers outnumber those expressing".into(),
        format!("{morph_seen} lineages carry variants; hidden in {morph_hidden}"),
        Verdict::soft(morph_seen == 0 || morph_hidden * 2 >= morph_seen),
        format!("Step rate {} per allele; the default is dominant.", b.morph_step_rate),
    ));
    // Selected vs neutral response.
    let mut sel = Vec::new();
    let mut neu = Vec::new();
    for w in &worlds {
        for li in 0..w.names.len() {
            if let Some(last) = w.rows.iter().rfind(|r| r.lineage == li) {
                if last.gen_mean >= 10.0 && last.drift_neutral.is_finite() {
                    sel.push(last.drift_genes);
                    neu.push(last.drift_neutral);
                }
            }
        }
    }
    let (s_m, _) = super::mean_se(&sel);
    let (n_m, _) = super::mean_se(&neu);
    checks.push(eco(
        "selection",
        "mean shift of expressed genes vs neutral loci (lineages ≥ 10 generations)".into(),
        "both move; selected traits respond".into(),
        format!("genes {} vs neutral {} ({} lineages)", num(s_m), num(n_m), sel.len()),
        Verdict::Info,
        "Neutral loci start at 0 (clamped), so mutation reflected at the bound pushes their mean up: their shift is drift plus the boundary. The genes' shift includes selection.".into(),
    ));
    // Hybridizing lineage pairs.
    let mut ptable = String::from(
        "| World | Pair | Hybrids born | Distance start → end | Fertility start → end |\n|---|---|---|---|---|\n",
    );
    let (mut p_up, mut p_total, mut flow_closer, mut flow_total) = (0, 0, 0, 0);
    for w in &worlds {
        let mut keys: Vec<(usize, usize)> = w.pairs.iter().map(|p| (p.a, p.b)).collect();
        keys.sort();
        keys.dedup();
        for (a, b) in keys {
            let ps: Vec<&PairRow> = w.pairs.iter().filter(|p| p.a == a && p.b == b).collect();
            let (Some(f), Some(l)) = (ps.first(), ps.last()) else { continue };
            if l.hybrids >= HYBRID_FLOW {
                flow_total += 1;
                flow_closer += (l.distance <= f.distance) as u32;
            } else {
                p_total += 1;
                p_up += (l.distance > f.distance) as u32;
            }
            let _ = writeln!(
                ptable,
                "| {} | {} × {} | {} | {} → {} | {} → {} |",
                f.world,
                w.names[a],
                w.names[b],
                l.hybrids,
                num(f.distance),
                num(l.distance),
                num(f.fertility),
                num(l.fertility)
            );
        }
    }
    sections.push(("Ecology: lineage pairs that could hybridize at founding".into(), ptable));
    checks.push(eco(
        "divergence",
        "interfertile lineage pairs without gene flow drift apart".into(),
        "distance grows (fertility falls)".into(),
        format!("{p_up} of {p_total} pairs more distant at the end"),
        Verdict::soft(p_total == 0 || p_up * 2 > p_total),
        format!("Pairs of lineages with non-zero fertility at founding and fewer than {HYBRID_FLOW} hybrid offspring: separate gene pools diverge by drift and selection."),
    ));
    checks.push(eco(
        "divergence",
        "interfertile lineage pairs with gene flow".into(),
        "introgression: distance stays or shrinks".into(),
        format!("{flow_closer} of {flow_total} pairs closer at the end"),
        Verdict::soft(flow_total == 0 || flow_closer * 2 > flow_total),
        format!("Pairs with at least {HYBRID_FLOW} hybrid offspring: hybrids carry alleles both ways, so the two gene pools converge instead of diverging (migration–drift: F_ST ≈ 1/(1 + 4Nm))."),
    ));
    let mut times = String::new();
    for w in &worlds {
        let _ = writeln!(
            times,
            "- world {}: {:.0} s, {} newborns inspected",
            w.rows.first().map_or(0, |r| r.world),
            w.seconds,
            w.births.newborns
        );
    }
    sections.push(("Ecology: runtimes".into(), times));
    // CSV time series.
    let mut csv = String::from(
        "world,day,lineage,name,alive,gen_mean,gen_max,h_genes,h_neutral,v_genes,v_neutral,drift_genes,drift_neutral,within,ew_net,ew_fertility,morph_alleles,morph_expressed,harmful_per_org,affected\n",
    );
    for w in &worlds {
        for r in &w.rows {
            let _ = writeln!(
                csv,
                "{},{},{},{},{},{:.3},{},{:.5},{:.5},{:.6},{:.6},{:.5},{:.5},{:.5},{:.5},{:.4},{:.6},{:.5},{:.4},{:.5}",
                r.world,
                r.day,
                r.lineage,
                r.name,
                r.alive,
                r.gen_mean,
                r.gen_max,
                r.h_genes,
                r.h_neutral,
                r.v_genes,
                r.v_neutral,
                r.drift_genes,
                r.drift_neutral,
                r.within,
                r.ew_net,
                r.ew_fertility,
                r.morph_alleles,
                r.morph_expressed,
                r.harmful_per_org,
                r.affected
            );
        }
    }
    EcoResults { checks, sections, csv }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A short world: the invariants hold (strict checks) and the harness
    /// reads without changing the world.
    #[test]
    fn short_world_invariants() {
        let r = run(&EcoOpts { days: 30, years: 0, size: 48, seeds: 1, sample_days: 5, seed: 3, meat: false }, false);
        let failed: Vec<&Check> = r.checks.iter().filter(|c| c.verdict == Verdict::Fail).collect();
        assert!(failed.is_empty(), "{failed:?}");
        assert!(r.csv.lines().count() > 10);
    }

    /// The long ecology run (many minutes): `cargo test --release -p
    /// evo-sim-cli -- --ignored long_ecology_runs`.
    #[test]
    #[ignore]
    fn long_ecology_runs() {
        let r = run(
            &EcoOpts { days: 12 * 120, years: 12, size: 96, seeds: 4, sample_days: 10, seed: 1, meat: false },
            true,
        );
        let failed: Vec<&Check> = r.checks.iter().filter(|c| c.verdict == Verdict::Fail).collect();
        assert!(failed.is_empty(), "{failed:?}");
    }
}
