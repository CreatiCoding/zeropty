//! The process-wide cold store: every deep-frozen screen, compressed (zstd), end to end in one backing
//! (`Backing`: an unlinked temporary file by default, memory with ZEROPTY_COLD=memory or when no file can
//! be made).
//!
//! Kept each in its own allocation, compressed screens sat among the pages the screens had used while
//! busy, and those pages stayed (400 idle sessions: 0.84 MB for 0.3 MB of data); in one allocation they
//! still cost their bytes, ~180 KB for 400 claude screens. In the file they cost the process nothing: the
//! kernel's page cache holds them (reclaimable, outside the process's footprint), and a screen quiet 10 s
//! that prints again pays a read. Removed entries leave holes; the store is rebuilt once they are half
//! of it.
//!
//! An entry (`stash`) is [unpacked length u32 LE][one zstd frame]; `fetch` gives back the unpacked bytes.

use std::io;
use std::sync::{Mutex, OnceLock};

/// Where the store's bytes live: written at offsets, read back by span, emptied for a rebuild.
trait Backing: Send {
    /// Writes `b` at `at`, at most at the current end (the store only appends or rewrites from 0).
    fn write_at(&mut self, at: usize, b: &[u8]) -> io::Result<()>;
    fn read_at(&self, at: usize, len: usize) -> io::Result<Vec<u8>>;
    /// Drops every byte, keeping room for `capacity`.
    fn reset(&mut self, capacity: usize) -> io::Result<()>;
}

impl Backing for Vec<u8> {
    fn write_at(&mut self, at: usize, b: &[u8]) -> io::Result<()> {
        if self.len() + b.len() > self.capacity() {
            // An eighth more at a time: doubling left up to half of a large buffer unused.
            self.reserve_exact((self.len() / 8).max(b.len()).max(4096));
        }
        self.truncate(at);
        self.extend_from_slice(b);
        Ok(())
    }

    fn read_at(&self, at: usize, len: usize) -> io::Result<Vec<u8>> {
        Ok(self[at..at + len].to_vec())
    }

    fn reset(&mut self, capacity: usize) -> io::Result<()> {
        *self = Vec::with_capacity(capacity);
        Ok(())
    }
}

impl Backing for std::fs::File {
    fn write_at(&mut self, at: usize, b: &[u8]) -> io::Result<()> {
        use std::os::unix::fs::FileExt as _;
        self.write_all_at(b, at as u64)
    }

    fn read_at(&self, at: usize, len: usize) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::FileExt as _;
        let mut v = vec![0; len];
        self.read_exact_at(&mut v, at as u64)?;
        Ok(v)
    }

    fn reset(&mut self, _capacity: usize) -> io::Result<()> {
        self.set_len(0)
    }
}

/// The offset of an entry whose bytes could not be read back when the store moved (`relocate`): its
/// screen is lost, and `get` says so.
const LOST: u32 = u32::MAX;

/// Entries by id, end to end in `backing`.
///
/// Invariants: `spans[id]` is (offset, length) of a live entry, (_, 0) for a free id, or (LOST, n) for
/// one that could not be moved; `end` is where the next entry goes; `live` sums the live lengths. A
/// file that fails a write (a full disk) is given up for memory (`relocate`), never a panic.
struct ColdStore {
    backing: Box<dyn Backing>,
    /// (offset, length) by id; length 0 is a free id.
    spans: Vec<(u32, u32)>,
    free: Vec<u32>,
    live: usize,
    /// Bytes written so far (the file's or the buffer's end).
    end: usize,
}

/// A temporary file for the store, already unlinked; None with ZEROPTY_COLD=memory or when none can be
/// made (ZEROPTY_COLD_DIR, else the temporary directory).
fn cold_file() -> Option<std::fs::File> {
    if std::env::var("ZEROPTY_COLD").is_ok_and(|v| v == "memory") {
        return None;
    }
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::var_os("ZEROPTY_COLD_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("zeropty-cold-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).ok()?;
    // Gone from the directory at once: nothing is left behind, whatever happens to the process.
    std::fs::remove_file(&path).ok()?;
    Some(file)
}

