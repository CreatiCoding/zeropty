//! The session table: sessions by slot (`Slots`), by name (`Names`), and the key that names one session
//! for as long as it lives (`SessKey`: its slot and that slot's generation, so an event or a wait queued
//! for a session that ended never reaches the next one in its slot).

use std::collections::HashMap;

/// A session for as long as it lives: its slot, and the generation that slot had when it was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessKey {
    pub(super) slot: usize,
    pub(super) generation: u32,
}

impl SessKey {
    /// As a kqueue tag: slot << 32 | generation. Never has crate::engine::HOST_TAG set.
    pub(super) fn udata(self) -> usize {
        (self.slot << 32) | self.generation as usize
    }

    pub(super) fn from_udata(tag: usize) -> Self {
        Self { slot: tag >> 32, generation: tag as u32 }
    }
}

/// Sessions per chunk of the session table.
pub(super) const SLOT_CHUNK: usize = 32;

/// The session table, in chunks that never move: one growing array was copied to a bigger block at every
/// growth and left the allocator's pages for blocks that size mostly empty (400 sessions: 192 KB held for
/// a 48 KB table).
pub(super) struct Slots<T> {
    chunks: Vec<Box<[Option<T>; SLOT_CHUNK]>>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self { chunks: Vec::new() }
    }
}

/// Something with a generation, so a `SessKey` can tell whether it is still the one it names.
pub(super) trait Generational {
    fn generation(&self) -> u32;
}

impl<T> Slots<T> {
    /// Adds a chunk; returns its first index.
    pub(super) fn grow(&mut self) -> usize {
        self.chunks.push(Box::new(std::array::from_fn(|_| None)));
        (self.chunks.len() - 1) * SLOT_CHUNK
    }

    /// Slots there are room for (used or not).
    pub(super) fn capacity(&self) -> usize {
        self.chunks.len() * SLOT_CHUNK
    }

    /// What slot `i` holds; None past the table.
    pub(super) fn get(&self, i: usize) -> Option<&T> {
        self.chunks.get(i / SLOT_CHUNK).and_then(|c| c[i % SLOT_CHUNK].as_ref())
    }

    pub(super) fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.chunks.get_mut(i / SLOT_CHUNK).and_then(|c| c[i % SLOT_CHUNK].as_mut())
    }

    /// Puts `v` in slot `i` (which must exist), returning what was there.
    pub(super) fn put(&mut self, i: usize, v: T) -> Option<T> {
        self.entry(i).replace(v)
    }

    /// Empties slot `i` (which must exist), returning what was there.
    pub(super) fn take(&mut self, i: usize) -> Option<T> {
        self.entry(i).take()
    }

    fn entry(&mut self, i: usize) -> &mut Option<T> {
        &mut self.chunks[i / SLOT_CHUNK][i % SLOT_CHUNK]
    }

    /// The used slots, in order.
    pub(super) fn used(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.capacity()).filter(|&i| self.get(i).is_some())
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        self.chunks.iter().flat_map(|c| c.iter()).flatten()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.chunks.iter_mut().flat_map(|c| c.iter_mut()).flatten()
    }
}

impl<T: Generational> Slots<T> {
    /// The session `key` names, if it is still there.
    pub(super) fn find(&self, key: SessKey) -> Option<&T> {
        self.get(key.slot).filter(|s| s.generation() == key.generation)
    }

    pub(super) fn find_mut(&mut self, key: SessKey) -> Option<&mut T> {
        self.get_mut(key.slot).filter(|s| s.generation() == key.generation)
    }
}

/// What `Names` needs of the table: the name in a slot, and how many slots to scan.
pub(super) trait Named {
    fn name_at(&self, slot: usize) -> Option<&str>;
    fn slots(&self) -> usize;
}

/// Session name -> slot, keyed by the name's hash: 12 bytes an entry, where a table keyed by the names
/// themselves held ~36 (400 sessions: 14 KB, alone on a page of its size class). A hash two names share
/// (possible, never seen) leaves the second unindexed, found by a scan of the sessions.
pub(super) struct Names {
    by_hash: HashMap<u64, u32>,
    /// Sessions named, for new-session's default name.
    count: usize,
    hash: fn(&str) -> u64,
}

