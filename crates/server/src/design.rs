//! JSON form of a founder design and its conversion to [`FounderDesign`].
//!
//! The simulation types carry no serde derives, so the server keeps its own
//! wire/storage form. Gene names are the sim's labels in `snake_case`
//! (`"body_size"`, `"armor_plates"`, `"large_eyes"`); the Rust variant name
//! (`"BodySize"`) is accepted too.
//!
//! The body plan's morphology (`docs/morphology-contract.md`) is an optional
//! `morph` object beside `traits` ([`MorphSpec`]): every field optional,
//! missing ones (or the whole object) take the contract defaults, so old
//! clients and stored designs are unchanged. It costs no points yet.

use std::collections::BTreeMap;

use evo_sim::design::{self, DesignError, DESIGN_BUDGET};
use evo_sim::morph::{EarShape, MorphDesign, MorphGene, MorphLocus, TailShape};
use evo_sim::{BirthType, FounderDesign, Gene, Thermo};
use serde::{Deserialize, Serialize};

pub const MAX_NAME_LEN: usize = 40;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThermoSpec {
    Warm,
    Cold,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BirthSpec {
    Live,
    IncubatedEggs,
    AbandonedEggs,
}

fn default_founders() -> u32 {
    design::BASE_FOUNDERS
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DesignSpec {
    pub name: String,
    pub thermo: ThermoSpec,
    pub birth: BirthSpec,
    pub litter_size: u32,
    /// Trait values in [0, 1] that differ from the defaults.
    #[serde(default)]
    pub traits: BTreeMap<String, f32>,
    #[serde(default = "default_founders")]
    pub founders: u32,
    /// The body plan's morphology; `None` (or missing fields) for the
    /// defaults. Left out of the JSON when absent, so stored designs read
    /// and write as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morph: Option<MorphSpec>,
}

/// The morphology part of a design (`DesignSpec::morph`): the contract's
/// parameters, each optional. Shapes are wire names (`"round"`, `"fin"`, …);
/// an unknown one is refused as `bad_morph`, like a value out of range.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MorphSpec {
    /// 1–6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_count: Option<u8>,
    /// Degrees, 0–180.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_azimuth: Option<f32>,
    /// 0, 2 or 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ear_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ear_shape: Option<String>,
    /// 0, 2, 4, 6 or 8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leg_count: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_shape: Option<String>,
    /// 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snout_length: Option<f32>,
}

impl MorphSpec {
    /// The sim's morphology, defaults filled in (validated with the design).
    pub fn to_morph(&self) -> Result<MorphDesign, SpecError> {
        let d = MorphDesign::default();
        let ear_shape = match &self.ear_shape {
            None => d.ear_shape,
            Some(s) => EarShape::from_key(s.trim()).ok_or(SpecError::BadMorph("ear_shape"))?,
        };
        let tail_shape = match &self.tail_shape {
            None => d.tail_shape,
            Some(s) => TailShape::from_key(s.trim()).ok_or(SpecError::BadMorph("tail_shape"))?,
        };
        Ok(MorphDesign {
            eye_count: self.eye_count.unwrap_or(d.eye_count),
            eye_azimuth: self.eye_azimuth.unwrap_or(d.eye_azimuth),
            ear_count: self.ear_count.unwrap_or(d.ear_count),
            ear_shape,
            leg_count: self.leg_count.unwrap_or(d.leg_count),
            tail_shape,
            snout_length: self.snout_length.unwrap_or(d.snout_length),
        })
    }
}

/// Why a design was refused. `code` is stable for clients.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecError {
    UnknownGene(String),
    BadName,
    /// An unknown shape name in `morph` (the field).
    BadMorph(&'static str),
    Design(DesignError),
}

impl SpecError {
    pub fn code(&self) -> &'static str {
        match self {
            SpecError::UnknownGene(_) => "unknown_gene",
            SpecError::BadName => "bad_name",
            SpecError::BadMorph(_) | SpecError::Design(DesignError::Morphology(_)) => "bad_morph",
            SpecError::Design(DesignError::OverBudget { .. }) => "over_budget",
            SpecError::Design(DesignError::ValueOutOfRange(_)) => "value_out_of_range",
            SpecError::Design(DesignError::ReservedGene(_)) => "reserved_gene",
            SpecError::Design(DesignError::LitterSize(_)) => "litter_size",
            SpecError::Design(DesignError::NoFounders) => "no_founders",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SpecError::UnknownGene(g) => format!("unknown gene {g:?}"),
            SpecError::BadName => format!("a lineage name needs 1 to {MAX_NAME_LEN} characters"),
            SpecError::BadMorph(field) => format!("unknown {}", field.replace('_', " ")),
            SpecError::Design(e) => e.to_string(),
        }
    }

    /// Extra fields for the error body (the budget numbers, the gene).
    pub fn detail(&self) -> Option<serde_json::Value> {
        match self {
            SpecError::Design(DesignError::OverBudget { cost, budget }) => {
                Some(serde_json::json!({ "cost": cost, "budget": budget }))
            }
            SpecError::Design(DesignError::ValueOutOfRange(g) | DesignError::ReservedGene(g)) => {
                Some(serde_json::json!({ "gene": gene_key(*g) }))
            }
            SpecError::UnknownGene(g) => Some(serde_json::json!({ "gene": g })),
            SpecError::BadMorph(field) => Some(serde_json::json!({ "field": field })),
            SpecError::Design(DesignError::Morphology(e)) => Some(serde_json::json!({ "field": e.field() })),
            _ => None,
        }
    }
}

