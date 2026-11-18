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
//! **Hysteresis** (snapshot version 42; `ScoringConfig::species_confirm`,
//! `species_confirm_adults`, `species_merge_confirm`, `species_revive_days`).
//! A population near the species threshold drifts back and forth across it,
//! and without hysteresis every crossing was a new numbered species ("L0
//! (2)" … "(7)"), announced and credited each time. Now:
//!
//! - A would-be split is a [`Candidate`] until it has been detected
//!   `species_confirm` times in a row (each time with `species_min_new`
//!   samples), and then has `species_confirm_adults` adults. Only then is it
//!   a species (dated to its first detection) and announced and credited, once.
//!   Its members are unclassified meanwhile.
//! - A merge must hold `species_merge_confirm` detections in a row. Until
//!   then the merging species stays alive with no adults of its own (they
//!   count in the species it is merging into), and recovers if it splits off
//!   again.
//! - A would-be split that matches a species which merged away within
//!   `species_revive_days` is that species again (its id, name and credit),
//!   with no announcement.
//!
//! Foundings and the first detection's species count at once. Settings of
//! 0 (worlds from before version 42) give the old tracker exactly.
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
use crate::morph::{morph_terms, MorphKey, MORPH_KEY};

/// Values in an [`Expr`]: the genes, then the morphology key.
pub const EXPR_LEN: usize = Gene::COUNT + MORPH_KEY;

/// Expressed gene values of one sampled organism, then its morphology key
/// (`Genome::morph().key()`, see [`crate::morph::morph_terms`]).
pub type Expr = [f32; EXPR_LEN];

/// The morphology key of the default morphology.
pub fn default_morph_key() -> MorphKey {
    crate::morph::MorphGenes::uniform(&crate::morph::MorphDesign::default(), WILD).key()
}

/// The expressed genes and morphology key of a genome.
pub fn expr_of(g: &crate::genome::Genome) -> Expr {
    let mut expr = [0.0; EXPR_LEN];
    for &gene in Gene::ALL {
        expr[gene.index()] = g.expr(gene);
    }
    expr[Gene::COUNT..].copy_from_slice(&g.morph().key());
    expr
}