impl Default for Names {
    fn default() -> Self {
        Self { by_hash: HashMap::new(), count: 0, hash: name_hash }
    }
}

fn name_hash(name: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}

impl Names {
    /// How many sessions are named (new-session's next default name).
    pub(super) fn count(&self) -> usize {
        self.count
    }

    /// The slot of the session named `name`.
    pub(super) fn find(&self, name: &str, table: &impl Named) -> Option<usize> {
        let slot = *self.by_hash.get(&(self.hash)(name))? as usize;
        if table.name_at(slot) == Some(name) {
            return Some(slot);
        }
        (0..table.slots()).find(|&i| table.name_at(i) == Some(name))
    }

    /// Notes session `slot` named `name`.
    pub(super) fn insert(&mut self, name: &str, slot: usize) {
        self.by_hash.entry((self.hash)(name)).or_insert(slot as u32);
        self.count += 1;
    }

    /// Forgets session `slot` (already out of `table`); one sharing its name's hash takes its place.
    pub(super) fn remove(&mut self, name: &str, slot: usize, table: &impl Named) {
        self.count -= 1;
        let h = (self.hash)(name);
        if self.by_hash.get(&h) == Some(&(slot as u32)) {
            self.by_hash.remove(&h);
            if let Some(other) = (0..table.slots()).find(|&i| table.name_at(i).is_some_and(|n| (self.hash)(n) == h)) {
                self.by_hash.insert(h, other as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Named, Names, SLOT_CHUNK, SessKey, Slots};

    struct Table(Vec<Option<&'static str>>);

    impl Named for Table {
        fn name_at(&self, slot: usize) -> Option<&str> {
            self.0.get(slot).copied().flatten()
        }
        fn slots(&self) -> usize {
            self.0.len()
        }
    }

    #[test]
    fn names_find_and_forget() {
        let mut t = Table(vec![Some("a"), Some("b"), None]);
        let mut n = Names::default();
        n.insert("a", 0);
        n.insert("b", 1);
        assert_eq!((n.find("a", &t), n.find("b", &t), n.find("c", &t)), (Some(0), Some(1), None));
        t.0[0] = None;
        n.remove("a", 0, &t);
        assert_eq!((n.find("a", &t), n.count()), (None, 1));
    }

    /// Names whose hashes collide are still found, before and after either goes.
    #[test]
    fn colliding_names() {
        let mut t = Table(vec![Some("x"), Some("y"), Some("z")]);
        let mut n = Names { hash: |_| 7, ..Names::default() };
        for (i, name) in ["x", "y", "z"].iter().enumerate() {
            n.insert(name, i);
        }
        assert_eq!((n.find("x", &t), n.find("y", &t), n.find("z", &t)), (Some(0), Some(1), Some(2)));
        t.0[0] = None;
        n.remove("x", 0, &t);
        assert_eq!((n.find("x", &t), n.find("y", &t), n.find("z", &t)), (None, Some(1), Some(2)));
        t.0[2] = None;
        n.remove("z", 2, &t);
        assert_eq!(n.find("y", &t), Some(1));
    }

    #[test]
    fn slots_grow_in_chunks_and_keys_round_trip() {
        let mut s: Slots<u32> = Slots::default();
        assert_eq!(s.grow(), 0);
        assert_eq!(s.grow(), SLOT_CHUNK);
        s.put(3, 30);
        s.put(SLOT_CHUNK + 1, 40);
        assert_eq!(s.used().collect::<Vec<_>>(), [3, SLOT_CHUNK + 1]);
        assert_eq!(s.iter().copied().collect::<Vec<_>>(), [30, 40]);
        assert_eq!(s.take(3), Some(30));
        assert_eq!((s.get(3), s.get(9999)), (None, None));
        let k = SessKey { slot: 70_000, generation: u32::MAX };
        assert_eq!(SessKey::from_udata(k.udata()), k);
        assert_eq!(k.udata() & crate::engine::HOST_TAG, 0);
    }
}
