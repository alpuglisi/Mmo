//! The compact state encoding (`/ws?proto=2`): binary delta states.
//!
//! Protocol 1 (the default, and what older clients get) sends every `state`
//! as a whole JSON [`StateView`]. Protocol 2 sends states as binary
//! WebSocket messages that carry only what changed since the state this
//! connection last *sent* (every other message is the same JSON as in
//! protocol 1). The socket writer encodes each state when it sends it, so a
//! state replaced in the connection's one-slot queue (`push.rs`) is simply
//! never encoded and the next delta is computed against what the client
//! actually has: nothing is lost.
//!
//! # Frame
//!
//! All integers are LEB128 varints (`v`), signed ones zigzag-encoded (`z`).
//!
//! ```text
//! u8   kind          1 keyframe (start from nothing), 2 delta
//! v    seq           this state's number on the connection (1, 2, ...)
//! v    base          delta: the seq it applies to (0 in a keyframe)
//! v    meta length, then that many bytes of UTF-8 JSON (see below)
//! own section, then nearby section, each:
//!   v  removed count, then that many ids, ascending, each as the
//!      difference from the previous one (the first from 0)
//!   v  upsert count, then per record: id (difference from the previous
//!      upserted id, ascending), v field mask, the masked fields
//! ```
//!
//! A record's fields are written against the client's copy of the record
//! (all zero / absent for a new id): positions, needs, age, growth and
//! health as zigzag differences of their quantized values, everything else
//! as the new value. Quantization: positions and `order` in 1/64 tile,
//! needs in 0.1, health in 0.01, age in 0.01 day, growth in 0.001;
//! `speed` is the exact `f32`. `action` and `look` are indices into
//! per-connection palettes that the meta JSON extends (`actions`, `looks`),
//! so a look costs a byte, once per organism.
//!
//! Own fields (bit: value): 0 lineage, 1 position (`z x, z y`), 2 action,
//! 3 flags (`u8`: 1 male, 2 matured, 4 possessed, 8 pregnant), 4 age,
//! 5 growth, 6 health, 7–14 needs (hunger, thirst, energy, cold, heat,
//! safety, breeding, social), 15 generation, 16 look, 17 order (`u8` 0 or
//! 1, then `z x, z y` absolute), 18 speed (`u8` 0 or 1, then `f32` LE),
//! 19 group (`v`).
//! Nearby fields: 0 owner, 1 lineage, 2 position, 3 action, 4 flags (1 male,
//! 2 matured), 5 look.
//!
//! The meta JSON holds the other [`StateView`] fields that changed, under
//! their own names (`clock`, `weather`, `player`, `lineage`, `slots_left`,
//! `population`, `migration`, `groups` (the whole list when any of it
//! changed), `view`, `balance`, `escrow`), `leaderboard`
//! as `{day, breeding_adults}` with its rows keyed by player (`lb_set`:
//! new or changed rows, `lb_del`: players gone), offers keyed by id
//! (`offers_set`, `offers_del`) and the palette additions (`looks`,
//! `actions`, appended in order). A keyframe has every field.
//!
//! Decoded lists are in id order (`organisms`, `nearby`, `offers`); the
//! leaderboard in rank order (share, then player), as the server sends it.
//!
//! A client that loses track (a delta whose `base` isn't the seq it has, or
//! a frame it can't read) drops states until the next keyframe and sends
//! `{"type":"resync"}`: the connection's next state is then a keyframe. The
//! writer also sends one every [`KEYFRAME_EVERY`].

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::time::Duration;

use serde::{Deserialize, Deserializer};

use crate::appearance::Appearance;
use crate::protocol::{
    ClockView, GroupView, LeaderRow, Leaderboard, NearbyOrganism, NeedsView, OfferView, OrgId, OwnOrganism, StateView,
    ViewWindow, WeatherView,
};
use evo_sim::PlayerId;

/// Whole JSON states (older clients).
pub const PROTO_JSON: u32 = 1;
/// Binary delta states.
pub const PROTO_DELTA: u32 = 2;

pub const KEYFRAME: u8 = 1;
pub const DELTA: u8 = 2;

/// A keyframe at least this often, even when the client never asks.
pub const KEYFRAME_EVERY: Duration = Duration::from_secs(60);

const POS: f64 = 64.0;
const NEED: f64 = 10.0;
const HEALTH: f64 = 100.0;
const AGE: f64 = 100.0;
const GROWTH: f64 = 1000.0;

/// Quantize (rounded; NaN is 0, out of range saturates).
fn q(v: f32, scale: f64) -> i64 {
    (v as f64 * scale).round() as i64
}

fn dq(q: i64, scale: f64) -> f32 {
    (q as f64 / scale) as f32
}

/// What a value looks like after a round trip through protocol 2.
fn rt(v: f32, scale: f64) -> f32 {
    dq(q(v, scale), scale)
}

const OWN_LINEAGE: u32 = 1 << 0;
const OWN_POS: u32 = 1 << 1;
const OWN_ACTION: u32 = 1 << 2;
const OWN_FLAGS: u32 = 1 << 3;
const OWN_AGE: u32 = 1 << 4;
const OWN_GROWTH: u32 = 1 << 5;
const OWN_HEALTH: u32 = 1 << 6;
const OWN_NEED0: u32 = 7;
const OWN_GENERATION: u32 = 1 << 15;
const OWN_LOOK: u32 = 1 << 16;
const OWN_ORDER: u32 = 1 << 17;
const OWN_SPEED: u32 = 1 << 18;
const OWN_GROUP: u32 = 1 << 19;

const NEAR_OWNER: u32 = 1 << 0;
const NEAR_LINEAGE: u32 = 1 << 1;
const NEAR_POS: u32 = 1 << 2;
const NEAR_ACTION: u32 = 1 << 3;
const NEAR_FLAGS: u32 = 1 << 4;
const NEAR_LOOK: u32 = 1 << 5;

const MALE: u8 = 1;
const MATURED: u8 = 2;
const POSSESSED: u8 = 4;
const PREGNANT: u8 = 8;

/// An own organism as the client holds it (quantized; palette indices).
#[derive(Clone, Debug, Default, PartialEq)]
struct OwnQ {
    lineage: u32,
    pos: [i64; 2],
    action: u32,
    flags: u8,
    age: i64,
    growth: i64,
    health: i64,
    needs: [i64; 8],
    generation: u32,
    look: u32,
    order: Option<[i64; 2]>,
    speed: Option<u32>,
    group: u8,
}

