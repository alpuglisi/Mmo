//! Species detection ("Genetics": outbreeding depression drives speciation;
//! "Scoring": species founded).
//!
//! There is no species object in the simulation: whether two organisms are
//! the same kind is decided pair by pair (`mating::is_kin`,
//! `mating::fertility`). Every few days (`ScoringConfig::species_interval_days`)
//! the world samples its breeding adults (`world::chronicle`) and this module
//! clusters the sample by single linkage: two samples are linked when they
//! would recognize each other as kin **and** be fertile together (at least
//! `species_min_fertility`); a species is a connected component.
//!
//! **Sampling** keeps it cheap at any population: adults are grouped into
//! strata by (founding lineage, 16-tile region), cohorts of abstract regions
//! being strata of their own, and each stratum contributes up to
//! `species_per_stratum` evenly spaced members (fewer when there are many
//! strata, so at most `species_max_samples` in all). Each sample stands for
//! its stratum's adults divided by its picks, so species sizes are
//! estimates. Pairs are only compared across different components and most
//! cross-species pairs fail on the four body-plan genes, so a detection over
//! 1200 samples takes a few milliseconds.
//!
//! **Stable ids.** Each species keeps a few representative genomes
//! (farthest-point picks, so every branch is covered). A new cluster's
//! overlap with an old species is the number of its samples linked to one of
//! the species' representatives. Each old species continues as the cluster
//! it overlaps most; when several old species continue as the same cluster
//! the largest overlap keeps its id and the others are recorded as merged
//! into it. A cluster that overlaps an old species but is not its
//! continuation is a **split** (parent: the old species it overlaps most). A
//! cluster with no overlap is a split of the species of its main founding
//! lineage if that species lives on elsewhere, else a new founding (a newly
//! designed lineage). New species need `species_min_new` samples; smaller
//! unmatched clusters (hybrids, stragglers) are left unclassified. A species
//! not detected for `species_extinct_after` detections in a row is extinct
//! (dated to its last detection).
//!
//! **Credit ("species founded").** A split is credited to the player whose
//! origin-tagged alleles hold the plurality among the new species' sampled
//! adults at detection: the same measure as the legacy score, so the genes
//! that make up the new species earn it, whoever owns the organisms. If the
//! wild ecosystem's alleles hold the plurality, nobody is credited. The
//! species present at the first detection and new foundings are not
//! speciation events and earn no credit.

use std::collections::BTreeMap;

use crate::genome::{Gene, PlayerId, DISTANCE_GENES, IN_DISTANCE, WILD};
use crate::mating::{distance_fertility, BODY_PLAN_FULL, BODY_PLAN_GENES, BODY_PLAN_STERILE};

/// Expressed gene values of one sampled organism.
pub type Expr = [f32; Gene::COUNT];

/// One sampled breeding adult (or cohort pool genome).
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub expr: Expr,
    /// Breeding adults this sample stands for.
    pub weight: f64,
    pub lineage: u32,
    pub owner: PlayerId,
    /// Allele count per origin tag (`Genome::tally_origins` with weight 1).
    pub origins: BTreeMap<PlayerId, f64>,
}

/// The linkage rule, from the world configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkRule {
    pub species_distance: f32,
    pub kin_body_plan_gap: f32,
    pub min_fertility: f32,
}

impl LinkRule {
    pub fn from_config(cfg: &crate::WorldConfig) -> Self {
        LinkRule {
            species_distance: cfg.species_distance,
            kin_body_plan_gap: cfg.kin_body_plan_gap,
            min_fertility: cfg.scoring.species_min_fertility,
        }
    }

    /// Would the two recognize each other as kin and be fertile together?
    /// Mirrors `mating::is_kin` and `mating::fertility` on expressed values.
    pub fn linked(&self, a: &Expr, b: &Expr) -> bool {
        let gap = BODY_PLAN_GENES.iter().map(|g| (a[g.index()] - b[g.index()]).abs()).fold(0.0, f32::max);
        if gap >= self.kin_body_plan_gap || ramp(gap, BODY_PLAN_FULL, BODY_PLAN_STERILE) < self.min_fertility {
            return false;
        }
        // Distance, stopping as soon as kinship is out of reach.
        let limit = self.species_distance * DISTANCE_GENES;
        let mut sum = 0.0;
        for k in 0..Gene::COUNT {
            if IN_DISTANCE[k] {
                sum += (a[k] - b[k]).abs();
                if sum >= limit {
                    return false;
                }
            }
        }
        let distance = sum / DISTANCE_GENES;
        let rest = 1.0 - distance / self.species_distance;
        rest > 0.0 && gap < rest * self.kin_body_plan_gap && distance_fertility(distance) >= self.min_fertility
    }
}

