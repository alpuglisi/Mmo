//! World snapshots (feature `serde`): the whole simulation state as bytes,
//! so a server can stop and carry on exactly where it was.
//!
//! All randomness is derived from `(seed, stream, tick, id)`, so the state
//! plus the tick fully determine the future: a restored world steps exactly
//! like the original (`tests::restored_world_continues_exactly`).
//!
//! Layout (integers little-endian):
//!
//! | bytes | field |
//! | --- | --- |
//! | 8 | magic `EVOSNAP\0` |
//! | 4 | format version ([`SNAPSHOT_VERSION`]) |
//! | 4 | flags (bit 0: payload is LZ4-compressed) |
//! | 8 | config hash ([`WorldConfig::snapshot_hash`]) |
//! | 8 | tick |
//! | 8 | organisms (agents, dead ones not yet removed included) |
//! | 8 | payload length before compression |
//! | 8 | FNV-1a 64 checksum of the stored payload |
//! | … | payload: `bincode` of `World`, LZ4 block-compressed |
//!
//! The header stays readable across format versions, so a server can say
//! why it refuses a snapshot. Skipped and rebuilt on restore: the id → index
//! map, the spatial grid (rebuilt every tick anyway), A* scratch buffers and
//! phase timings. Everything else, caches that steer the future included
//! (hourly density, cohort representatives, LOD passability, cache and
//! clutch lookups), is stored.

use std::fmt;

use super::spatial::SpatialGrid;
use super::World;
use crate::config::WorldConfig;

/// Bump on any change to the serialized state (a field added, removed,
/// reordered or retyped in any type the world holds).
pub const SNAPSHOT_VERSION: u32 = 3;

const MAGIC: &[u8; 8] = b"EVOSNAP\0";
const HEADER_LEN: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8 + 8;
const FLAG_LZ4: u32 = 1;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a snapshot, or cut short.
    BadHeader,
    /// Written by another format version; it can't be read by this build.
    Version { found: u32, expected: u32 },
    /// The payload doesn't match its checksum (corrupt or truncated file).
    Checksum,
    /// The payload doesn't decode (or its config doesn't match the header).
    Decode(String),
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
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return Err(SnapshotError::BadHeader);
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        Ok(SnapshotHeader {
            version: u32_at(8),
            flags: u32_at(12),
            config_hash: u64_at(16),
            tick: u64_at(24),
            organisms: u64_at(32),
            raw_len: u64_at(40),
            checksum: u64_at(48),
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        for v in [self.config_hash, self.tick, self.organisms, self.raw_len, self.checksum] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

impl World {
    /// The whole simulation state as bytes (see the module docs for the
    /// format). [`World::restore`] continues it exactly.
    pub fn snapshot(&self) -> Vec<u8> {
        let raw = bincode::serialize(self).expect("world serializes");
        let payload = lz4_flex::block::compress(&raw);
        let header = SnapshotHeader {
            version: SNAPSHOT_VERSION,
            flags: FLAG_LZ4,
            config_hash: self.config.snapshot_hash(),
            tick: self.tick,
            organisms: self.organisms.len() as u64,
            raw_len: raw.len() as u64,
            checksum: fnv1a(&payload),
        };
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        header.write(&mut out);
        out.extend_from_slice(&payload);
        out
    }

    /// A world from [`World::snapshot`] bytes, with its derived lookups
    /// rebuilt. Fails on another format version or a corrupt payload.
    pub fn restore(bytes: &[u8]) -> Result<World, SnapshotError> {
        let h = SnapshotHeader::read(bytes)?;
        if h.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version { found: h.version, expected: SNAPSHOT_VERSION });
        }
        let payload = &bytes[HEADER_LEN..];
        if fnv1a(payload) != h.checksum {
            return Err(SnapshotError::Checksum);
        }
        let decompressed;
        let raw = if h.flags & FLAG_LZ4 != 0 {
            let len = usize::try_from(h.raw_len).map_err(|_| SnapshotError::BadHeader)?;
            decompressed = lz4_flex::block::decompress(payload, len).map_err(|e| SnapshotError::Decode(e.to_string()))?;
            &decompressed[..]
        } else {
            payload
        };
        let mut w: World = bincode::deserialize(raw).map_err(|e| SnapshotError::Decode(e.to_string()))?;
        if w.config.snapshot_hash() != h.config_hash || w.tick != h.tick {
            return Err(SnapshotError::Decode("payload does not match its header".into()));
        }
        w.rebuild_derived();
        Ok(w)
    }

    /// Rebuild what snapshots skip.
    fn rebuild_derived(&mut self) {
        self.index = self.organisms.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
        self.grid = SpatialGrid::new(self.config.width, self.config.height, super::GRID_CELL);
        self.grid.rebuild(&self.organisms);
    }
}

#[cfg(test)]
mod tests;
