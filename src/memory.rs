// Memory profiling and trimming.
//
// - Rust heap: every allocation goes through `Counting`, so `heap_bytes()` is exactly what Waffle's
//   own data holds right now (messages, images, caches), and `heap_peak()` the most it ever held.
// - Process totals: on Windows the private working set Task Manager shows as "Memory", the working
//   set, committed private bytes, and GPU memory (dedicated and shared) from DXGI; on macOS the
//   footprint Activity Monitor shows as "Memory", which already includes GPU memory (unified memory).
// - Gauges: the caches that can grow (see `report`).
//
// `WAFFLE_MEMORY=1` logs a report every 10 s to stderr; `WAFFLE_MEMORY=<MB>` also sets the
// budget (default 100 MB). Going over the budget is always logged, once per crossing.
//
// `trim()` hands memory Waffle no longer uses back to the system: it compacts the process heaps and,
// on Windows, empties the working set. Pages still in use fault back in when touched, so it's cheap to call when
// the app goes to the background.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub struct Counting;

struct Tally {
    live: AtomicUsize,
    peak: AtomicUsize,
}

impl Tally {
    const fn new() -> Self {
        Self { live: AtomicUsize::new(0), peak: AtomicUsize::new(0) }
    }

    fn grow(&self, by: usize) {
        let now = self.live.fetch_add(by, Ordering::Relaxed) + by;
        self.peak.fetch_max(now, Ordering::Relaxed);
    }

    fn shrink(&self, by: usize) {
        self.live.fetch_sub(by, Ordering::Relaxed);
    }

    fn resize(&self, from: usize, to: usize) {
        if to >= from { self.grow(to - from) } else { self.shrink(from - to) }
    }
}

static HEAP: Tally = Tally::new();

// Large-allocation sites (`WAFFLE_MEMORY_SITES=1`): every live allocation of `BIG` bytes or more
// keeps the return addresses of its call stack, so the report can say which code holds the memory.
// Images, video frames and buffers are all this size; small allocations aren't tracked, which keeps
// the cost to a stack walk per large allocation. Names are only looked up when a report is printed.
// The smallest allocation tracked; WAFFLE_MEMORY_SITES=<KB> lowers or raises it.
static BIG: AtomicUsize = AtomicUsize::new(256 << 10);
const DEPTH: usize = 24;
static SITES: AtomicBool = AtomicBool::new(false);
type Stack = [usize; DEPTH];
static LIVE_BIG: Mutex<Option<HashMap<usize, (usize, Stack)>>> = Mutex::new(None);

thread_local! {
    // Set while the tracker itself runs, so its own allocations aren't tracked (and can't recurse).
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

fn untracked<T>(f: impl FnOnce() -> T) -> Option<T> {
    BUSY.with(|busy| {
        if busy.replace(true) {
            return None;
        }
        let out = f();
        busy.set(false);
        Some(out)
    })
}

// The largest single allocation of `HUGE` bytes or more seen so far, and its call stack: caught
// whether or not site tracking is on, since one this size is almost certainly a bug.
const HUGE: usize = 256 << 20;
static LARGEST: Mutex<Option<(usize, Stack)>> = Mutex::new(None);

fn note_huge(size: usize) {
    untracked(|| {
        let mut stack = [0usize; DEPTH];
        let mut n = 0;
        backtrace::trace(|frame| {
            stack[n] = frame.ip() as usize;
            n += 1;
            n < DEPTH
        });
        if let Ok(mut largest) = LARGEST.lock()
            && largest.is_none_or(|(s, _)| size > s)
        {
            *largest = Some((size, stack));
        }
    });
}

fn track(ptr: *mut u8, size: usize) {
    if size >= HUGE {
        note_huge(size);
    }
    if size < BIG.load(Ordering::Relaxed) || !SITES.load(Ordering::Relaxed) {
        return;
    }
    untracked(|| {
        let mut stack = [0usize; DEPTH];
        let mut n = 0;
        backtrace::trace(|frame| {
            stack[n] = frame.ip() as usize;
            n += 1;
            n < DEPTH
        });
        if let Ok(mut live) = LIVE_BIG.lock() {
            live.get_or_insert_with(HashMap::new).insert(ptr as usize, (size, stack));
        }
    });
}

fn untrack(ptr: *mut u8, size: usize) {
    if size < BIG.load(Ordering::Relaxed) || !SITES.load(Ordering::Relaxed) {
        return;
    }
    untracked(|| {
        if let Ok(mut live) = LIVE_BIG.lock()
            && let Some(live) = live.as_mut()
        {
            live.remove(&(ptr as usize));
        }
    });
}

// The tracker and allocator themselves, and the standard library's plumbing.
const SKIP: [&str; 11] = ["memory::track", "memory::untracked", "memory::impl$", "alloc::", "core::", "std::", "backtrace", "__rust", "hashbrown", "rust_begin", "<unknown>"];

// The first `count` frames of `stack` that aren't the allocator or the standard library. One address
// can stand for several functions when some were inlined into others: innermost first.
fn describe(stack: &Stack, count: usize, names: &mut HashMap<usize, Vec<String>>) -> String {
    let mut kept: Vec<String> = Vec::new();
    'frames: for ip in stack.iter().copied().filter(|ip| *ip != 0) {
        let at = names.entry(ip).or_insert_with(|| {
            let mut found = Vec::new();
            backtrace::resolve(ip as *mut std::ffi::c_void, |symbol| {
                if let Some(n) = symbol.name() {
                    found.push(format!("{n:#}"));
                }
            });
            found
        });
        for name in at.iter() {
            if !SKIP.iter().any(|s| name.contains(s)) && kept.last() != Some(name) {
                kept.push(name.clone());
                if kept.len() == count {
                    break 'frames;
                }
            }
        }
    }
    if kept.is_empty() { "(no symbols)".to_owned() } else { kept.join(" < ") }
}

