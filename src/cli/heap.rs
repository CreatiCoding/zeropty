//! The process's allocator: mimalloc, counting the bytes asked for. Against the footprint, that shows
//! what fragmentation costs (`stats`'s heap_bytes); live allocations by size feed `mstats`.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

/// mimalloc, counting.
pub struct Counting;

static HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
/// Live allocations by size, in power-of-two buckets (bucket k: sizes up to 2^k), for `mstats`.
static HEAP_LIVE: [AtomicIsize; 32] = [const { AtomicIsize::new(0) }; 32];

fn bucket(n: usize) -> usize {
    (usize::BITS - n.saturating_sub(1).leading_zeros()).min(31) as usize
}

// SAFETY: every method forwards to mimalloc's GlobalAlloc with the caller's arguments unchanged, so
// Counting keeps its contract; the counters are atomics touched without allocating.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        HEAP_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        HEAP_LIVE[bucket(l.size())].fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract for `alloc` is mimalloc's.
        unsafe { mimalloc::MiMalloc.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        HEAP_BYTES.fetch_sub(l.size(), Ordering::Relaxed);
        HEAP_LIVE[bucket(l.size())].fetch_sub(1, Ordering::Relaxed);
        // SAFETY: `p` came from this allocator, i.e. from mimalloc, with layout `l`.
        unsafe { mimalloc::MiMalloc.dealloc(p, l) }
    }
    unsafe fn alloc_zeroed(&self, l: std::alloc::Layout) -> *mut u8 {
        HEAP_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        HEAP_LIVE[bucket(l.size())].fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract for `alloc_zeroed` is mimalloc's.
        unsafe { mimalloc::MiMalloc.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        HEAP_BYTES.fetch_add(n.wrapping_sub(l.size()), Ordering::Relaxed);
        HEAP_LIVE[bucket(l.size())].fetch_sub(1, Ordering::Relaxed);
        HEAP_LIVE[bucket(n)].fetch_add(1, Ordering::Relaxed);
        // SAFETY: `p` came from mimalloc with layout `l`; the caller's contract for `realloc` is mimalloc's.
        unsafe { mimalloc::MiMalloc.realloc(p, l, n) }
    }
}

/// Bytes allocated now.
pub fn bytes() -> usize {
    HEAP_BYTES.load(Ordering::Relaxed)
}

/// Live allocations by size: (largest size of the bucket, count) for the non-empty buckets.
pub fn live_by_size() -> impl Iterator<Item = (usize, isize)> {
    HEAP_LIVE.iter().enumerate().map(|(k, c)| (1usize << k, c.load(Ordering::Relaxed))).filter(|&(_, c)| c != 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn buckets() {
        assert_eq!([0, 1, 2, 3, 4, 5, 1024, 1025].map(super::bucket), [0, 0, 1, 2, 2, 3, 10, 11]);
        assert_eq!(super::bucket(usize::MAX), 31);
    }
}