/// The wire name of a gene: its label in `snake_case`.
pub fn gene_key(g: Gene) -> String {
    g.label().replace(' ', "_")
}

pub fn parse_gene(name: &str) -> Option<Gene> {
    let norm = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    Gene::ALL
        .iter()
        .copied()
        .find(|&g| gene_key(g) == norm || format!("{g:?}").to_ascii_lowercase() == norm.replace('_', ""))
}

impl DesignSpec {
    /// Convert to the sim's design and check it against the point budget.
    pub fn to_design(&self) -> Result<FounderDesign, SpecError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_LEN || name.chars().any(char::is_control) {
            return Err(SpecError::BadName);
        }
        let thermo = match self.thermo {
            ThermoSpec::Warm => Thermo::Warm,
            ThermoSpec::Cold => Thermo::Cold,
        };
        let birth = match self.birth {
            BirthSpec::Live => BirthType::Live,
            BirthSpec::IncubatedEggs => BirthType::IncubatedEggs,
            BirthSpec::AbandonedEggs => BirthType::AbandonedEggs,
        };
        let mut d = FounderDesign::new(name, thermo, birth, self.litter_size).founders(self.founders);
        if let Some(m) = &self.morph {
            d = d.with_morph(m.to_morph()?);
        }
        for (k, &v) in &self.traits {
            let g = parse_gene(k).ok_or_else(|| SpecError::UnknownGene(k.clone()))?;
            d = d.with(g, v);
        }
        d.validate().map_err(SpecError::Design)?;
        Ok(d)
    }
}

/// One row of `GET /api/genes`: what a design can set and what it costs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneInfo {
    pub name: String,
    pub default: f32,
    /// Points per unit above the default.
    pub cost: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneCatalog {
    pub budget: f32,
    pub base_founders: u32,
    pub cost_per_extra_founder: f32,
    pub cost_per_extra_offspring: f32,
    pub max_litter: u32,
    pub genes: Vec<GeneInfo>,
    /// The morphology parameters a design's `morph` object sets.
    pub morph: Vec<MorphInfo>,
}

/// One row of `GET /api/genes`' `morph` section.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MorphInfo {
    /// Wire name (`"eye_count"`, `"ear_shape"`, …).
    pub name: String,
    /// `count`, `enum` or `axis`.
    pub kind: String,
    /// Allowed values: counts as numbers, shapes as names (absent for an
    /// axis).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<serde_json::Value>,
    /// An axis's range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    pub default: serde_json::Value,
    /// Points (0 for now: morphology isn't priced yet).
    pub cost: f32,
}

fn morph_catalog() -> Vec<MorphInfo> {
    use serde_json::json;
    let d = MorphDesign::default();
    let mut out: Vec<MorphInfo> = MorphLocus::ALL
        .iter()
        .map(|&l| {
            let (lo, hi) = l.range();
            let (values, default) = match l {
                MorphLocus::EarShape => {
                    (EarShape::ALL.iter().map(|s| json!(s.key())).collect(), json!(d.ear_shape.key()))
                }
                MorphLocus::TailShape => {
                    (TailShape::ALL.iter().map(|s| json!(s.key())).collect(), json!(d.tail_shape.key()))
                }
                _ => ((lo..=hi).filter(|&v| l.is_valid(v)).map(|v| json!(v)).collect(), json!(l.default_value())),
            };
            let kind = if l.is_shape() { "enum" } else { "count" };
            MorphInfo { name: l.key().into(), kind: kind.into(), values, min: None, max: None, default, cost: 0.0 }
        })
        .collect();
    for g in MorphGene::ALL {
        out.push(MorphInfo {
            name: g.key().into(),
            kind: "axis".into(),
            values: Vec::new(),
            min: Some(0.0),
            max: Some(g.scale()),
            default: json!(g.default_param()),
            cost: 0.0,
        });
    }
    out
}