fn store() -> std::sync::MutexGuard<'static, ColdStore> {
    static C: OnceLock<Mutex<ColdStore>> = OnceLock::new();
    let c = C.get_or_init(|| {
        let backing: Box<dyn Backing> = match cold_file() {
            Some(f) => Box::new(f),
            None => Box::new(Vec::new()),
        };
        Mutex::new(ColdStore::new(backing))
    });
    // A panic while holding it left the store as consistent as any other moment: spans are updated
    // only after their bytes are written.
    c.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ColdStore {
    fn new(backing: Box<dyn Backing>) -> Self {
        Self { backing, spans: Vec::new(), free: Vec::new(), live: 0, end: 0 }
    }

    fn put(&mut self, b: &[u8]) -> u32 {
        if let Err(e) = self.backing.write_at(self.end, b) {
            self.relocate(Some(e));
            // Memory takes it.
            let _ = self.backing.write_at(self.end, b);
        }
        let span = (self.end as u32, b.len() as u32);
        self.end += b.len();
        self.live += b.len();
        match self.free.pop() {
            Some(id) => {
                self.spans[id as usize] = span;
                id
            }
            None => {
                self.spans.push(span);
                (self.spans.len() - 1) as u32
            }
        }
    }

    fn len(&self, id: u32) -> usize {
        self.spans[id as usize].1 as usize
    }

    /// Entry `id`'s bytes; an error when they could not be read (the file failed, or lost them).
    fn get(&self, id: u32) -> io::Result<Vec<u8>> {
        let (off, len) = self.spans[id as usize];
        if off == LOST {
            return Err(io::Error::other("cold store entry lost"));
        }
        self.backing.read_at(off as usize, len as usize)
    }

    fn remove(&mut self, id: u32) {
        let (_, len) = std::mem::take(&mut self.spans[id as usize]);
        self.live -= len as usize;
        self.free.push(id);
        if self.end > 2 * self.live + (64 << 10) {
            self.relocate(None);
        }
    }

    /// Writes the entries again from the start, without the holes. With `failed` (the backing's write
    /// error), or when the backing fails meanwhile, into memory instead: the entries are read out first,
    /// so none is lost but those the backing cannot give back.
    fn relocate(&mut self, failed: Option<io::Error>) {
        let entries: Vec<(usize, Option<Vec<u8>>)> = (0..self.spans.len()).filter(|&i| self.spans[i].1 > 0).map(|i| (i, self.get(i as u32).ok())).collect();
        let capacity = self.live + self.live / 8;
        let mut failed = failed;
        if failed.is_none() {
            match self.lay_out(capacity, &entries) {
                Ok(()) => return,
                Err(e) => failed = Some(e),
            }
        }
        if let Some(e) = failed {
            eprintln!("zeropty: cold store file: {e}; compressed screens are kept in memory from now on");
        }
        self.backing = Box::new(Vec::<u8>::new());
        // Memory does not fail.
        let _ = self.lay_out(capacity, &entries);
    }

    /// Resets the backing and writes `entries` end to end, updating their spans.
    fn lay_out(&mut self, capacity: usize, entries: &[(usize, Option<Vec<u8>>)]) -> io::Result<()> {
        self.backing.reset(capacity)?;
        self.end = 0;
        for (i, bytes) in entries {
            match bytes {
                Some(bytes) => {
                    self.backing.write_at(self.end, bytes)?;
                    self.spans[*i].0 = self.end as u32;
                    self.end += bytes.len();
                }
                None => self.spans[*i].0 = LOST,
            }
        }
        Ok(())
    }
}

/// Compresses `raw` into a new entry; None if compression failed.
pub(super) fn stash(raw: &[u8]) -> Option<u32> {
    let z = zmem::compress(raw, 1)?;
    let mut deep = Vec::with_capacity(4 + z.len());
    deep.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    deep.extend_from_slice(&z);
    Some(store().put(&deep))
}

/// An entry's bytes unpacked; None if they are not what `stash` wrote (a backing that returned
/// something else).
fn unpack(deep: &[u8]) -> Option<Vec<u8>> {
    let raw_len = u32::from_le_bytes(deep.get(..4)?.try_into().ok()?) as usize;
    zmem::decompress(&deep[4..], raw_len)
}

/// Entry `id`, unpacked; it stays in the store. None when the store could not give it back.
pub(super) fn fetch(id: u32) -> Option<Vec<u8>> {
    let deep = store().get(id).ok()?;
    unpack(&deep)
}