/// A foreign organism as the client holds it.
#[derive(Clone, Debug, Default, PartialEq)]
struct NearQ {
    owner: PlayerId,
    lineage: u32,
    pos: [i64; 2],
    action: u32,
    flags: u8,
    look: u32,
}

fn needs_arr(n: &NeedsView) -> [f32; 8] {
    [n.hunger, n.thirst, n.energy, n.cold, n.heat, n.safety, n.breeding, n.social]
}

fn needs_view(a: [f32; 8]) -> NeedsView {
    NeedsView {
        hunger: a[0],
        thirst: a[1],
        energy: a[2],
        cold: a[3],
        heat: a[4],
        safety: a[5],
        breeding: a[6],
        social: a[7],
    }
}

fn sex_flag(sex: &str) -> u8 {
    if sex == "male" {
        MALE
    } else {
        0
    }
}

fn sex_of(flags: u8) -> String {
    if flags & MALE != 0 { "male" } else { "female" }.into()
}

// ------------------------------------------------------------------ bytes

struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, b: u8) {
        self.0.push(b);
    }

    fn v(&mut self, mut x: u64) {
        while x >= 0x80 {
            self.0.push((x as u8) | 0x80);
            x >>= 7;
        }
        self.0.push(x as u8);
    }

    fn z(&mut self, x: i64) {
        self.v(((x << 1) ^ (x >> 63)) as u64);
    }

    fn f32(&mut self, bits: u32) {
        self.0.extend_from_slice(&bits.to_le_bytes());
    }
}

struct In<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    fn u8(&mut self) -> Result<u8, DecodeError> {
        let b = *self.b.get(self.at).ok_or(DecodeError::Malformed("truncated"))?;
        self.at += 1;
        Ok(b)
    }

    fn v(&mut self) -> Result<u64, DecodeError> {
        let mut x = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            x |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(x);
            }
        }
        Err(DecodeError::Malformed("varint too long"))
    }

    fn v32(&mut self) -> Result<u32, DecodeError> {
        u32::try_from(self.v()?).map_err(|_| DecodeError::Malformed("value out of range"))
    }

    fn z(&mut self) -> Result<i64, DecodeError> {
        let u = self.v()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn f32(&mut self) -> Result<u32, DecodeError> {
        let s = self.b.get(self.at..self.at + 4).ok_or(DecodeError::Malformed("truncated"))?;
        self.at += 4;
        Ok(u32::from_le_bytes(s.try_into().unwrap()))
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let s = self.b.get(self.at..self.at.saturating_add(n)).ok_or(DecodeError::Malformed("truncated"))?;
        self.at += n;
        Ok(s)
    }
}

/// A fast hash for the encoder's maps (server-made keys: organism ids,
/// players, offers, palette values), after rustc's FxHasher.
#[derive(Default, Clone, Copy)]
struct Fx(u64);

impl Fx {
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        for &b in chunks.remainder() {
            self.add(b as u64);
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

// ------------------------------------------------------------------ palettes

/// Values the client knows by index; additions go out in the meta JSON.
struct Palette<K> {
    index: FxMap<K, u32>,
    added: Vec<K>,
}

impl<K: std::hash::Hash + Eq + Clone> Palette<K> {
    fn new() -> Self {
        Palette { index: FxMap::default(), added: Vec::new() }
    }

    fn get(&mut self, k: &K) -> u32 {
        if let Some(&i) = self.index.get(k) {
            return i;
        }
        let i = self.index.len() as u32;
        self.index.insert(k.clone(), i);
        self.added.push(k.clone());
        i
    }
}

// ------------------------------------------------------------------ encoder

/// The meta fields as the client last got them (`None`: never sent).
#[derive(Default)]
struct Sent {
    clock: Option<ClockView>,
    weather: Option<WeatherView>,
    player: Option<PlayerId>,
    lineage: Option<Option<u32>>,
    slots_left: Option<u32>,
    population: Option<usize>,
    migration: Option<Option<[f32; 2]>>,
    groups: Option<Vec<GroupView>>,
    view: Option<ViewWindow>,
    balance: Option<u64>,
    escrow: Option<u64>,
    lb_head: Option<(Option<u64>, usize)>,
}

/// One connection's encoder: what that client was last sent. Values are
/// compared as they are and serialized only when they changed, so a
/// sub-step state (where almost nothing did) costs little.
pub struct DeltaEncoder {
    seq: u64,
    own: FxMap<OrgId, (OwnQ, u64)>,
    near: FxMap<OrgId, (NearQ, u64)>,
    sent: Sent,
    lb_rows: FxMap<PlayerId, (LeaderRow, u64)>,
    offers: FxMap<u64, (OfferView, u64)>,
    looks: Palette<Appearance>,
    actions: Palette<String>,
}

impl Default for DeltaEncoder {
    fn default() -> Self {
        Self::new()
    }
}

fn json<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).expect("wire types serialize")
}

/// The meta JSON object, built from already serialized values.
struct Meta(String);

impl Meta {
    fn field(&mut self, key: &str, value: &str) {
        self.0.push(if self.0.is_empty() { '{' } else { ',' });
        self.0.push('"');
        self.0.push_str(key);
        self.0.push_str("\":");
        self.0.push_str(value);
    }

    fn list<'a>(&mut self, key: &str, items: impl IntoIterator<Item = &'a str>) {
        let mut s = String::from("[");
        for (i, it) in items.into_iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(it);
        }
        s.push(']');
        if s.len() > 2 {
            self.field(key, &s);
        }
    }

    fn finish(mut self) -> String {
        if !self.0.is_empty() {
            self.0.push('}');
        }
        self.0
    }
}

/// Fields of `cur` that differ from `prev`, written after its mask.
trait Record: Default + PartialEq {
    fn mask(&self, prev: &Self) -> u32;
    fn write(&self, prev: &Self, mask: u32, out: &mut Out);
}

