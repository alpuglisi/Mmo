//! Saved games: a world snapshot (`World::snapshot`) plus the game-side
//! state around it ([`SavedGame`]), as one blob for the store.
//!
//! Layout (saved game version 5, [`crate::game::SAVED_GAME_VERSION`]):
//! magic `EVOSAV5\0`, the length of the compressed game JSON (u64
//! little-endian), the game JSON deflated, then the world snapshot bytes
//! with their own versioned header (`evo_sim::world::SnapshotHeader`: format
//! 11 keeps the map's tiles apart from the rest of the world, as deflated
//! byte planes, about 40% smaller than before). Version 4 saved games
//! (magic `EVOSAVE\0`, the game JSON uncompressed, world snapshot format
//! 10) still restore.
//!
//! The world snapshot leaves out the static terrain (biomes, relief,
//! rivers: most of a large map) and names it by content hash; the store
//! keeps it once per world ([`TerrainBlob`], `Store::put_terrain`), keyed by
//! the saved game's world id and that hash. [`terrain_key`] reads both from
//! a saved game, and [`decode`] needs the terrain back. Saved games of
//! another world snapshot version (before 10) don't restore (a new world is
//! generated). The terrain is generated from the seed, but it stays stored
//! in full rather than as a delta against a regenerated map: it is written
//! once per world, not with every save, and a stored copy keeps a world
//! restorable when map generation changes.
//!
//! Saving is split so the world thread never stalls on encoding: it takes a
//! [`Capture`] (a clone of the world, which shares the terrain and the
//! genomes, and the game state: about 10 ms at 26k agents), and the
//! background saver (`crate::saver`) encodes it (bincode + LZ4, the tiles
//! deflated on a second thread: about 100 ms at 26k agents) and hands the
//! bytes to the store task.

use std::borrow::Cow;
use std::io::{Read, Write};
use std::sync::Arc;

use evo_sim::terrain::Terrain;
use evo_sim::world::{SnapshotError, SnapshotHeader};
use evo_sim::World;

use crate::game::{Game, SavedGame, TerrainBlob};

/// Saved game version 5 on: the game JSON deflated.
const MAGIC: &[u8; 8] = b"EVOSAV5\0";
/// Saved game version 4 and before: the game JSON as is.
const MAGIC_V4: &[u8; 8] = b"EVOSAVE\0";
/// Largest game JSON inflated (far above any real one), so a corrupt saved
/// game can't take all memory.
const MAX_GAME_JSON: u64 = 1 << 30;

/// Snapshot the world and the game around it (on the calling thread). The
/// terrain it names is [`terrain_blob`] of the same world.
pub fn encode(game: &Game) -> Vec<u8> {
    encode_parts(&game.world, &game.saved())
}

/// The world's static terrain, encoded for the store under `world_id`.
pub fn terrain_blob(world: &World, world_id: &str) -> TerrainBlob {
    let terrain = world.map.terrain();
    let bytes = terrain.encode();
    TerrainBlob { world_id: world_id.to_string(), hash: terrain.hash(), bytes }
}

/// A copy of everything a saved game holds, taken between ticks, to encode
/// on another thread.
pub struct Capture {
    world: World,
    saved: SavedGame,
}

impl Capture {
    /// Copy the world and the game state around it. This is all a save
    /// costs the world thread; the static terrain is shared, not copied.
    pub fn new(game: &Game) -> Capture {
        Capture { world: game.world.clone(), saved: game.saved() }
    }

    pub fn tick(&self) -> u64 {
        self.world.tick()
    }

    pub fn day(&self) -> u64 {
        self.world.day()
    }

    pub fn world_id(&self) -> &str {
        self.saved.world_id()
    }

    /// The terrain the saved game names.
    pub fn terrain(&self) -> &Arc<Terrain> {
        self.world.map.terrain()
    }

    /// The terrain, encoded for the store.
    pub fn terrain_blob(&self) -> TerrainBlob {
        terrain_blob(&self.world, self.world_id())
    }

    /// The saved-game bytes, identical to [`encode`] of the game when it
    /// was captured.
    pub fn encode(&self) -> Vec<u8> {
        encode_parts(&self.world, &self.saved)
    }
}

fn encode_parts(world: &World, saved: &SavedGame) -> Vec<u8> {
    let world = world.snapshot();
    let json = serde_json::to_vec(saved).expect("saved game serializes");
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&json).expect("deflate into memory");
    let saved = e.finish().expect("deflate into memory");
    let mut out = Vec::with_capacity(16 + saved.len() + world.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(saved.len() as u64).to_le_bytes());
    out.extend_from_slice(&saved);
    out.extend_from_slice(&world);
    out
}

