//! Lineage genetics views for the dashboard (`LineageStats`): per-gene
//! distributions and allele origins.
//!
//! Allele origins need no extra state in the simulation: every allele
//! already carries the id of the player it originated with
//! (`evo_sim::genome`), so the shares are a tally over the living members'
//! genomes when the dashboard is asked for, the same tally the legacy score
//! makes over the whole world.

use std::collections::BTreeMap;

use evo_sim::genome::Genome;
use evo_sim::PlayerId;

use crate::protocol::{AlleleOrigin, GENE_BINS};

/// Counts per equal-width bin over `[0, 1]` ([`GENE_BINS`] bins). Values
/// are clamped into range, so 1.0 falls in the last bin; NaN is skipped.
pub fn histogram(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
    let mut bins = vec![0u32; GENE_BINS];
    for v in values {
        if v.is_nan() {
            continue;
        }
        let k = ((v.clamp(0.0, 1.0) * GENE_BINS as f32) as usize).min(GENE_BINS - 1);
        bins[k] += 1;
    }
    bins
}

/// Whose alleles the `genomes` carry, largest share first (ties by player
/// id). `describe` names a player and their lineage.
pub fn allele_origins<'a>(
    genomes: impl IntoIterator<Item = &'a Genome>,
    describe: impl Fn(PlayerId) -> (String, Option<String>),
) -> Vec<AlleleOrigin> {
    let mut tally = BTreeMap::new();
    for g in genomes {
        g.tally_origins(1.0, &mut tally);
    }
    let total: f64 = tally.values().sum();
    let mut rows: Vec<AlleleOrigin> = tally
        .into_iter()
        .map(|(player, n)| {
            let (name, lineage) = describe(player);
            AlleleOrigin { player, name, lineage, alleles: n as u64, share: n / total }
        })
        .collect();
    rows.sort_by(|a, b| b.alleles.cmp(&a.alleles).then(a.player.cmp(&b.player)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_sim::genome::{Gene, MutationParams, LOAD_LOCI};
    use evo_sim::rng::rng_for;

    #[test]
    fn histogram_bins_cover_the_unit_interval() {
        let bins = histogram([0.0, 0.05, 0.1, 0.55, 0.999, 1.0, 1.5, -0.2, f32::NAN]);
        assert_eq!(bins.len(), GENE_BINS);
        assert_eq!(bins[0], 3, "0, 0.05 and the clamped -0.2");
        assert_eq!(bins[1], 1);
        assert_eq!(bins[5], 1);
        assert_eq!(bins[GENE_BINS - 1], 3, "0.999, 1.0 and the clamped 1.5");
        assert_eq!(bins.iter().sum::<u32>(), 8, "NaN is skipped");
        assert_eq!(histogram(std::iter::empty()), vec![0; GENE_BINS]);
    }

    #[test]
    fn origins_follow_allele_tags_through_a_cross() {
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = rng_for(7, 0, 0, 0);
        let mine = Genome::uniform(&[0.3; Gene::COUNT], 1);
        let stud = Genome::uniform(&[0.7; Gene::COUNT], 2);
        // An F1 carries half of each parent's alleles; a backcross to my
        // line a quarter of the stud's.
        let f1 = Genome::inherit(&mine, &stud, 1, none, 0.0, &mut rng);
        let back = Genome::inherit(&f1, &mine, 1, none, 0.0, &mut rng);
        let names = |p: PlayerId| (format!("p{p}"), (p == 2).then(|| "Studs".to_string()));
        let rows = allele_origins([&mine, &f1, &back], names);
        let per = (2 * (Gene::COUNT + LOAD_LOCI)) as u64;
        assert_eq!(rows.iter().map(|r| r.alleles).sum::<u64>(), 3 * per);
        assert_eq!(rows[0].player, 1);
        assert_eq!(rows[1].player, 2);
        assert_eq!(rows[1].lineage.as_deref(), Some("Studs"));
        assert_eq!(rows[0].name, "p1");
        // The F1 holds exactly half from the stud; the backcross some.
        let from_stud = rows[1].alleles;
        assert!(from_stud > per / 2 && from_stud < per, "{from_stud}");
        assert!((rows.iter().map(|r| r.share).sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(allele_origins(std::iter::empty(), names).is_empty());
    }
}