impl Record for OwnQ {
    fn mask(&self, p: &Self) -> u32 {
        let mut m = 0;
        let mut set = |bit: u32, changed: bool| {
            if changed {
                m |= bit;
            }
        };
        set(OWN_LINEAGE, self.lineage != p.lineage);
        set(OWN_POS, self.pos != p.pos);
        set(OWN_ACTION, self.action != p.action);
        set(OWN_FLAGS, self.flags != p.flags);
        set(OWN_AGE, self.age != p.age);
        set(OWN_GROWTH, self.growth != p.growth);
        set(OWN_HEALTH, self.health != p.health);
        for i in 0..8 {
            set(1 << (OWN_NEED0 + i as u32), self.needs[i] != p.needs[i]);
        }
        set(OWN_GENERATION, self.generation != p.generation);
        set(OWN_LOOK, self.look != p.look);
        set(OWN_ORDER, self.order != p.order);
        set(OWN_SPEED, self.speed != p.speed);
        set(OWN_GROUP, self.group != p.group);
        m
    }

    fn write(&self, p: &Self, m: u32, out: &mut Out) {
        if m & OWN_LINEAGE != 0 {
            out.v(self.lineage as u64);
        }
        if m & OWN_POS != 0 {
            out.z(self.pos[0] - p.pos[0]);
            out.z(self.pos[1] - p.pos[1]);
        }
        if m & OWN_ACTION != 0 {
            out.v(self.action as u64);
        }
        if m & OWN_FLAGS != 0 {
            out.u8(self.flags);
        }
        if m & OWN_AGE != 0 {
            out.z(self.age - p.age);
        }
        if m & OWN_GROWTH != 0 {
            out.z(self.growth - p.growth);
        }
        if m & OWN_HEALTH != 0 {
            out.z(self.health - p.health);
        }
        for i in 0..8 {
            if m & (1 << (OWN_NEED0 + i as u32)) != 0 {
                out.z(self.needs[i] - p.needs[i]);
            }
        }
        if m & OWN_GENERATION != 0 {
            out.v(self.generation as u64);
        }
        if m & OWN_LOOK != 0 {
            out.v(self.look as u64);
        }
        if m & OWN_ORDER != 0 {
            match self.order {
                Some([x, y]) => {
                    out.u8(1);
                    out.z(x);
                    out.z(y);
                }
                None => out.u8(0),
            }
        }
        if m & OWN_SPEED != 0 {
            match self.speed {
                Some(bits) => {
                    out.u8(1);
                    out.f32(bits);
                }
                None => out.u8(0),
            }
        }
        if m & OWN_GROUP != 0 {
            out.v(self.group as u64);
        }
    }
}

impl Record for NearQ {
    fn mask(&self, p: &Self) -> u32 {
        let mut m = 0;
        if self.owner != p.owner {
            m |= NEAR_OWNER;
        }
        if self.lineage != p.lineage {
            m |= NEAR_LINEAGE;
        }
        if self.pos != p.pos {
            m |= NEAR_POS;
        }
        if self.action != p.action {
            m |= NEAR_ACTION;
        }
        if self.flags != p.flags {
            m |= NEAR_FLAGS;
        }
        if self.look != p.look {
            m |= NEAR_LOOK;
        }
        m
    }

    fn write(&self, p: &Self, m: u32, out: &mut Out) {
        if m & NEAR_OWNER != 0 {
            out.v(self.owner as u64);
        }
        if m & NEAR_LINEAGE != 0 {
            out.v(self.lineage as u64);
        }
        if m & NEAR_POS != 0 {
            out.z(self.pos[0] - p.pos[0]);
            out.z(self.pos[1] - p.pos[1]);
        }
        if m & NEAR_ACTION != 0 {
            out.v(self.action as u64);
        }
        if m & NEAR_FLAGS != 0 {
            out.u8(self.flags);
        }
        if m & NEAR_LOOK != 0 {
            out.v(self.look as u64);
        }
    }
}

/// Write one section (removals, then new and changed records) and make
/// `cur` (sorted by id) what the client holds. Records are kept with the
/// seq that last saw them, so the ones gone are those not seen now.
fn section<R: Record>(last: &mut FxMap<OrgId, (R, u64)>, cur: Vec<(OrgId, R)>, seq: u64, out: &mut Out) {
    let seen = cur.len();
    let mut upserts = Out(Vec::with_capacity(seen * 4));
    let mut n = 0u64;
    let mut prev_id = 0;
    let default = R::default();
    let mut put = |id: OrgId, m: u32, rec: &R, old: &R| {
        upserts.v(id - prev_id);
        upserts.v(m as u64);
        rec.write(old, m, &mut upserts);
        prev_id = id;
        n += 1;
    };
    for (id, rec) in cur {
        match last.entry(id) {
            Entry::Occupied(mut e) => {
                let (old, at) = e.get_mut();
                *at = seq;
                let m = rec.mask(old);
                if m != 0 {
                    put(id, m, &rec, old);
                    *old = rec;
                }
            }
            // A new record goes out even if it equals the default.
            Entry::Vacant(e) => {
                put(id, rec.mask(&default), &rec, &default);
                e.insert((rec, seq));
            }
        }
    }
    let mut gone = Vec::new();
    if last.len() > seen {
        last.retain(|&id, (_, at)| {
            let keep = *at == seq;
            if !keep {
                gone.push(id);
            }
            keep
        });
        gone.sort_unstable();
    }
    out.v(gone.len() as u64);
    let mut prev = 0;
    for id in gone {
        out.v(id - prev);
        prev = id;
    }
    out.v(n);
    out.0.extend_from_slice(&upserts.0);
}

impl DeltaEncoder {
    pub fn new() -> Self {
        DeltaEncoder {
            seq: 0,
            own: FxMap::default(),
            near: FxMap::default(),
            sent: Sent::default(),
            lb_rows: FxMap::default(),
            offers: FxMap::default(),
            looks: Palette::new(),
            actions: Palette::new(),
        }
    }