#[derive(Debug)]
pub enum DecodeError {
    /// Not a saved game, or cut short.
    BadHeader,
    World(SnapshotError),
    Game(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::BadHeader => write!(f, "not a saved game (bad or truncated header)"),
            DecodeError::World(e) => write!(f, "{e}"),
            DecodeError::Game(e) => write!(f, "saved game state does not decode: {e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The game JSON part (still compressed in version 5) and the world
/// snapshot of a saved game.
fn split(bytes: &[u8]) -> Result<(&[u8], &[u8]), DecodeError> {
    if bytes.len() < 16 || (&bytes[..8] != MAGIC && &bytes[..8] != MAGIC_V4) {
        return Err(DecodeError::BadHeader);
    }
    let len = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let len = usize::try_from(len).map_err(|_| DecodeError::BadHeader)?;
    let rest = &bytes[16..];
    if rest.len() < len {
        return Err(DecodeError::BadHeader);
    }
    Ok(rest.split_at(len))
}

/// The world snapshot's header, without decoding anything.
pub fn header(bytes: &[u8]) -> Result<SnapshotHeader, DecodeError> {
    let (_, world) = split(bytes)?;
    SnapshotHeader::read(world).map_err(DecodeError::World)
}

/// The game JSON of a saved game, inflated if it is deflated.
fn game_json(bytes: &[u8]) -> Result<Cow<'_, [u8]>, DecodeError> {
    let (saved, _) = split(bytes)?;
    if &bytes[..8] == MAGIC_V4 {
        return Ok(saved.into());
    }
    let mut json = Vec::new();
    flate2::read::DeflateDecoder::new(saved)
        .take(MAX_GAME_JSON)
        .read_to_end(&mut json)
        .map_err(|e| DecodeError::Game(e.to_string()))?;
    Ok(json.into())
}

/// Only the game-side state of a saved game, without restoring the world
/// (cheap: `--fresh` reads the discarded world's open agreements from it).
pub fn decode_game(bytes: &[u8]) -> Result<SavedGame, DecodeError> {
    serde_json::from_slice(&game_json(bytes)?).map_err(|e| DecodeError::Game(e.to_string()))
}

/// The stored terrain a saved game needs, as `(world id, content hash)`;
/// `None` for one of a world snapshot format version that doesn't restore,
/// which [`decode`] refuses anyway (before version 6 the header names no
/// terrain).
pub fn terrain_key(bytes: &[u8]) -> Result<Option<(String, u64)>, DecodeError> {
    let h = header(bytes)?;
    let Some(hash) = h.terrain.filter(|_| h.restorable()) else { return Ok(None) };
    Ok(Some((decode_game(bytes)?.world_id().to_string(), hash)))
}

/// Restore a saved game with the terrain it names ([`terrain_key`]). Fails
/// cleanly (`SnapshotError::MissingTerrain`) without it.
pub fn decode(bytes: &[u8], terrain: Option<Arc<Terrain>>) -> Result<(World, SavedGame), DecodeError> {
    let (_, world) = split(bytes)?;
    let saved = decode_game(bytes)?;
    let world = World::restore(world, terrain).map_err(DecodeError::World)?;
    Ok((world, saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{build_world, GameConfig, WorldSettings, SAVED_GAME_VERSION};

    fn version(saved: &SavedGame) -> u64 {
        serde_json::to_value(saved).unwrap()["version"].as_u64().unwrap()
    }

    /// A version 4 saved game, written by the build before version 5 (the
    /// world of seed 5, 32×32, with the wild ecosystem, 30 ticks in), and
    /// its stored terrain still restore, to exactly the world it saved; saved
    /// again, it is a smaller version 5 saved game that restores too.
    #[test]
    fn version_4_saved_games_still_load() {
        let bytes: &[u8] = include_bytes!("../tests/fixtures/save-v4.evosave");
        let terrain = Terrain::decode(include_bytes!("../tests/fixtures/save-v4.terrain")).unwrap();
        assert_eq!(&bytes[..8], MAGIC_V4);
        assert_eq!(header(bytes).unwrap().version, 10);
        let (world_id, hash) = terrain_key(bytes).unwrap().expect("a restorable saved game");
        assert_eq!(hash, terrain.hash());
        assert_eq!(version(&decode_game(bytes).unwrap()), 4);
        let (world, saved) = decode(bytes, Some(Arc::new(terrain))).unwrap();
        assert_eq!(saved.world_id(), world_id);

        // Not compared with a world rebuilt and stepped by this build: that
        // ties the fixture to every later balance change and to the
        // platform's float math (it failed on CI's runners). The world must
        // come back whole, with its organisms, and survive the re-save.
        assert_eq!(world.tick(), 30);
        assert_eq!(world.map.terrain().hash(), hash);
        assert!(world.alive_count() > 0, "the restored world has no organisms");

        let game = Game::resume(world, GameConfig::default(), saved, Vec::new());
        let again = encode(&game);
        assert_eq!(&again[..8], MAGIC);
        assert!(again.len() < bytes.len(), "{} bytes, not under {}", again.len(), bytes.len());
        let (world, saved) = decode(&again, Some(game.world.map.terrain().clone())).unwrap();
        assert_eq!((version(&saved), saved.world_id()), (u64::from(SAVED_GAME_VERSION), world_id.as_str()));
        assert!(world.snapshot() == game.world.snapshot());
    }

    /// A version 5 saved game round-trips, its game state readable alone;
    /// one cut short or of another kind is refused.
    #[test]
    fn saved_games_round_trip() {
        let settings = WorldSettings { seed: 6, width: 32, height: 32, ..Default::default() };
        let mut game = Game::new(build_world(&settings), GameConfig::default());
        game.world.step();
        let bytes = encode(&game);
        assert_eq!(&bytes[..8], MAGIC);
        let json = |s: &SavedGame| serde_json::to_string(s).unwrap();
        assert_eq!(json(&decode_game(&bytes).unwrap()), json(&game.saved()));
        let (world, _) = decode(&bytes, Some(game.world.map.terrain().clone())).unwrap();
        assert!(world.snapshot() == game.world.snapshot());
        assert!(matches!(decode_game(&bytes[..20]), Err(DecodeError::BadHeader)));
        let mut other = bytes.clone();
        other[..8].copy_from_slice(b"EVOSAVX\0");
        assert!(matches!(decode_game(&other), Err(DecodeError::BadHeader)));
    }
}
