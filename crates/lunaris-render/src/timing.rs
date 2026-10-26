//! What takes long when a streamed world is drawn, for whoever tunes it: with the environment variable `LUNARIS_TIMING=1` every guarded step that takes 20 ms or more (`LUNARIS_TIMING_MIN` sets the limit) says so on stderr
//! (`[timing] upload texture 8192x8192 1450 ms`). With `LUNARIS_TIMING=sum` nothing is printed: the times of every step add up under its name, and `take_sums` hands them over (`lunaris play --perf` prints them
//! once a second). Without the variable the guard is a branch on a cached flag.
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Off,
    Print,
    Sum,
}

fn mode() -> Mode {
    static MODE: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("LUNARIS_TIMING") {
        Ok(v) if v == "sum" => Mode::Sum,
        Ok(v) if v != "0" && !v.is_empty() => Mode::Print,
        _ => Mode::Off,
    })
}

/// The least a step must take to be reported (milliseconds; `LUNARIS_TIMING_MIN`, default 20).
fn min_ms() -> f32 {
    static MIN: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *MIN.get_or_init(|| std::env::var("LUNARIS_TIMING_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0))
}

fn sums() -> &'static std::sync::Mutex<Vec<(String, f32)>> {
    static SUMS: std::sync::OnceLock<std::sync::Mutex<Vec<(String, f32)>>> = std::sync::OnceLock::new();
    SUMS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// The milliseconds added up per step since the last call (empty unless `LUNARIS_TIMING=sum`).
pub fn take_sums() -> Vec<(String, f32)> {
    std::mem::take(&mut *sums().lock().unwrap_or_else(|e| e.into_inner()))
}

/// Times the scope it lives in.
pub struct Timed {
    start: Option<(Instant, String)>,
}

impl Timed {
    /// `what` is only worked out when timing is on.
    pub fn new(what: impl FnOnce() -> String) -> Timed {
        Timed { start: (mode() != Mode::Off).then(|| (Instant::now(), what())) }
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        if let Some((t, what)) = self.start.take() {
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            match mode() {
                Mode::Print if ms >= min_ms() => eprintln!("[timing] {what} {ms:.2} ms"),
                Mode::Sum => {
                    let mut s = sums().lock().unwrap_or_else(|e| e.into_inner());
                    match s.iter_mut().find(|e| e.0 == what) {
                        Some(e) => e.1 += ms,
                        None => s.push((what, ms)),
                    }
                }
                _ => {}
            }
        }
    }
}