    /// The seq of the last state encoded (0: none yet).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Encode `s` as a delta against the last state encoded, or as a
    /// keyframe (always the first). The result is what the client has next.
    pub fn encode(&mut self, s: &StateView, keyframe: bool) -> Vec<u8> {
        let keyframe = keyframe || self.seq == 0;
        if keyframe {
            let seq = self.seq;
            *self = DeltaEncoder::new();
            self.seq = seq;
        }
        let base = self.seq;
        self.seq += 1;

        let mut own: Vec<(OrgId, OwnQ)> = s.organisms.iter().map(|o| (o.id, self.own_q(o))).collect();
        own.sort_unstable_by_key(|r| r.0);
        let mut near: Vec<(OrgId, NearQ)> = s.nearby.iter().map(|o| (o.id, self.near_q(o))).collect();
        near.sort_unstable_by_key(|r| r.0);

        let mut meta = Meta(String::new());
        let sent = &mut self.sent;
        macro_rules! scalar {
            ($field:ident) => {
                if sent.$field.as_ref() != Some(&s.$field) {
                    meta.field(stringify!($field), &json(&s.$field));
                    sent.$field = Some(s.$field.clone());
                }
            };
        }
        scalar!(clock);
        scalar!(weather);
        scalar!(player);
        scalar!(lineage);
        scalar!(slots_left);
        scalar!(population);
        scalar!(migration);
        scalar!(groups);
        scalar!(view);
        scalar!(balance);
        scalar!(escrow);
        let lb = &s.leaderboard;
        let head = (lb.day, lb.breeding_adults);
        if sent.lb_head != Some(head) {
            meta.field(
                "leaderboard",
                &json(&serde_json::json!({ "day": lb.day, "breeding_adults": lb.breeding_adults })),
            );
            sent.lb_head = Some(head);
        }
        let (set, del) = keyed(&mut self.lb_rows, lb.rows.iter().map(|r| (r.player, r)), self.seq);
        meta.list("lb_set", set.iter().map(String::as_str));
        meta.list("lb_del", del.iter().map(String::as_str));
        let (set, del) = keyed(&mut self.offers, s.offers.iter().map(|o| (o.id, o)), self.seq);
        meta.list("offers_set", set.iter().map(String::as_str));
        meta.list("offers_del", del.iter().map(String::as_str));
        let looks: Vec<String> = self.looks.added.drain(..).map(|l| json(&l)).collect();
        meta.list("looks", looks.iter().map(String::as_str));
        let actions: Vec<String> = self.actions.added.drain(..).map(|a| json(&a)).collect();
        meta.list("actions", actions.iter().map(String::as_str));
        let meta = meta.finish();

        let mut out = Out(Vec::with_capacity(64 + meta.len() + 8 * (own.len() + near.len())));
        out.u8(if keyframe { KEYFRAME } else { DELTA });
        out.v(self.seq);
        out.v(base);
        out.v(meta.len() as u64);
        out.0.extend_from_slice(meta.as_bytes());
        section(&mut self.own, own, self.seq, &mut out);
        section(&mut self.near, near, self.seq, &mut out);
        out.0
    }

    fn own_q(&mut self, o: &OwnOrganism) -> OwnQ {
        let mut flags = sex_flag(&o.sex);
        if o.matured {
            flags |= MATURED;
        }
        if o.possessed {
            flags |= POSSESSED;
        }
        if o.pregnant {
            flags |= PREGNANT;
        }
        let n = needs_arr(&o.needs);
        OwnQ {
            lineage: o.lineage,
            pos: [q(o.x, POS), q(o.y, POS)],
            action: self.actions.get(&o.action),
            flags,
            age: q(o.age_days, AGE),
            growth: q(o.growth, GROWTH),
            health: q(o.health, HEALTH),
            needs: n.map(|v| q(v, NEED)),
            generation: o.generation,
            look: self.looks.get(&o.look),
            order: o.order.map(|[x, y]| [q(x, POS), q(y, POS)]),
            speed: o.speed.map(f32::to_bits),
            group: o.group,
        }
    }

    fn near_q(&mut self, o: &NearbyOrganism) -> NearQ {
        let mut flags = sex_flag(&o.sex);
        if o.matured {
            flags |= MATURED;
        }
        NearQ {
            owner: o.owner,
            lineage: o.lineage,
            pos: [q(o.x, POS), q(o.y, POS)],
            action: self.actions.get(&o.action),
            flags,
            look: self.looks.get(&o.look),
        }
    }
}

/// Rows keyed by `K`: the new or changed ones and the keys gone (as JSON),
/// and make `cur` what the client holds (kept with the seq that saw them).
fn keyed<'a, K, V>(
    last: &mut FxMap<K, (V, u64)>,
    cur: impl Iterator<Item = (K, &'a V)>,
    seq: u64,
) -> (Vec<String>, Vec<String>)
where
    K: std::hash::Hash + Eq + Copy + serde::Serialize,
    V: PartialEq + Clone + serde::Serialize + 'a,
{
    let mut set = Vec::new();
    let mut seen = 0;
    for (k, v) in cur {
        seen += 1;
        match last.entry(k) {
            Entry::Occupied(mut e) => {
                let (old, at) = e.get_mut();
                *at = seq;
                if old != v {
                    set.push(json(v));
                    *old = v.clone();
                }
            }
            Entry::Vacant(e) => {
                set.push(json(v));
                e.insert((v.clone(), seq));
            }
        }
    }
    let mut del = Vec::new();
    if last.len() > seen {
        last.retain(|k, (_, at)| {
            let keep = *at == seq;
            if !keep {
                del.push(json(k));
            }
            keep
        });
    }
    (set, del)
}

