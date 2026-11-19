//! World snapshots (feature `serde`): the whole simulation state as bytes,
//! so a server can stop and carry on exactly where it was.
//!
//! All randomness is derived from `(seed, stream, tick, id)`, so the state
//! plus the tick fully determine the future: a restored world steps exactly
//! like the original (`tests::restored_world_continues_exactly`).
//!
//! The static map ([`Terrain`]: biomes, relief, rivers, ground cover) is not
//! in a snapshot: the snapshot names it by content hash, and the caller
//! stores it once per world ([`Terrain::encode`]) and hands it back to
//! [`World::restore`]. Everything that changes (vegetation, water levels,
//! snow and ice, weather, drought and flood) is in the snapshot.
//!
//! Layout (integers little-endian):
//!
//! | bytes | field |
//! | --- | --- |
//! | 8 | magic `EVOSNAP\0` |
//! | 4 | format version ([`SNAPSHOT_VERSION`]) |
//! | 4 | flags (bit 0: payload is LZ4-compressed; bit 1: tiles apart, from version 11) |
//! | 8 | config hash ([`WorldConfig::snapshot_hash`]) |
//! | 8 | tick |
//! | 8 | organisms (agents, dead ones not yet removed included) |
//! | 8 | payload length before compression |
//! | 8 | FNV-1a 64 checksum of everything after the header |
//! | 8 | terrain content hash ([`Terrain::hash`]; from version 6) |
//! | 8 | stored payload length (only with tiles apart) |
//! | … | payload: `bincode` of `World` (varint integers from version 7), LZ4 block-compressed |
//! | … | tiles (only with tiles apart): deflate of the tile planes |
//!
//! With tiles apart (version 11), the payload writes the map's tiles as an
//! empty list (`serde_util::tiles`) and they follow it as planes: for each
//! of the 12 `f32` fields of a [`Tile`](crate::terrain::Tile) in declaration
//! order, each tile's bits XORed with those of its western neighbour (its
//! northern one at the start of a row, nothing for the first tile), split
//! into four byte planes (least significant first) of one byte per tile.
//! Fields that vary smoothly across the map (temperature, wind, wetness)
//! then leave long runs of zero high bytes, and deflate (fast level) stores
//! the tiles in about 60% of the space LZ4 took for them interleaved. The
//! two parts are encoded, and decoded, on two threads.
//!
//! The header stays readable across format versions, so a server can say
//! why it refuses a snapshot, and can look up the terrain before decoding.
//! Skipped and rebuilt on restore: the id → index map, the spatial grid
//! (rebuilt every tick anyway), A* scratch buffers, phase timings, and each
//! organism's genome-derived caches (phenotype, traits, recessive load:
//! pure functions of the genome, recomputed by `Organism::refresh_derived`
//! to the bit). Everything else, caches that steer the future included
//! (A* paths, hourly density, cohort representatives, LOD passability,
//! cache and clutch lookups), is stored.
//!
//! Genomes are written compactly but exactly (`genome::wire`: origin tags
//! interned per genome, 0.0 and 1.0 alleles in two bits, other values
//! bit-packed with their exponent in three bits), and a pregnancy's father
//! genome as the index of the father while he is in the world
//! (`serde_util::father`), so the restore shares it with him again. From
//! version 18 an organism's genome is written just before it, and a
//! clutch's eggs just before it, against the parents' genomes when both
//! parents are in the world (earlier in the list, for an organism):
//! a choice bit per allele whose parent is heterozygous there, and the few
//! mutated alleles in full (`genome::wire::Parents`, `serde_util::organisms`,
//! `serde_util::clutches`), about 11 bytes instead of about 250.
//!
//! Snapshots of this format version restore, and of versions 10 to 16
//! ([`SnapshotHeader::restorable`]): version 10 holds the tiles inside the
//! payload, 10 and 11 lack `Balance::cold_gestation`, which reads as 1.4 (the
//! rule they ran under; `serde_util::before_v12`), and all three lack
//! `Balance::nest_predation`, which reads as 0 (`serde_util::before_v13`);
//! and versions 13 and 14 restore too. Versions 10 to 13 lack `Gene::Obedience`
//! (every organism's obedience locus reads as the design default, 0.5, with
//! its wanderlust alleles' origin tags) and `Balance::tameness`
//! (`serde_util::before_v14`). Versions 10 to 14 lack the rally orders
//! (none were standing: `serde_util::before_v15`). Version 5 moved the
//! terrain out of the snapshot; version 6 added lakes and floodplains to map
//! generation (`Ground::plain`, `Tile::silt`) and a wind direction to the
//! weather cells, which older maps can't be given deterministically, so
//! saves from before version 6 don't load (start the world afresh).
//! Version 7 is the leaner organism encoding above; a version 6 save is
//! refused like any other version. Version 8 adds three balance parameters
//! to the saved config (`Balance::threat_caution`, `alert_guard`,
//! `pasture_search`). Version 9 adds each hunter's search image
//! (`Organism::search_image`) and the `Balance::search_image` parameter.
//! Version 10 adds player groups and territories (`Organism::group`,
//! migration orders keyed by player and group, the world's claimed
//! territories and claim times). Version 11 writes the tiles apart (above).
//! Version 12 adds the `Balance::cold_gestation` parameter to the saved
//! config, version 13 the `Balance::nest_predation` parameter. Version 14
//! adds the `Gene::Obedience` locus to every genome and species
//! representative, and the `Balance::tameness` parameter. Version 15 adds
//! the world's rally orders (`World::rally_group`). Version 16 adds the
//! `Balance::migration_onset` parameter; versions 10 to 15 read it as 1,
//! the rule they ran under (`serde_util::before_v16`). Version 17 adds the
//! `Balance::home_range` parameters (offline protection's home range);
//! versions 10 to 16 read them as their defaults (`serde_util::before_v17`). Version 18 writes
//! the genomes of young and eggs against their parents' (above); versions
//! 10 to 17 have every genome whole (`serde_util::before_v18`). Version 19
//! adds the `Balance::lactation` and `Balance::warm_upkeep` parameters;
//! versions 10 to 18 read both as 0, the rules they ran under
//! (`serde_util::before_v19`). Version 20 adds the `Balance::fang_grazing`
//! and `Balance::scale_cover` parameters; versions 10 to 19 read both as 0,
//! the rules they ran under (`serde_util::before_v20`).
//! Version 21 adds the `Balance::morph_*` tunables (body plan effects);
//! versions 10 to 20 read them as 0, the rules they ran under
//! (`serde_util::before_v21`). Version 22 adds `Balance::directional_vision`,
//! `Balance::vision_scan` and each organism's heading; versions 10 to 21
//! read the parameters as their defaults (off) and give each organism its
//! birth heading (`serde_util::before_v22`). Version 23 adds every
//! genome's morphology segment (`crate::morph`), species representatives'
//! morphology keys and `Balance::morph_step_rate`; versions 10 to 22 restore
//! with the default morphology for every organism, its origin tags copied
//! from each locus's anchor gene, and the step rate 0
//! (`serde_util::before_v23`). Version 24 adds `Balance::morph_legacy`;
//! versions 10 to 23 read it as 0, morphology alleles left out of the
//! legacy tally (`serde_util::before_v24`). Version 25 adds
//! `Balance::roaming_forage`; versions 10 to 24 read it as 100, roaming
//! hunters eating nothing (`serde_util::before_v25`). Version 26 adds the
//! carnivore-diet tunables (`Balance::carcass_keep`, `carnivore_fat`,
//! `carcass_scent`, `carrion`, `provisioning`, `breeding_territory`);
//! versions 10 to 25 read them all as 0, the rules they ran under
//! (`serde_util::before_v26`). Version 27 adds the predator brakes
//! (`Balance::prey_switching`, `condition_litter`); versions 10 to 26 read
//! them as 0, off (`serde_util::before_v27`). Version 29 (numbered apart
//! from a concurrent version 28) adds the Phase 3 morphology loci (horn size
//! and form, wing aspect, coat pattern) to every genome's morphology segment
//! and their tunables (`Balance::morph_horns`, `morph_wing_shape`,
//! `morph_pattern`); older versions restore with those loci at their
//! defaults, tagged like their anchor genes, and the tunables 0
//! (`serde_util::before_v29`). Version 30 adds `Balance::strike_fixes` (a
//! hunter killed in its strike does nothing more that turn; a kill is eaten
//! once); versions 10 to 29 read it as 0, the old strike
//! (`serde_util::before_v30`). Version 32 (31 is reserved for the food
//! rules) adds staggered decisions: `Balance::decide_interval` and the
//! intents organisms keep between decisions; versions 10 to 31 read the
//! interval as 1 (every organism decides every tick) and keep none
//! (`serde_util::before_v32`). Version 31 adds the food values
//! (`Balance::meat_energy`, `fruit_crop`, `fruit_energy`, `insect_crop`,
//! `switching_power`) and the map's fruit and invertebrate layers
//! (`Map::fruit`, `Map::insects`, one `f32` per tile each in the payload);
//! versions 10 to 30 read the values as 0 (every food worth the same per
//! unit, no fruit or invertebrates, prey switching on the square), keep the
//! `carnivore_hunger_discount`, `larval_competition` and carnivore-diet
//! values they stored, and restore with no fruit or invertebrates on any
//! tile (`serde_util::before_v31`). Version 28 was never written (it was
//! reserved for this change). Version 35 (33 was reserved for it) adds
//! the lineage crowding tunables (the soft lineage cap:
//! `Balance::lineage_crowding`, `crowding_ceiling`, `crowding_juveniles`,
//! `crowding_hunger`) and the mating slots players bought
//! (`World::bonus_slots`); versions 10 to 34 read the tunables as 0 (the
//! old hard cap) and no slots bought (`serde_util::before_v35`).
//! Version 36 adds the predator tunables (pack hunts, territory defence
//! against larger intruders, the attack grace: `Balance::pack_hunt`,
//! `territory_harass`, `attack_grace`) and the possessed animals' attack
//! orders (`World::attack_orders`); versions 10 to 35 read the tunables as
//! 0 (off) and no attack orders (`serde_util::before_v36`).
//! Version 41 (37 to 40 are other changes') adds migration deadlines and
//! the settle circles of groups that just arrived (`World::group_plans`);
//! versions 10 to 36 read none (`serde_util::before_v41`): a standing
//! migration's deadline then runs from the restore.
//! Version 42 (37, 38 and 40 are other changes') adds species hysteresis: the
//! settings `ScoringConfig::species_confirm`, `species_confirm_adults`,
//! `species_merge_confirm` and `species_revive_days`, and the tracker's
//! state for them (`SpeciesTracker::candidates`, `Species::merging`);
//! versions 10 to 41 read the settings as 0 (the old tracker: a split
//! counts at once, merges at once, no identity re-used) and no candidates
//! (`serde_util::before_v42`).
//! Version 43 adds the playtest 2 competitive balance tunables
//! (`Balance::egg_hybrid`, `age_spread`, `dormancy_toll`, `emergence_lag`);
//! versions 10 to 42 read them as 0 (off, `serde_util::before_v43`).
//! Version 44 (43 is the balance session's) adds the playtest 2 fixes
//! (learned avoidance of toxic prey, carnivore energetics, player-led
//! packs, dry dormancy and founding sites, drinking first:
//! `Balance::toxic_learning` and the rest) and the lineages' lessons
//! (`World::toxic_lessons`); versions 10 to 42 read the tunables as 0 (off)
//! and no lessons (`serde_util::before_v44`).
//! Version 45 adds a migration's headway (`MigrationPlan::best_median`,
//! `progress_tick`); versions 41 to 44 read none, measured afresh at the
//! next check (`serde_util::before_v45`).
//! Version 47 (46 and before are other changes') adds the playtest 3
//! founding and drinking tunables (`Balance::reachable_water`,
//! `founding_conditions`) and each lineage's first home
//! (`World::lineage_homes`); versions 10 to 46 read the tunables as 0 (the
//! old rules) and know no homes (`serde_util::before_v47`).
//! Version 48 adds territory claims waiting on a migration and where
//! migrations started (`GroupPlans::pending`); versions 41 to 47 read none
//! (`serde_util::before_v48`).
//! Version 50 (49 is another change's) adds the playtest 4 founding safety
//! tunable (`Balance::founding_safety`); versions 10 to 49 read it as 0
//! (the old rules, `serde_util::before_v50`).
//! Version 51 adds the playtest 4 predator and dormancy tunables
//! (`Balance::player_packs`, `dormant_water`, `possessed_sleep`); versions
//! 10 to 50 read them as 0 (off, `serde_util::before_v51`).
//! Version 52 adds the `DeathCause::Heat` variant (appended: older
//! snapshots decode as before, their heat deaths counted as `Exposure`).
//! Version 55 (53 and 54 are other changes') adds the playtest 5 eggs-and-young tunable
//! (`Balance::brood_care`); versions 10 to 54 read it as 0 (the old rules,
//! `serde_util::before_v55`).
//! Version 56 adds the playtest 5 balance tunables
//! (`Balance::thermo_hybrid`, `flight_upkeep`, `avoid_cap`, `cold_heat`,
//! `species_credit`); versions 10 to 55 read them as 0 (off,
//! `serde_util::before_v56`).
//! Version 57 adds the playtest 5 survival tunables (`Balance::dry_beds`,
//! `drink_reach`); versions 10 to 56 read them as 0 (off,
//! `serde_util::before_v57`).
//! Version 58 adds the era's daily niche tallies (`Chronicle::niche_tally`);
//! versions 10 to 57 read none (an era closed with none falls back to its
//! final day, `serde_util::before_v58`).
//! Version 61 (59 and 60 are other changes') adds the playtest 6 founding
//! tunable (`Balance::founding_drought`); versions 10 to 60 read it as 0
//! (off, `serde_util::before_v61`).
//! Version 62 adds the playtest 6 balance tunables
//! (`Balance::toxin_upkeep`, `hungry_hunters`, `toxin_source`,
//! `speciation_credit`, `clutch_cover`); versions 10 to 61 read them as 0
//! (off, `serde_util::before_v62`).
//! Version 63 adds the playtest 6 predator tunable (`Balance::prey_larder`;
//! versions 10 to 62 read it as 0, `serde_util::before_v63`) and the
//! `DeathCause::FoughtBack` variant (appended: older snapshots decode as
//! before, such deaths counted as `Predation`).
//! Version 68 (64 to 67 are other changes') adds the playtest 7 founding
//! tunable (`Balance::founding_rivals`); versions 10 to 67 read it as 0
//! (off, `serde_util::before_v68`).
//!
//! A terrain blob ([`Terrain::encode`]): magic `EVOTERR\0`, format version
//! ([`TERRAIN_VERSION`], u32), flags (u32, bit 0 LZ4), content hash, width,
//! height, payload length before compression, payload checksum (u64 each),
//! then `bincode` of the [`Terrain`], LZ4-compressed. The content hash is
//! FNV-1a 64 of that `bincode`.

