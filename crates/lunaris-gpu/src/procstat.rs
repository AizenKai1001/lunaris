//! What the machine was doing while a picture was drawn (Linux only; nothing elsewhere).
//!
//! With `LUNARIS_GPU_PROFILE=1` the picture's numbers carry `proc.*` and `sys.*` entries next to the `cpu_ms.*` laps, so a slow picture says why on its own:
//! a thread that was slow but busy (`thread_cpu_ms` close to the wall time) did the work itself; one that was slow and idle waited (the card, a lock, the
//! disk); `thread_majflt` above zero is memory read back from swap or the disk; `thread_ctxsw_invol` is the scheduler taking the processor away (other
//! jobs on the machine); `sys.mem_available_mb` and `sys.swap_used_mb` are the machine's memory at that moment.

/// A reading of the calling thread and of the machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    /// Processor time of the calling thread in milliseconds (user + system).
    pub thread_cpu_ms: f64,
    /// Processor time of the whole process in milliseconds.
    pub process_cpu_ms: f64,
    pub thread_majflt: u64,
    pub thread_minflt: u64,
    pub thread_ctxsw_vol: u64,
    pub thread_ctxsw_invol: u64,
}

/// The machine's state at one moment (not a difference).
#[derive(Clone, Copy, Debug, Default)]
pub struct Machine {
    pub load1: f64,
    pub mem_available_mb: f64,
    pub swap_used_mb: f64,
}

/// The fields after the process name of a `/proc/.../stat` line: (utime + stime in ticks, minflt, majflt).
fn parse_stat(text: &str) -> Option<(u64, u64, u64)> {
    // the name is in parentheses and may hold spaces or parentheses: everything after the last one is plain numbers, the first of them is field 3
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let at = |n: usize| -> Option<u64> { f.get(n - 3)?.parse().ok() };
    Some((at(14)? + at(15)?, at(10)?, at(12)?))
}

fn status_field(text: &str, key: &str) -> u64 {
    text.lines().find_map(|l| l.strip_prefix(key)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()).unwrap_or(0)
}

/// Reads the calling thread and the process; `None` where `/proc` does not exist.
pub fn snapshot() -> Option<Snapshot> {
    // a clock tick is a hundredth of a second on every Linux that matters (USER_HZ)
    let ms = |ticks: u64| ticks as f64 * 10.0;
    let thread = parse_stat(&std::fs::read_to_string("/proc/thread-self/stat").ok()?)?;
    let process = parse_stat(&std::fs::read_to_string("/proc/self/stat").ok()?)?;
    let status = std::fs::read_to_string("/proc/thread-self/status").unwrap_or_default();
    Some(Snapshot {
        thread_cpu_ms: ms(thread.0),
        process_cpu_ms: ms(process.0),
        thread_majflt: thread.2,
        thread_minflt: thread.1,
        thread_ctxsw_vol: status_field(&status, "voluntary_ctxt_switches:"),
        thread_ctxsw_invol: status_field(&status, "nonvoluntary_ctxt_switches:"),
    })
}

/// The machine's load and memory now.
pub fn machine() -> Option<Machine> {
    let load1 = std::fs::read_to_string("/proc/loadavg").ok()?.split_whitespace().next()?.parse().ok()?;
    let mem = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| status_field(&mem, key) as f64;
    Some(Machine { load1, mem_available_mb: kb("MemAvailable:") / 1024.0, swap_used_mb: (kb("SwapTotal:") - kb("SwapFree:")) / 1024.0 })
}

/// The entries a picture's numbers get from two readings, `wall_ms` being the time between them.
pub fn entries(before: &Snapshot, after: &Snapshot, wall_ms: f64) -> Vec<(String, f64)> {
    let mut out = vec![
        ("proc.wall_ms".to_string(), wall_ms),
        ("proc.thread_cpu_ms".to_string(), (after.thread_cpu_ms - before.thread_cpu_ms).max(0.0)),
        ("proc.process_cpu_ms".to_string(), (after.process_cpu_ms - before.process_cpu_ms).max(0.0)),
        ("proc.thread_majflt".to_string(), after.thread_majflt.saturating_sub(before.thread_majflt) as f64),
        ("proc.thread_minflt".to_string(), after.thread_minflt.saturating_sub(before.thread_minflt) as f64),
        ("proc.thread_ctxsw_vol".to_string(), after.thread_ctxsw_vol.saturating_sub(before.thread_ctxsw_vol) as f64),
        ("proc.thread_ctxsw_invol".to_string(), after.thread_ctxsw_invol.saturating_sub(before.thread_ctxsw_invol) as f64),
    ];
    if let Some(m) = machine() {
        out.push(("sys.load1".to_string(), m.load1));
        out.push(("sys.mem_available_mb".to_string(), m.mem_available_mb));
        out.push(("sys.swap_used_mb".to_string(), m.swap_used_mb));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stat_line_with_a_hard_name_reads() {
        // the name holds a space and a closing parenthesis; utime 120, stime 30 ticks, minflt 77, majflt 5
        let line = "4242 (lunaris (gpu) x) S 1 4242 4242 0 -1 4194560 77 0 5 0 120 30 0 0 20 0 9 0 12345 1000 200 18446744073709551615";
        assert_eq!(parse_stat(line), Some((150, 77, 5)));
        assert_eq!(parse_stat("no parentheses"), None);
        assert_eq!(parse_stat("1 (x) S 1 2"), None);
    }

    #[test]
    fn the_memory_file_reads() {
        let mem = "MemTotal:       32000000 kB\nMemAvailable:    8192000 kB\nSwapTotal:       2097152 kB\nSwapFree:        1048576 kB\n";
        assert_eq!(status_field(mem, "MemAvailable:"), 8_192_000);
        assert_eq!(status_field(mem, "SwapTotal:") - status_field(mem, "SwapFree:"), 1_048_576);
        assert_eq!(status_field(mem, "Nope:"), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_running_thread_can_be_read() {
        let a = snapshot().expect("proc exists on linux");
        let mut x = 0u64;
        for i in 0..40_000_000u64 {
            x = x.wrapping_add(i * i);
        }
        assert!(x != 1);
        let b = snapshot().expect("proc exists on linux");
        let e = entries(&a, &b, 1.0);
        assert!(e.iter().any(|(k, v)| k == "proc.thread_cpu_ms" && *v >= 0.0));
        assert!(e.iter().any(|(k, _)| k == "sys.mem_available_mb"));
    }
}
