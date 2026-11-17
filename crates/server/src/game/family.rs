//! The family tree (`GET /api/lineage/tree`): what a player's lineage looked
//! like on each recorded day and the species its members split into, for the
//! client's family tree view (`client/src/familyTree.ts`).
//!
//! - **Timeline.** Every daily record that kept the lineage's gene means
//!   (the history's retention policy keeps them every `GENE_STRIDE` days in
//!   the dashboard window), with its typical body plan where the record has
//!   one ([`body_plan_mode`], recorded with the gene means since the family
//!   tree), then today's living members.
//! - **Species.** Every species whose last detection counted the lineage's
//!   members, and their ancestors, oldest first. A species' genes are its
//!   representatives' means and its counts and shapes their majority; the
//!   tracker keeps nothing else, so placement, snout, horns, wing shape and
//!   pattern come from the lineage's sample nearest the species' last
//!   sighting.

use std::collections::{BTreeMap, BTreeSet};

use evo_sim::genome::Gene;
use evo_sim::morph::{EarShape, HornForm, MorphGene, MorphLocus, Morphology, Pattern, TailShape};

use super::Game;
use crate::design::gene_key;
use crate::protocol::{FamilySample, FamilySpecies, FamilyTree};
use crate::store::BodyPlanMode;

/// Rounded to 3 decimals (the client draws and compares at that grain).
fn r3(x: f64) -> f32 {
    ((x * 1000.0).round() / 1000.0) as f32
}

/// The most common value (the smallest on a tie, so it is deterministic).
fn majority<T: Ord + Copy>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    let best = counts.values().copied().max()?;
    counts.into_iter().find(|&(_, n)| n == best).map(|(v, _)| v)
}

/// A group's typical body plan: counts and shapes by majority, placement,
/// snout, horn size and wing shape by mean. `None` for an empty group.
pub fn body_plan_mode<'a>(plans: impl Iterator<Item = &'a Morphology> + Clone) -> Option<BodyPlanMode> {
    let n = plans.clone().count();
    if n == 0 {
        return None;
    }
    let mean = |f: fn(&Morphology) -> f32| r3(plans.clone().map(|m| f(m) as f64).sum::<f64>() / n as f64);
    Some(BodyPlanMode {
        eye_count: majority(plans.clone().map(|m| m.eye_count))?,
        eye_azimuth: mean(|m| m.eye_azimuth),
        ear_count: majority(plans.clone().map(|m| m.ear_count))?,
        ear_shape: majority(plans.clone().map(|m| m.ear_shape))?.key().into(),
        leg_count: majority(plans.clone().map(|m| m.leg_count))?,
        tail_shape: majority(plans.clone().map(|m| m.tail_shape))?.key().into(),
        snout_length: mean(|m| m.snout_length),
        // Horns count only where they show: a few sprouting founders don't
        // give the whole lineage stubs.
        horn_size: mean(|m| if m.horns_present { m.horn_size } else { 0.0 }),
        horn_form: majority(plans.clone().filter(|m| m.horns_present).map(|m| m.horn_form))
            .unwrap_or_default()
            .key()
            .into(),
        wing_aspect: mean(|m| m.wing_aspect),
        pattern: majority(plans.clone().map(|m| m.pattern))?.key().into(),
    })
}

/// The default body plan (what a record without one shows).
pub fn default_body_plan() -> BodyPlanMode {
    BodyPlanMode {
        eye_count: MorphLocus::EyeCount.default_value(),
        eye_azimuth: MorphGene::EyeAzimuth.default_param(),
        ear_count: MorphLocus::EarCount.default_value(),
        ear_shape: EarShape::default().key().into(),
        leg_count: MorphLocus::LegCount.default_value(),
        tail_shape: TailShape::default().key().into(),
        snout_length: MorphGene::SnoutLength.default_param(),
        horn_size: MorphGene::HornSize.default_param(),
        horn_form: HornForm::default().key().into(),
        wing_aspect: MorphGene::WingAspect.default_param(),
        pattern: Pattern::default().key().into(),
    }
}

/// Mean expressed value of one gene over representatives (`None` without any).
pub fn rep_mean(rows: &[evo_sim::species::Expr], g: Gene) -> Option<f32> {
    (!rows.is_empty()).then(|| r3(rows.iter().map(|e| e[g.index()] as f64).sum::<f64>() / rows.len() as f64))
}

/// The usual value of a discrete morphology locus over representatives.
pub fn rep_count(rows: &[evo_sim::species::Expr], l: MorphLocus) -> Option<u8> {
    majority(rows.iter().map(|e| e[Gene::COUNT + l.index()].round().max(0.0) as u8)).map(|p| l.value_at(p))
}