use std::fmt;
use std::io::{Read, Write};
use std::sync::Arc;

use bincode::Options;

use super::spatial::SpatialGrid;
use super::World;
use crate::config::WorldConfig;
use crate::terrain::{Terrain, Tile};

/// Bump on any change to the serialized state (a field added, removed,
/// reordered or retyped in any type the world holds).
pub const SNAPSHOT_VERSION: u32 = 68;
/// The oldest format version that still restores: version 11's state with
/// the tiles inside the payload, and both without `Balance::cold_gestation`
/// (nor, with version 12, `Balance::nest_predation`).
const OLDEST_RESTORABLE: u32 = 10;
/// The first format version with `Balance::cold_gestation`.
const COLD_GESTATION_VERSION: u32 = 12;
/// The first format version with `Balance::nest_predation`.
const NEST_PREDATION_VERSION: u32 = 13;
/// The first format version with `Gene::Obedience` and `Balance::tameness`.
const OBEDIENCE_VERSION: u32 = 14;
/// The first format version with rally orders.
const RALLY_VERSION: u32 = 15;
/// The first format version with `Balance::migration_onset`.
const MIGRATION_ONSET_VERSION: u32 = 16;
/// The first format version with `Balance::home_range`.
const HOME_RANGE_VERSION: u32 = 17;
/// The first format version with genomes written against their parents'.
const BY_PARENTS_VERSION: u32 = 18;
/// The first format version with `Balance::lactation` and
/// `Balance::warm_upkeep` (endotherms' costs).
const WARM_COSTS_VERSION: u32 = 19;
/// The first format version with `Balance::fang_grazing` and
/// `Balance::scale_cover` (traits' contextual costs).
const TRAIT_COSTS_VERSION: u32 = 20;
/// The first format version with the `Balance::morph_*` tunables (body plan
/// effects).
const MORPH_EFFECTS_VERSION: u32 = 21;
/// The first format version with `Balance::directional_vision`,
/// `Balance::vision_scan` and `Organism::heading`.
const DIRECTIONAL_VISION_VERSION: u32 = 22;
/// The first format version with the genomes' morphology segment.
const MORPH_VERSION: u32 = 23;
/// The first version with `Balance::morph_legacy`.
const MORPH_LEGACY_VERSION: u32 = 24;
/// The first version with `Balance::roaming_forage`.
const ROAMING_FORAGE_VERSION: u32 = 25;
/// The first version with the carnivore-diet tunables (`Balance::carcass_keep`
/// and the rest, `serde_util::before_v26`).
const CARNIVORE_DIET_VERSION: u32 = 26;
/// The first version with the predator brakes (`Balance::prey_switching`,
/// `condition_litter`, `serde_util::before_v27`).
const PREDATOR_BRAKES_VERSION: u32 = 27;
/// The first version with the Phase 3 morphology loci and their tunables
/// (`serde_util::before_v29`). Numbered apart from a concurrent version 28;
/// renumber at merge.
const MORPH_EXTRA_VERSION: u32 = 29;
/// The first version with `Balance::strike_fixes` (`serde_util::before_v30`).
const STRIKE_FIXES_VERSION: u32 = 30;
/// The first version with the food values (`Balance::meat_energy`,
/// `fruit_crop`, `fruit_energy`, `insect_crop`, `switching_power`) and the
/// fruit and invertebrate layers (`Map::fruit`, `Map::insects`,
/// `serde_util::before_v31`). Version 28 was never written.
const FOOD_VALUES_VERSION: u32 = 31;
/// The first version with staggered decisions (`Balance::decide_interval`
/// and the kept intents, `serde_util::before_v32`). Version 31 is reserved
/// for the food rules; renumber at merge if needed.
const STAGGER_VERSION: u32 = 32;
/// The first version with the playtest 1 ecology fixes
/// (`Balance::desperate_risk`, `serde_util::before_v34`). Version 33 is
/// another change's (founder pricing); renumber at merge if needed.
const PLAYTEST_FIXES_VERSION: u32 = 34;
/// The first version with lineage crowding (the soft lineage cap) and
/// bought mating slots (`serde_util::before_v35`).
const CROWDING_VERSION: u32 = 35;
/// The first version with the predator tunables (`Balance::pack_hunt`,
/// `territory_harass`, `attack_grace`) and attack orders
/// (`World::attack_orders`, `serde_util::before_v36`).
const PREDATOR_PLAY_VERSION: u32 = 36;
/// The first version with species hysteresis (`ScoringConfig::species_confirm`
/// and the rest, the tracker's candidates and merges in waiting,
/// `serde_util::before_v42`). Versions 37, 38 and 40 are other changes'.
const SPECIES_HYSTERESIS_VERSION: u32 = 42;
/// The first version with migration deadlines and settle circles
/// (`World::group_plans`, `serde_util::before_v41`). Versions 37 to 40 are
/// other changes'; renumber at merge if needed.
const GROUP_PLANS_VERSION: u32 = 41;
/// The first version with the playtest 2 competitive balance tunables
/// (`Balance::egg_hybrid`, `age_spread`, `dormancy_toll`, `emergence_lag`,
/// `serde_util::before_v43`). Gated at 38 on its branch; renumbered at
/// merge after 41 and 42.
const COMPETITIVE_BALANCE_VERSION: u32 = 43;
/// The first version with the playtest 2 fixes (`Balance::toxic_learning`
/// and the rest, `World::toxic_lessons`, `serde_util::before_v44`). Version
/// 43 is another change's (the balance session's); renumber at merge if
/// needed.
const PLAYTEST2_VERSION: u32 = 44;
/// The first version with a migration's headway (`MigrationPlan::best_median`,
/// `progress_tick`, `serde_util::before_v45`).
const MIGRATION_HEADWAY_VERSION: u32 = 45;
/// The first version with the playtest 3 balance tunables
/// (`Balance::eye_hybrid`, `winter_toll`, `wake_spread`,
/// `serde_util::before_v46`). Gated at 45 on its branch; renumbered at
/// merge after 44 and 45.
const WINTER_RULES_VERSION: u32 = 46;
/// The first version with the playtest 3 founding and drinking fixes
/// (`Balance::reachable_water`, `founding_conditions`, `World::lineage_homes`,
/// `serde_util::before_v47`). Gated at 45 on its branch; renumbered at
/// merge after 45 and 46.
const FOUNDING_FIXES_VERSION: u32 = 47;
/// The first version with pending territory claims and migration starts
/// (`GroupPlans::pending`, `serde_util::before_v48`).
const PENDING_PLANS_VERSION: u32 = 48;
/// The first version with the playtest 4 balance tunables
/// (`Balance::winter_rest`, `toxic_avoidance`, `egg_attendance`,
/// `player_species`, `raised_cap`, `serde_util::before_v49`). Provisional;
/// renumber at merge if needed.
const BALANCE_PT4_VERSION: u32 = 49;
/// The first version with the playtest 4 founding safety tunable
/// (`Balance::founding_safety`, `serde_util::before_v50`). Gated at 48 on
/// its branch; renumbered at merge after 48 and 49.
const FOUNDING_SAFETY_VERSION: u32 = 50;
/// The first version with the playtest 4 predator and dormancy tunables
/// (`Balance::player_packs`, `dormant_water`, `possessed_sleep`,
/// `serde_util::before_v51`). Gated at 48 on its branch; renumbered at
/// merge after 48 to 50.
const PLAYTEST4_PREDATORS_VERSION: u32 = 51;
/// The first version with the playtest 5 eggs-and-young tunable
/// (`Balance::brood_care`, `serde_util::before_v55`). Provisional: renumber
/// at merge if needed.
const BROOD_CARE_VERSION: u32 = 55;
/// The first version with the playtest 5 balance tunables
/// (`Balance::thermo_hybrid`, `flight_upkeep`, `avoid_cap`, `cold_heat`,
/// `species_credit`, `serde_util::before_v56`). Taken as 52 on its branch;
/// renumbered at merge after 52 to 55.
const BALANCE_PT5_VERSION: u32 = 56;
/// The first version with the playtest 5 survival tunables
/// (`Balance::dry_beds`, `drink_reach`, `serde_util::before_v57`). Gated at
/// 52 on its branch; renumbered at merge after 52 to 56.
const PLAYTEST5_SURVIVAL_VERSION: u32 = 57;
/// The first version with the era's daily niche tallies (`Chronicle::niche_tally`,
/// `serde_util::before_v58`): niche dominance from the era mean. Provisional.
const NICHE_MEANS_VERSION: u32 = 58;
/// The first version with the playtest 6 founding tunable
/// (`Balance::founding_drought`, `serde_util::before_v61`). Taken as 58 on
/// its branch; 61 as assigned (59 and 60 are other changes').
const FOUNDING_DROUGHT_VERSION: u32 = 61;
/// The first version with the playtest 6 balance tunables
/// (`Balance::toxin_upkeep`, `hungry_hunters`, `toxin_source`,
/// `speciation_credit`, `clutch_cover`, `serde_util::before_v62`). Taken as
/// 58 on its branch; renumbered at merge after 58 to 61.
const BALANCE_PT6_VERSION: u32 = 62;
/// The first version with the playtest 6 predator tunable
/// (`Balance::prey_larder`, `serde_util::before_v63`) and
/// `DeathCause::FoughtBack`. Gated at 58 on its branch; renumbered at merge
/// after 58 to 62.
const PLAYTEST6_PREDATORS_VERSION: u32 = 63;
/// The first version with the playtest 7 founding tunable
/// (`Balance::founding_rivals`, `serde_util::before_v68`). Provisional:
/// renumber at merge if needed.
const FOUNDING_RIVALS_VERSION: u32 = 68;
/// Bump on any change to [`Terrain`]'s serialized form.
pub const TERRAIN_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"EVOSNAP\0";
/// Header length before version 6 (no terrain hash).
const HEADER_LEN_OLD: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8 + 8;
const HEADER_LEN: usize = HEADER_LEN_OLD + 8;
const FLAG_LZ4: u32 = 1;
const FLAG_TILES_APART: u32 = 2;
/// `f32` fields of a [`Tile`], each written as four byte planes.
const TILE_FIELDS: usize = 12;
const TERRAIN_MAGIC: &[u8; 8] = b"EVOTERR\0";
const TERRAIN_HEADER_LEN: usize = 8 + 4 + 4 + 8 * 5;