pub fn catalog() -> GeneCatalog {
    GeneCatalog {
        budget: DESIGN_BUDGET,
        base_founders: design::BASE_FOUNDERS,
        cost_per_extra_founder: design::COST_PER_EXTRA_FOUNDER,
        cost_per_extra_offspring: design::COST_PER_EXTRA_OFFSPRING,
        max_litter: design::MAX_LITTER,
        genes: Gene::ALL
            .iter()
            .filter(|g| !matches!(g, Gene::Endothermy | Gene::BirthMode | Gene::LitterSize))
            .map(|&g| GeneInfo { name: gene_key(g), default: design::default_value(g), cost: design::trait_cost(g) })
            .collect(),
        morph: morph_catalog(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(traits: &[(&str, f32)]) -> DesignSpec {
        DesignSpec {
            name: "Test".into(),
            thermo: ThermoSpec::Warm,
            birth: BirthSpec::Live,
            litter_size: 2,
            traits: traits.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            founders: 20,
            morph: None,
        }
    }

    #[test]
    fn morphology_is_optional_and_validated() {
        // Old clients and stored designs: no `morph`, defaults, same JSON.
        let old = r#"{"name":"Old","thermo":"warm","birth":"live","litter_size":2,"traits":{"fur":0.4},"founders":20}"#;
        let s: DesignSpec = serde_json::from_str(old).unwrap();
        assert_eq!(s.morph, None);
        assert_eq!(s.to_design().unwrap().morph, MorphDesign::default());
        assert_eq!(serde_json::to_string(&s).unwrap(), old);
        // A partial object fills in the rest.
        let mut s = spec(&[]);
        s.morph = Some(MorphSpec {
            leg_count: Some(6),
            ear_shape: Some("pointed".into()),
            tail_shape: Some("prehensile".into()),
            eye_azimuth: Some(120.0),
            ..Default::default()
        });
        let d = s.to_design().unwrap();
        assert_eq!((d.morph.leg_count, d.morph.eye_count), (6, 2));
        assert_eq!(d.morph.ear_shape, EarShape::Pointed);
        assert_eq!(d.morph.tail_shape, TailShape::Prehensile);
        assert_eq!(d.point_cost(), spec(&[]).to_design().unwrap().point_cost(), "morphology is free");
        let back: DesignSpec = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Out of range or unknown: bad_morph, naming the field.
        let bad = |m: MorphSpec| {
            let mut s = spec(&[]);
            s.morph = Some(m);
            let e = s.to_design().unwrap_err();
            (e.code(), e.detail().unwrap()["field"].as_str().unwrap().to_string())
        };
        let m = MorphSpec::default;
        assert_eq!(bad(MorphSpec { leg_count: Some(5), ..m() }), ("bad_morph", "leg_count".into()));
        assert_eq!(bad(MorphSpec { ear_count: Some(3), ..m() }), ("bad_morph", "ear_count".into()));
        assert_eq!(bad(MorphSpec { eye_count: Some(7), ..m() }), ("bad_morph", "eye_count".into()));
        assert_eq!(bad(MorphSpec { eye_azimuth: Some(-1.0), ..m() }), ("bad_morph", "eye_azimuth".into()));
        assert_eq!(bad(MorphSpec { snout_length: Some(2.0), ..m() }), ("bad_morph", "snout_length".into()));
        assert_eq!(bad(MorphSpec { ear_shape: Some("square".into()), ..m() }), ("bad_morph", "ear_shape".into()));
        assert_eq!(bad(MorphSpec { tail_shape: Some("".into()), ..m() }), ("bad_morph", "tail_shape".into()));
    }

    #[test]
    fn catalog_lists_the_morphology() {
        let c = catalog();
        let names: Vec<&str> = c.morph.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["eye_count", "ear_count", "ear_shape", "leg_count", "tail_shape", "eye_azimuth", "snout_length"]
        );
        let legs = &c.morph[3];
        assert_eq!(legs.values, [0, 2, 4, 6, 8].map(|v| serde_json::json!(v)));
        assert_eq!(legs.default, serde_json::json!(4));
        assert_eq!(c.morph[1].values, [0, 2, 4].map(|v| serde_json::json!(v)));
        assert_eq!(c.morph[4].default, serde_json::json!("thin"));
        assert_eq!(c.morph[5].max, Some(180.0));
        assert!(c.morph.iter().all(|m| m.cost == 0.0));
    }

    #[test]
    fn gene_names_parse() {
        assert_eq!(parse_gene("body_size"), Some(Gene::BodySize));
        assert_eq!(parse_gene("BodySize"), Some(Gene::BodySize));
        assert_eq!(parse_gene("armor plates"), Some(Gene::Armor));
        assert_eq!(parse_gene("wings"), Some(Gene::Wings));
        assert_eq!(parse_gene("laser_eyes"), None);
    }

    #[test]
    fn budget_is_enforced() {
        assert!(spec(&[("fur", 0.5), ("large_eyes", 0.4)]).to_design().is_ok());
        let mut over = spec(&[
            ("wings", 1.0),
            ("armor_plates", 1.0),
            ("toxins", 1.0),
            ("muscle", 1.0),
            ("body_size", 1.0),
            ("claws", 1.0),
            ("fangs", 1.0),
        ]);
        over.founders = 300;
        let err = over.to_design().unwrap_err();
        assert_eq!(err.code(), "over_budget");
        assert!(err.detail().unwrap()["cost"].as_f64().unwrap() > 100.0);
        assert_eq!(spec(&[("teleport", 1.0)]).to_design().unwrap_err().code(), "unknown_gene");
        assert_eq!(spec(&[("endothermy", 1.0)]).to_design().unwrap_err().code(), "reserved_gene");
        assert_eq!(spec(&[("fur", 1.5)]).to_design().unwrap_err().code(), "value_out_of_range");
    }
}
