//! Genetic-legacy score ("Scoring: genetic legacy" in the design doc).
//!
//! `score_p = alleles tagged p in breeding adults / all alleles in breeding
//! adults`. Genes count wherever they live, including in rivals' organisms.
//! The wild ecosystem's alleles ([`crate::WILD`]) count in the denominator and
//! are reported as their own share.

use std::collections::BTreeMap;

use rayon::prelude::*;

use crate::genome::{Genome, PlayerId};

/// Daily legacy snapshots a world keeps (`World::legacy`). Only the latest
/// is read (scores, the daily record, the chronicle); the store keeps the
/// history (`history.jsonl`). Unbounded, the list grew by a map of every
/// player's share each simulated day: in memory and in every save, about
/// 1,800 days a real day at the 1 s clock.
pub const LEGACY_KEPT: usize = 8;

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LegacySnapshot {
    pub day: u64,
    pub breeding_adults: usize,
    pub total_alleles: f64,
    pub alleles: BTreeMap<PlayerId, f64>,
    /// Optional fitness weighting: each adult's alleles × offspring raised to
    /// adulthood, so junk genes earn nothing.
    pub weighted_total: f64,
    pub weighted: BTreeMap<PlayerId, f64>,
    /// The running mean of the world's total of alleles
    /// (`Balance::share_memory`, `World::smooth_legacy`): `total_alleles`
    /// is at least this, so shares count against the world as big as it
    /// has been lately. 0 with the rule off, and in snapshots before
    /// version 92.
    #[cfg_attr(feature = "serde", serde(default, with = "crate::serde_util::before_v92::wide"))]
    pub world_alleles: f64,
}