/// Gene means of a group of expressed-value rows (genes first).
fn gene_means(rows: &[evo_sim::species::Expr]) -> BTreeMap<String, f32> {
    let n = rows.len().max(1) as f64;
    Gene::ALL.iter().map(|&g| (gene_key(g), r3(rows.iter().map(|e| e[g.index()] as f64).sum::<f64>() / n))).collect()
}

/// Counts and shapes by majority of the representatives' morphology keys
/// (positions on each locus's axis), over `base` for the rest.
fn core_plan(rows: &[evo_sim::species::Expr], mut base: BodyPlanMode) -> BodyPlanMode {
    let value = |l: MorphLocus| {
        majority(rows.iter().map(|e| e[Gene::COUNT + l.index()].round().max(0.0) as u8)).map(|p| l.value_at(p))
    };
    if let Some(v) = value(MorphLocus::EyeCount) {
        base.eye_count = v;
    }
    if let Some(v) = value(MorphLocus::EarCount) {
        base.ear_count = v;
    }
    if let Some(v) = value(MorphLocus::EarShape).and_then(EarShape::from_code) {
        base.ear_shape = v.key().into();
    }
    if let Some(v) = value(MorphLocus::LegCount) {
        base.leg_count = v;
    }
    if let Some(v) = value(MorphLocus::TailShape).and_then(TailShape::from_code) {
        base.tail_shape = v.key().into();
    }
    base
}

impl Game {
    /// The founders as they landed: the family tree's first stage.
    pub(super) fn founding_sample(&self, founders: &[evo_sim::organism::OrgId]) -> FamilySample {
        let members: Vec<&evo_sim::organism::Organism> =
            founders.iter().filter_map(|&id| self.world.organism(id)).collect();
        let n = members.len().max(1) as f64;
        FamilySample {
            day: self.world.day(),
            alive: members.len(),
            genes: Gene::ALL
                .iter()
                .map(|&g| (gene_key(g), r3(members.iter().map(|o| o.pheno[g] as f64).sum::<f64>() / n)))
                .collect(),
            morph: body_plan_mode(members.iter().map(|o| &o.morph)),
        }
    }

