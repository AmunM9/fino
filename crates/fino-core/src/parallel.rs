//! Batch scheduling.
//!
//! Each photo already parallelizes internally (the metric uses every core), so a batch must
//! not run inside a small thread pool — that would starve the metric. Instead a few worker
//! threads pull photos from a shared queue; their inner work lands on the global rayon pool.
//! The cap bounds memory: a 24 MP photo in flight needs roughly 1 GB.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Budget per photo in flight: a 24 MP frame peaks near 0.9 GB; the rest is headroom for
/// larger frames and for everything else running on the Mac.
const BYTES_PER_PHOTO: u64 = 3 << 30;
const MAX_IN_FLIGHT: usize = 5;

/// Photos processed concurrently: enough to overlap single-threaded encodes with the
/// parallel metric (measured best around half the cores), bounded by installed memory.
pub fn default_in_flight() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let by_memory =
        total_memory().map_or(MAX_IN_FLIGHT, |bytes| (bytes / BYTES_PER_PHOTO) as usize);
    (cores / 2).min(by_memory).clamp(2, MAX_IN_FLIGHT)
}

#[cfg(target_os = "macos")]
fn total_memory() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: "hw.memsize" is a NUL-terminated name; `bytes`/`len` describe a valid u64 out
    // buffer of the size sysctl expects for this key.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && bytes > 0).then_some(bytes)
}

#[cfg(windows)]
fn total_memory() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is a plain C struct; the API requires `dwLength` to be set to
    // its size before the call and fills the rest.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn total_memory() -> Option<u64> {
    None
}

/// Runs `f` over `items` with at most `in_flight` calls at once. Results keep input order.
pub fn map_bounded<T, R, F>(items: &[T], in_flight: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..in_flight.clamp(1, items.len().max(1)) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let value = f(i, item);
                if let Ok(mut slots) = results.lock() {
                    slots[i] = Some(value);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn reads_installed_memory() {
        let bytes = total_memory().expect("installed memory");
        assert!(bytes >= 1 << 30, "{bytes}");
    }
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn keeps_order_and_never_exceeds_the_cap() {
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items: Vec<u32> = (0..40).collect();
        let out = map_bounded(&items, 3, |_, &x| {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(2));
            live.fetch_sub(1, Ordering::SeqCst);
            x * 2
        });
        assert_eq!(out, items.iter().map(|x| x * 2).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= 3);
    }

    #[test]
    fn in_flight_is_bounded() {
        let n = default_in_flight();
        assert!((2..=MAX_IN_FLIGHT).contains(&n));
    }

    #[test]
    fn handles_empty_input() {
        let out: Vec<u8> = map_bounded(&[] as &[u8], 4, |_, &x| x);
        assert!(out.is_empty());
    }
}