/// Entry `id`, unpacked, and out of the store. None (and out of the store) when the store could not
/// give it back.
pub(super) fn fetch_and_free(id: u32) -> Option<Vec<u8>> {
    let mut c = store();
    let raw = c.get(id).ok().and_then(|deep| unpack(&deep));
    c.remove(id);
    raw
}

/// Bytes entry `id` takes in the store (compressed).
pub(super) fn len(id: u32) -> usize {
    store().len(id)
}

/// Frees entry `id`.
pub(super) fn remove(id: u32) {
    store().remove(id)
}

/// zstd with its work memory from the Rust allocator (whatever the host made it). Through libc malloc,
/// as zstd does by default, the contexts' memory stayed in malloc's free lists after each use (400 idle
/// sessions compressed: ~48 KB the allocator would otherwise have returned).
mod zmem {
    use std::alloc::{Layout, alloc, dealloc};
    use std::ffi::c_void;
    use zstd_sys as z;

    /// Each block starts with its size, for zfree; 16 also keeps the block zstd sees 16-aligned.
    const HEAD: usize = 16;

    /// zstd's allocation hook: `size` bytes, after a HEAD that remembers the size.
    unsafe extern "C" fn zalloc(_: *mut c_void, size: usize) -> *mut c_void {
        let Ok(layout) = Layout::from_size_align(size + HEAD, HEAD) else { return std::ptr::null_mut() };
        // SAFETY: `layout` has a non-zero size (HEAD at least).
        let p = unsafe { alloc(layout) };
        if p.is_null() {
            return p.cast();
        }
        // SAFETY: `p` is a fresh block of at least HEAD bytes, aligned to 16, so a usize fits at its start.
        unsafe { (p as *mut usize).write(size) };
        // SAFETY: HEAD <= the block's size, so `p + HEAD` is within it (or one past its end for size 0).
        unsafe { p.add(HEAD) }.cast()
    }

    /// zstd's free hook, for blocks from `zalloc` only (zstd frees only what it allocated through the
    /// same ZSTD_customMem).
    unsafe extern "C" fn zfree(_: *mut c_void, p: *mut c_void) {
        if p.is_null() {
            return;
        }
        // SAFETY: `p` came from zalloc, HEAD bytes into its block: going back HEAD stays in the block.
        let base = unsafe { (p as *mut u8).sub(HEAD) };
        // SAFETY: zalloc wrote the size at the block's start, aligned for a usize.
        let size = unsafe { (base as *mut usize).read() };
        // SAFETY: `base` was allocated by `alloc` with exactly this layout (zalloc checked it is valid).
        unsafe { dealloc(base, Layout::from_size_align_unchecked(size + HEAD, HEAD)) };
    }

    fn mem() -> z::ZSTD_customMem {
        z::ZSTD_customMem { customAlloc: Some(zalloc), customFree: Some(zfree), opaque: std::ptr::null_mut() }
    }

    pub fn compress(src: &[u8], level: i32) -> Option<Vec<u8>> {
        // SAFETY: the context is checked for null and freed once used; zstd writes at most `cap` (its
        // compress bound) bytes into `dst`'s capacity and reads `src.len()` from `src`; `set_len` covers
        // only the `n` bytes it reported writing.
        unsafe {
            let cctx = z::ZSTD_createCCtx_advanced(mem());
            if cctx.is_null() {
                return None;
            }
            let cap = z::ZSTD_compressBound(src.len());
            let mut dst: Vec<u8> = Vec::with_capacity(cap);
            let n = z::ZSTD_compressCCtx(cctx, dst.as_mut_ptr().cast(), cap, src.as_ptr().cast(), src.len(), level);
            z::ZSTD_freeCCtx(cctx);
            if z::ZSTD_isError(n) != 0 {
                return None;
            }
            dst.set_len(n);
            Some(dst)
        }
    }

    pub fn decompress(src: &[u8], size: usize) -> Option<Vec<u8>> {
        // SAFETY: as in compress: zstd writes at most `size` bytes into `dst`'s capacity of `size`, and
        // `set_len` covers only the `n` (== size) bytes it reported writing.
        unsafe {
            let dctx = z::ZSTD_createDCtx_advanced(mem());
            if dctx.is_null() {
                return None;
            }
            let mut dst: Vec<u8> = Vec::with_capacity(size);
            let n = z::ZSTD_decompressDCtx(dctx, dst.as_mut_ptr().cast(), size, src.as_ptr().cast(), src.len());
            z::ZSTD_freeDCtx(dctx);
            if z::ZSTD_isError(n) != 0 || n != size {
                return None;
            }
            dst.set_len(n);
            Some(dst)
        }
    }
}