    /// The family tree of a player's lineage (see the module docs).
    pub fn family_tree(&self, player: evo_sim::PlayerId) -> FamilyTree {
        let lineage = self.lineage_of(player);
        let day = self.world.day();
        let members: Vec<&evo_sim::organism::Organism> = match lineage {
            Some(l) => self.world.organisms.iter().filter(|o| o.alive && o.lineage == l).collect(),
            None => Vec::new(),
        };
        let mut timeline: Vec<FamilySample> = Vec::new();
        if let Some(l) = lineage {
            // The founding as it happened, when recorded; then the days after it.
            let founding = self.founded.get(&l).cloned();
            let after = founding.as_ref().map(|f| f.day);
            timeline.extend(founding);
            for r in self.history.iter().filter(|r| after.is_none_or(|d| r.day > d)) {
                let Some(e) = r.lineages.iter().find(|e| e.lineage == l) else { continue };
                if e.genes.is_empty() {
                    continue;
                }
                timeline.push(FamilySample {
                    day: r.day,
                    alive: e.alive,
                    genes: e.genes.iter().map(|(k, &v)| (k.clone(), r3(v as f64))).collect(),
                    morph: e.morph.clone(),
                });
            }
            if !members.is_empty() {
                let n = members.len() as f64;
                timeline.push(FamilySample {
                    day,
                    alive: members.len(),
                    genes: Gene::ALL
                        .iter()
                        .map(|&g| (gene_key(g), r3(members.iter().map(|o| o.pheno[g] as f64).sum::<f64>() / n)))
                        .collect(),
                    morph: body_plan_mode(members.iter().map(|o| &o.morph)),
                });
            }
        }
        // Species that counted the lineage, and their ancestors.
        let tracker = &self.world.chronicle.species;
        let mut ids: BTreeSet<u32> = BTreeSet::new();
        if let Some(l) = lineage {
            for s in tracker.species.iter().filter(|s| s.lineages.iter().any(|&(k, _)| k == l)) {
                let mut cur = Some(s.id);
                while let Some(id) = cur {
                    if !ids.insert(id) {
                        break;
                    }
                    cur = tracker.get(id).and_then(|p| p.parent);
                }
            }
        }
        let near = |d: u64| -> BodyPlanMode {
            timeline
                .iter()
                .filter_map(|t| t.morph.as_ref().map(|m| (t.day.abs_diff(d), m)))
                .min_by_key(|&(gap, _)| gap)
                .map(|(_, m)| m.clone())
                .unwrap_or_else(default_body_plan)
        };
        let mut species: Vec<FamilySpecies> = ids
            .iter()
            .filter_map(|&id| tracker.get(id))
            .map(|s| {
                let reps = s.reps();
                FamilySpecies {
                    id: s.id,
                    name: s.name.clone(),
                    origin: s.origin.label().into(),
                    parent: s.parent,
                    founded_day: s.founded_day,
                    extinct_day: s.extinct_day,
                    merged_into: s.merged_into,
                    adults: s.adults,
                    mine: lineage.and_then(|l| s.lineages.iter().find(|&&(k, _)| k == l)).map_or(0.0, |&(_, a)| a),
                    genes: gene_means(reps),
                    morph: core_plan(reps, near(s.extinct_day.unwrap_or(s.last_seen))),
                }
            })
            .collect();
        species.sort_by_key(|s| (s.founded_day, s.id));
        FamilyTree {
            player,
            lineage,
            name: lineage.and_then(|l| self.world.lineages.get(l as usize)).map(|l| l.name.clone()),
            day,
            alive: members.len(),
            timeline,
            species,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameConfig;
    use crate::game::{build_world, WorldSettings};
    use evo_sim::morph::MorphDesign;
    use evo_sim::{BirthType, FounderDesign, Thermo};

    fn run_days(g: &mut Game, days: u32) {
        for _ in 0..days * g.world.config.ticks_per_day {
            g.world.step();
            let _ = g.after_tick();
        }
    }

    /// A lineage with antlers and stripes, a few recorded days.
    fn tree() -> (Game, FamilyTree) {
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        g.set_name(1, "ann");
        let d = FounderDesign::new("Stags", Thermo::Warm, BirthType::Live, 2)
            .founders(8)
            .with(Gene::LegLength, 0.4)
            .with_morph(MorphDesign {
                horn_size: 0.8,
                horn_form: HornForm::Branched,
                pattern: Pattern::Stripes,
                leg_count: 4,
                ..Default::default()
            });
        g.found(1, &d).unwrap();
        run_days(&mut g, 3);
        let t = g.family_tree(1);
        (g, t)
    }

    #[test]
    fn the_tree_follows_the_history_and_today() {
        let (g, t) = tree();
        assert_eq!((t.lineage, t.name.as_deref()), (Some(0), Some("Stags")));
        assert!(t.alive > 0);
        // Three recorded days with gene means, then today.
        assert_eq!(t.timeline.len(), 4, "{:?}", t.timeline.iter().map(|s| s.day).collect::<Vec<_>>());
        assert!(t.timeline.windows(2).all(|w| w[0].day <= w[1].day));
        let now = t.timeline.last().unwrap();
        assert_eq!(now.alive, t.alive);
        assert_eq!(now.genes.len(), Gene::COUNT);
        for s in &t.timeline {
            let m = s.morph.as_ref().expect("records carry the body plan");
            assert_eq!((m.pattern.as_str(), m.horn_form.as_str(), m.leg_count), ("stripes", "branched", 4));
            assert!(m.horn_size > 0.6, "{m:?}");
        }
        // The species tracker's species of the lineage, if it ran yet.
        for s in &t.species {
            assert_eq!(s.genes.len(), Gene::COUNT);
        }
        // Nobody's lineage: an empty tree.
        let none = g.family_tree(9);
        assert!(none.timeline.is_empty() && none.species.is_empty() && none.lineage.is_none());
    }

    #[test]
    fn body_plans_by_majority_and_mean() {
        let p = evo_sim::genome::Phenotype([0.3; Gene::COUNT]);
        let plan = |d: MorphDesign| Morphology::new(&evo_sim::morph::MorphGenes::uniform(&d, 1), &p);
        let six = plan(MorphDesign { leg_count: 6, pattern: Pattern::Spots, ..Default::default() });
        let four = plan(MorphDesign { horn_size: 0.5, ..Default::default() });
        let m = body_plan_mode([&six, &six, &four].into_iter()).unwrap();
        assert_eq!((m.leg_count, m.pattern.as_str()), (6, "spots"));
        // One horned animal in three: a third of its horn size, its form.
        assert!((m.horn_size - 0.167).abs() < 1e-3, "{m:?}");
        assert_eq!(body_plan_mode(std::iter::empty()), None);
        assert_eq!(body_plan_mode([&plan(MorphDesign::default())].into_iter()), Some(default_body_plan()));
    }

    /// `client/src/familyTree.fixture.json`: a real tree, which the client's
    /// `familyTree.test.ts` must lay out. Rerun with `UPDATE_FAMILY_FIXTURE=1`
    /// after a format change.
    #[test]
    fn the_client_family_fixture_is_current() {
        let (_, t) = tree();
        let text = serde_json::to_string_pretty(&t).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/familyTree.fixture.json");
        if std::env::var_os("UPDATE_FAMILY_FIXTURE").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == text, "{} is stale: rerun with UPDATE_FAMILY_FIXTURE=1", path.display());
    }
}
