//! Process-level measurements: heap allocations, CPU time and memory footprint.
//!
//! Every measurement here is taken the same way in every configuration, so its own cost cancels
//! out of the comparison.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// The system allocator, counting what passes through it.
pub struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static PEAK_LIVE_BYTES: AtomicU64 = AtomicU64::new(0);

fn grow(bytes: u64) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED_BYTES.fetch_add(bytes, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
}

fn shrink(bytes: u64) {
    LIVE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            grow(layout.size() as u64);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            grow(layout.size() as u64);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        shrink(layout.size() as u64);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            shrink(layout.size() as u64);
            grow(new_size as u64);
        }
        new_pointer
    }
}

/// Heap counters at one moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeapSample {
    pub allocations: u64,
    pub allocated_bytes: u64,
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
}

pub fn heap() -> HeapSample {
    HeapSample {
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        live_bytes: LIVE_BYTES.load(Ordering::Relaxed),
        peak_live_bytes: PEAK_LIVE_BYTES.load(Ordering::Relaxed),
    }
}

/// CPU time the calling thread has used, where the target can tell.
#[cfg(unix)]
pub fn thread_cpu() -> Option<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    (result == 0).then(|| Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

/// CPU time the whole process has used, every thread, user and system.
#[cfg(unix)]
pub fn process_cpu() -> Option<Duration> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let time =
        |value: libc::timeval| Duration::new(value.tv_sec as u64, value.tv_usec as u32 * 1_000);
    (result == 0).then(|| time(usage.ru_utime) + time(usage.ru_stime))
}

#[cfg(windows)]
fn filetime(time: windows_sys::Win32::Foundation::FILETIME) -> Duration {
    // 100-nanosecond intervals.
    Duration::from_nanos(((time.dwHighDateTime as u64) << 32 | time.dwLowDateTime as u64) * 100)
}

#[cfg(windows)]
pub fn thread_cpu() -> Option<Duration> {
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::*};
    let mut times: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [created, exited, kernel, user] = &mut times;
    let ok = unsafe { GetThreadTimes(GetCurrentThread(), created, exited, kernel, user) };
    (ok != 0).then(|| filetime(times[2]) + filetime(times[3]))
}

#[cfg(windows)]
pub fn process_cpu() -> Option<Duration> {
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::*};
    let mut times: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [created, exited, kernel, user] = &mut times;
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) };
    (ok != 0).then(|| filetime(times[2]) + filetime(times[3]))
}

/// The browser does not tell a page its CPU time; the web runner reads Chrome's own task
/// timings over the DevTools protocol instead.
#[cfg(target_family = "wasm")]
pub fn thread_cpu() -> Option<Duration> {
    None
}

#[cfg(target_family = "wasm")]
pub fn process_cpu() -> Option<Duration> {
    None
}

/// The process's memory at one moment, in bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemorySample {
    /// What macOS calls the process's memory (Activity Monitor's "Memory"): dirty and
    /// compressed pages plus the GPU allocations the process owns. Resident memory elsewhere.
    pub footprint: u64,
    /// The highest footprint so far.
    pub footprint_peak: u64,
    /// Resident pages.
    pub resident: u64,
}

#[cfg(target_vendor = "apple")]
pub fn memory() -> MemorySample {
    // `task_vm_info_data_t` up to `ledger_phys_footprint_peak` (revision 2 and later).
    #[repr(C)]
    #[derive(Default)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        resident_size_peak: u64,
        device: u64,
        device_peak: u64,
        internal: u64,
        internal_peak: u64,
        external: u64,
        external_peak: u64,
        reusable: u64,
        reusable_peak: u64,
        purgeable_volatile_pmap: u64,
        purgeable_volatile_resident: u64,
        purgeable_volatile_virtual: u64,
        compressed: u64,
        compressed_peak: u64,
        compressed_lifetime: u64,
        phys_footprint: u64,
        min_address: u64,
        max_address: u64,
        ledger_phys_footprint_peak: i64,
    }
    const TASK_VM_INFO: i32 = 22;
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    }
    let mut info = TaskVmInfo::default();
    let mut count = (std::mem::size_of::<TaskVmInfo>() / std::mem::size_of::<i32>()) as u32;
    let result = unsafe {
        task_info(
            mach_task_self_,
            TASK_VM_INFO,
            &mut info as *mut TaskVmInfo as *mut i32,
            &mut count,
        )
    };
    if result != 0 {
        return MemorySample::default();
    }
    MemorySample {
        footprint: info.phys_footprint,
        footprint_peak: (info.ledger_phys_footprint_peak.max(0) as u64).max(info.phys_footprint),
        resident: info.resident_size,
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn memory() -> MemorySample {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|kilobytes| kilobytes.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    MemorySample {
        footprint: field("VmRSS:"),
        footprint_peak: field("VmHWM:"),
        resident: field("VmRSS:"),
    }
}

/// Percentiles of a set of samples, in the samples' unit.
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

pub fn summarize(samples: &[f64]) -> Summary {
    if samples.is_empty() {
        return Summary::default();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |quantile: f64| {
        let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
        sorted[index]
    };
    Summary {
        mean: sorted.iter().sum::<f64>() / sorted.len() as f64,
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: *sorted.last().unwrap(),
    }
}

#[cfg(windows)]
pub fn memory() -> MemorySample {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
            counters.cb,
        )
    };
    if ok == 0 {
        return MemorySample::default();
    }
    MemorySample {
        // Private commit is what Task Manager calls a process's memory.
        footprint: counters.PrivateUsage as u64,
        footprint_peak: counters.PeakPagefileUsage as u64,
        resident: counters.WorkingSetSize as u64,
    }
}

/// The WebAssembly memory the module has grown to; it never shrinks, so it is its own peak.
#[cfg(target_family = "wasm")]
pub fn memory() -> MemorySample {
    let bytes = core::arch::wasm32::memory_size(0) as u64 * 65_536;
    MemorySample {
        footprint: bytes,
        footprint_peak: bytes,
        resident: bytes,
    }
}

/// Starts recording how long the GPU spends on each frame, where the renderer can tell (Metal).
pub fn gpu_times_begin() {
    #[cfg(target_os = "macos")]
    gpui_apple::metal_renderer::record_gpu_frame_times(true);
}

/// The GPU time of each frame since [`gpu_times_begin`], in milliseconds; none where the renderer
/// cannot tell.
pub fn gpu_times_take() -> Vec<f64> {
    #[cfg(target_os = "macos")]
    {
        gpui_apple::metal_renderer::record_gpu_frame_times(false);
        return gpui_apple::metal_renderer::take_gpu_frame_times();
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}
