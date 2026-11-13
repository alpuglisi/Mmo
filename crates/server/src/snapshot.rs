//! Saved games: a world snapshot (`World::snapshot`) plus the game-side
//! state around it ([`SavedGame`]), as one blob for the store.
//!
//! Layout: magic `EVOSAVE\0`, the length of the game JSON (u64
//! little-endian), the game JSON, then the world snapshot bytes with their
//! own versioned header (`evo_sim::world::SnapshotHeader`).
//!
//! Saving is split so the world thread never stalls on encoding: it takes a
//! [`Capture`] (a clone of the world and the game state, tens of ms on a
//! large world), and the background saver (`crate::saver`) encodes it
//! (bincode + LZ4, hundreds of ms) and hands the bytes to the store task.

use evo_sim::world::{SnapshotError, SnapshotHeader};
use evo_sim::World;

use crate::game::{Game, SavedGame};

const MAGIC: &[u8; 8] = b"EVOSAVE\0";

/// Snapshot the world and the game around it (on the calling thread).
pub fn encode(game: &Game) -> Vec<u8> {
    encode_parts(&game.world, &game.saved())
}

/// A copy of everything a saved game holds, taken between ticks, to encode
/// on another thread.
pub struct Capture {
    world: World,
    saved: SavedGame,
}

impl Capture {
    /// Copy the world and the game state around it. This is all a save
    /// costs the world thread.
    pub fn new(game: &Game) -> Capture {
        Capture { world: game.world.clone(), saved: game.saved() }
    }

    pub fn tick(&self) -> u64 {
        self.world.tick()
    }

    pub fn day(&self) -> u64 {
        self.world.day()
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

pub fn decode(bytes: &[u8]) -> Result<(World, SavedGame), DecodeError> {
    let (saved, world) = split(bytes)?;
    let saved: SavedGame = serde_json::from_slice(saved).map_err(|e| DecodeError::Game(e.to_string()))?;
    let world = World::restore(world).map_err(DecodeError::World)?;
    Ok((world, saved))
}