fn ramp(x: f32, full: f32, sterile: f32) -> f32 {
    if x <= full {
        1.0
    } else {
        (1.0 - (x - full) / (sterile - full)).max(0.0)
    }
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Single-linkage clusters of `samples` under `rule`: each cluster is a list
/// of sample indices in ascending order; clusters are ordered by their first
/// index.
pub fn cluster(samples: &[Sample], rule: &LinkRule) -> Vec<Vec<usize>> {
    let n = samples.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
            if ri != rj && rule.linked(&samples[i].expr, &samples[j].expr) {
                parent[ri.max(rj)] = ri.min(rj);
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        by_root.entry(r).or_default().push(i);
    }
    by_root.into_values().collect()
}

/// How a species came to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpeciesOrigin {
    /// Present at the first detection.
    Initial,
    /// A newly founded lineage of its own kind (a new design).
    Founding,
    /// Split off an existing species (speciation).
    Split,
}

impl SpeciesOrigin {
    pub fn label(self) -> &'static str {
        match self {
            SpeciesOrigin::Initial => "initial",
            SpeciesOrigin::Founding => "founding",
            SpeciesOrigin::Split => "split",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Species {
    pub id: u32,
    pub name: String,
    pub origin: SpeciesOrigin,
    pub parent: Option<u32>,
    pub founded_day: u64,
    /// Day of the last detection that found it, once it is extinct.
    pub extinct_day: Option<u64>,
    /// Set when it went extinct by merging back into another species.
    pub merged_into: Option<u32>,
    /// Player credited with founding it (splits only; see the module docs).
    pub founder: Option<PlayerId>,
    /// Origin tag with the most alleles in its adults, and that share.
    pub dominant: PlayerId,
    pub dominant_share: f64,
    /// Estimated breeding adults at the last detection, and the most ever.
    pub adults: f64,
    pub peak_adults: f64,
    /// Estimated adults per founding lineage (mother's line), largest first.
    pub lineages: Vec<(u32, f64)>,
    /// Mean expressed diet (carnivory) and body size.
    pub carnivory: f32,
    pub body_size: f32,
    pub last_seen: u64,
    /// Detections in a row that missed it.
    pub missed: u32,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::array_vec"))]
    reps: Vec<Expr>,
}

impl Species {
    pub fn is_alive(&self) -> bool {
        self.extinct_day.is_none()
    }

    /// Main founding lineage at the last detection.
    pub fn main_lineage(&self) -> Option<u32> {
        self.lineages.first().map(|l| l.0)
    }
}

/// Something that happened at a detection.
#[derive(Clone, Debug, PartialEq)]
pub enum SpeciesEvent {
    Appeared { species: u32, origin: SpeciesOrigin, parent: Option<u32>, founder: Option<PlayerId> },
    Extinct { species: u32, merged_into: Option<u32> },
}

/// All species ever detected, with stable ids.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpeciesTracker {
    pub species: Vec<Species>,
    /// Speciation events credited per player.
    pub founded: BTreeMap<PlayerId, u32>,
    /// Day of the last detection.
    pub last_detection: Option<u64>,
    /// Samples taken at the last detection, and clusters left unclassified.
    pub last_samples: usize,
    pub unclassified: usize,
}

/// Summary of one cluster's samples.
struct Summary {
    adults: f64,
    lineages: Vec<(u32, f64)>,
    dominant: PlayerId,
    dominant_share: f64,
    carnivory: f32,
    body_size: f32,
}

fn summarize(samples: &[Sample], members: &[usize]) -> Summary {
    let mut adults = 0.0;
    let mut lineages: BTreeMap<u32, f64> = BTreeMap::new();
    let mut origins: BTreeMap<PlayerId, f64> = BTreeMap::new();
    let (mut carn, mut size) = (0.0f64, 0.0f64);
    for &i in members {
        let s = &samples[i];
        adults += s.weight;
        *lineages.entry(s.lineage).or_default() += s.weight;
        for (&p, &n) in &s.origins {
            *origins.entry(p).or_default() += n * s.weight;
        }
        carn += s.expr[Gene::Carnivory.index()] as f64 * s.weight;
        size += s.expr[Gene::BodySize.index()] as f64 * s.weight;
    }
    let mut lineages: Vec<(u32, f64)> = lineages.into_iter().collect();
    lineages.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: f64 = origins.values().sum();
    let (dominant, top) = origins.iter().fold((WILD, -1.0), |best, (&p, &n)| if n > best.1 { (p, n) } else { best });
    let w = adults.max(1e-9);
    Summary {
        adults,
        lineages,
        dominant,
        dominant_share: if total > 0.0 { top.max(0.0) / total } else { 0.0 },
        carnivory: (carn / w) as f32,
        body_size: (size / w) as f32,
    }
}

/// Up to `k` farthest-point picks (by L1 distance over expressed values),
/// starting from the first member.
fn pick_reps(samples: &[Sample], members: &[usize], k: usize) -> Vec<Expr> {
    if members.is_empty() || k == 0 {
        return Vec::new();
    }
    let dist = |a: &Expr, b: &Expr| a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>();
    let mut chosen = vec![members[0]];
    let mut near: Vec<f32> = members.iter().map(|&m| dist(&samples[m].expr, &samples[members[0]].expr)).collect();
    while chosen.len() < k.min(members.len()) {
        let (best, d) = near.iter().enumerate().fold((0, -1.0f32), |b, (i, &d)| if d > b.1 { (i, d) } else { b });
        if d <= 0.0 {
            break;
        }
        let m = members[best];
        chosen.push(m);
        for (i, &o) in members.iter().enumerate() {
            near[i] = near[i].min(dist(&samples[o].expr, &samples[m].expr));
        }
    }
    chosen.into_iter().map(|m| samples[m].expr).collect()
}

/// Settings of one detection.
#[derive(Clone, Copy, Debug)]
pub struct DetectParams {
    pub rule: LinkRule,
    pub min_new: usize,
    pub extinct_after: u32,
    pub reps: usize,
}

impl DetectParams {
    pub fn from_config(cfg: &crate::WorldConfig) -> Self {
        DetectParams {
            rule: LinkRule::from_config(cfg),
            min_new: cfg.scoring.species_min_new.max(1),
            extinct_after: cfg.scoring.species_extinct_after,
            reps: cfg.scoring.species_reps.max(1),
        }
    }
}

impl SpeciesTracker {
    pub fn get(&self, id: u32) -> Option<&Species> {
        self.species.get(id as usize)
    }

