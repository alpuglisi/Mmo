//! Genetic-legacy score ("Scoring: genetic legacy" in the design doc).
//!
//! `score_p = alleles tagged p in breeding adults / all alleles in breeding
//! adults`. Genes count wherever they live, including in rivals' organisms.
//! The wild ecosystem's alleles ([`crate::WILD`]) count in the denominator and
//! are reported as their own share.

use std::collections::BTreeMap;

use crate::genome::{Genome, PlayerId};

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
}

impl LegacySnapshot {
    /// `adults` yields each breeding adult's genome and offspring raised.
    pub fn compute<'a>(day: u64, adults: impl Iterator<Item = (&'a Genome, u32)>) -> Self {
        let mut s = LegacySnapshot { day, ..Default::default() };
        for (genome, raised) in adults {
            s.breeding_adults += 1;
            genome.tally_origins(1.0, &mut s.alleles);
            if raised > 0 {
                genome.tally_origins(raised as f64, &mut s.weighted);
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
        let s = LegacySnapshot::compute(0, [(&pure, 0), (&hybrid, 2)].into_iter());
        assert!((s.share(1) - 0.75).abs() < 1e-9);
        assert!((s.share(2) - 0.25).abs() < 1e-9);
        // Weighted: only the hybrid raised offspring.
        assert!((s.weighted_share(1) - 0.5).abs() < 1e-9);
    }
}