// ------------------------------------------------------------------ decoder

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// No keyframe yet (or since the last error).
    NeedKeyframe,
    /// A delta for another state than the one held.
    BaseMismatch {
        have: u64,
        base: u64,
    },
    Malformed(&'static str),
    Meta(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::NeedKeyframe => write!(f, "waiting for a keyframe"),
            DecodeError::BaseMismatch { have, base } => write!(f, "delta on state {base}, have {have}"),
            DecodeError::Malformed(m) => write!(f, "malformed frame: {m}"),
            DecodeError::Meta(m) => write!(f, "bad meta: {m}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Present (even as `null`) vs absent.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MetaIn {
    clock: Option<ClockView>,
    weather: Option<WeatherView>,
    player: Option<PlayerId>,
    #[serde(deserialize_with = "some")]
    lineage: Option<Option<u32>>,
    slots_left: Option<u32>,
    population: Option<usize>,
    #[serde(deserialize_with = "some")]
    migration: Option<Option<[f32; 2]>>,
    groups: Option<Vec<GroupView>>,
    view: Option<ViewWindow>,
    balance: Option<u64>,
    escrow: Option<u64>,
    leaderboard: Option<LbHead>,
    lb_set: Vec<LeaderRow>,
    lb_del: Vec<PlayerId>,
    offers_set: Vec<OfferView>,
    offers_del: Vec<u64>,
    looks: Vec<crate::appearance::Appearance>,
    actions: Vec<String>,
}

#[derive(Deserialize, Clone)]
struct LbHead {
    day: Option<u64>,
    breeding_adults: usize,
}

#[derive(Clone)]
struct Head {
    clock: ClockView,
    weather: WeatherView,
    player: PlayerId,
    lineage: Option<u32>,
    slots_left: u32,
    population: usize,
    migration: Option<[f32; 2]>,
    groups: Vec<GroupView>,
    view: ViewWindow,
    balance: u64,
    escrow: u64,
    lb: LbHead,
}

/// The client side of protocol 2 (the web client has its own, in
/// `client/src/wire.ts`): rebuilds whole states from the frames. Used by
/// the load generator and the tests.
#[derive(Default)]
pub struct DeltaDecoder {
    seq: Option<u64>,
    head: Option<Head>,
    own: BTreeMap<OrgId, OwnQ>,
    near: BTreeMap<OrgId, NearQ>,
    lb: BTreeMap<PlayerId, LeaderRow>,
    offers: BTreeMap<u64, OfferView>,
    looks: Vec<Appearance>,
    actions: Vec<String>,
}

impl DeltaDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The seq of the state held (`None` until a keyframe).
    pub fn seq(&self) -> Option<u64> {
        self.seq
    }

    /// Apply one frame and return the whole state. On an error the decoder
    /// waits for a keyframe (the client should send `resync`).
    pub fn apply(&mut self, frame: &[u8]) -> Result<StateView, DecodeError> {
        let r = self.apply_inner(frame);
        if r.is_err() {
            *self = DeltaDecoder::new();
        }
        r
    }

    fn apply_inner(&mut self, frame: &[u8]) -> Result<StateView, DecodeError> {
        let mut r = In { b: frame, at: 0 };
        let kind = r.u8()?;
        let seq = r.v()?;
        let base = r.v()?;
        match kind {
            KEYFRAME => *self = DeltaDecoder::new(),
            DELTA => match self.seq {
                None => return Err(DecodeError::NeedKeyframe),
                Some(have) if have != base => return Err(DecodeError::BaseMismatch { have, base }),
                Some(_) => {}
            },
            _ => return Err(DecodeError::Malformed("unknown frame kind")),
        }
        let n = r.v()? as usize;
        let meta = r.bytes(n)?;
        let m: MetaIn = if meta.is_empty() {
            MetaIn::default()
        } else {
            serde_json::from_slice(meta).map_err(|e| DecodeError::Meta(e.to_string()))?
        };
        self.looks.extend(m.looks);
        self.actions.extend(m.actions);
        let head = match self.head.take() {
            Some(mut h) => {
                if let Some(v) = m.clock {
                    h.clock = v;
                }
                if let Some(v) = m.weather {
                    h.weather = v;
                }
                if let Some(v) = m.player {
                    h.player = v;
                }
                if let Some(v) = m.lineage {
                    h.lineage = v;
                }
                if let Some(v) = m.slots_left {
                    h.slots_left = v;
                }
                if let Some(v) = m.population {
                    h.population = v;
                }
                if let Some(v) = m.migration {
                    h.migration = v;
                }
                if let Some(v) = m.groups {
                    h.groups = v;
                }
                if let Some(v) = m.view {
                    h.view = v;
                }
                if let Some(v) = m.balance {
                    h.balance = v;
                }
                if let Some(v) = m.escrow {
                    h.escrow = v;
                }
                if let Some(v) = m.leaderboard {
                    h.lb = v;
                }
                h
            }
            None => {
                let missing = || DecodeError::Meta("a keyframe lacks a field".into());
                Head {
                    clock: m.clock.ok_or_else(missing)?,
                    weather: m.weather.ok_or_else(missing)?,
                    player: m.player.ok_or_else(missing)?,
                    lineage: m.lineage.ok_or_else(missing)?,
                    slots_left: m.slots_left.ok_or_else(missing)?,
                    population: m.population.ok_or_else(missing)?,
                    migration: m.migration.ok_or_else(missing)?,
                    groups: m.groups.ok_or_else(missing)?,
                    view: m.view.ok_or_else(missing)?,
                    balance: m.balance.ok_or_else(missing)?,
                    escrow: m.escrow.ok_or_else(missing)?,
                    lb: m.leaderboard.ok_or_else(missing)?,
                }
            }
        };
        for p in m.lb_del {
            self.lb.remove(&p);
        }
        for row in m.lb_set {
            self.lb.insert(row.player, row);
        }
        for id in m.offers_del {
            self.offers.remove(&id);
        }
        for o in m.offers_set {
            self.offers.insert(o.id, o);
        }
        read_section(&mut r, &mut self.own, read_own)?;
        read_section(&mut r, &mut self.near, read_near)?;
        if r.at != frame.len() {
            return Err(DecodeError::Malformed("trailing bytes"));
        }
        self.seq = Some(seq);
        let state = self.state(&head)?;
        self.head = Some(head);
        Ok(state)
    }

    fn look(&self, i: u32) -> Result<Appearance, DecodeError> {
        self.looks.get(i as usize).copied().ok_or(DecodeError::Malformed("look not in the palette"))
    }

    fn action(&self, i: u32) -> Result<String, DecodeError> {
        self.actions.get(i as usize).cloned().ok_or(DecodeError::Malformed("action not in the palette"))
    }

    fn state(&self, h: &Head) -> Result<StateView, DecodeError> {
        let organisms = self
            .own
            .iter()
            .map(|(&id, o)| {
                Ok(OwnOrganism {
                    id,
                    lineage: o.lineage,
                    x: dq(o.pos[0], POS),
                    y: dq(o.pos[1], POS),
                    action: self.action(o.action)?,
                    sex: sex_of(o.flags),
                    age_days: dq(o.age, AGE),
                    growth: dq(o.growth, GROWTH),
                    matured: o.flags & MATURED != 0,
                    health: dq(o.health, HEALTH),
                    needs: needs_view(o.needs.map(|v| dq(v, NEED))),
                    possessed: o.flags & POSSESSED != 0,
                    pregnant: o.flags & PREGNANT != 0,
                    generation: o.generation,
                    look: self.look(o.look)?,
                    order: o.order.map(|[x, y]| [dq(x, POS), dq(y, POS)]),
                    speed: o.speed.map(f32::from_bits),
                    group: o.group,
                })
            })
            .collect::<Result<_, DecodeError>>()?;
        let nearby = self
            .near
            .iter()
            .map(|(&id, o)| {
                Ok(NearbyOrganism {
                    id,
                    owner: o.owner,
                    lineage: o.lineage,
                    x: dq(o.pos[0], POS),
                    y: dq(o.pos[1], POS),
                    action: self.action(o.action)?,
                    sex: sex_of(o.flags),
                    matured: o.flags & MATURED != 0,
                    look: self.look(o.look)?,
                })
            })
            .collect::<Result<_, DecodeError>>()?;
        let mut rows: Vec<LeaderRow> = self.lb.values().cloned().collect();
        sort_rows(&mut rows);
        Ok(StateView {
            clock: h.clock.clone(),
            weather: h.weather.clone(),
            player: h.player,
            lineage: h.lineage,
            slots_left: h.slots_left,
            population: h.population,
            migration: h.migration,
            groups: h.groups.clone(),
            view: h.view,
            organisms,
            nearby,
            leaderboard: Leaderboard { day: h.lb.day, breeding_adults: h.lb.breeding_adults, rows },
            offers: self.offers.values().cloned().collect(),
            balance: h.balance,
            escrow: h.escrow,
        })
    }
}

/// The server's leaderboard order: share, highest first, then player.
fn sort_rows(rows: &mut [LeaderRow]) {
    rows.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.player.cmp(&b.player)));
}