    pub fn alive(&self) -> impl Iterator<Item = &Species> {
        self.species.iter().filter(|s| s.is_alive())
    }

    /// Cluster `samples` taken on `day` and update the species list.
    /// `name_of` names a new species after a founding lineage.
    pub fn detect(
        &mut self,
        day: u64,
        samples: &[Sample],
        p: &DetectParams,
        name_of: impl Fn(u32) -> String,
    ) -> Vec<SpeciesEvent> {
        let clusters = cluster(samples, &p.rule);
        let first = self.last_detection.is_none();
        self.last_detection = Some(day);
        self.last_samples = samples.len();
        self.unclassified = 0;
        let live: Vec<usize> = (0..self.species.len()).filter(|&s| self.species[s].is_alive()).collect();

        // overlap[c][k]: samples of cluster c linked to a representative of live[k].
        let overlap: Vec<Vec<usize>> = clusters
            .iter()
            .map(|members| {
                live.iter()
                    .map(|&s| {
                        let reps = &self.species[s].reps;
                        members.iter().filter(|&&m| reps.iter().any(|r| p.rule.linked(&samples[m].expr, r))).count()
                    })
                    .collect()
            })
            .collect();
        // Each old species continues as the cluster it overlaps most.
        let successor: Vec<Option<usize>> = (0..live.len())
            .map(|k| {
                let (c, n) =
                    (0..clusters.len()).fold((0, 0), |b, c| if overlap[c][k] > b.1 { (c, overlap[c][k]) } else { b });
                (n > 0).then_some(c)
            })
            .collect();

        let mut events = Vec::new();
        let mut assigned: Vec<Option<u32>> = vec![None; clusters.len()];
        let mut seen = vec![false; live.len()];
        for (c, members) in clusters.iter().enumerate() {
            let sum = summarize(samples, members);
            // Old species continuing as this cluster, largest overlap first.
            let mut cont: Vec<usize> = (0..live.len()).filter(|&k| successor[k] == Some(c)).collect();
            cont.sort_by(|&a, &b| overlap[c][b].cmp(&overlap[c][a]).then(a.cmp(&b)));
            if let Some((&keep, rest)) = cont.split_first() {
                let id = live[keep] as u32;
                seen[keep] = true;
                for &k in rest {
                    seen[k] = true;
                    let s = &mut self.species[live[k]];
                    s.extinct_day = Some(day);
                    s.merged_into = Some(id);
                    events.push(SpeciesEvent::Extinct { species: s.id, merged_into: Some(id) });
                }
                self.update(id, day, &sum, samples, members, p.reps);
                assigned[c] = Some(id);
                continue;
            }
            if members.len() < p.min_new {
                self.unclassified += 1;
                continue;
            }
            // A new species: split of the old species it overlaps most, or of
            // its main lineage's species, else a founding.
            let by_overlap = (0..live.len())
                .filter(|&k| overlap[c][k] > 0)
                .max_by(|&a, &b| overlap[c][a].cmp(&overlap[c][b]).then(b.cmp(&a)));
            let parent = by_overlap.map(|k| live[k] as u32).or_else(|| {
                let main = sum.lineages.first().map(|l| l.0)?;
                live.iter().find(|&&s| self.species[s].main_lineage() == Some(main)).map(|&s| s as u32)
            });
            let origin = match (first, parent) {
                (true, _) => SpeciesOrigin::Initial,
                (false, Some(_)) => SpeciesOrigin::Split,
                (false, None) => SpeciesOrigin::Founding,
            };
            let founder = (origin == SpeciesOrigin::Split && sum.dominant != WILD).then_some(sum.dominant);
            if let Some(f) = founder {
                *self.founded.entry(f).or_default() += 1;
            }
            let id = self.species.len() as u32;
            let base = sum.lineages.first().map(|l| name_of(l.0)).unwrap_or_else(|| format!("Species {id}"));
            let same =
                self.species.iter().filter(|s| s.name == base || s.name.starts_with(&format!("{base} ("))).count();
            let name = if same == 0 { base } else { format!("{base} ({})", same + 1) };
            self.species.push(Species {
                id,
                name,
                origin,
                parent: if origin == SpeciesOrigin::Split { parent } else { None },
                founded_day: day,
                extinct_day: None,
                merged_into: None,
                founder,
                dominant: sum.dominant,
                dominant_share: sum.dominant_share,
                adults: 0.0,
                peak_adults: 0.0,
                lineages: Vec::new(),
                carnivory: 0.0,
                body_size: 0.0,
                last_seen: day,
                missed: 0,
                reps: Vec::new(),
            });
            self.update(id, day, &sum, samples, members, p.reps);
            assigned[c] = Some(id);
            events.push(SpeciesEvent::Appeared {
                species: id,
                origin,
                parent: self.species[id as usize].parent,
                founder,
            });
        }
        // Old species found nowhere.
        for (k, &s) in live.iter().enumerate() {
            if seen[k] {
                continue;
            }
            let sp = &mut self.species[s];
            sp.missed += 1;
            sp.adults = 0.0;
            if sp.missed >= p.extinct_after.max(1) {
                sp.extinct_day = Some(sp.last_seen);
                events.push(SpeciesEvent::Extinct { species: sp.id, merged_into: None });
            }
        }
        events
    }

