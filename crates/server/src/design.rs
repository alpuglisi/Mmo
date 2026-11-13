//! JSON form of a founder design and its conversion to [`FounderDesign`].
//!
//! The simulation types carry no serde derives, so the server keeps its own
//! wire/storage form. Gene names are the sim's labels in `snake_case`
//! (`"body_size"`, `"armor_plates"`, `"large_eyes"`); the Rust variant name
//! (`"BodySize"`) is accepted too.

use std::collections::BTreeMap;

use evo_sim::design::{self, DesignError, DESIGN_BUDGET};
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
}

/// Why a design was refused. `code` is stable for clients.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecError {
    UnknownGene(String),
    BadName,
    Design(DesignError),
}

impl SpecError {
    pub fn code(&self) -> &'static str {
        match self {
            SpecError::UnknownGene(_) => "unknown_gene",
            SpecError::BadName => "bad_name",
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
        }
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