// The live large allocations grouped by where they were made, biggest first.
fn sites() -> Vec<(usize, usize, String)> {
    untracked(|| {
        let live: Vec<(usize, Stack)> = LIVE_BIG.lock().ok().and_then(|l| l.as_ref().map(|l| l.values().copied().collect())).unwrap_or_default();
        let mut names = HashMap::new();
        let mut by_site: HashMap<String, (usize, usize)> = HashMap::new();
        for (size, stack) in live {
            let entry = by_site.entry(describe(&stack, 3, &mut names)).or_default();
            entry.0 += size;
            entry.1 += 1;
        }
        let mut sites: Vec<(usize, usize, String)> = by_site.into_iter().map(|(site, (bytes, count))| (bytes, count, site)).collect();
        sites.sort_by_key(|site| std::cmp::Reverse(site.0));
        sites
    })
    .unwrap_or_default()
}

// Where the largest single allocation over `HUGE` came from, if there was one.
pub fn largest_allocation() -> Option<String> {
    let (size, stack) = (*LARGEST.lock().ok()?)?;
    untracked(|| format!("largest single allocation {:.0} MB at {}", size as f64 / MB, describe(&stack, 6, &mut HashMap::new())))
}

// SAFETY: forwards every call to the system allocator unchanged; only counts sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            HEAP.grow(layout.size());
            track(ptr, layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            HEAP.grow(layout.size());
            track(ptr, layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        untrack(ptr, layout.size());
        unsafe { System.dealloc(ptr, layout) };
        HEAP.shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            untrack(ptr, layout.size());
            HEAP.resize(layout.size(), new_size);
            track(new, new_size);
        }
        new
    }
}

pub fn heap_bytes() -> usize {
    #[cfg(feature = "dhat-heap")]
    return dhat::HeapStats::get().curr_bytes;
    #[cfg(not(feature = "dhat-heap"))]
    HEAP.live.load(Ordering::Relaxed)
}

pub fn heap_peak() -> usize {
    #[cfg(feature = "dhat-heap")]
    return dhat::HeapStats::get().max_bytes;
    #[cfg(not(feature = "dhat-heap"))]
    HEAP.peak.load(Ordering::Relaxed)
}