    fn update(&mut self, id: u32, day: u64, sum: &Summary, samples: &[Sample], members: &[usize], reps: usize) {
        let s = &mut self.species[id as usize];
        s.adults = sum.adults;
        s.peak_adults = s.peak_adults.max(sum.adults);
        s.lineages = sum.lineages.clone();
        s.dominant = sum.dominant;
        s.dominant_share = sum.dominant_share;
        s.carnivory = sum.carnivory;
        s.body_size = sum.body_size;
        s.last_seen = day;
        s.missed = 0;
        s.reps = pick_reps(samples, members, reps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::genome::Genome;
    use crate::mating::{fertility, is_kin};

    fn sample(g: &Genome, lineage: u32, owner: PlayerId) -> Sample {
        let mut expr = [0.0; Gene::COUNT];
        for &gene in Gene::ALL {
            expr[gene.index()] = g.expr(gene);
        }
        let mut origins = BTreeMap::new();
        g.tally_origins(1.0, &mut origins);
        Sample { expr, weight: 1.0, lineage, owner, origins }
    }

    fn rule() -> LinkRule {
        LinkRule::from_config(&crate::WorldConfig::default())
    }

    fn params() -> DetectParams {
        let mut cfg = crate::WorldConfig::default();
        cfg.scoring.species_extinct_after = 1;
        DetectParams::from_config(&cfg)
    }

    fn base() -> FounderDesign {
        FounderDesign::new("a", Thermo::Warm, BirthType::Live, 4).with(Gene::BodySize, 0.3).with(Gene::Carnivory, 0.1)
    }

    /// `n` founders of `design` owned by `owner`.
    fn group(design: &FounderDesign, n: usize, lineage: u32, owner: PlayerId, seed: u64) -> Vec<Sample> {
        let mut rng = crate::rng::rng_for(seed, 0, 0, 0);
        (0..n).map(|_| sample(&design.founder_genome(owner, &mut rng), lineage, owner)).collect()
    }

    #[test]
    fn link_rule_matches_mating_rules() {
        let cfg = crate::WorldConfig::default();
        let r = rule();
        let mut rng = crate::rng::rng_for(5, 0, 0, 0);
        let designs = crate::presets::wild_designs()
            .into_iter()
            .map(|(_, d)| d)
            .chain(crate::presets::player_designs())
            .collect::<Vec<_>>();
        let mut genomes = Vec::new();
        for d in &designs {
            for k in 0..6 {
                // Some drifted variants too.
                let shift = 0.03 * k as f32;
                let d2 = d.clone().with(Gene::BodySize, (d.values()[Gene::BodySize.index()] + shift).min(1.0));
                genomes.push(d2.founder_genome(1, &mut rng));
            }
        }
        let mut agree = 0;
        for a in &genomes {
            for b in &genomes {
                let want = is_kin(a, b, a.distance(b), cfg.species_distance, cfg.kin_body_plan_gap)
                    && fertility(a, b) >= r.min_fertility;
                assert_eq!(r.linked(&sample(a, 0, 1).expr, &sample(b, 0, 1).expr), want);
                agree += want as usize;
            }
        }
        assert!(agree > genomes.len(), "some pairs beyond self-pairs are linked");
    }

    #[test]
    fn two_diverged_groups_are_two_species() {
        let a = group(&base(), 30, 0, 1, 1);
        let b = group(&base().with(Gene::BodySize, 0.75), 30, 0, 1, 2);
        let samples: Vec<Sample> = a.into_iter().chain(b).collect();
        let clusters = cluster(&samples, &rule());
        assert_eq!(clusters.len(), 2, "{clusters:?}");
        assert_eq!(clusters[0], (0..30).collect::<Vec<_>>());
    }

    #[test]
    fn one_varying_population_is_one_species() {
        // A cline: body size drifting from 0.30 to 0.60 in small steps. The
        // ends are not kin, but every neighbor is, so it is one species.
        let mut samples = Vec::new();
        for k in 0..31 {
            samples.extend(group(&base().with(Gene::BodySize, 0.3 + 0.01 * k as f32), 3, 0, 1, k as u64));
        }
        let r = rule();
        assert!(!r.linked(&samples[0].expr, &samples[samples.len() - 1].expr));
        assert_eq!(cluster(&samples, &r).len(), 1);
    }

    #[test]
    fn ids_stay_stable_and_splits_and_extinctions_are_recorded() {
        let p = params();
        let name = |l: u32| format!("L{l}");
        let mut t = SpeciesTracker::default();
        // Day 0: two kinds (lineage 0 of player 1, lineage 1 wild).
        let herb = group(&base(), 20, 0, 1, 1);
        let cold = group(&FounderDesign::new("c", Thermo::Cold, BirthType::IncubatedEggs, 4), 20, 1, WILD, 2);
        let day0: Vec<Sample> = herb.iter().cloned().chain(cold.iter().cloned()).collect();
        let ev = t.detect(0, &day0, &p, name);
        assert_eq!(ev.len(), 2);
        assert!(t.species.iter().all(|s| s.origin == SpeciesOrigin::Initial && s.founder.is_none()));
        let herb_id = t.species.iter().find(|s| s.main_lineage() == Some(0)).unwrap().id;
        assert_eq!(t.species[herb_id as usize].dominant, 1);

        // Day 5: different individuals (new sample), same kinds: same ids,
        // listed in the other order.
        let day5: Vec<Sample> =
            group(&FounderDesign::new("c", Thermo::Cold, BirthType::IncubatedEggs, 4), 20, 1, WILD, 3)
                .into_iter()
                .chain(group(&base(), 25, 0, 1, 4))
                .collect();
        assert!(t.detect(5, &day5, &p, name).is_empty());
        assert_eq!(t.species.len(), 2);
        assert!((t.species[herb_id as usize].adults - 25.0).abs() < 1e-9);

        // Day 10: part of the herbivores drifted through intermediates (one
        // species still), carrying player 2's alleles.
        let mid = group(&base().with(Gene::BodySize, 0.45), 10, 0, 2, 5);
        let far = group(&base().with(Gene::BodySize, 0.6), 10, 0, 2, 6);
        let day10: Vec<Sample> =
            group(&base(), 20, 0, 1, 7).into_iter().chain(mid).chain(far.iter().cloned()).chain(cold.clone()).collect();
        assert!(t.detect(10, &day10, &p, name).is_empty());
        // Day 15: the intermediates are gone: the far group split off.
        let day15: Vec<Sample> = group(&base(), 20, 0, 1, 8).into_iter().chain(far).chain(cold.clone()).collect();
        let ev = t.detect(15, &day15, &p, name);
        assert_eq!(ev.len(), 1, "{ev:?}");
        let SpeciesEvent::Appeared { species, origin, parent, founder } = ev[0] else { panic!("{ev:?}") };
        assert_eq!((origin, parent, founder), (SpeciesOrigin::Split, Some(herb_id), Some(2)));
        assert_eq!(t.founded.get(&2), Some(&1));
        assert_eq!(t.species[species as usize].name, "L0 (2)");

        // Day 20: the cold-blooded species is gone.
        let day20: Vec<Sample> = group(&base(), 20, 0, 1, 9);
        let ev = t.detect(20, &day20, &p, name);
        let cold_id = t.species.iter().find(|s| s.main_lineage() == Some(1)).unwrap().id;
        assert!(ev.contains(&SpeciesEvent::Extinct { species: cold_id, merged_into: None }));
        assert_eq!(t.species[cold_id as usize].extinct_day, Some(15));
        assert!(t.species[herb_id as usize].is_alive());
    }

    #[test]
    fn a_new_design_is_a_founding_and_tiny_clusters_are_unclassified() {
        let p = params();
        let mut t = SpeciesTracker::default();
        t.detect(0, &group(&base(), 20, 0, 1, 1), &p, |l| format!("L{l}"));
        let frogs = FounderDesign::new("f", Thermo::Cold, BirthType::AbandonedEggs, 8);
        let samples: Vec<Sample> = group(&base(), 20, 0, 1, 2)
            .into_iter()
            .chain(group(&frogs, 10, 1, 3, 3))
            .chain(group(&base().with(Gene::BodySize, 0.9), 2, 0, 1, 4))
            .collect();
        let ev = t.detect(5, &samples, &p, |l| format!("L{l}"));
        assert_eq!(ev.len(), 1);
        assert_eq!(t.species[1].origin, SpeciesOrigin::Founding);
        assert_eq!(t.species[1].founder, None);
        assert_eq!(t.unclassified, 1);
        assert!(t.founded.is_empty());
    }
}
