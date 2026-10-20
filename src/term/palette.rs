//! A screen's attribute sets, numbered: a cell carries a 16-bit palette id instead of its attributes.
//!
//! Invariants: id 0 is the default attributes; ids are only ever appended (a palette is renumbered as a
//! whole, by `Screen::compact_palette`), so an id, and a cache entry naming it, stays right for the
//! palette's life.

use super::attrs::Attrs;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// A screen's distinct attribute sets as `Attrs::key`s; cells hold an index (0 = default).
#[derive(Debug)]
pub(super) struct Palette {
    keys: Keys,
    /// Built once the list outgrows a linear scan; until then a thaw has nothing to rebuild. Boxed: 8
    /// bytes in every palette instead of a map's 48, and most palettes never build one.
    #[allow(clippy::box_collection)]
    ids: Option<Box<HashMap<u64, u16, std::hash::BuildHasherDefault<Fnv>>>>,
    /// The last lookup: programs set the same few attributes over and over.
    last: (u64, u16),
    /// Tags this palette's entries in RECENT. Unique per palette (a clone gets its own): keys are only
    /// ever appended, so an entry stays right for as long as its palette lives.
    generation: u32,
}

/// A palette's keys: ids into the process's SHARED table (2 bytes each, not 8) while that has room, the
/// keys themselves after. Claude sessions use the same hundred or so attribute sets, so the table stays
/// small while each live screen's palette shrinks from ~650 to ~160 bytes.
#[derive(Clone, Debug)]
enum Keys {
    Shared(Vec<u16>),
    Own(Vec<u64>),
}

#[derive(Default)]
struct SharedKeys {
    keys: Vec<u64>,
    ids: HashMap<u64, u16, std::hash::BuildHasherDefault<Fnv>>,
}

/// One for the whole process, so a screen can be used from any thread (an embedding host calls the engine
/// from its own threads); read-locked on a lookup that misses the palette's caches, written when an
/// attribute set is seen for the first time.
fn shared() -> &'static RwLock<SharedKeys> {
    static S: OnceLock<RwLock<SharedKeys>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(SharedKeys::default()))
}

/// The shared table's size: past it, palettes that need a new key keep their own keys.
const SHARED_MAX: usize = 65535;

/// `key`'s id in the shared table, added if new and there is room.
fn shared_id(key: u64) -> Option<u16> {
    if let Some(&id) = shared().read().unwrap().ids.get(&key) {
        return Some(id);
    }
    let mut s = shared().write().unwrap();
    if let Some(&id) = s.ids.get(&key) {
        return Some(id);
    }
    (s.keys.len() < SHARED_MAX).then(|| {
        let id = s.keys.len() as u16;
        s.keys.push(key);
        s.ids.insert(key, id);
        id
    })
}

fn shared_key(id: u16) -> u64 {
    shared().read().unwrap().keys[usize::from(id)]
}

impl Keys {
    fn from_keys(keys: Vec<u64>) -> Self {
        match keys.iter().map(|&k| shared_id(k)).collect::<Option<Vec<u16>>>() {
            Some(ids) => Keys::Shared(ids),
            None => Keys::Own(keys),
        }
    }

    fn len(&self) -> usize {
        match self {
            Keys::Shared(v) => v.len(),
            Keys::Own(v) => v.len(),
        }
    }

    fn capacity(&self) -> usize {
        match self {
            Keys::Shared(v) => v.capacity(),
            Keys::Own(v) => v.capacity(),
        }
    }

    fn key(&self, i: usize) -> u64 {
        match self {
            Keys::Shared(v) => shared_key(v[i]),
            Keys::Own(v) => v[i],
        }
    }

    fn position(&self, key: u64) -> Option<usize> {
        match self {
            Keys::Shared(v) => {
                let id = shared().read().unwrap().ids.get(&key).copied()?;
                v.iter().position(|&x| x == id)
            }
            Keys::Own(v) => v.iter().position(|&k| k == key),
        }
    }

    fn push(&mut self, key: u64) {
        // A quarter more at a time, not twice: a claude screen has ~80 attributes and doubling left most
        // live screens holding 128.
        fn grow<T>(v: &mut Vec<T>) {
            if v.len() == v.capacity() {
                v.reserve_exact((v.len() / 4).max(4));
            }
        }
        match self {
            Keys::Shared(v) => match shared_id(key) {
                Some(id) => {
                    grow(v);
                    v.push(id);
                }
                None => {
                    let mut own: Vec<u64> = v.iter().map(|&id| shared_key(id)).collect();
                    own.push(key);
                    *self = Keys::Own(own);
                }
            },
            Keys::Own(v) => {
                grow(v);
                v.push(key);
            }
        }
    }
}

impl Clone for Palette {
    fn clone(&self) -> Self {
        Self { keys: self.keys.clone(), ids: self.ids.clone(), last: self.last, generation: next_generation() }
    }
}