// Records allocation sites for WAFFLE_DHAT_SECS (default 60) seconds, then writes dhat-heap.json in
// the working directory. Use the app normally meanwhile; the file shows what held memory at the peak.
#[cfg(feature = "dhat-heap")]
pub fn start_heap_profile() {
    let secs = std::env::var("WAFFLE_DHAT_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let profiler = dhat::Profiler::new_heap();
    let _ = std::thread::Builder::new().name("heap-profile".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        drop(profiler);
        eprintln!("heap profile written to dhat-heap.json");
    });
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Process {
    // What Task Manager (Windows) or Activity Monitor (macOS, the footprint) shows as "Memory".
    pub private_working_set: u64,
    pub working_set: u64,
    // Committed private memory, resident or paged out.
    pub private: u64,
    pub gpu_dedicated: u64,
    pub gpu_shared: u64,
}

#[cfg(target_os = "windows")]
pub fn process() -> Option<Process> {
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
    // SAFETY: the counters struct is as large as `cb` says; EX2 extends the base struct.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS, counters.cb) }.ok()?;
    let (gpu_dedicated, gpu_shared) = gpu_usage();
    Some(Process {
        private_working_set: counters.PrivateWorkingSetSize as u64,
        working_set: counters.WorkingSetSize as u64,
        private: counters.PrivateUsage as u64,
        gpu_dedicated,
        gpu_shared,
    })
}

// This process's video memory on every adapter, as DXGI reports it (dedicated, shared).
#[cfg(target_os = "windows")]
fn gpu_usage() -> (u64, u64) {
    use windows::{
        Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory1,
        },
        core::Interface,
    };
    // SAFETY: plain DXGI queries; the info structs live for each call.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return (0, 0) };
        let (mut local, mut shared) = (0, 0);
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(adapter) = adapter.cast::<IDXGIAdapter3>() else { continue };
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() {
                local += info.CurrentUsage;
            }
            if adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut info).is_ok() {
                shared += info.CurrentUsage;
            }
        }
        (local, shared)
    }
}

#[cfg(target_os = "macos")]
pub fn process() -> Option<Process> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: RUSAGE_INFO_V2 fills exactly a rusage_info_v2.
    let status = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
    if status != 0 {
        return None;
    }
    // SAFETY: proc_pid_rusage succeeded, so it wrote the struct.
    let info = unsafe { info.assume_init() };
    // GPU memory is unified and already counted in the footprint, so the gpu fields stay 0.
    Some(Process { private_working_set: info.ri_phys_footprint, working_set: info.ri_resident_size, private: info.ri_phys_footprint, gpu_dedicated: 0, gpu_shared: 0 })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn process() -> Option<Process> {
    None
}

// Gives memory Waffle has freed back to the system without paging anything out: cheap, and
// nothing has to come back from disk afterwards.
#[cfg(target_os = "windows")]
pub fn compact() {
    use windows::Win32::{
        Foundation::HANDLE,
        System::Memory::{GetProcessHeaps, HEAP_NONE, HeapCompact},
    };
    // SAFETY: compacting our own heaps is always allowed.
    unsafe {
        let mut heaps = [HANDLE::default(); 64];
        let count = GetProcessHeaps(&mut heaps) as usize;
        for heap in &heaps[..count.min(heaps.len())] {
            HeapCompact(*heap, HEAP_NONE);
        }
    }
}