/// The fixed header of a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub version: u32,
    pub flags: u32,
    pub config_hash: u64,
    pub tick: u64,
    pub organisms: u64,
    pub raw_len: u64,
    pub checksum: u64,
    /// Content hash of the terrain the snapshot needs ([`Terrain::hash`]);
    /// `None` before version 6 (such snapshots no longer restore, but their
    /// header still reads, so the version can be reported).
    pub terrain: Option<u64>,
}

impl SnapshotHeader {
    fn len(&self) -> usize {
        if self.version >= 6 {
            HEADER_LEN
        } else {
            HEADER_LEN_OLD
        }
    }

    /// Whether [`World::restore`] reads snapshots of this format version
    /// (this one, or versions 10 to 16).
    pub fn restorable(&self) -> bool {
        (OLDEST_RESTORABLE..=SNAPSHOT_VERSION).contains(&self.version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a snapshot (or terrain blob), or cut short.
    BadHeader,
    /// Written by another format version; it can't be read by this build.
    Version { found: u32, expected: u32 },
    /// The payload doesn't match its checksum (corrupt or truncated file).
    Checksum,
    /// The payload doesn't decode (or its config doesn't match the header).
    Decode(String),
    /// The snapshot needs a terrain that was not given (not stored, or not
    /// found).
    MissingTerrain { hash: u64 },
    /// The terrain given is not the one the snapshot names.
    WrongTerrain { expected: u64, found: u64 },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SnapshotError::BadHeader => write!(f, "not a world snapshot (bad or truncated header)"),
            SnapshotError::Version { found, expected } => {
                write!(f, "snapshot format version {found}, this build reads version {expected}")
            }
            SnapshotError::Checksum => write!(f, "snapshot payload checksum mismatch (corrupt or truncated)"),
            SnapshotError::Decode(e) => write!(f, "snapshot payload does not decode: {e}"),
            SnapshotError::MissingTerrain { hash } => {
                write!(f, "the snapshot's terrain {hash:016x} is missing (it is stored once per world)")
            }
            SnapshotError::WrongTerrain { expected, found } => {
                write!(f, "the snapshot needs terrain {expected:016x}, not {found:016x}")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

fn fnv1a(bytes: &[u8]) -> u64 {
    // Eight bytes at a time: fast enough for tens of MB (not a standard
    // FNV, but stable, which is all a checksum needs).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for c in chunks {
        h = (h ^ u64::from_le_bytes(*c)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    for &b in rest {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `bincode` settings of a world payload (from version 7): integers as
/// varints (small counts, ids, ticks and enum tags take a byte or two
/// instead of four or eight), little-endian, no trailing bytes. Floats are
/// written whole either way. Terrain blobs and hashes keep plain
/// `bincode::serialize` (fixed-width integers).
fn payload_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
}

fn decode_err(e: impl fmt::Display) -> SnapshotError {
    SnapshotError::Decode(e.to_string())
}

/// The LZ4 payload after a header, checked and decompressed.
fn payload(
    stored: &[u8],
    flags: u32,
    raw_len: u64,
    checksum: u64,
) -> Result<std::borrow::Cow<'_, [u8]>, SnapshotError> {
    if fnv1a(stored) != checksum {
        return Err(SnapshotError::Checksum);
    }
    decompress(stored, flags, raw_len)
}

/// A payload decompressed (if its flags say LZ4).
fn decompress(stored: &[u8], flags: u32, raw_len: u64) -> Result<std::borrow::Cow<'_, [u8]>, SnapshotError> {
    if flags & FLAG_LZ4 == 0 {
        return Ok(stored.into());
    }
    let len = usize::try_from(raw_len).map_err(|_| SnapshotError::BadHeader)?;
    Ok(lz4_flex::block::decompress(stored, len).map_err(decode_err)?.into())
}

/// A tile's fields as bits, in declaration order (destructured, so a new
/// field fails to compile here until the encoding takes it).
fn tile_bits(t: &Tile) -> [u32; TILE_FIELDS] {
    let Tile { temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt } = *t;
    [temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt].map(f32::to_bits)
}

fn tile_from_bits(b: [u32; TILE_FIELDS]) -> Tile {
    let [temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt] = b.map(f32::from_bits);
    Tile { temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt }
}

/// The tile whose bits predict tile `i`'s: its western neighbour, else its
/// northern one, else none.
fn predictor(i: usize, width: usize) -> Option<usize> {
    if !i.is_multiple_of(width) {
        Some(i - 1)
    } else {
        i.checked_sub(width)
    }
}

/// The tiles as deflated byte planes (see the module docs).
fn encode_tiles(tiles: &[Tile], width: usize) -> Vec<u8> {
    let n = tiles.len();
    let bits: Vec<[u32; TILE_FIELDS]> = tiles.iter().map(tile_bits).collect();
    let xored: Vec<[u32; TILE_FIELDS]> = (0..n)
        .map(|i| {
            let pred = predictor(i, width).map_or([0; TILE_FIELDS], |p| bits[p]);
            std::array::from_fn(|f| bits[i][f] ^ pred[f])
        })
        .collect();
    drop(bits);
    // Plane by plane, writing one byte stream at a time.
    let mut planes = vec![0u8; n * TILE_FIELDS * 4];
    for (f, field) in planes.chunks_exact_mut(n * 4).enumerate() {
        let (p0, rest) = field.split_at_mut(n);
        let (p1, rest) = rest.split_at_mut(n);
        let (p2, p3) = rest.split_at_mut(n);
        for (i, x) in xored.iter().enumerate() {
            let [b0, b1, b2, b3] = x[f].to_le_bytes();
            (p0[i], p1[i], p2[i], p3[i]) = (b0, b1, b2, b3);
        }
    }
    drop(xored);
    let mut e = flate2::write::DeflateEncoder::new(Vec::with_capacity(planes.len() / 3), flate2::Compression::fast());
    e.write_all(&planes).expect("deflate into memory");
    e.finish().expect("deflate into memory")
}

/// The tiles of a `width × height` map from [`encode_tiles`] bytes.
fn decode_tiles(stored: &[u8], width: usize, height: usize) -> Result<Vec<Tile>, SnapshotError> {
    let n = width * height;
    let len = n * TILE_FIELDS * 4;
    let mut planes = Vec::with_capacity(len);
    flate2::read::DeflateDecoder::new(stored).take(len as u64 + 1).read_to_end(&mut planes).map_err(decode_err)?;
    if planes.len() != len {
        return Err(decode_err("the tiles do not match the map's size"));
    }
    let mut bits = vec![[0u32; TILE_FIELDS]; n];
    for (f, field) in planes.chunks_exact(n * 4).enumerate() {
        let (p0, rest) = field.split_at(n);
        let (p1, rest) = rest.split_at(n);
        let (p2, p3) = rest.split_at(n);
        for (i, b) in bits.iter_mut().enumerate() {
            b[f] = u32::from_le_bytes([p0[i], p1[i], p2[i], p3[i]]);
        }
    }
    drop(planes);
    for i in 0..n {
        if let Some(p) = predictor(i, width) {
            let pred = bits[p];
            for (b, q) in bits[i].iter_mut().zip(pred) {
                *b ^= q;
            }
        }
    }
    Ok(bits.into_iter().map(tile_from_bits).collect())
}

impl Terrain {
    /// Content hash: FNV-1a 64 of the terrain's `bincode`. Computed once,
    /// then cached (`Map::edit_terrain` forgets it).
    pub fn hash(&self) -> u64 {
        *self.hash.get_or_init(|| fnv1a(&bincode::serialize(self).expect("terrain serializes")))
    }

    /// The terrain as bytes, to store once per world (see the module docs
    /// for the layout). [`Terrain::decode`] reads it back.
    pub fn encode(&self) -> Vec<u8> {
        let raw = bincode::serialize(self).expect("terrain serializes");
        let hash = fnv1a(&raw);
        let _ = self.hash.set(hash);
        let payload = lz4_flex::block::compress(&raw);
        let mut out = Vec::with_capacity(TERRAIN_HEADER_LEN + payload.len());
        out.extend_from_slice(TERRAIN_MAGIC);
        out.extend_from_slice(&TERRAIN_VERSION.to_le_bytes());
        out.extend_from_slice(&FLAG_LZ4.to_le_bytes());
        for v in [hash, self.width as u64, self.height as u64, raw.len() as u64, fnv1a(&payload)] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&payload);
        out
    }

    /// The content hash in a terrain blob's header, without decoding it.
    pub fn encoded_hash(bytes: &[u8]) -> Result<u64, SnapshotError> {
        if bytes.len() < TERRAIN_HEADER_LEN || &bytes[..8] != TERRAIN_MAGIC {
            return Err(SnapshotError::BadHeader);
        }
        Ok(u64::from_le_bytes(bytes[16..24].try_into().unwrap()))
    }

    /// A terrain from [`Terrain::encode`] bytes. Fails on another format
    /// version, a corrupt payload or one that doesn't match its hash.
    pub fn decode(bytes: &[u8]) -> Result<Terrain, SnapshotError> {
        let hash = Terrain::encoded_hash(bytes)?;
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let (version, flags) = (u32_at(8), u32_at(12));
        if version != TERRAIN_VERSION {
            return Err(SnapshotError::Version { found: version, expected: TERRAIN_VERSION });
        }
        let (width, height, raw_len, checksum) = (u64_at(24), u64_at(32), u64_at(40), u64_at(48));
        let raw = payload(&bytes[TERRAIN_HEADER_LEN..], flags, raw_len, checksum)?;
        if fnv1a(&raw) != hash {
            return Err(decode_err("terrain does not match its content hash"));
        }
        let t: Terrain = bincode::deserialize(&raw).map_err(decode_err)?;
        let n = t.width * t.height;
        if (t.width as u64, t.height as u64) != (width, height) || t.ground.len() != n || t.catchment.len() != n {
            return Err(decode_err("terrain does not match its header"));
        }
        let _ = t.hash.set(hash);
        Ok(t)
    }
}

impl WorldConfig {
    /// Stable hash of every setting (balance included), stored in snapshot
    /// headers so a server can tell whether its configuration changed.
    pub fn snapshot_hash(&self) -> u64 {
        fnv1a(&bincode::serialize(self).expect("config serializes"))
    }

    /// Can a world snapshotted under `self` continue under `other`? The map,
    /// clock and calendar must match; balance and caps may change.
    pub fn snapshot_compatible(&self, other: &WorldConfig) -> bool {
        self.seed == other.seed
            && self.width == other.width
            && self.height == other.height
            && self.ticks_per_day == other.ticks_per_day
            && self.days_per_year == other.days_per_year
            && self.lod.enabled == other.lod.enabled
    }
}

impl SnapshotHeader {
    /// Read the header without decoding the payload.
    pub fn read(bytes: &[u8]) -> Result<SnapshotHeader, SnapshotError> {
        if bytes.len() < HEADER_LEN_OLD || &bytes[..8] != MAGIC {
            return Err(SnapshotError::BadHeader);
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let version = u32_at(8);
        let terrain = if version >= 6 {
            if bytes.len() < HEADER_LEN {
                return Err(SnapshotError::BadHeader);
            }
            Some(u64_at(56))
        } else {
            None
        };
        Ok(SnapshotHeader {
            version,
            flags: u32_at(12),
            config_hash: u64_at(16),
            tick: u64_at(24),
            organisms: u64_at(32),
            raw_len: u64_at(40),
            checksum: u64_at(48),
            terrain,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        for v in [self.config_hash, self.tick, self.organisms, self.raw_len, self.checksum] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        if let Some(t) = self.terrain {
            out.extend_from_slice(&t.to_le_bytes());
        }
    }
}

impl World {
    /// The whole simulation state as bytes, except the static terrain
    /// (`self.map.terrain()`), which the snapshot names by hash (see the
    /// module docs for the format). [`World::restore`] continues it exactly.
    pub fn snapshot(&self) -> Vec<u8> {
        let (payload, raw_len, tiles) = std::thread::scope(|s| {
            let tiles = s.spawn(|| encode_tiles(&self.map.tiles, self.map.width));
            let raw = {
                let _apart = crate::serde_util::tiles::Apart::new();
                let _scope = crate::serde_util::by_parents::Scope::new();
                payload_options().serialize(self).expect("world serializes")
            };
            let payload = lz4_flex::block::compress(&raw);
            (payload, raw.len(), tiles.join().expect("tile encoding panicked"))
        });
        let mut body = Vec::with_capacity(8 + payload.len() + tiles.len());
        body.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        body.extend_from_slice(&payload);
        body.extend_from_slice(&tiles);
        self.snapshot_with(SNAPSHOT_VERSION, FLAG_LZ4 | FLAG_TILES_APART, raw_len, &body)
    }

    fn snapshot_with(&self, version: u32, flags: u32, raw_len: usize, body: &[u8]) -> Vec<u8> {
        let header = SnapshotHeader {
            version,
            flags,
            config_hash: self.config.snapshot_hash(),
            tick: self.tick,
            organisms: self.organisms.len() as u64,
            raw_len: raw_len as u64,
            checksum: fnv1a(body),
            terrain: Some(self.map.terrain().hash()),
        };
        let mut out = Vec::with_capacity(HEADER_LEN + body.len());
        header.write(&mut out);
        out.extend_from_slice(body);
        out
    }

    /// A version 10 snapshot (tiles inside the payload, no
    /// `Balance::cold_gestation` or `nest_predation`), as earlier builds
    /// wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v10(&self) -> Vec<u8> {
        let _old = crate::serde_util::before_v12::Guard::new();
        let _old13 = crate::serde_util::before_v13::Guard::new();
        let _old14 = crate::serde_util::before_v14::Guard::new();
        let _old15 = crate::serde_util::before_v15::Guard::new();
        let _old16 = crate::serde_util::before_v16::Guard::new();
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let raw = payload_options().serialize(self).expect("world serializes");
        self.snapshot_with(OLDEST_RESTORABLE, FLAG_LZ4, raw.len(), &lz4_flex::block::compress(&raw))
    }

    /// A version 12 snapshot (no `Balance::nest_predation`), as the builds
    /// before version 13 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v12(&self) -> Vec<u8> {
        let _old13 = crate::serde_util::before_v13::Guard::new();
        let _old14 = crate::serde_util::before_v14::Guard::new();
        let _old15 = crate::serde_util::before_v15::Guard::new();
        let _old16 = crate::serde_util::before_v16::Guard::new();
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        // The header's layout is unchanged since version 6: only the version
        // field differs.
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&COLD_GESTATION_VERSION.to_le_bytes());
        out
    }

    /// A version 13 snapshot (no obedience locus, no `Balance::tameness`),
    /// as the builds before `Gene::Obedience` wrote, to test that they still
    /// restore.
    #[cfg(test)]
    fn snapshot_v13(&self) -> Vec<u8> {
        let _old14 = crate::serde_util::before_v14::Guard::new();
        let _old15 = crate::serde_util::before_v15::Guard::new();
        let _old16 = crate::serde_util::before_v16::Guard::new();
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&NEST_PREDATION_VERSION.to_le_bytes());
        out
    }

    /// A version 14 snapshot (no rally orders), as the builds before the
    /// rally command wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v14(&self) -> Vec<u8> {
        let _old15 = crate::serde_util::before_v15::Guard::new();
        let _old16 = crate::serde_util::before_v16::Guard::new();
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&OBEDIENCE_VERSION.to_le_bytes());
        out
    }

