//! Headless simulation rules for the Evolution MMO prototype.
//!
//! This crate is step 1 of the build order in `docs/game-design.md`: the
//! genetics, needs, terrain and scoring rules, running with no players
//! attached. The future server wraps a [`world::World`] and translates player
//! commands into the calls in [`world::commands`].
//!
//! Layout:
//! - [`genome`]: diploid genes with origin-tagged alleles, inheritance, mutation.
//! - [`design`]: founder designs and the point budget.
//! - [`stats`]: phenotype → stats and derived physical properties.
//! - [`terrain`]: map generation, seasons, weather and movement rules.
//! - [`mating`]: mate preference, override cost, outbreeding depression.
//! - [`group`]: critical-mass group traits.
//! - [`legacy`]: the genetic-legacy score.
//! - [`world`]: the tick loop (needs, utility AI, predation, breeding).

pub mod config;
pub mod design;
pub mod genome;
pub mod group;
pub mod legacy;
pub mod mating;
pub mod math;
pub mod organism;
pub mod presets;
pub mod render;
pub mod rng;
pub mod stats;
pub mod terrain;
pub mod world;

pub use config::WorldConfig;
pub use design::{BirthType, FounderDesign, Thermo};
pub use genome::{Gene, Genome, PlayerId, WILD};
pub use world::World;