fn next_generation() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// Recent lookups, direct-mapped by key, as (palette generation, key, id): claude alternates between
// dozens of colors, which missed `last` and then scanned the list. Shared by every screen, as only one
// parses at a time; it was 256 B in each.
thread_local!(static RECENT: std::cell::RefCell<[(u32, u64, u16); 16]> = const { std::cell::RefCell::new([(0, NO_KEY, 0); 16]) });

/// No real key has bit 63 set (they use 60 bits).
const NO_KEY: u64 = u64::MAX;

const PALETTE_SCAN: usize = 128;

/// FNV-1a: the keys come from the program itself, so SipHash's DoS resistance buys nothing here and its
/// cost showed up in profiles.
#[derive(Default)]
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
        self.0 = h;
    }
}

impl Palette {
    /// The default attributes only.
    pub(super) fn new() -> Self {
        Self::from_keys(vec![0])
    }

    /// A palette whose id i stands for `keys[i]`.
    pub(super) fn from_keys(keys: Vec<u64>) -> Self {
        let mut p = Self { keys: Keys::from_keys(keys), ids: None, last: (0, 0), generation: next_generation() };
        p.index();
        p
    }

    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }

    /// Entries the palette has room for without growing (`mem` reports).
    pub(super) fn capacity(&self) -> usize {
        self.keys.capacity()
    }

    /// The key id `i` stands for.
    pub(super) fn key(&self, i: usize) -> u64 {
        self.keys.key(i)
    }

    /// The attributes id `id` stands for.
    pub(super) fn get(&self, id: u16) -> Attrs {
        Attrs::from_key(self.keys.key(usize::from(id)))
    }

    fn index(&mut self) {
        if self.ids.is_none() && self.keys.len() > PALETTE_SCAN {
            self.ids = Some(Box::new((0..self.keys.len()).map(|i| (self.keys.key(i), i as u16)).collect()));
        }
    }

    /// The id of `a`, added if new. None when all 65536 ids are taken (the screen then compacts and
    /// retries). Runs for every SGR sequence parsed, hence its three caches.
    pub(super) fn id(&mut self, a: Attrs) -> Option<u16> {
        let key = a.key();
        if self.last.0 == key {
            return Some(self.last.1);
        }
        let r = ((key ^ key >> 17 ^ key >> 31) & 15) as usize;
        let generation = self.generation;
        if let Some(id) = RECENT.with_borrow(|c| (c[r].0 == generation && c[r].1 == key).then_some(c[r].2)) {
            self.last = (key, id);
            return Some(id);
        }
        let found = match &self.ids {
            Some(ids) => ids.get(&key).copied(),
            None => self.keys.position(key).map(|i| i as u16),
        };
        let id = match found {
            Some(id) => id,
            None => {
                let id = u16::try_from(self.keys.len()).ok()?;
                self.keys.push(key);
                if let Some(ids) = &mut self.ids {
                    ids.insert(key, id);
                }
                self.index();
                id
            }
        };
        self.last = (key, id);
        RECENT.with_borrow_mut(|c| c[r] = (generation, key, id));
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::Palette;
    use crate::term::attrs::{Attrs, Color};

    fn attrs(i: u8) -> Attrs {
        let mut a = Attrs::default();
        a.fg = Color::Idx(i);
        a
    }

    #[test]
    fn ids_are_stable_and_appended() {
        let mut p = Palette::new();
        assert_eq!(p.id(Attrs::default()), Some(0));
        // Past PALETTE_SCAN, so the index map is built too.
        let ids: Vec<u16> = (0..=200u8).map(|i| p.id(attrs(i)).unwrap()).collect();
        assert_eq!(ids, (1..=201).collect::<Vec<u16>>());
        for i in (0..=200u8).rev() {
            assert_eq!(p.id(attrs(i)), Some(u16::from(i) + 1), "found again");
            assert_eq!(p.get(u16::from(i) + 1), attrs(i));
        }
        assert_eq!(p.len(), 202);
    }

    /// A clone gets ids of its own from then on: the shared RECENT cache must not hand one palette's id
    /// to the other.
    #[test]
    fn clones_part_ways() {
        let mut a = Palette::new();
        a.id(attrs(1));
        let mut b = a.clone();
        assert_eq!(a.id(attrs(2)), Some(2));
        assert_eq!(b.id(attrs(3)), Some(2));
        assert_eq!(a.get(2), attrs(2));
        assert_eq!(b.get(2), attrs(3));
        assert_eq!(a.id(attrs(3)), Some(3));
    }

    #[test]
    fn from_keys_keeps_the_numbering() {
        let keys: Vec<u64> = [Attrs::default(), attrs(9), attrs(4)].iter().map(Attrs::key).collect();
        let mut p = Palette::from_keys(keys.clone());
        assert_eq!((0..3).map(|i| p.key(i)).collect::<Vec<_>>(), keys);
        assert_eq!(p.id(attrs(4)), Some(2));
    }
}
