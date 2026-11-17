//! The worlds (shards) one server process hosts.
//!
//! Each configured world has its own world thread, saver, store task and
//! metrics; accounts and players' settings are shared by all of them. A
//! world is named by a short id (`main`, `isle-2`) that scopes everything it
//! stores (`Store`'s `shard` argument) and that clients pass to choose it
//! (`?world=<id>` on the API and the WebSocket; the first world when absent).
//!
//! The id is not the `world_id` inside saved games and records: that one
//! names one generated map of a world and changes when `--fresh` starts it
//! over, while the configured id stays.

use serde::{Deserialize, Serialize};

use crate::game::WorldSettings;

/// The id of the world a server without a world list hosts, and the world
/// data stored before there were several migrates to.
pub const DEFAULT_WORLD: &str = "main";
/// Display name of the default world.
pub const DEFAULT_WORLD_NAME: &str = "Main";
/// Longest world id.
pub const MAX_WORLD_ID: usize = 32;
/// Longest display name.
pub const MAX_WORLD_NAME: usize = 40;
/// Most worlds one process hosts (each is an OS thread ticking a world).
pub const MAX_WORLDS: usize = 64;
/// Players a world takes before `GET /api/assign` sends new players
/// elsewhere, unless its spec says otherwise: one world thread kept heavy
/// players at a steady 1 s tick up to 4000 on a 4-core box (see
/// `docs/load-test.md`), so this leaves room for bursts.
pub const DEFAULT_WORLD_CAPACITY: usize = 3000;
/// The share of its capacity a world is filled to before `assign` starts
/// spreading new players over emptier worlds (a busy world is a livelier
/// one).
pub const FILL_TARGET: f64 = 0.8;

/// Whether `id` can name a world: 1 to [`MAX_WORLD_ID`] characters of
/// lowercase ASCII letters, digits, `-` and `_` (ids name directories in
/// the file store and appear in URLs).
pub fn valid_world_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_WORLD_ID
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// One configured world. Settings left out come from the server's world
/// settings (`ServerConfig::world`: `--seed`, `--size`, `--no-wild`, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSpec {
    pub id: String,
    /// Shown to players; the id when absent.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    /// Width and height in tiles (32 to 1024).
    #[serde(default)]
    pub size: Option<usize>,
    /// Players online before `GET /api/assign` sends new players to other
    /// worlds ([`DEFAULT_WORLD_CAPACITY`] when absent). Players with a
    /// lineage here still come here.
    #[serde(default)]
    pub capacity: Option<usize>,
}

impl WorldSpec {
    pub fn new(id: &str) -> Self {
        WorldSpec { id: id.to_string(), name: None, seed: None, size: None, capacity: None }
    }

    pub fn capacity(&self) -> usize {
        self.capacity.unwrap_or(DEFAULT_WORLD_CAPACITY)
    }

    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            if self.id == DEFAULT_WORLD {
                DEFAULT_WORLD_NAME.to_string()
            } else {
                self.id.clone()
            }
        })
    }

    /// The world's settings: `base` with this world's seed and size.
    pub fn settings(&self, base: &WorldSettings) -> WorldSettings {
        let mut s = base.clone();
        if let Some(seed) = self.seed {
            s.seed = seed;
        }
        if let Some(n) = self.size {
            s.width = n;
            s.height = n;
        }
        s
    }

    /// Parse `ID[,seed=N][,size=N][,capacity=N][,name=TEXT]` (`--world`,
    /// `EVO_WORLDS`).
    /// The name runs to the end, so it may contain commas.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (id, mut rest) = match text.split_once(',') {
            Some((id, rest)) => (id.trim(), rest),
            None => (text, ""),
        };
        let mut spec = WorldSpec::new(id);
        while !rest.trim().is_empty() {
            let (key, after) = rest.split_once('=').ok_or_else(|| format!("world {id:?}: expected key=value"))?;
            let key = key.trim();
            if key == "name" {
                spec.name = Some(after.trim().to_string());
                break;
            }
            let (value, next) = after.split_once(',').unwrap_or((after, ""));
            let value = value.trim();
            match key {
                "seed" => spec.seed = Some(value.parse().map_err(|e| format!("world {id:?}: seed: {e}"))?),
                "size" => spec.size = Some(value.parse().map_err(|e| format!("world {id:?}: size: {e}"))?),
                "capacity" => spec.capacity = Some(value.parse().map_err(|e| format!("world {id:?}: capacity: {e}"))?),
                other => return Err(format!("world {id:?}: unknown setting {other:?} (seed, size, capacity, name)")),
            }
            rest = next;
        }
        Ok(spec)
    }

    /// Parse a list separated by `;` (the `EVO_WORLDS` variable).
    pub fn parse_list(text: &str) -> Result<Vec<Self>, String> {
        text.split(';').filter(|s| !s.trim().is_empty()).map(Self::parse).collect()
    }

    /// Parse a world list file: a JSON array of `{"id", "name", "seed",
    /// "size"}` objects, or an object with a `"worlds"` array.
    pub fn parse_file(text: &str) -> Result<Vec<Self>, String> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum File {
            List(Vec<WorldSpec>),
            Object { worlds: Vec<WorldSpec> },
        }
        match serde_json::from_str::<File>(text).map_err(|e| e.to_string())? {
            File::List(v) | File::Object { worlds: v } => Ok(v),
        }
    }
}