#[cfg(test)]
mod tests {
    /// Entries keep their bytes across the rebuild that drops removed ones, and ids are reused: in memory
    /// and in a file.
    #[test]
    fn rebuild_keeps_entries() {
        let file = super::cold_file();
        assert!(file.is_some(), "a cold file can be made");
        let backings: [Box<dyn super::Backing>; 2] = [Box::new(Vec::new()), Box::new(file.unwrap())];
        for backing in backings {
            let mut c = super::ColdStore::new(backing);
            let blob = |k: u32| (0..500 + k % 97).map(|j| (k * 31 + j) as u8).collect::<Vec<u8>>();
            let ids: Vec<u32> = (0..600).map(|k| c.put(&blob(k))).collect();
            let before = c.end;
            for k in (0..600).filter(|k| k % 3 != 0) {
                c.remove(ids[k as usize]);
            }
            assert!(c.end < before / 2, "rebuilt: {} -> {}", before, c.end);
            for k in (0..600).filter(|k| k % 3 == 0) {
                assert_eq!(c.get(ids[k as usize]).unwrap(), blob(k), "entry {k}");
            }
            let again = c.put(b"again");
            assert!(ids.contains(&again), "a freed id is reused");
            assert_eq!(c.get(again).unwrap(), b"again");
        }
    }

    #[test]
    fn stash_and_fetch() {
        let raw: Vec<u8> = (0..20_000u32).map(|i| (i % 7) as u8).collect();
        let id = super::stash(&raw).unwrap();
        assert!(super::len(id) < raw.len() / 10, "compressed");
        assert_eq!(super::fetch(id).unwrap(), raw);
        assert_eq!(super::fetch_and_free(id).unwrap(), raw);
        let other = super::stash(b"x").unwrap();
        assert_eq!(super::fetch(other).unwrap(), b"x");
        super::remove(other);
    }

    /// A backing whose writes fail after a while (a disk filling up), whose reads may fail too.
    struct Failing {
        bytes: Vec<u8>,
        writes_left: usize,
        reads_fail: bool,
    }

    impl super::Backing for Failing {
        fn write_at(&mut self, at: usize, b: &[u8]) -> std::io::Result<()> {
            if self.writes_left == 0 {
                return Err(std::io::Error::from_raw_os_error(libc::ENOSPC));
            }
            self.writes_left -= 1;
            self.bytes.write_at(at, b)
        }
        fn read_at(&self, at: usize, len: usize) -> std::io::Result<Vec<u8>> {
            if self.reads_fail { Err(std::io::Error::from_raw_os_error(libc::EIO)) } else { self.bytes.read_at(at, len) }
        }
        fn reset(&mut self, capacity: usize) -> std::io::Result<()> {
            self.bytes.reset(capacity)
        }
    }

    /// A full disk moves the store into memory with every entry, and it goes on working.
    #[test]
    fn a_failing_file_falls_back_to_memory() {
        let mut c = super::ColdStore::new(Box::new(Failing { bytes: Vec::new(), writes_left: 3, reads_fail: false }));
        let ids: Vec<u32> = (0..5u8).map(|k| c.put(&[k; 100])).collect();
        for (k, id) in ids.iter().enumerate() {
            assert_eq!(c.get(*id).unwrap(), [k as u8; 100], "entry {k} kept across the move");
        }
        c.remove(ids[0]);
        assert_eq!(c.get(ids[4]).unwrap(), [4u8; 100]);
        // Entries the failing backing cannot give back are lost, not a panic.
        let mut c = super::ColdStore::new(Box::new(Failing { bytes: Vec::new(), writes_left: 1, reads_fail: true }));
        let first = c.put(b"first");
        let second = c.put(b"second");
        assert!(c.get(first).is_err(), "lost with the file");
        assert_eq!(c.get(second).unwrap(), b"second", "written to memory");
        c.remove(first);
        assert_eq!(c.put(b"third"), first, "a lost entry's id is freed as any other");
    }
}
