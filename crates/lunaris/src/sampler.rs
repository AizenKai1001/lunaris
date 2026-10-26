//! A sampling profiler for the whole process: `LUNARIS_SAMPLE=samples.txt lunaris server ...` (Linux). Every millisecond of processor time (`ITIMER_PROF`) the thread that is running is interrupted and
//! its call stack (the instruction it stopped at and the return addresses above it, 20 deep) is written to a buffer; when the command ends the buffer is written to the file, one line a sample.
//! `tools/sample_report.py` turns the file into the functions that took the time (self and with everything they called, inlined functions included), using `addr2line` on the binary, which is
//! why a release build with line tables (`debug = 1`, the workspace's default) is enough.
//!
//! The stage timers of the simulation (`profile`, `--profile-sim`) say WHICH part of a tick is slow; this says which function inside it, where `perf` is not allowed (an unprivileged user on a locked-down kernel).
//! The timer costs a few microseconds a millisecond of work (a few percent); nothing here is in the simulation's way and nothing in it is read back by the simulation. Off, it is not there at all.
//!
//! `%p` in the file name is replaced by the process number (a command that starts others gives each its own file). `LUNARIS_SAMPLE_HZ` (default 1000, 10 to 4000) is the rate in samples per second of processor time, `LUNARIS_SAMPLE_MAX` (default 400,000) the most samples kept.

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::atomic::{AtomicUsize, Ordering};

    const DEPTH: usize = 20;
    const WORDS: usize = DEPTH + 2; // thread id, depth, the addresses
    const ITIMER_PROF: i32 = 2;
    const SIGPROF: i32 = 27;
    const SYS_GETTID: i64 = 186;

    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }
    #[repr(C)]
    struct Itimerval {
        interval: Timeval,
        value: Timeval,
    }

    extern "C" {
        fn backtrace(buf: *mut *mut core::ffi::c_void, size: i32) -> i32;
        fn setitimer(which: i32, new: *const Itimerval, old: *mut Itimerval) -> i32;
        fn signal(sig: i32, handler: usize) -> usize;
        fn syscall(num: i64, ...) -> i64;
    }

    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static CAP: AtomicUsize = AtomicUsize::new(0);
    static BUF: AtomicUsize = AtomicUsize::new(0);
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    extern "C" fn on_prof(_sig: i32) {
        let i = COUNT.fetch_add(1, Ordering::Relaxed);
        if i >= CAP.load(Ordering::Relaxed) {
            return;
        }
        let base = BUF.load(Ordering::Relaxed) as *mut usize;
        let mut frames = [core::ptr::null_mut::<core::ffi::c_void>(); DEPTH];
        // SAFETY: `backtrace` fills at most DEPTH pointers; the slot `i` of the buffer is ours alone (the index came from a fetch_add) and lies inside the allocation (i < CAP).
        unsafe {
            let n = backtrace(frames.as_mut_ptr(), DEPTH as i32).max(0) as usize;
            let slot = base.add(i * WORDS);
            *slot = syscall(SYS_GETTID) as usize;
            *slot.add(1) = n;
            for (k, f) in frames.iter().take(n).enumerate() {
                *slot.add(2 + k) = *f as usize;
            }
        }
    }

    pub fn begin() {
        let Ok(path) = std::env::var("LUNARIS_SAMPLE") else { return };
        if path.is_empty() {
            return;
        }
        let hz: u64 = std::env::var("LUNARIS_SAMPLE_HZ").ok().and_then(|v| v.parse().ok()).map(|v: u64| v.clamp(10, 4000)).unwrap_or(1000);
        let cap: usize = std::env::var("LUNARIS_SAMPLE_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(400_000);
        let buf = vec![0usize; cap * WORDS].leak();
        BUF.store(buf.as_mut_ptr() as usize, Ordering::Relaxed);
        CAP.store(cap, Ordering::Relaxed);
        // (`%p` in the name is the process number: a command that starts others, a server and its bots, gives each its own file)
        let _ = PATH.set(path.replace("%p", &std::process::id().to_string()));
        // (the first call of `backtrace` loads the unwinder, which a signal handler must not do)
        let mut warm = [core::ptr::null_mut::<core::ffi::c_void>(); 4];
        // SAFETY: as in the handler; the timer and the handler are set up once, before the command runs.
        unsafe {
            backtrace(warm.as_mut_ptr(), 4);
            signal(SIGPROF, on_prof as *const () as usize);
            let us = (1_000_000 / hz) as i64;
            let t = Itimerval { interval: Timeval { sec: 0, usec: us }, value: Timeval { sec: 0, usec: us } };
            setitimer(ITIMER_PROF, &t, core::ptr::null_mut());
        }
    }

    pub fn end() {
        let Some(path) = PATH.get() else { return };
        // SAFETY: switches the timer off.
        unsafe {
            let zero = Itimerval { interval: Timeval { sec: 0, usec: 0 }, value: Timeval { sec: 0, usec: 0 } };
            setitimer(ITIMER_PROF, &zero, core::ptr::null_mut());
        }
        let n = COUNT.load(Ordering::Relaxed).min(CAP.load(Ordering::Relaxed));
        let base = BUF.load(Ordering::Relaxed) as *const usize;
        // the load address of the executable, to turn the addresses into offsets in the file (a position independent executable is loaded anywhere)
        let exe = std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let load = maps.lines().find(|l| l.ends_with(&exe) && l.split_whitespace().nth(2) == Some("00000000")).and_then(|l| usize::from_str_radix(l.split('-').next().unwrap_or("0"), 16).ok()).unwrap_or(0);
        let mut out = String::with_capacity(n * 64);
        out.push_str(&format!("# lunaris samples: exe {exe} load {load:x} samples {n}\n"));
        // the code the process runs: its own and the libraries' (memcpy, malloc ... are where the time of many a function really goes)
        for l in maps.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() >= 6 && f[1].starts_with("r-x") && f[5].starts_with('/') {
                let mut r = f[0].split('-');
                out.push_str(&format!("# map {} {} {} {}\n", r.next().unwrap_or("0"), r.next().unwrap_or("0"), f[2], f[5]));
            }
        }
        for i in 0..n {
            // SAFETY: slot i was written by the handler (i < n <= CAP) and the timer is off.
            let slot = unsafe { std::slice::from_raw_parts(base.add(i * WORDS), WORDS) };
            let depth = slot[1].min(DEPTH);
            out.push_str(&format!("{}", slot[0]));
            for a in &slot[2..2 + depth] {
                out.push_str(&format!(" {:x}", a));
            }
            out.push('\n');
        }
        match std::fs::write(path, out) {
            Ok(()) => eprintln!("sampler: {n} samples written to {path} (tools/sample_report.py turns them into functions)"),
            Err(e) => eprintln!("sampler: could not write {path}: {e}"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn begin() {
        if std::env::var_os("LUNARIS_SAMPLE").is_some() {
            eprintln!("sampler: LUNARIS_SAMPLE works on Linux only");
        }
    }
    pub fn end() {}
}

pub use imp::{begin, end};