/// Check a world list: at least one world, at most [`MAX_WORLDS`], valid
/// unique ids, sizes in range, names of reasonable length.
pub fn validate(worlds: &[WorldSpec]) -> Result<(), String> {
    if worlds.is_empty() {
        return Err("the world list is empty".into());
    }
    if worlds.len() > MAX_WORLDS {
        return Err(format!("at most {MAX_WORLDS} worlds per process"));
    }
    for (i, w) in worlds.iter().enumerate() {
        if !valid_world_id(&w.id) {
            return Err(format!("world id {:?}: use 1 to {MAX_WORLD_ID} lowercase letters, digits, '-' or '_'", w.id));
        }
        if worlds[..i].iter().any(|o| o.id == w.id) {
            return Err(format!("world id {:?} appears twice", w.id));
        }
        if let Some(n) = w.size {
            if !(32..=1024).contains(&n) {
                return Err(format!("world {:?}: size must be in 32..=1024", w.id));
            }
        }
        if w.capacity == Some(0) {
            return Err(format!("world {:?}: the capacity must be at least 1", w.id));
        }
        if let Some(name) = &w.name {
            let len = name.chars().count();
            if len == 0 || len > MAX_WORLD_NAME || name.chars().any(char::is_control) {
                return Err(format!("world {:?}: the name must be 1 to {MAX_WORLD_NAME} printable characters", w.id));
            }
        }
    }
    Ok(())
}

/// `GET /api/assign` (and `evo-lobby`'s answer, which adds the server).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssignView {
    /// What `?world=` takes.
    pub world: String,
    pub name: String,
    pub reason: AssignReason,
    pub players_online: usize,
    pub capacity: usize,
    /// The name (or token) belongs to an account on this server.
    pub has_account: bool,
}

/// What `assign` knows of a world.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldLoad {
    pub players_online: usize,
    pub capacity: usize,
    /// The player has a lineage here.
    pub has_lineage: bool,
}

/// Why a world was picked (`GET /api/assign`'s `reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignReason {
    /// The player's lineage lives there.
    Lineage,
    /// The busiest world still under its fill target.
    Fill,
    /// Every world is past its fill target: the emptiest one with room.
    Spread,
}