fn read_section<R: Default>(
    r: &mut In,
    held: &mut BTreeMap<OrgId, R>,
    read: fn(&mut In, &mut R, u32) -> Result<(), DecodeError>,
) -> Result<(), DecodeError> {
    let gone = r.v()?;
    let mut id = 0u64;
    for _ in 0..gone {
        id = id.checked_add(r.v()?).ok_or(DecodeError::Malformed("id overflow"))?;
        if held.remove(&id).is_none() {
            return Err(DecodeError::Malformed("removal of an unknown id"));
        }
    }
    let n = r.v()?;
    let mut id = 0u64;
    for _ in 0..n {
        id = id.checked_add(r.v()?).ok_or(DecodeError::Malformed("id overflow"))?;
        let mask = r.v32()?;
        read(r, held.entry(id).or_default(), mask)?;
    }
    Ok(())
}

fn read_own(r: &mut In, o: &mut OwnQ, m: u32) -> Result<(), DecodeError> {
    if m >> 20 != 0 {
        return Err(DecodeError::Malformed("unknown own field"));
    }
    if m & OWN_LINEAGE != 0 {
        o.lineage = r.v32()?;
    }
    if m & OWN_POS != 0 {
        o.pos[0] = o.pos[0].wrapping_add(r.z()?);
        o.pos[1] = o.pos[1].wrapping_add(r.z()?);
    }
    if m & OWN_ACTION != 0 {
        o.action = r.v32()?;
    }
    if m & OWN_FLAGS != 0 {
        o.flags = r.u8()?;
    }
    if m & OWN_AGE != 0 {
        o.age = o.age.wrapping_add(r.z()?);
    }
    if m & OWN_GROWTH != 0 {
        o.growth = o.growth.wrapping_add(r.z()?);
    }
    if m & OWN_HEALTH != 0 {
        o.health = o.health.wrapping_add(r.z()?);
    }
    for i in 0..8 {
        if m & (1 << (OWN_NEED0 + i as u32)) != 0 {
            o.needs[i] = o.needs[i].wrapping_add(r.z()?);
        }
    }
    if m & OWN_GENERATION != 0 {
        o.generation = r.v32()?;
    }
    if m & OWN_LOOK != 0 {
        o.look = r.v32()?;
    }
    if m & OWN_ORDER != 0 {
        o.order = match r.u8()? {
            0 => None,
            _ => Some([r.z()?, r.z()?]),
        };
    }
    if m & OWN_SPEED != 0 {
        o.speed = match r.u8()? {
            0 => None,
            _ => Some(r.f32()?),
        };
    }
    if m & OWN_GROUP != 0 {
        o.group = u8::try_from(r.v()?).map_err(|_| DecodeError::Malformed("group out of range"))?;
    }
    Ok(())
}

fn read_near(r: &mut In, o: &mut NearQ, m: u32) -> Result<(), DecodeError> {
    if m >> 6 != 0 {
        return Err(DecodeError::Malformed("unknown nearby field"));
    }
    if m & NEAR_OWNER != 0 {
        o.owner = r.v32()?;
    }
    if m & NEAR_LINEAGE != 0 {
        o.lineage = r.v32()?;
    }
    if m & NEAR_POS != 0 {
        o.pos[0] = o.pos[0].wrapping_add(r.z()?);
        o.pos[1] = o.pos[1].wrapping_add(r.z()?);
    }
    if m & NEAR_ACTION != 0 {
        o.action = r.v32()?;
    }
    if m & NEAR_FLAGS != 0 {
        o.flags = r.u8()?;
    }
    if m & NEAR_LOOK != 0 {
        o.look = r.v32()?;
    }
    Ok(())
}