    /// A version 15 snapshot (no `Balance::migration_onset`), as the builds
    /// before version 16 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v15(&self) -> Vec<u8> {
        let _old16 = crate::serde_util::before_v16::Guard::new();
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&RALLY_VERSION.to_le_bytes());
        out
    }

    /// A version 16 snapshot (no `Balance::home_range`, every genome whole), as the builds
    /// before version 17 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v16(&self) -> Vec<u8> {
        let _old17 = crate::serde_util::before_v17::Guard::new();
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MIGRATION_ONSET_VERSION.to_le_bytes());
        out
    }

    /// A version 17 snapshot (every genome written whole), as the builds
    /// before version 18 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v17(&self) -> Vec<u8> {
        let _old18 = crate::serde_util::before_v18::Guard::new();
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&HOME_RANGE_VERSION.to_le_bytes());
        out
    }

    /// A version 18 snapshot (no `Balance::lactation` nor
    /// `Balance::warm_upkeep`), as the builds before version 19 wrote, to
    /// test that they still restore.
    #[cfg(test)]
    fn snapshot_v18(&self) -> Vec<u8> {
        let _old19 = crate::serde_util::before_v19::Guard::new();
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&BY_PARENTS_VERSION.to_le_bytes());
        out
    }

    /// A version 19 snapshot (no `Balance::fang_grazing` nor
    /// `Balance::scale_cover`), as the builds before version 20 wrote, to
    /// test that they still restore.
    #[cfg(test)]
    fn snapshot_v19(&self) -> Vec<u8> {
        let _old20 = crate::serde_util::before_v20::Guard::new();
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&WARM_COSTS_VERSION.to_le_bytes());
        out
    }

    /// A version 20 snapshot (no `Balance::morph_*` tunables), as the builds
    /// before version 21 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v20(&self) -> Vec<u8> {
        let _old21 = crate::serde_util::before_v21::Guard::new();
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&TRAIT_COSTS_VERSION.to_le_bytes());
        out
    }

    /// A version 21 snapshot (no `Balance::directional_vision`,
    /// `Balance::vision_scan` nor `Organism::heading`), as the builds before
    /// version 22 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v21(&self) -> Vec<u8> {
        let _old22 = crate::serde_util::before_v22::Guard::new();
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MORPH_EFFECTS_VERSION.to_le_bytes());
        out
    }

    /// A version 22 snapshot (no morphology segment), as the builds before
    /// version 23 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v22(&self) -> Vec<u8> {
        let _old23 = crate::serde_util::before_v23::Guard::new();
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&DIRECTIONAL_VISION_VERSION.to_le_bytes());
        out
    }

    /// A version 23 snapshot (no `Balance::morph_legacy`), as the builds
    /// before version 24 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v23(&self) -> Vec<u8> {
        let _old24 = crate::serde_util::before_v24::Guard::new();
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MORPH_VERSION.to_le_bytes());
        out
    }

    /// A version 24 snapshot (no `Balance::roaming_forage`), as the
    /// builds before version 25 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v24(&self) -> Vec<u8> {
        let _old25 = crate::serde_util::before_v25::Guard::new();
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MORPH_LEGACY_VERSION.to_le_bytes());
        out
    }

    /// A version 25 snapshot (no carnivore-diet tunables), as the builds
    /// before version 26 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v25(&self) -> Vec<u8> {
        let _old26 = crate::serde_util::before_v26::Guard::new();
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&ROAMING_FORAGE_VERSION.to_le_bytes());
        out
    }

    /// A version 26 snapshot (no predator brakes), as the builds before
    /// version 27 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v26(&self) -> Vec<u8> {
        let _old27 = crate::serde_util::before_v27::Guard::new();
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&CARNIVORE_DIET_VERSION.to_le_bytes());
        out
    }

    /// A version 27 snapshot (no Phase 3 morphology loci nor their
    /// tunables), as the builds before version 29 wrote, to test that they
    /// still restore.
    #[cfg(test)]
    fn snapshot_v27(&self) -> Vec<u8> {
        let _old29 = crate::serde_util::before_v29::Guard::new();
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PREDATOR_BRAKES_VERSION.to_le_bytes());
        out
    }

    /// A version 29 snapshot (no `Balance::strike_fixes`), as the builds
    /// before version 30 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v29(&self) -> Vec<u8> {
        let _old30 = crate::serde_util::before_v30::Guard::new();
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MORPH_EXTRA_VERSION.to_le_bytes());
        out
    }

    /// A version 30 snapshot (no staggered decisions), as the builds
    /// before version 32 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v30(&self) -> Vec<u8> {
        let _old31 = crate::serde_util::before_v31::Guard::new();
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&STRIKE_FIXES_VERSION.to_le_bytes());
        out
    }

    /// A version 31 snapshot (no staggered decisions), as the builds with
    /// the food values before version 32 wrote, to test that they still
    /// restore.
    #[cfg(test)]
    fn snapshot_v31(&self) -> Vec<u8> {
        let _old32 = crate::serde_util::before_v32::Guard::new();
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&FOOD_VALUES_VERSION.to_le_bytes());
        out
    }

    /// A version 32 snapshot (no playtest 1 ecology fixes), as the builds
    /// before version 34 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v32(&self) -> Vec<u8> {
        let _old34 = crate::serde_util::before_v34::Guard::new();
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&STAGGER_VERSION.to_le_bytes());
        out
    }

    /// A version 34 snapshot (no lineage crowding, no bought mating slots),
    /// as the builds before version 35 wrote, to test that they still
    /// restore.
    #[cfg(test)]
    fn snapshot_v34(&self) -> Vec<u8> {
        let _old35 = crate::serde_util::before_v35::Guard::new();
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PLAYTEST_FIXES_VERSION.to_le_bytes());
        out
    }

    /// A version 35 snapshot (no predator tunables, no attack orders), as
    /// the builds before version 36 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v35(&self) -> Vec<u8> {
        let _old36 = crate::serde_util::before_v36::Guard::new();
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&CROWDING_VERSION.to_le_bytes());
        out
    }

    /// A version 36 snapshot (no species hysteresis, no migration deadlines
    /// or settle circles), as the builds before versions 41 and 42 wrote, to
    /// test that they still restore.
    #[cfg(test)]
    fn snapshot_v36(&self) -> Vec<u8> {
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let _old41 = crate::serde_util::before_v41::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PREDATOR_PLAY_VERSION.to_le_bytes());
        out
    }

    /// A version 41 snapshot (no species hysteresis), as the builds before
    /// version 42 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v41(&self) -> Vec<u8> {
        let _old42 = crate::serde_util::before_v42::Guard::new();
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&GROUP_PLANS_VERSION.to_le_bytes());
        out
    }

    /// A version 42 snapshot (no playtest 2 balance tunables), as the
    /// builds before version 43 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v42(&self) -> Vec<u8> {
        let _old43 = crate::serde_util::before_v43::Guard::new();
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&SPECIES_HYSTERESIS_VERSION.to_le_bytes());
        out
    }

    /// A version 43 snapshot (no playtest 2 fixes, no toxic lessons, no
    /// migration headway), as the builds before version 44 wrote, to test
    /// that they still restore.
    #[cfg(test)]
    fn snapshot_v43(&self) -> Vec<u8> {
        let _old44 = crate::serde_util::before_v44::Guard::new();
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&COMPETITIVE_BALANCE_VERSION.to_le_bytes());
        out
    }

    /// A version 44 snapshot (no migration headway), as the builds before
    /// version 45 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v44(&self) -> Vec<u8> {
        let _old45 = crate::serde_util::before_v45::Guard::new();
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PLAYTEST2_VERSION.to_le_bytes());
        out
    }

    /// A version 45 snapshot (no playtest 3 balance tunables), as the
    /// builds before version 46 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v45(&self) -> Vec<u8> {
        let _old46 = crate::serde_util::before_v46::Guard::new();
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&MIGRATION_HEADWAY_VERSION.to_le_bytes());
        out
    }

    /// A version 46 snapshot (no playtest 3 founding and drinking fixes), as
    /// the builds before version 47 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v46(&self) -> Vec<u8> {
        let _old47 = crate::serde_util::before_v47::Guard::new();
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&WINTER_RULES_VERSION.to_le_bytes());
        out
    }

    /// A version 47 snapshot (no pending territory claims or migration
    /// starts), as the builds before version 48 wrote, to test that they
    /// still restore.
    #[cfg(test)]
    fn snapshot_v47(&self) -> Vec<u8> {
        let _old48 = crate::serde_util::before_v48::Guard::new();
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&FOUNDING_FIXES_VERSION.to_le_bytes());
        out
    }

    /// A version 48 snapshot (no playtest 4 balance tunables), as the builds
    /// before version 49 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v48(&self) -> Vec<u8> {
        let _old49 = crate::serde_util::before_v49::Guard::new();
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PENDING_PLANS_VERSION.to_le_bytes());
        out
    }

    /// A version 49 snapshot (no founding safety tunable), as the builds
    /// before version 50 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v49(&self) -> Vec<u8> {
        let _old50 = crate::serde_util::before_v50::Guard::new();
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&BALANCE_PT4_VERSION.to_le_bytes());
        out
    }

    /// A version 50 snapshot (no playtest 4 predator and dormancy
    /// tunables), as the builds before version 51 wrote, to test that they
    /// still restore.
    #[cfg(test)]
    fn snapshot_v50(&self) -> Vec<u8> {
        let _old51 = crate::serde_util::before_v51::Guard::new();
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&FOUNDING_SAFETY_VERSION.to_le_bytes());
        out
    }

    /// A version 51 snapshot (no eggs-and-young tunable), as the builds
    /// before version 52 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v51(&self) -> Vec<u8> {
        let _old55 = crate::serde_util::before_v55::Guard::new();
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PLAYTEST4_PREDATORS_VERSION.to_le_bytes());
        out
    }

    /// A version 55 snapshot (no playtest 5 balance tunables), as the
    /// builds before version 56 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v55(&self) -> Vec<u8> {
        let _old56 = crate::serde_util::before_v56::Guard::new();
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&BROOD_CARE_VERSION.to_le_bytes());
        out
    }

    /// A version 56 snapshot (no playtest 5 survival tunables), as the
    /// builds before version 57 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v56(&self) -> Vec<u8> {
        let _old57 = crate::serde_util::before_v57::Guard::new();
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&BALANCE_PT5_VERSION.to_le_bytes());
        out
    }

    /// A version 57 snapshot (no niche tallies), as the builds before
    /// version 58 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v57(&self) -> Vec<u8> {
        let _old58 = crate::serde_util::before_v58::Guard::new();
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PLAYTEST5_SURVIVAL_VERSION.to_le_bytes());
        out
    }

    /// [`World::snapshot`] as version 58 wrote it (tests): without the
    /// founding tunable.
    #[cfg(test)]
    fn snapshot_v58(&self) -> Vec<u8> {
        let _old61 = crate::serde_util::before_v61::Guard::new();
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&NICHE_MEANS_VERSION.to_le_bytes());
        out
    }

    /// A version 61 snapshot (no playtest 6 balance tunables), as the
    /// builds before version 62 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v61(&self) -> Vec<u8> {
        let _old62 = crate::serde_util::before_v62::Guard::new();
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&FOUNDING_DROUGHT_VERSION.to_le_bytes());
        out
    }

    /// A version 62 snapshot (no playtest 6 predator tunable), as the
    /// builds before version 63 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v62(&self) -> Vec<u8> {
        let _old63 = crate::serde_util::before_v63::Guard::new();
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&BALANCE_PT6_VERSION.to_le_bytes());
        out
    }

    /// A version 63 snapshot (no playtest 7 founding tunable), as the
    /// builds before version 68 wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v63(&self) -> Vec<u8> {
        let _old68 = crate::serde_util::before_v68::Guard::new();
        let mut out = self.snapshot();
        out[8..12].copy_from_slice(&PLAYTEST6_PREDATORS_VERSION.to_le_bytes());
        out
    }

    /// A world from [`World::snapshot`] bytes (or a version 10 to 67 snapshot's)
    /// and the terrain it names (`SnapshotHeader::terrain`), with its
    /// derived lookups rebuilt. Fails on another format version, a corrupt
    /// payload, or a missing or wrong terrain.
    pub fn restore(bytes: &[u8], terrain: Option<Arc<Terrain>>) -> Result<World, SnapshotError> {
        let h = SnapshotHeader::read(bytes)?;
        if !h.restorable() {
            return Err(SnapshotError::Version { found: h.version, expected: SNAPSHOT_VERSION });
        }
        let hash = h.terrain.ok_or(SnapshotError::BadHeader)?;
        // Read (and hash) the config without the parameters added since.
        let _old = (h.version < COLD_GESTATION_VERSION).then(crate::serde_util::before_v12::Guard::new);
        let _old13 = (h.version < NEST_PREDATION_VERSION).then(crate::serde_util::before_v13::Guard::new);
        let _old14 = (h.version < OBEDIENCE_VERSION).then(crate::serde_util::before_v14::Guard::new);
        let _old15 = (h.version < RALLY_VERSION).then(crate::serde_util::before_v15::Guard::new);
        let _old16 = (h.version < MIGRATION_ONSET_VERSION).then(crate::serde_util::before_v16::Guard::new);
        let _old17 = (h.version < HOME_RANGE_VERSION).then(crate::serde_util::before_v17::Guard::new);
        let _old18 = (h.version < BY_PARENTS_VERSION).then(crate::serde_util::before_v18::Guard::new);
        let _old19 = (h.version < WARM_COSTS_VERSION).then(crate::serde_util::before_v19::Guard::new);
        let _old20 = (h.version < TRAIT_COSTS_VERSION).then(crate::serde_util::before_v20::Guard::new);
        let _old21 = (h.version < MORPH_EFFECTS_VERSION).then(crate::serde_util::before_v21::Guard::new);
        let _old22 = (h.version < DIRECTIONAL_VISION_VERSION).then(crate::serde_util::before_v22::Guard::new);
        let _old23 = (h.version < MORPH_VERSION).then(crate::serde_util::before_v23::Guard::new);
        let _old24 = (h.version < MORPH_LEGACY_VERSION).then(crate::serde_util::before_v24::Guard::new);
        let _old25 = (h.version < ROAMING_FORAGE_VERSION).then(crate::serde_util::before_v25::Guard::new);
        let _old26 = (h.version < CARNIVORE_DIET_VERSION).then(crate::serde_util::before_v26::Guard::new);
        let _old27 = (h.version < PREDATOR_BRAKES_VERSION).then(crate::serde_util::before_v27::Guard::new);
        let _old29 = (h.version < MORPH_EXTRA_VERSION).then(crate::serde_util::before_v29::Guard::new);
        let _old30 = (h.version < STRIKE_FIXES_VERSION).then(crate::serde_util::before_v30::Guard::new);
        let _old31 = (h.version < FOOD_VALUES_VERSION).then(crate::serde_util::before_v31::Guard::new);
        let _old32 = (h.version < STAGGER_VERSION).then(crate::serde_util::before_v32::Guard::new);
        let _old34 = (h.version < PLAYTEST_FIXES_VERSION).then(crate::serde_util::before_v34::Guard::new);
        let _old35 = (h.version < CROWDING_VERSION).then(crate::serde_util::before_v35::Guard::new);
        let _old36 = (h.version < PREDATOR_PLAY_VERSION).then(crate::serde_util::before_v36::Guard::new);
        let _old42 = (h.version < SPECIES_HYSTERESIS_VERSION).then(crate::serde_util::before_v42::Guard::new);
        let _old43 = (h.version < COMPETITIVE_BALANCE_VERSION).then(crate::serde_util::before_v43::Guard::new);
        let _old44 = (h.version < PLAYTEST2_VERSION).then(crate::serde_util::before_v44::Guard::new);
        let _old45 = (h.version < MIGRATION_HEADWAY_VERSION).then(crate::serde_util::before_v45::Guard::new);
        let _old46 = (h.version < WINTER_RULES_VERSION).then(crate::serde_util::before_v46::Guard::new);
        let _old47 = (h.version < FOUNDING_FIXES_VERSION).then(crate::serde_util::before_v47::Guard::new);
        let _old48 = (h.version < PENDING_PLANS_VERSION).then(crate::serde_util::before_v48::Guard::new);
        let _old49 = (h.version < BALANCE_PT4_VERSION).then(crate::serde_util::before_v49::Guard::new);
        let _old50 = (h.version < FOUNDING_SAFETY_VERSION).then(crate::serde_util::before_v50::Guard::new);
        let _old51 = (h.version < PLAYTEST4_PREDATORS_VERSION).then(crate::serde_util::before_v51::Guard::new);
        let _old55 = (h.version < BROOD_CARE_VERSION).then(crate::serde_util::before_v55::Guard::new);
        let _old56 = (h.version < BALANCE_PT5_VERSION).then(crate::serde_util::before_v56::Guard::new);
        let _old57 = (h.version < PLAYTEST5_SURVIVAL_VERSION).then(crate::serde_util::before_v57::Guard::new);
        let _old58 = (h.version < NICHE_MEANS_VERSION).then(crate::serde_util::before_v58::Guard::new);
        let _old61 = (h.version < FOUNDING_DROUGHT_VERSION).then(crate::serde_util::before_v61::Guard::new);
        let _old62 = (h.version < BALANCE_PT6_VERSION).then(crate::serde_util::before_v62::Guard::new);
        let _old63 = (h.version < PLAYTEST6_PREDATORS_VERSION).then(crate::serde_util::before_v63::Guard::new);
        let _old68 = (h.version < FOUNDING_RIVALS_VERSION).then(crate::serde_util::before_v68::Guard::new);
        let _old41 = (h.version < GROUP_PLANS_VERSION).then(crate::serde_util::before_v41::Guard::new);
        let body = &bytes[h.len()..];
        if fnv1a(body) != h.checksum {
            return Err(SnapshotError::Checksum);
        }
        let terrain = terrain.ok_or(SnapshotError::MissingTerrain { hash })?;
        if terrain.hash() != hash {
            return Err(SnapshotError::WrongTerrain { expected: hash, found: terrain.hash() });
        }
        let (stored, tiles) = if h.flags & FLAG_TILES_APART != 0 {
            let (len, rest) = body.split_at_checked(8).ok_or(SnapshotError::BadHeader)?;
            let len =
                usize::try_from(u64::from_le_bytes(len.try_into().unwrap())).map_err(|_| SnapshotError::BadHeader)?;
            let (stored, tiles) = rest.split_at_checked(len).ok_or(SnapshotError::BadHeader)?;
            (stored, Some(tiles))
        } else {
            (body, None)
        };
        let (world, tiles) = std::thread::scope(|s| {
            let tiles = tiles.map(|t| s.spawn(|| decode_tiles(t, terrain.width, terrain.height)));
            let _scope = crate::serde_util::by_parents::Scope::new();
            let world = decompress(stored, h.flags, h.raw_len)
                .and_then(|raw| payload_options().deserialize::<World>(&raw).map_err(decode_err));
            (world, tiles.map(|t| t.join().expect("tile decoding panicked")))
        });
        let mut w = world?;
        // Lineages from before version 36 count as founded at tick 0.
        w.lineage_founded.resize(w.lineages.len(), 0);
        // Lineages from before version 47 have no home recorded.
        w.lineage_homes.resize(w.lineages.len(), None);
        w.rebuild_grace_starts();
        if let Some(tiles) = tiles {
            if !w.map.tiles.is_empty() {
                return Err(decode_err("tiles both inside and apart from the payload"));
            }
            w.map.tiles = tiles?;
        }
        let m = &w.map;
        if m.terrain().hash() != hash
            || (terrain.width, terrain.height) != (m.width, m.height)
            || terrain.ground.len() != m.tiles.len()
        {
            return Err(decode_err("payload does not match its terrain"));
        }
        w.map.set_terrain(terrain);
        // Before version 31 there are no fruit and invertebrate layers:
        // none on any tile.
        for layer in [&mut w.map.fruit, &mut w.map.insects] {
            if layer.is_empty() {
                *layer = vec![0.0; w.map.tiles.len()];
            } else if layer.len() != w.map.tiles.len() {
                return Err(decode_err("a food layer does not match the map's size"));
            }
        }
        if w.config.snapshot_hash() != h.config_hash || w.tick != h.tick {
            return Err(SnapshotError::Decode("payload does not match its header".into()));
        }
        if h.version < DIRECTIONAL_VISION_VERSION {
            // No headings saved: each faces its birth heading until it moves.
            for o in &mut w.organisms {
                o.heading = crate::vision::birth_heading(o.id);
            }
        }
        w.rebuild_derived();
        Ok(w)
    }

    /// Rebuild what snapshots skip.
    fn rebuild_derived(&mut self) {
        self.index = self.organisms.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
        // Body plan effects follow the restored config, as at birth.
        let bal = self.config.balance;
        for o in &mut self.organisms {
            crate::morph_effects::apply(&mut o.traits, &o.pheno, &o.morph, &bal);
        }
        self.lod.reapply_morph_effects(&bal);
        self.grid = SpatialGrid::new(self.config.width, self.config.height, super::GRID_CELL);
        self.grid.rebuild(&self.organisms);
    }
}

#[cfg(test)]
mod tests;
