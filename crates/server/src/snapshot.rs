//! Saved games: a world snapshot (`World::snapshot`) plus the game-side
//! state around it ([`SavedGame`]), as one blob for the store.
//!
//! Layout: magic `EVOSAVE\0`, the length of the game JSON (u64
//! little-endian), the game JSON, then the world snapshot bytes with their
//! own versioned header (`evo_sim::world::SnapshotHeader`).
//!
//! The world snapshot leaves out the static terrain (biomes, relief,
//! rivers: most of a large map) and names it by content hash; the store
//! keeps it once per world ([`TerrainBlob`], `Store::put_terrain`), keyed by
//! the saved game's world id and that hash. [`terrain_key`] reads both from
//! a saved game, and [`decode`] needs the terrain back. Saved games from
//! before world snapshot version 6 don't restore (a new world is generated,
//! as for any other format version).
//!
//! Saving is split so the world thread never stalls on encoding: it takes a
//! [`Capture`] (a clone of the world, which shares the terrain, and the
//! game state: tens of ms on a large world), and the background saver
//! (`crate::saver`) encodes it (bincode + LZ4, hundreds of ms) and hands the
//! bytes to the store task.

use std::sync::Arc;

use evo_sim::terrain::Terrain;
use evo_sim::world::{SnapshotError, SnapshotHeader, SNAPSHOT_VERSION};
use evo_sim::World;

use crate::game::{Game, SavedGame, TerrainBlob};

const MAGIC: &[u8; 8] = b"EVOSAVE\0";

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
    let saved = serde_json::to_vec(saved).expect("saved game serializes");
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

fn split(bytes: &[u8]) -> Result<(&[u8], &[u8]), DecodeError> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
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

/// Only the game-side state of a saved game, without restoring the world
/// (cheap: `--fresh` reads the discarded world's open agreements from it).
pub fn decode_game(bytes: &[u8]) -> Result<SavedGame, DecodeError> {
    let (saved, _) = split(bytes)?;
    serde_json::from_slice(saved).map_err(|e| DecodeError::Game(e.to_string()))
}

/// The stored terrain a saved game needs, as `(world id, content hash)`;
/// `None` for one of another format version, which [`decode`] refuses
/// anyway (before version 6 the header names no terrain).
pub fn terrain_key(bytes: &[u8]) -> Result<Option<(String, u64)>, DecodeError> {
    let h = header(bytes)?;
    let Some(hash) = h.terrain.filter(|_| h.version == SNAPSHOT_VERSION) else { return Ok(None) };
    Ok(Some((decode_game(bytes)?.world_id().to_string(), hash)))
}

/// Restore a saved game with the terrain it names ([`terrain_key`]). Fails
/// cleanly (`SnapshotError::MissingTerrain`) without it.
pub fn decode(bytes: &[u8], terrain: Option<Arc<Terrain>>) -> Result<(World, SavedGame), DecodeError> {
    let (saved, world) = split(bytes)?;
    let saved: SavedGame = serde_json::from_slice(saved).map_err(|e| DecodeError::Game(e.to_string()))?;
    let world = World::restore(world, terrain).map_err(DecodeError::World)?;
    Ok((world, saved))
}