// Returns malloc's free pages to the system; the allocator otherwise keeps them after a peak.
#[cfg(target_os = "macos")]
pub fn compact() {
    unsafe extern "C" {
        // <malloc/malloc.h>: a null zone means every zone, a goal of 0 as much as possible.
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: documented to be callable at any time from any thread.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn compact() {}

// `compact`, then empties the working set: Task Manager's number drops to almost nothing, but every
// page touched afterwards comes back from memory or disk, so this is only for when nobody is looking.
#[cfg(target_os = "windows")]
pub fn trim() {
    use windows::Win32::{
        Foundation::HANDLE,
        System::{
            Memory::{GetProcessHeaps, HEAP_NONE, HeapCompact},
            ProcessStatus::K32EmptyWorkingSet,
            Threading::GetCurrentProcess,
        },
    };
    // SAFETY: compacting heaps and trimming our own working set are always allowed; pages in use come back on demand.
    unsafe {
        let mut heaps = [HANDLE::default(); 64];
        let count = GetProcessHeaps(&mut heaps) as usize;
        for heap in &heaps[..count.min(heaps.len())] {
            HeapCompact(*heap, HEAP_NONE);
        }
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(target_os = "windows"))]
pub fn trim() {
    compact();
}

const MB: f64 = 1024. * 1024.;

// One line: process totals, the Rust heap, and the caches that can grow.
pub fn report() -> String {
    let process = process().map_or_else(String::new, |p| {
        format!(
            "memory {:.0} MB (working set {:.0}, committed {:.0}), gpu {:.0} MB + shared {:.0}; ",
            p.private_working_set as f64 / MB,
            p.working_set as f64 / MB,
            p.private as f64 / MB,
            p.gpu_dedicated as f64 / MB,
            p.gpu_shared as f64 / MB
        )
    });
    let gifs = crate::gif::stats();
    let images = crate::images::last_stats();
    let (renders, layer) = crate::gif::draws_since_last();
    format!(
        "{process}rust heap {:.1} MB (peak {:.1}); images {} decoded {:.1} MB, downloads {:.1} MB; {}; gifs playing {}, clip cache {:.1} MB; app renders {renders}, gif layer draws {layer}",
        heap_bytes() as f64 / MB,
        heap_peak() as f64 / MB,
        images.images,
        images.decoded_bytes as f64 / MB,
        crate::downloads::bytes() as f64 / MB,
        crate::store::report(),
        gifs.playing,
        gifs.clip_cache_bytes as f64 / MB
    )
}

fn budget() -> (bool, u64) {
    match std::env::var("WAFFLE_MEMORY") {
        Ok(v) => (true, v.trim().parse::<u64>().ok().filter(|mb| *mb > 1).unwrap_or(100)),
        Err(_) => (false, 100),
    }
}

// The biggest holders of large allocations, one per line (`WAFFLE_MEMORY_SITES=1`).
pub fn sites_report(top: usize) -> String {
    let sites = sites();
    let total: usize = sites.iter().map(|s| s.0).sum();
    let mut out = format!("large allocations: {:.1} MB live", total as f64 / MB);
    for (bytes, count, site) in sites.into_iter().take(top) {
        out.push_str(&format!("\n  {:7.1} MB in {count:4}  {site}", bytes as f64 / MB));
    }
    out
}

// Runs for the life of the app on a plain thread (sampling is a few syscalls every 10 s; with sites
// on, every 30 s it also prints where the large allocations are).
pub fn start_reporting() {
    let sites = std::env::var("WAFFLE_MEMORY_SITES").ok();
    if let Some(kb) = sites.as_deref().and_then(|v| v.trim().parse::<usize>().ok()).filter(|kb| *kb > 1) {
        BIG.store(kb << 10, Ordering::Relaxed);
    }
    SITES.store(sites.is_some(), Ordering::Relaxed);
    let sites_on = SITES.load(Ordering::Relaxed);
    let (verbose, budget_mb) = budget();
    let _ = std::thread::Builder::new().name("memory-report".into()).spawn(move || {
        let over = AtomicBool::new(false);
        let mut round = 0u32;
        let mut reported_huge = String::new();
        loop {
            std::thread::sleep(Duration::from_secs(10));
            round += 1;
            if sites_on && round.is_multiple_of(3) {
                eprintln!("{}", sites_report(12));
            }
            let used = process().map_or(0, |p| p.private_working_set) / (1024 * 1024);
            let is_over = used > budget_mb;
            if verbose || (is_over && !over.swap(true, Ordering::Relaxed)) {
                eprintln!("{}{}", if is_over { format!("over the {budget_mb} MB budget: ") } else { String::new() }, report());
            }
            if let Some(huge) = largest_allocation()
                && huge != reported_huge
            {
                eprintln!("{huge}");
                reported_huge = huge;
            }
            if !is_over {
                over.store(false, Ordering::Relaxed);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Other test threads allocate through the global counter, so this checks the tally on its own.
    #[test]
    fn tally_tracks_live_and_peak() {
        let tally = Tally::new();
        tally.grow(4096);
        tally.resize(4096, 1024);
        assert_eq!(tally.live.load(Ordering::Relaxed), 1024);
        tally.resize(1024, 2048);
        tally.shrink(2048);
        assert_eq!(tally.live.load(Ordering::Relaxed), 0);
        assert_eq!(tally.peak.load(Ordering::Relaxed), 4096);
        assert!(heap_bytes() > 0, "the global allocator counts this test binary too");
    }

    // A large allocation made here shows up under this test's name, and goes when freed.
    #[test]
    fn large_allocation_sites() {
        SITES.store(true, Ordering::Relaxed);
        let block = std::hint::black_box(vec![7u8; 4 << 20]);
        let report = sites_report(50);
        SITES.store(false, Ordering::Relaxed);
        assert!(report.contains("large_allocation_sites"), "{report}");
        drop(block);
    }

    // An allocation of `HUGE` or more is caught with its call stack even with site tracking off.
    #[test]
    fn catches_huge_allocations() {
        let block = std::hint::black_box(vec![0u8; 300 << 20]);
        let report = largest_allocation().expect("caught");
        assert!(report.contains("300 MB") && report.contains("catches_huge_allocations"), "{report}");
        drop(block);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_process_memory_macos() {
        let p = process().expect("footprint");
        assert!(p.private_working_set > 1024 * 1024 && p.working_set > 1024 * 1024, "{p:?}");
        compact();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn reads_process_memory() {
        let p = process().expect("process counters");
        assert!(p.working_set > 1024 * 1024 && p.private > 0);
        trim();
        assert!(report().contains("rust heap"));
    }
}