/// What `s` decodes to after protocol 2: quantized, lists in id order,
/// leaderboard in rank order.
pub fn canonical(s: &StateView) -> StateView {
    let mut s = s.clone();
    for o in &mut s.organisms {
        o.x = rt(o.x, POS);
        o.y = rt(o.y, POS);
        o.age_days = rt(o.age_days, AGE);
        o.growth = rt(o.growth, GROWTH);
        o.health = rt(o.health, HEALTH);
        o.needs = needs_view(needs_arr(&o.needs).map(|v| rt(v, NEED)));
        o.order = o.order.map(|[x, y]| [rt(x, POS), rt(y, POS)]);
        if o.sex != "male" {
            o.sex = "female".into();
        }
    }
    for o in &mut s.nearby {
        o.x = rt(o.x, POS);
        o.y = rt(o.y, POS);
        if o.sex != "male" {
            o.sex = "female".into();
        }
    }
    s.organisms.sort_by_key(|o| o.id);
    s.nearby.sort_by_key(|o| o.id);
    s.offers.sort_by_key(|o| o.id);
    sort_rows(&mut s.leaderboard.rows);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{build_world, Game, GameConfig, Target, WorldSettings};
    use crate::protocol::{ClientMsg, ServerMsg};
    use evo_sim::{BirthType, FounderDesign, Thermo};
    use rand::{Rng, SeedableRng};

    /// The wild ecosystem and six players, each connected; player 1 steers
    /// an organism, player 2 has a pinned view and an offer out.
    fn game() -> Game {
        let world = build_world(&WorldSettings { seed: 5, width: 96, height: 96, wild: true, ..Default::default() });
        let mut g = Game::new(world, GameConfig::default());
        for p in 1..=6u32 {
            g.set_name(p, &format!("p{p}"));
            let d = FounderDesign::new(format!("L{p}"), Thermo::Warm, BirthType::Live, 2).founders(6);
            g.found(p, &d).unwrap();
            g.connect(p as u64, p, &format!("p{p}"));
        }
        g.world.run_days(1);
        let _ = g.after_tick();
        let first = |g: &Game, p: u32| g.world.organisms.iter().find(|o| o.alive && o.owner == p).unwrap().id;
        let org = first(&g, 1);
        g.command(1, 1, None, ClientMsg::Possess { organism: org });
        g.command(1, 1, None, ClientMsg::OrderMove { organism: org, x: 80.0, y: 80.0 });
        // Player 1 splits its herd and has the new group hold ground.
        g.command(1, 1, None, ClientMsg::SplitGroup { group: 0, organisms: None });
        g.command(1, 1, None, ClientMsg::ClaimTerritory { group: 1, x: 40.0, y: 40.0, radius: 6.0 });
        g.command(2, 2, None, ClientMsg::SetView { x: 48.0, y: 48.0, radius: 30.0 });
        let (m, p) = (first(&g, 2), first(&g, 3));
        g.command(2, 2, None, ClientMsg::RequestMating { mine: m, partner: p, price: Some(1) });
        g
    }

    fn states(out: Vec<(Target, ServerMsg)>) -> Vec<(u64, StateView)> {
        out.into_iter()
            .filter_map(|(t, m)| match (t, m) {
                (Target::Conn(c), ServerMsg::State(s)) => Some((c, *s)),
                _ => None,
            })
            .collect()
    }

    /// Meta JSON and the counts that follow it.
    fn peek(frame: &[u8]) -> (serde_json::Value, u64, u64) {
        let mut r = In { b: frame, at: 0 };
        let _ = (r.u8(), r.v(), r.v());
        let n = r.v().unwrap() as usize;
        let meta = r.bytes(n).unwrap();
        let meta = if meta.is_empty() { serde_json::json!({}) } else { serde_json::from_slice(meta).unwrap() };
        let gone = r.v().unwrap();
        let mut id = 0;
        for _ in 0..gone {
            id += r.v().unwrap();
        }
        let _ = id;
        (meta, gone, r.v().unwrap())
    }

    /// Ticks and sub-steps of a live game, with states dropped at random
    /// (replaced in the queue before the writer got to them) and random
    /// keyframes: every decoded state equals the original after the
    /// round trip, and deltas are much smaller than keyframes.
    #[test]
    fn deltas_rebuild_every_state_through_drops_and_keyframes() {
        let mut g = game();
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let mut enc: HashMap<u64, DeltaEncoder> = HashMap::new();
        let mut dec: HashMap<u64, DeltaDecoder> = HashMap::new();
        let mut held: HashMap<u64, Vec<OrgId>> = HashMap::new();
        let (mut key_bytes, mut keys, mut delta_bytes, mut deltas) = (0usize, 0usize, 0usize, 0usize);
        let (mut sub_bytes, mut subs) = (0usize, 0usize);
        // Removals, palette additions after the keyframe, offers.
        let mut saw = (false, false, false);
        for round in 0..40 {
            let mut batch = states(g.states());
            let n_tick = batch.len();
            for _ in 0..3 {
                batch.extend(states(g.substep()));
            }
            for (i, (c, s)) in batch.into_iter().enumerate() {
                if rng.gen_bool(0.2) {
                    continue; // replaced before it was sent
                }
                let e = enc.entry(c).or_default();
                let frame = e.encode(&s, rng.gen_bool(0.05));
                let key = frame[0] == KEYFRAME;
                if key {
                    key_bytes += frame.len();
                    keys += 1;
                } else if i < n_tick {
                    delta_bytes += frame.len();
                    deltas += 1;
                } else {
                    sub_bytes += frame.len();
                    subs += 1;
                }
                let d = dec.entry(c).or_default();
                let got = d.apply(&frame).unwrap_or_else(|e| panic!("conn {c} round {round}: {e}"));
                assert_eq!(got, canonical(&s), "conn {c} round {round}");
                assert_eq!(d.seq(), Some(e.seq()));
                let ids: Vec<OrgId> =
                    got.organisms.iter().map(|o| o.id).chain(got.nearby.iter().map(|o| o.id)).collect();
                if let Some(before) = held.insert(c, ids.clone()) {
                    saw.0 |= !key && before.iter().any(|id| !ids.contains(id));
                }
                if !key {
                    let (meta, _, _) = peek(&frame);
                    saw.1 |= meta.get("looks").is_some() || meta.get("actions").is_some();
                    saw.2 |= meta.get("offers_set").is_some() || meta.get("offers_del").is_some();
                }
            }
            g.world.step();
            let _ = g.after_tick();
        }
        assert!(saw.0 && saw.1, "{saw:?}");
        assert!(keys > 6 && deltas > 100 && subs > 20, "{keys} {deltas} {subs}");
        let (key, delta, sub) = (key_bytes / keys, delta_bytes / deltas, sub_bytes / subs);
        assert!(delta * 3 < key, "keyframe {key} B, delta {delta} B");
        assert!(sub * 10 < key, "keyframe {key} B, sub-step {sub} B");
    }

    /// A sub-step state carries only what moved: the possessed organism and
    /// the view that follows it.
    #[test]
    fn a_substep_delta_carries_only_the_possessed_organism() {
        let mut g = game();
        let mut e = DeltaEncoder::new();
        let mut d = DeltaDecoder::new();
        // (The first sub-step after the order changes its action too.)
        let _ = g.substep();
        d.apply(&e.encode(&g.state_for(1).unwrap(), false)).unwrap();
        let (c, s) = states(g.substep()).pop().expect("the steering player gets a state");
        assert_eq!(c, 1);
        let frame = e.encode(&s, false);
        assert_eq!(frame[0], DELTA);
        assert!(frame.len() < 120, "{} bytes", frame.len());
        let (meta, gone, upserts) = peek(&frame);
        assert_eq!(meta.as_object().unwrap().keys().collect::<Vec<_>>(), ["view"]);
        assert_eq!((gone, upserts), (0, 1), "no removals, one organism");
        assert_eq!(d.apply(&frame).unwrap(), canonical(&s));
        // Nothing changed: an empty delta (kind, seq, base, no meta, four
        // zero counts).
        let frame = e.encode(&s, false);
        assert_eq!(frame.len(), 8);
        assert_eq!(d.apply(&frame).unwrap(), canonical(&s));
    }

    /// The decoder refuses a delta on a state it doesn't hold and anything
    /// before a keyframe, and recovers from the next keyframe.
    #[test]
    fn a_lost_or_garbled_frame_waits_for_a_keyframe() {
        let mut g = game();
        let mut e = DeltaEncoder::new();
        let mut d = DeltaDecoder::new();
        d.apply(&e.encode(&g.state_for(2).unwrap(), false)).unwrap();
        g.world.step();
        let _lost = e.encode(&g.state_for(2).unwrap(), false);
        g.world.step();
        let s3 = g.state_for(2).unwrap();
        let f3 = e.encode(&s3, false);
        assert_eq!(d.apply(&f3), Err(DecodeError::BaseMismatch { have: 1, base: 2 }));
        // Now nothing but a keyframe will do.
        assert_eq!(d.apply(&f3), Err(DecodeError::NeedKeyframe));
        let f4 = e.encode(&s3, true);
        assert_eq!(f4[0], KEYFRAME);
        assert_eq!(d.apply(&f4).unwrap(), canonical(&s3));
        // Truncated and garbled frames are errors, not panics.
        g.world.step();
        let f5 = e.encode(&g.state_for(2).unwrap(), false);
        for cut in 0..f5.len() {
            let mut d2 = DeltaDecoder::new();
            d2.apply(&f4).unwrap();
            assert!(d2.apply(&f5[..cut]).is_err(), "cut at {cut}");
        }
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for _ in 0..500 {
            let mut bad = f5.clone();
            let i = rng.gen_range(0..bad.len());
            bad[i] = rng.gen();
            let mut d2 = DeltaDecoder::new();
            d2.apply(&f4).unwrap();
            let _ = d2.apply(&bad);
        }
    }

    /// Quantization keeps what the client draws and shows.
    #[test]
    fn quantization_is_fine_enough() {
        let mut g = game();
        g.world.step();
        let s = g.state_for(1).unwrap();
        let c = canonical(&s);
        let mut own = s.organisms.clone();
        own.sort_by_key(|o| o.id);
        assert!(own.iter().any(|o| o.speed.is_some()));
        for (a, b) in own.iter().zip(c.organisms.iter()) {
            assert!((a.x - b.x).abs() <= 1.0 / 128.0 + 1e-4 && (a.y - b.y).abs() <= 1.0 / 128.0 + 1e-4);
            assert!((a.needs.hunger - b.needs.hunger).abs() <= 0.05 + 1e-4);
            assert!((a.health - b.health).abs() <= 0.005 + 1e-4);
            assert!((a.age_days - b.age_days).abs() <= 0.005 + 1e-4);
            assert_eq!(a.speed, b.speed);
        }
    }

    /// A shared fixture for the web client's decoder (`client/src/wire.test.ts`):
    /// the frames of a short session and the states they decode to. Set
    /// `UPDATE_WIRE_FIXTURE=1` to rewrite it after a format change.
    #[test]
    fn the_client_fixture_is_current() {
        use base64::Engine;
        let mut g = game();
        let mut e = DeltaEncoder::new();
        let mut frames = Vec::new();
        for round in 0..4 {
            let mut batch = states(g.states());
            batch.extend(states(g.substep()));
            for (c, s) in batch {
                if c == 1 {
                    let frame = e.encode(&s, round == 3 && frames.len() % 2 == 0);
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&frame);
                    frames.push(serde_json::json!({ "frame": b64, "state": canonical(&s) }));
                }
            }
            g.world.step();
            let _ = g.after_tick();
        }
        let text = serde_json::to_string(&serde_json::json!({ "frames": frames })).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/wire.fixture.json");
        if std::env::var_os("UPDATE_WIRE_FIXTURE").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == text, "{} is stale: rerun with UPDATE_WIRE_FIXTURE=1", path.display());
    }

    /// Encoding cost against JSON, for the README (`cargo test --release -p
    /// evo-server --lib wire::tests::encoding_cost -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn encoding_cost() {
        let world = build_world(&WorldSettings { seed: 5, width: 128, height: 128, wild: true, ..Default::default() });
        let mut g = Game::new(world, GameConfig { default_view_radius: 60.0, ..Default::default() });
        for p in 1..=60u32 {
            g.set_name(p, &format!("p{p}"));
            let d = FounderDesign::new(format!("L{p}"), Thermo::Warm, BirthType::Live, 2).founders(12);
            g.found(p, &d).unwrap();
            g.connect(p as u64, p, &format!("p{p}"));
        }
        g.world.run_days(3);
        let mut ticks = Vec::new();
        for _ in 0..20 {
            ticks.push(g.state_for(1).unwrap());
            g.world.step();
            let _ = g.after_tick();
        }
        let s0 = &ticks[0];
        eprintln!("own {} nearby {} rows {}", s0.organisms.len(), s0.nearby.len(), s0.leaderboard.rows.len());
        let time = |f: &mut dyn FnMut() -> usize, n: usize| {
            let t = std::time::Instant::now();
            let mut bytes = 0;
            for _ in 0..n {
                bytes += f();
            }
            (t.elapsed().as_secs_f64() * 1e6 / n as f64, bytes / n)
        };
        let mut i = 0;
        let (json_us, json_b) = time(
            &mut || {
                i += 1;
                serde_json::to_string(&ticks[i % 20]).unwrap().len()
            },
            2000,
        );
        let mut e = DeltaEncoder::new();
        let mut i = 0;
        let (delta_us, delta_b) = time(
            &mut || {
                i += 1;
                e.encode(&ticks[i % 20], false).len()
            },
            2000,
        );
        let (same_us, same_b) = time(&mut || e.encode(&ticks[0], false).len(), 2000);
        let (key_us, key_b) = time(&mut || e.encode(&ticks[0], true).len(), 2000);
        eprintln!(
            "json {json_us:.1} us {json_b} B; tick delta {delta_us:.1} us {delta_b} B; \
             unchanged {same_us:.1} us {same_b} B; keyframe {key_us:.1} us {key_b} B"
        );
    }
}