fn morph_key(e: &Expr) -> &MorphKey {
    e[Gene::COUNT..].try_into().unwrap()
}

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
    /// Mirrors `mating::is_kin` and `mating::fertility` on expressed values
    /// (the morphology's body-plan gap and distance term included).
    pub fn linked(&self, a: &Expr, b: &Expr) -> bool {
        let (morph, morph_gap) = morph_terms(morph_key(a), morph_key(b));
        let gap =
            BODY_PLAN_GENES.iter().map(|g| (a[g.index()] - b[g.index()]).abs()).fold(0.0, f32::max).max(morph_gap);
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
        sum += morph;
        if sum >= limit {
            return false;
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

/// Slots of [`Packed`]: the distance genes, padded to whole groups of four.
const PACKED: usize = (DISTANCE_GENES as usize).div_ceil(4) * 4;

/// A sample's distance genes, packed in gene order (then zeros), for a
/// quick sum of differences that vectorizes.
type Packed = [f32; PACKED];

fn pack(e: &Expr) -> Packed {
    let mut out = [0.0; PACKED];
    let mut n = 0;
    for (k, &v) in e[..Gene::COUNT].iter().enumerate() {
        if IN_DISTANCE[k] {
            out[n] = v;
            n += 1;
        }
    }
    out
}

impl LinkRule {
    /// [`Self::linked`], deciding from bounds on the distance where it can.
    /// `linked` sums the differences in one serial chain; four running sums
    /// over the packed genes add the same non-negative terms in another
    /// order, which moves the sum by a few millionths of itself at most
    /// (`SLACK` is far wider). Past the body-plan test, `linked` is true
    /// below some distance and false above it (the early stop, kinship and
    /// fertility all fall with distance), so when both bounds give the same
    /// answer it is `linked`'s; otherwise (rarely) `linked` decides.
    fn linked_packed(&self, a: &Expr, b: &Expr, pa: &Packed, pb: &Packed) -> bool {
        const SLACK: f32 = 1e-4;
        let (morph, morph_gap) = morph_terms(morph_key(a), morph_key(b));
        let gap =
            BODY_PLAN_GENES.iter().map(|g| (a[g.index()] - b[g.index()]).abs()).fold(0.0, f32::max).max(morph_gap);
        if gap >= self.kin_body_plan_gap || ramp(gap, BODY_PLAN_FULL, BODY_PLAN_STERILE) < self.min_fertility {
            return false;
        }
        let mut lanes = [0.0f32; 4];
        for (a, b) in pa.chunks_exact(4).zip(pb.chunks_exact(4)) {
            for (l, lane) in lanes.iter_mut().enumerate() {
                *lane += (a[l] - b[l]).abs();
            }
        }
        let sum = (lanes[0] + lanes[1]) + (lanes[2] + lanes[3]) + morph;
        let limit = self.species_distance * DISTANCE_GENES;
        let pass = |sum: f32| {
            if sum >= limit {
                return false;
            }
            let distance = sum / DISTANCE_GENES;
            let rest = 1.0 - distance / self.species_distance;
            rest > 0.0 && gap < rest * self.kin_body_plan_gap && distance_fertility(distance) >= self.min_fertility
        };
        if pass(sum * (1.0 + SLACK)) {
            true
        } else if !pass(sum * (1.0 - SLACK)) {
            false
        } else {
            self.linked(a, b)
        }
    }
}

/// Single-linkage clusters of `samples` under `rule`: each cluster is a list
/// of sample indices in ascending order; clusters are ordered by their first
/// index. These are the connected components of the link graph, so they do
/// not depend on which pairs are tested in what order, only on every linked
/// pair ending up in one component:
///
/// - Two samples can only be linked if every body-plan gene differs by less
///   than `kin_body_plan_gap`, so samples are bucketed by body plan (cells
///   a little wider than the gap in each of the four genes) and each is
///   tested only against samples in its own and the adjacent cells.
/// - Pairs already in one component are not tested. Each sample carries its
///   component's label (its smallest index), and a merge relabels the
///   members of the one with the larger label.
pub fn cluster(samples: &[Sample], rule: &LinkRule) -> Vec<Vec<usize>> {
    let n = samples.len();
    let limit = rule.kin_body_plan_gap;
    if limit.is_nan() || limit <= 0.0 {
        // `linked` needs a body-plan gap below the limit: nothing links.
        return (0..n).map(|i| vec![i]).collect();
    }
    let packed: Vec<Packed> = samples.iter().map(|s| pack(&s.expr)).collect();
    // Cells 0.1% wider than the gap limit: a pair within the limit in a gene
    // lies in the same or adjacent cells of it, rounding included.
    let width = limit * 1.001;
    let cell_of = |s: &Sample| BODY_PLAN_GENES.map(|g| (s.expr[g.index()] / width).floor() as i32);
    let mut cells: BTreeMap<[i32; 4], Vec<u32>> = BTreeMap::new();
    for (i, s) in samples.iter().enumerate() {
        cells.entry(cell_of(s)).or_default().push(i as u32);
    }
    let cells: Vec<([i32; 4], Vec<u32>)> = cells.into_iter().collect();
    // Each cell's neighborhood: itself and the adjacent occupied cells.
    let near: Vec<Vec<usize>> = cells
        .iter()
        .map(|(c, _)| (0..cells.len()).filter(|&d| cells[d].0.iter().zip(c).all(|(a, b)| (a - b).abs() <= 1)).collect())
        .collect();
    // Each sample's cell and place in it.
    let mut at = vec![(0usize, 0usize); n];
    for (k, (_, list)) in cells.iter().enumerate() {
        for (p, &i) in list.iter().enumerate() {
            at[i as usize] = (k, p);
        }
    }
    // Labels, by cell in the cells' order, so the scan past a component's
    // members reads them contiguously.
    let mut labels: Vec<Vec<u32>> = cells.iter().map(|(_, list)| list.clone()).collect();
    let mut members: Vec<Vec<u32>> = (0..n as u32).map(|i| vec![i]).collect();
    for i in 0..n {
        for &d in &near[at[i].0] {
            // Samples after `i` in the cell (each cell's list is ascending).
            let list = &cells[d].1;
            let mut p = list.partition_point(|&j| j as usize <= i);
            while p < list.len() {
                let li = labels[at[i].0][at[i].1];
                // Skip samples already in i's component, eight at a time (a
                // single species is often most of a cell).
                while p + 8 <= list.len() && labels[d][p..p + 8].iter().all(|&l| l == li) {
                    p += 8;
                }
                if p == list.len() {
                    break;
                }
                let (j, lj) = (list[p] as usize, labels[d][p]);
                if li != lj && rule.linked_packed(&samples[i].expr, &samples[j].expr, &packed[i], &packed[j]) {
                    let (keep, gone) = (li.min(lj), li.max(lj));
                    let moved = std::mem::take(&mut members[gone as usize]);
                    for &m in &moved {
                        let (c, q) = at[m as usize];
                        labels[c][q] = keep;
                    }
                    members[keep as usize].extend(moved);
                }
                p += 1;
            }
        }
    }
    members
        .into_iter()
        .filter(|m| !m.is_empty())
        .map(|m| {
            let mut m: Vec<usize> = m.into_iter().map(|k| k as usize).collect();
            m.sort_unstable();
            m
        })
        .collect()
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
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v14::reps"))]
    reps: Vec<Expr>,
    /// Detections in a row it has continued as the same cluster as a larger
    /// species (a merge in waiting, `ScoringConfig::species_merge_confirm`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub merging: u32,
}

/// A cluster that would be a new species (a split) if it holds: detected
/// `hits` times in a row so far (`ScoringConfig::species_confirm`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Candidate {
    pub first_day: u64,
    pub hits: u32,
    /// The species it splits from, at its first detection.
    pub parent: Option<u32>,
    /// Estimated adults at the last detection.
    pub adults: f64,
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v14::reps"))]
    reps: Vec<Expr>,
}

impl Species {
    pub fn is_alive(&self) -> bool {
        self.extinct_day.is_none()
    }

    /// Representative members' expressed values (genes, then the
    /// morphology key) from its last detection: what it looked like.
    pub fn reps(&self) -> &[Expr] {
        &self.reps
    }

    /// No adults found: the estimate and every lineage's count drop to 0
    /// (the lineages stay listed, in order, as what it was), so a lineage
    /// isn't counted here and in the species it is now part of, and an
    /// extinct species shows none.
    fn clear_counts(&mut self) {
        self.adults = 0.0;
        for l in &mut self.lineages {
            l.1 = 0.0;
        }
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
    /// Splits waiting to be confirmed (`ScoringConfig::species_confirm`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub candidates: Vec<Candidate>,
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

/// Whether a cluster's adults are mostly one player lineage's (not the
/// wild ecosystem's): it then keeps a species of its own
/// (`DetectParams::player_species`).
fn led_by_player(samples: &[Sample], members: &[usize]) -> bool {
    let mut by: BTreeMap<u32, (f64, PlayerId)> = BTreeMap::new();
    let mut total = 0.0;
    for &m in members {
        let s = &samples[m];
        let e = by.entry(s.lineage).or_insert((0.0, s.owner));
        e.0 += s.weight;
        total += s.weight;
    }
    by.values().any(|&(n, owner)| owner != WILD && n * 2.0 > total)
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
    /// Hysteresis (`ScoringConfig::species_confirm` and the rest; 0 or 1:
    /// off).
    pub confirm: u32,
    pub confirm_adults: f64,
    pub merge_confirm: u32,
    pub revive_days: u64,
    /// Player species identity (`Balance::player_species`): a cluster most
    /// of whose adults are one player lineage's is split from its
    /// wild-owned members and never continues an initial wild species.
    pub player_species: bool,
}

impl DetectParams {
    pub fn from_config(cfg: &crate::WorldConfig) -> Self {
        DetectParams {
            rule: LinkRule::from_config(cfg),
            min_new: cfg.scoring.species_min_new.max(1),
            extinct_after: cfg.scoring.species_extinct_after,
            reps: cfg.scoring.species_reps.max(1),
            confirm: cfg.scoring.species_confirm,
            confirm_adults: cfg.scoring.species_confirm_adults as f64,
            merge_confirm: cfg.scoring.species_merge_confirm,
            revive_days: cfg.scoring.species_revive_days as u64,
            player_species: cfg.balance.player_species > 0.0,
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
        let mut clusters = cluster(samples, &p.rule);
        let first = self.last_detection.is_none();
        // Player species identity: a cluster led by a player lineage keeps
        // its own identity apart from the wild animals it interbreeds with
        // (playtest 4: every starter lineage was filed under the Steppe
        // antelope). Its wild-owned members form their own cluster.
        let mut player_led = vec![false; clusters.len()];
        if p.player_species && !first {
            let mut extra = Vec::new();
            for (c, members) in clusters.iter_mut().enumerate() {
                if !led_by_player(samples, members) {
                    continue;
                }
                player_led[c] = true;
                let wild: Vec<usize> = members.iter().copied().filter(|&m| samples[m].owner == WILD).collect();
                if !wild.is_empty() {
                    members.retain(|&m| samples[m].owner != WILD);
                    extra.push(wild);
                }
            }
            player_led.extend(extra.iter().map(|_| false));
            clusters.extend(extra);
        }
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
        // Each old species continues as the cluster it overlaps most; an
        // initial wild species never as a player-led cluster.
        let successor: Vec<Option<usize>> = (0..live.len())
            .map(|k| {
                let sp = &self.species[live[k]];
                let wild_initial = sp.origin == SpeciesOrigin::Initial && sp.founder.is_none() && sp.dominant == WILD;
                let (c, n) = (0..clusters.len()).fold((0, 0), |b, c| {
                    if overlap[c][k] > b.1 && !(wild_initial && player_led[c]) {
                        (c, overlap[c][k])
                    } else {
                        b
                    }
                });
                (n > 0).then_some(c)
            })
            .collect();

        let mut events = Vec::new();
        // Candidates matched this detection, those confirmed, and merged
        // species brought back.
        let mut hit = vec![false; self.candidates.len()];
        let mut promoted: Vec<usize> = Vec::new();
        let mut revived: Vec<usize> = Vec::new();
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
                    let gone = live[k] as u32;
                    let s = &mut self.species[live[k]];
                    // A merge must hold: until then it stays alive, its
                    // members counted in the species it is merging into.
                    s.merging += 1;
                    if s.merging < p.merge_confirm {
                        s.clear_counts();
                        s.missed = 0;
                        continue;
                    }
                    s.extinct_day = Some(day);
                    s.merged_into = Some(id);
                    // Its members are counted in the species it merged into
                    // now, not here as well.
                    s.clear_counts();
                    events.push(SpeciesEvent::Extinct { species: s.id, merged_into: Some(id) });
                    // Species that had merged into it now point at where it
                    // went: "merged into" always names a living successor or
                    // the last one, never reads round in a circle.
                    for o in self.species.iter_mut().filter(|o| o.merged_into == Some(gone)) {
                        o.merged_into = Some(id);
                    }
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
            let mut origin = match (first, parent) {
                (true, _) => SpeciesOrigin::Initial,
                (false, Some(_)) => SpeciesOrigin::Split,
                (false, None) => SpeciesOrigin::Founding,
            };
            let mut founded_day = day;
            let mut parent = parent;
            if origin == SpeciesOrigin::Split && p.confirm >= 2 {
                // A species that merged away lately, detected again: the same
                // species (no announcement, no credit), not a new one.
                if p.revive_days > 0 {
                    let revive = (0..self.species.len())
                        .filter(|&s| {
                            let o = &self.species[s];
                            o.merged_into.is_some()
                                && o.extinct_day.is_some_and(|d| d + p.revive_days >= day)
                                && !revived.contains(&s)
                        })
                        .map(|s| {
                            let reps = &self.species[s].reps;
                            let n = members
                                .iter()
                                .filter(|&&m| reps.iter().any(|r| p.rule.linked(&samples[m].expr, r)))
                                .count();
                            (s, n)
                        })
                        .filter(|&(_, n)| n > 0)
                        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
                    if let Some((s, _)) = revive {
                        revived.push(s);
                        let sp = &mut self.species[s];
                        sp.extinct_day = None;
                        sp.merged_into = None;
                        sp.merging = 0;
                        let id = s as u32;
                        self.update(id, day, &sum, samples, members, p.reps);
                        assigned[c] = Some(id);
                        continue;
                    }
                }
                // Otherwise a candidate until it has held `confirm` detections.
                let matched = (0..self.candidates.len())
                    .filter(|&k| !hit[k])
                    .map(|k| {
                        let reps = &self.candidates[k].reps;
                        let n = members
                            .iter()
                            .filter(|&&m| reps.iter().any(|r| p.rule.linked(&samples[m].expr, r)))
                            .count();
                        (k, n)
                    })
                    .filter(|&(_, n)| n > 0)
                    .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
                let k = match matched {
                    Some((k, _)) => {
                        let cand = &mut self.candidates[k];
                        cand.hits += 1;
                        cand.adults = sum.adults;
                        cand.reps = pick_reps(samples, members, p.reps);
                        k
                    }
                    None => {
                        self.candidates.push(Candidate {
                            first_day: day,
                            hits: 1,
                            parent,
                            adults: sum.adults,
                            reps: pick_reps(samples, members, p.reps),
                        });
                        hit.push(false);
                        self.candidates.len() - 1
                    }
                };
                hit[k] = true;
                let cand = &self.candidates[k];
                if cand.hits < p.confirm || cand.adults < p.confirm_adults {
                    self.unclassified += 1;
                    continue;
                }
                // Confirmed: a species since its first detection, split from
                // its first parent if that one still lives.
                founded_day = cand.first_day;
                if let Some(first_parent) = cand.parent {
                    if self.species.get(first_parent as usize).is_some_and(|s| s.is_alive()) {
                        parent = Some(first_parent);
                    }
                }
                promoted.push(k);
                origin = SpeciesOrigin::Split;
            }
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
                founded_day,
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
                merging: 0,
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
        // Candidates must hold in a row: drop those not seen this time, and
        // the confirmed ones.
        let mut k = 0;
        self.candidates.retain(|_| {
            let keep = hit[k] && !promoted.contains(&k);
            k += 1;
            keep
        });
        // Old species found nowhere.
        for (k, &s) in live.iter().enumerate() {
            if seen[k] {
                continue;
            }
            let sp = &mut self.species[s];
            sp.missed += 1;
            sp.clear_counts();
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
        s.merging = 0;
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
        let expr = expr_of(g);
        let mut origins = BTreeMap::new();
        g.tally_origins(1.0, &mut origins);
        Sample { expr, weight: 1.0, lineage, owner, origins }
    }

    fn rule() -> LinkRule {
        LinkRule::from_config(&crate::WorldConfig::default())
    }

    /// The tracker without hysteresis (as before snapshot version 42).
    fn params() -> DetectParams {
        let mut cfg = crate::WorldConfig::default();
        cfg.scoring.species_extinct_after = 1;
        cfg.scoring.species_confirm = 0;
        cfg.scoring.species_merge_confirm = 0;
        cfg.scoring.species_revive_days = 0;
        DetectParams::from_config(&cfg)
    }

    /// With hysteresis: splits hold 3 detections, merges 3, re-use within 100 days.
    fn hysteresis() -> DetectParams {
        let mut cfg = crate::WorldConfig::default();
        cfg.scoring.species_extinct_after = 1;
        cfg.scoring.species_confirm = 3;
        cfg.scoring.species_confirm_adults = 5.0;
        cfg.scoring.species_merge_confirm = 3;
        cfg.scoring.species_revive_days = 100;
        DetectParams::from_config(&cfg)
    }

    /// Herbivores, and a far group joined to them (`bridged`) by
    /// intermediates or not: one species or two. Fresh individuals each day.
    fn flapping_day(day: u64, bridged: bool) -> Vec<Sample> {
        let mut v = group(&base(), 20, 0, 1, 100 + day);
        if bridged {
            v.extend(group(&base().with(Gene::BodySize, 0.42), 10, 0, 2, 200 + day));
            v.extend(group(&base().with(Gene::BodySize, 0.54), 10, 0, 2, 400 + day));
        }
        v.extend(group(&base().with(Gene::BodySize, 0.66), 10, 0, 2, 300 + day));
        v
    }

    fn appeared(ev: &[SpeciesEvent]) -> Vec<u32> {
        ev.iter()
            .filter_map(|e| if let SpeciesEvent::Appeared { species, .. } = e { Some(*species) } else { None })
            .collect()
    }

    #[test]
    fn flapping_days_are_one_or_two_clusters() {
        for day in (0..200).step_by(5) {
            let n = |b| cluster(&flapping_day(day, b), &rule()).iter().filter(|c| c.len() >= 4).count();
            assert_eq!((n(true), n(false)), (1, 2), "day {day}");
        }
    }

    #[test]
    fn flapping_without_hysteresis_makes_numbered_copies() {
        let p = params();
        let mut t = SpeciesTracker::default();
        t.detect(0, &flapping_day(0, true), &p, |l| format!("L{l}"));
        let mut new = 0;
        for k in 1..=8u64 {
            new += appeared(&t.detect(5 * k, &flapping_day(5 * k, k % 2 == 0), &p, |l| format!("L{l}"))).len();
        }
        assert_eq!(new, 4, "a new species every other detection");
        assert!(t.species.iter().any(|s| s.name == "L0 (5)"));
    }

    #[test]
    fn hysteresis_counts_a_split_once_and_reuses_a_merged_identity() {
        let p = hysteresis();
        let name = |l: u32| format!("L{l}");
        let mut t = SpeciesTracker::default();
        t.detect(0, &flapping_day(0, true), &p, name);
        let mut day = 0;
        let mut step = |t: &mut SpeciesTracker, bridged: bool| {
            day += 5;
            t.detect(day, &flapping_day(day, bridged), &p, name)
        };
        // Flapping: a split never holds 3 detections, so nothing counts.
        for k in 0..8 {
            assert!(appeared(&step(&mut t, k % 2 == 1)).is_empty());
            assert!(t.founded.is_empty());
        }
        assert_eq!(t.species.len(), 1);
        // A split that holds: one species, dated to its first detection,
        // credited once.
        assert!(step(&mut t, false).is_empty()); // day 45
        assert!(step(&mut t, false).is_empty());
        let ev = step(&mut t, false);
        let new = appeared(&ev);
        assert_eq!(new.len(), 1, "{ev:?}");
        let id = new[0];
        assert_eq!(t.species[id as usize].founded_day, 45);
        assert_eq!(t.species[id as usize].parent, Some(0));
        assert_eq!(t.founded.get(&2), Some(&1));
        // Bridged again briefly: not a merge yet, no events, still alive
        // (with its adults counted in the species it is merging into).
        assert!(step(&mut t, true).is_empty());
        assert!(step(&mut t, true).is_empty());
        assert!(t.species[id as usize].is_alive());
        assert_eq!(t.species[id as usize].adults, 0.0);
        assert!(step(&mut t, false).is_empty());
        assert!(t.species[id as usize].adults > 0.0);
        // A merge that holds.
        step(&mut t, true);
        step(&mut t, true);
        let ev = step(&mut t, true);
        assert_eq!(ev, vec![SpeciesEvent::Extinct { species: id, merged_into: Some(0) }]);
        // Split off again: the same species, revived silently.
        let ev = step(&mut t, false);
        assert!(ev.is_empty(), "{ev:?}");
        assert!(t.species[id as usize].is_alive());
        assert_eq!(t.species.len(), 2);
        assert_eq!(t.founded.get(&2), Some(&1));
        assert!(t.candidates.is_empty());
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

    /// `cluster` as it was: every pair, union-find, the plain link test.
    fn cluster_pairwise(samples: &[Sample], rule: &LinkRule) -> Vec<Vec<usize>> {
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
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

    /// The bucketed clustering and the bounded link test give exactly what
    /// testing every pair with `linked` does, over every preset body plan
    /// with drifted variants (clines that link across body-plan cells
    /// included), under the default rule and tighter and looser ones.
    #[test]
    fn fast_clustering_matches_every_pair() {
        let mut rng = crate::rng::rng_for(9, 0, 0, 0);
        let designs: Vec<FounderDesign> = crate::presets::wild_designs()
            .into_iter()
            .map(|(_, d)| d)
            .chain(crate::presets::player_designs())
            .collect();
        let mut samples = Vec::new();
        for (l, d) in designs.iter().enumerate() {
            for k in 0..12 {
                let size = (d.values()[Gene::BodySize.index()] + 0.02 * k as f32).min(1.0);
                let carn = (d.values()[Gene::Carnivory.index()] + 0.015 * k as f32).min(1.0);
                let d2 = d.clone().with(Gene::BodySize, size).with(Gene::Carnivory, carn);
                for _ in 0..3 {
                    samples.push(sample(&d2.founder_genome(1, &mut rng), l as u32, 1));
                }
            }
        }
        // Interleave the lineages so components are found out of order.
        let n = samples.len();
        let order: Vec<usize> = (0..n).map(|k| (k * 37) % n).collect();
        assert_eq!(order.iter().collect::<std::collections::BTreeSet<_>>().len(), n);
        let samples: Vec<Sample> = order.iter().map(|&k| samples[k].clone()).collect();
        for (sd, gap, fert) in [(0.12, 0.25, 0.5), (0.05, 0.1, 0.9), (0.3, 0.6, 0.1), (0.12, 0.0, 0.5)] {
            let r = LinkRule { species_distance: sd, kin_body_plan_gap: gap, min_fertility: fert };
            let packed: Vec<Packed> = samples.iter().map(|s| pack(&s.expr)).collect();
            for (a, pa) in samples.iter().zip(&packed) {
                for (b, pb) in samples.iter().zip(&packed) {
                    assert_eq!(r.linked_packed(&a.expr, &b.expr, pa, pb), r.linked(&a.expr, &b.expr));
                }
            }
            let want = cluster_pairwise(&samples, &r);
            assert_eq!(cluster(&samples, &r), want, "rule {r:?}");
            if gap > 0.0 {
                assert!(want.len() > 1 && want.len() < n, "rule {r:?}: {} clusters of {n}", want.len());
            }
        }
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

    /// A species that merges into another counts no adults any more (its
    /// lineages aren't counted in two species at once), and "merged into"
    /// follows a chain to the last successor (A into C, then C into E:
    /// A reads "merged into E", never round in a circle).
    #[test]
    fn merged_species_count_nothing_and_merge_chains_point_at_the_survivor() {
        let p = params();
        let name = |l: u32| format!("L{l}");
        let mut t = SpeciesTracker::default();
        let kind = |size: f32| base().with(Gene::BodySize, size);
        // Lineage 0 at body size 0.1, 1 at 0.5, 2 at 0.9; `bridge` adds
        // intermediates (of the lineage given) in steps of 0.1.
        let day = |seed: u64, bridges: &[(u32, PlayerId, f32)]| {
            let mut v: Vec<Sample> = (group(&kind(0.1), 10, 0, 1, seed).into_iter())
                .chain(group(&kind(0.5), 20, 1, 2, seed + 1))
                .chain(group(&kind(0.9), 160, 2, 3, seed + 2))
                .collect();
            for (k, &(l, owner, size)) in bridges.iter().enumerate() {
                v.extend(group(&kind(size), 6, l, owner, seed + 3 + k as u64));
            }
            v
        };
        t.detect(0, &day(1, &[]), &p, name);
        assert_eq!(t.species.len(), 3, "three kinds");
        let id_of = |t: &SpeciesTracker, l: u32| t.species.iter().find(|s| s.main_lineage() == Some(l)).unwrap().id;
        let (a, c, e) = (id_of(&t, 0), id_of(&t, 1), id_of(&t, 2));
        // Day 5: intermediates join A and C into one; A, the smaller, merges.
        let ac = [(0, 1, 0.2), (0, 1, 0.3), (0, 1, 0.4)];
        t.detect(5, &day(10, &ac), &p, name);
        let sa = &t.species[a as usize];
        assert_eq!(
            sa.merged_into,
            Some(c),
            "{:?}",
            t.species.iter().map(|s| (s.id, s.merged_into)).collect::<Vec<_>>()
        );
        assert_eq!(sa.adults, 0.0);
        assert!(sa.lineages.iter().all(|l| l.1 == 0.0), "a merged species counts no lineage: {:?}", sa.lineages);
        // Day 10: C (with A's members) joins E, the larger: both A and C
        // read "merged into E".
        let all = [(0, 1, 0.2), (0, 1, 0.3), (0, 1, 0.4), (1, 2, 0.6), (1, 2, 0.7), (1, 2, 0.8)];
        t.detect(10, &day(20, &all), &p, name);
        let state: Vec<_> = t.species.iter().map(|s| (s.id, s.merged_into, s.adults)).collect();
        assert_eq!(t.species[c as usize].merged_into, Some(e), "{state:?}");
        assert_eq!(t.species[a as usize].merged_into, Some(e), "the chain points at the survivor");
        // No lineage is counted in two living species.
        let mut counted: BTreeMap<u32, u32> = BTreeMap::new();
        for sp in &t.species {
            for &(l, n) in &sp.lineages {
                if n > 0.0 {
                    *counted.entry(l).or_default() += 1;
                }
            }
        }
        assert!(counted.values().all(|&n| n == 1), "{counted:?}");
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