impl LegacySnapshot {
    /// `adults` yields each breeding adult's genome and offspring raised;
    /// with `morph` the morphology segment's alleles count too
    /// (`Balance::morph_legacy`).
    pub fn compute<'a>(day: u64, adults: impl Iterator<Item = (&'a Genome, u32)>, morph: bool) -> Self {
        let mut s = LegacySnapshot { day, ..Default::default() };
        // Whole numbers throughout (allele counts times offspring raised,
        // far below 2^53), so adding each genome's counts at once sums to
        // exactly what adding allele by allele (`Genome::tally_origins`) did.
        let mut counts = Vec::new();
        for (genome, raised) in adults {
            s.breeding_adults += 1;
            genome.count_origins_with(&mut counts, morph);
            for &(p, n) in &counts {
                *s.alleles.entry(p).or_default() += n as f64;
                if raised > 0 {
                    *s.weighted.entry(p).or_default() += n as f64 * raised as f64;
                }
            }
        }
        s.total_alleles = s.alleles.values().sum();
        s.weighted_total = s.weighted.values().sum();
        s
    }

    /// [`Self::compute`] on all cores, with the same result bit for bit:
    /// every tally is a whole number (allele counts, times offspring raised
    /// for the weighted one), counted exactly in integers and converted
    /// once, so the order the adults are counted in doesn't matter.
    pub fn compute_par<'a>(day: u64, adults: impl ParallelIterator<Item = (&'a Genome, u32)>, morph: bool) -> Self {
        // Per origin: alleles, and alleles times offspring raised.
        type Tally = BTreeMap<PlayerId, (u64, u64)>;
        let (breeding_adults, tally) = adults
            .fold(
                || (0, Tally::new(), Vec::new()),
                |(mut n, mut tally, mut counts): (usize, Tally, Vec<(PlayerId, u32)>), (genome, raised)| {
                    n += 1;
                    genome.count_origins_with(&mut counts, morph);
                    for &(p, k) in &counts {
                        let t = tally.entry(p).or_insert((0, 0));
                        t.0 += k as u64;
                        t.1 += k as u64 * raised as u64;
                    }
                    (n, tally, counts)
                },
            )
            .map(|(n, tally, _)| (n, tally))
            .reduce(
                || (0, Tally::new()),
                |(n, mut a): (usize, Tally), (m, b): (usize, Tally)| {
                    for (p, (k, w)) in b {
                        let t = a.entry(p).or_insert((0, 0));
                        t.0 += k;
                        t.1 += w;
                    }
                    (n + m, a)
                },
            );
        let mut s = LegacySnapshot { day, breeding_adults, ..Default::default() };
        for (p, (k, w)) in tally {
            s.alleles.insert(p, k as f64);
            // `compute` only lists an origin among the weighted once an
            // adult carrying it has raised offspring.
            if w > 0 {
                s.weighted.insert(p, w as f64);
            }
        }
        s.total_alleles = s.alleles.values().sum();
        s.weighted_total = s.weighted.values().sum();
        s
    }

    pub fn share(&self, player: PlayerId) -> f64 {
        if self.total_alleles == 0.0 {
            return 0.0;
        }
        self.alleles.get(&player).copied().unwrap_or(0.0) / self.total_alleles
    }

    pub fn weighted_share(&self, player: PlayerId) -> f64 {
        if self.weighted_total == 0.0 {
            return 0.0;
        }
        self.weighted.get(&player).copied().unwrap_or(0.0) / self.weighted_total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Gene;

    #[test]
    fn shares_match_allele_counts_across_owners() {
        // One adult entirely of player 1, one hybrid of players 1 and 2:
        // player 1 holds 3/4 of the alleles wherever they live.
        let pure = Genome::uniform(&[0.5; Gene::COUNT], 1);
        let other = Genome::uniform(&[0.5; Gene::COUNT], 2);
        let none = crate::genome::MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = crate::rng::rng_for(0, 0, 0, 0);
        let hybrid = Genome::inherit(&pure, &other, 2, none, 0.0, &mut rng);
        let s = LegacySnapshot::compute(0, [(&pure, 0), (&hybrid, 2)].into_iter(), false);
        assert!((s.share(1) - 0.75).abs() < 1e-9);
        assert!((s.share(2) - 0.25).abs() < 1e-9);
        // Weighted: only the hybrid raised offspring.
        assert!((s.weighted_share(1) - 0.5).abs() < 1e-9);
    }

    /// The parallel tally is the sequential one, bit for bit, over a world's
    /// adults (many origins, hybrids, harmful recessives, offspring raised).
    #[test]
    fn parallel_tally_matches_allele_by_allele() {
        let mut w = crate::presets::standard_world(crate::WorldConfig { seed: 4, ..Default::default() }, 1);
        w.run_days(12);
        let adults: Vec<(&Genome, u32)> =
            w.organisms.iter().filter(|o| o.alive).enumerate().map(|(k, o)| (&*o.genome, (k % 4) as u32)).collect();
        assert!(adults.len() > 100);
        for morph in [false, true] {
            let mut want = LegacySnapshot { day: 7, breeding_adults: adults.len(), ..Default::default() };
            for &(g, raised) in &adults {
                g.tally_origins_with(1.0, &mut want.alleles, morph);
                if raised > 0 {
                    g.tally_origins_with(raised as f64, &mut want.weighted, morph);
                }
            }
            want.total_alleles = want.alleles.values().sum();
            want.weighted_total = want.weighted.values().sum();
            assert!(want.alleles.len() > 3);
            assert_eq!(LegacySnapshot::compute(7, adults.iter().copied(), morph), want);
            assert_eq!(LegacySnapshot::compute_par(7, adults.par_iter().copied(), morph), want);
        }
    }

    /// With `morph` each genome counts its 14 morphology alleles too, under
    /// their own tags: a designed founder's all go to its designer, so a pure
    /// lineage's share is unchanged, and a morphology mutation is counted for
    /// the owner it is tagged with.
    #[test]
    fn morphology_alleles_count_when_asked() {
        let pure = Genome::uniform(&[0.5; Gene::COUNT], 1);
        let mut mutant = Genome::uniform(&[0.5; Gene::COUNT], 1);
        let mut m = *mutant.morph();
        m.discrete[0].a.origin = 2;
        mutant.set_morph(m);
        let off = LegacySnapshot::compute(0, [(&pure, 0), (&mutant, 0)].into_iter(), false);
        let on = LegacySnapshot::compute(0, [(&pure, 0), (&mutant, 0)].into_iter(), true);
        assert_eq!(off.share(2), 0.0);
        let per_genome = off.total_alleles / 2.0;
        assert_eq!(on.total_alleles, 2.0 * (per_genome + crate::morph::MORPH_ALLELES as f64));
        assert_eq!(on.alleles[&2], 1.0);
        let solo = LegacySnapshot::compute(0, [(&pure, 0)].into_iter(), true);
        assert_eq!(solo.share(1), 1.0);
    }
}