/// The world a player should play in, from each world's load (`None`: all
/// full). A player with a lineage goes to its world (the first, if several),
/// full or not. Anyone else goes to the busiest world still under
/// [`FILL_TARGET`] of its capacity, so new players meet others, and once
/// every world is past that to the one with the most room relative to its
/// capacity. Ties go to the earlier world. Used by `GET /api/assign` over
/// one process's worlds and by `evo-lobby` over several processes.
pub fn assign(worlds: &[WorldLoad]) -> Option<(usize, AssignReason)> {
    if let Some(i) = worlds.iter().position(|w| w.has_lineage) {
        return Some((i, AssignReason::Lineage));
    }
    let under = |w: &WorldLoad| (w.players_online as f64) < FILL_TARGET * w.capacity as f64;
    let fill = worlds
        .iter()
        .enumerate()
        .filter(|(_, w)| under(w))
        .max_by(|(i, a), (j, b)| a.players_online.cmp(&b.players_online).then(j.cmp(i)));
    if let Some((i, _)) = fill {
        return Some((i, AssignReason::Fill));
    }
    let free = |w: &WorldLoad| 1.0 - w.players_online as f64 / w.capacity.max(1) as f64;
    worlds
        .iter()
        .enumerate()
        .filter(|(_, w)| w.players_online < w.capacity)
        .max_by(|(i, a), (j, b)| free(a).total_cmp(&free(b)).then(j.cmp(i)))
        .map(|(i, _)| (i, AssignReason::Spread))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_specs_parse() {
        assert_eq!(WorldSpec::parse("main").unwrap(), WorldSpec::new("main"));
        let w = WorldSpec::parse("isle-2, seed=9 ,size=96,name=Isle, the second").unwrap();
        assert_eq!(
            w,
            WorldSpec {
                id: "isle-2".into(),
                name: Some("Isle, the second".into()),
                seed: Some(9),
                size: Some(96),
                capacity: None
            }
        );
        assert!(WorldSpec::parse("a,depth=3").is_err());
        assert!(WorldSpec::parse("a,seed").is_err());
        assert!(WorldSpec::parse("a,size=big").is_err());
        let list = WorldSpec::parse_list("main;second,seed=2;").unwrap();
        assert_eq!(list.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["main", "second"]);
        let file = WorldSpec::parse_file(r#"[{"id":"main"},{"id":"b","seed":3,"size":64,"name":"Bee"}]"#).unwrap();
        assert_eq!(
            file[1],
            WorldSpec { id: "b".into(), name: Some("Bee".into()), seed: Some(3), size: Some(64), capacity: None }
        );
        assert_eq!(WorldSpec::parse("c,capacity=500").unwrap().capacity(), 500);
        assert_eq!(WorldSpec::new("c").capacity(), DEFAULT_WORLD_CAPACITY);
        assert!(validate(&[WorldSpec { capacity: Some(0), ..WorldSpec::new("a") }]).is_err());
        let obj = WorldSpec::parse_file(r#"{"worlds":[{"id":"x"}]}"#).unwrap();
        assert_eq!(obj, vec![WorldSpec::new("x")]);
        assert!(WorldSpec::parse_file(r#"[{"id":"x","colour":1}]"#).is_err());
    }

    #[test]
    fn world_lists_are_validated() {
        assert!(validate(&[WorldSpec::new("main")]).is_ok());
        assert!(validate(&[]).is_err());
        for bad in ["", "Main", "a/b", "../x", "a b", &"x".repeat(33)] {
            assert!(validate(&[WorldSpec::new(bad)]).is_err(), "{bad:?}");
        }
        assert!(validate(&[WorldSpec::new("a"), WorldSpec::new("a")]).is_err());
        assert!(validate(&[WorldSpec { size: Some(16), ..WorldSpec::new("a") }]).is_err());
        assert!(validate(&[WorldSpec { name: Some(String::new()), ..WorldSpec::new("a") }]).is_err());
    }

    #[test]
    fn assign_keeps_lineages_fills_then_spreads() {
        let w = |players_online, capacity, has_lineage| WorldLoad { players_online, capacity, has_lineage };
        assert_eq!(assign(&[]), None);
        // A lineage wins, even in a full world.
        assert_eq!(assign(&[w(10, 100, false), w(100, 100, true)]), Some((1, AssignReason::Lineage)));
        // The busiest world under 80% of its capacity.
        assert_eq!(assign(&[w(10, 100, false), w(70, 100, false), w(85, 100, false)]), Some((1, AssignReason::Fill)));
        assert_eq!(assign(&[w(0, 100, false), w(0, 100, false)]), Some((0, AssignReason::Fill)));
        // All past the target: the most room for its size.
        assert_eq!(assign(&[w(90, 100, false), w(170, 200, false)]), Some((1, AssignReason::Spread)));
        // All full.
        assert_eq!(assign(&[w(100, 100, false), w(250, 200, false)]), None);
    }

    #[test]
    fn settings_take_the_world_overrides() {
        let base = WorldSettings::default();
        let s = WorldSpec { seed: Some(5), size: Some(64), ..WorldSpec::new("a") }.settings(&base);
        assert_eq!((s.seed, s.width, s.height, s.wild), (5, 64, 64, base.wild));
        assert_eq!(WorldSpec::new("a").settings(&base).seed, base.seed);
        assert_eq!(WorldSpec::new("main").display_name(), "Main");
        assert_eq!(WorldSpec::new("b").display_name(), "b");
    }
}
