//! What a film run records, once a day: each lineage's head count and the
//! mean expressed value of a few genes, plus the chronicle's species,
//! catastrophes and extinctions. Saved as `timeline.json`; story mining and
//! the charts read only this, so they can be rerun without simulating.

use evo_sim::chronicle::CatastropheKind;
use evo_sim::species::SpeciesOrigin;
use evo_sim::{Gene, World};
use serde::{Deserialize, Serialize};

/// Genes followed per lineage (the ones that tell a story: diet, size,
/// social life, temperament, breeding).
pub const GENES: [Gene; 10] = [
    Gene::Carnivory,
    Gene::BodySize,
    Gene::PackInstinct,
    Gene::GroupTolerance,
    Gene::Aggression,
    Gene::Boldness,
    Gene::LitterSize,
    Gene::FatStores,
    Gene::Pickiness,
    Gene::Nose,
];

#[derive(Serialize, Deserialize, Clone)]
pub struct LineageInfo {
    pub name: String,
    pub owner: u32,
    /// `[r, g, b]` as drawn in the film and charts.
    pub color: [u8; 3],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DaySample {
    pub day: u64,
    /// Living organisms per lineage (index into `lineages`).
    pub population: Vec<u32>,
    /// Mean expressed value per lineage per gene of `genes` (null when the
    /// lineage has no members).
    pub genes: Vec<Vec<Option<f32>>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SpeciesEvent {
    pub id: u32,
    pub name: String,
    /// "initial", "founding" or "split".
    pub origin: String,
    pub parent: Option<String>,
    pub founded_day: u64,
    pub extinct_day: Option<u64>,
    pub peak_adults: f64,
    /// The founding lineage with most adults in it, by name.
    pub main_lineage: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CatastropheEvent {
    pub kind: String,
    pub severity: f32,
    pub start_day: u64,
    pub end_day: u64,
    pub population_before: usize,
    pub population_after: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Timeline {
    pub seed: u64,
    pub size: usize,
    pub days_per_year: u32,
    pub meat: bool,
    pub genes: Vec<String>,
    pub lineages: Vec<LineageInfo>,
    pub days: Vec<DaySample>,
    pub species: Vec<SpeciesEvent>,
    pub catastrophes: Vec<CatastropheEvent>,
    /// How the film was cut (0 frames: no film), so `--from` can list it.
    #[serde(default)]
    pub film: FilmInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct FilmInfo {
    pub frames: u32,
    pub frame_days: u32,
    pub fps: u32,
}

impl Timeline {
    pub fn new(world: &World, meat: bool) -> Self {
        Timeline {
            seed: world.config.seed,
            size: world.map.width,
            days_per_year: world.config.days_per_year,
            meat,
            genes: GENES.iter().map(|g| g.label().to_string()).collect(),
            lineages: Vec::new(),
            days: Vec::new(),
            species: Vec::new(),
            catastrophes: Vec::new(),
            film: FilmInfo::default(),
        }
    }

    /// Record today's numbers.
    pub fn sample(&mut self, world: &World) {
        while self.lineages.len() < world.lineages.len() {
            let k = self.lineages.len();
            let l = &world.lineages[k];
            self.lineages.push(LineageInfo {
                name: l.name.clone(),
                owner: l.owner,
                color: super::art::lineage_color(k),
            });
        }
        let n = world.lineages.len();
        let mut population = vec![0u32; n];
        let mut sums = vec![[0.0f64; GENES.len()]; n];
        for o in world.organisms.iter().filter(|o| o.alive) {
            let l = o.lineage as usize;
            population[l] += 1;
            for (s, g) in sums[l].iter_mut().zip(GENES) {
                *s += o.pheno[g] as f64;
            }
        }
        let genes = sums
            .iter()
            .zip(&population)
            .map(|(s, &p)| s.iter().map(|v| (p > 0).then(|| (v / p as f64) as f32)).collect())
            .collect();
        self.days.push(DaySample { day: world.day(), population, genes });
    }

    /// Copy the chronicle's species and catastrophes (at the end of a run).
    pub fn finish(&mut self, world: &World) {
        let ch = &world.chronicle;
        let name_of = |id: u32| ch.species.species.iter().find(|s| s.id == id).map(|s| s.name.clone());
        self.species = ch
            .species
            .species
            .iter()
            .map(|s| SpeciesEvent {
                id: s.id,
                name: s.name.clone(),
                origin: match s.origin {
                    SpeciesOrigin::Initial => "initial",
                    SpeciesOrigin::Founding => "founding",
                    SpeciesOrigin::Split => "split",
                }
                .to_string(),
                parent: s.parent.and_then(name_of),
                founded_day: s.founded_day,
                extinct_day: s.extinct_day,
                peak_adults: s.peak_adults,
                main_lineage: s
                    .lineages
                    .first()
                    .and_then(|&(l, _)| world.lineages.get(l as usize).map(|l| l.name.clone())),
            })
            .collect();
        self.catastrophes = ch
            .catastrophes
            .iter()
            .filter(|c| c.started)
            .map(|c| CatastropheEvent {
                kind: kind_name(c.kind).to_string(),
                severity: c.severity,
                start_day: c.start_day,
                end_day: c.end_day,
                population_before: c.population_before,
                population_after: c.population_after,
            })
            .collect();
    }

    /// "year 2, spring (day 151)".
    pub fn date(&self, day: u64) -> String {
        let dpy = self.days_per_year.max(4) as u64;
        let season = ["spring", "summer", "autumn", "winter"][((day % dpy) * 4 / dpy) as usize];
        format!("year {}, {season} (day {day})", day / dpy + 1)
    }
}

fn kind_name(k: CatastropheKind) -> &'static str {
    match k {
        CatastropheKind::SevereWinter => "severe winter",
        CatastropheKind::Megadrought => "megadrought",
        CatastropheKind::Epidemic => "epidemic",
        CatastropheKind::FloodYear => "flood year",
    }
}
