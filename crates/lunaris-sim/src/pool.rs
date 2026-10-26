//! A small pool of worker threads that lives as long as the simulation, for work that is too short to pay for starting threads every tick (a tick of
//! physics for a thousand bodies takes about a millisecond; starting and joining eight threads takes about as long).
//!
//! `Pool::run(tasks, f)` calls `f(0)`, `f(1)`, ... `f(tasks - 1)` on the workers and on the calling thread, and returns when all are done. Which thread
//! runs which task is not defined, so a caller must make the tasks independent and combine their results in the order of the task numbers; then the answer
//! does not depend on the number of threads (every system of the engine is written that way).
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// The job of one `run`: a closure whose lifetime was erased (it is valid until `run` returns, and `run` waits for every worker before it does).
#[derive(Clone, Copy)]
struct Job {
    f: *const (dyn Fn(usize) + Sync + 'static),
    tasks: usize,
}

// SAFETY: the closure behind the pointer is `Sync`, and `run` keeps it alive until every worker has finished with it.
unsafe impl Send for Job {}

struct State {
    generation: u64,
    job: Option<Job>,
    /// Workers that have not finished the current job.
    running: usize,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
    next: AtomicUsize,
    panicked: AtomicBool,
}

pub struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl Pool {
    /// A pool with `workers` threads besides the one that calls `run` (0 makes `run` plain sequential code).
    pub fn new(workers: usize) -> Pool {
        let shared = Arc::new(Shared { state: Mutex::new(State { generation: 0, job: None, running: 0, shutdown: false }), wake: Condvar::new(), done: Condvar::new(), next: AtomicUsize::new(0), panicked: AtomicBool::new(false) });
        let handles = (0..workers)
            .map(|k| {
                let shared = shared.clone();
                std::thread::Builder::new().name(format!("lunaris-pool-{k}")).spawn(move || worker(&shared)).expect("a pool thread starts")
            })
            .collect();
        Pool { shared, workers: handles }
    }

    /// The number of threads that work on a `run` (the workers and the caller).
    pub fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Runs `f(i)` for every `i` in `0..tasks` and returns when all are done. A panic in a task is raised again here.
    pub fn run(&self, tasks: usize, f: &(dyn Fn(usize) + Sync)) {
        if tasks == 0 {
            return;
        }
        if self.workers.is_empty() || tasks == 1 {
            for i in 0..tasks {
                f(i);
            }
            return;
        }
        // SAFETY: only the lifetime is erased. The pointer is used by the workers until `running` is back to 0, and this function does not return (not even
        // by unwinding: `Wait` waits in its `drop`) before that.
        let erased: *const (dyn Fn(usize) + Sync + 'static) = unsafe { std::mem::transmute::<&(dyn Fn(usize) + Sync), &'static (dyn Fn(usize) + Sync)>(f) };
        self.shared.panicked.store(false, Ordering::Relaxed);
        self.shared.next.store(0, Ordering::Relaxed);
        {
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.job = Some(Job { f: erased, tasks });
            st.running = self.workers.len();
            st.generation += 1;
        }
        self.shared.wake.notify_all();
        let _wait = Wait { shared: &self.shared };
        // the caller works too
        loop {
            let i = self.shared.next.fetch_add(1, Ordering::Relaxed);
            if i >= tasks {
                break;
            }
            if catch_unwind(AssertUnwindSafe(|| f(i))).is_err() {
                self.shared.panicked.store(true, Ordering::Relaxed);
            }
        }
        drop(_wait);
        if self.shared.panicked.load(Ordering::Relaxed) {
            panic!("a task of the thread pool panicked");
        }
    }
}

/// Waits (when dropped) until every worker is done with the job.
struct Wait<'a> {
    shared: &'a Shared,
}

impl Drop for Wait<'_> {
    fn drop(&mut self) {
        let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        while st.running > 0 {
            st = self.shared.done.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        st.job = None;
    }
}

fn worker(shared: &Shared) {
    let mut seen = 0u64;
    loop {
        let job;
        {
            let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while st.generation == seen && !st.shutdown {
                st = shared.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            if st.shutdown {
                return;
            }
            seen = st.generation;
            job = st.job;
        }
        if let Some(job) = job {
            loop {
                let i = shared.next.fetch_add(1, Ordering::Relaxed);
                if i >= job.tasks {
                    break;
                }
                // SAFETY: see `Job`: the closure lives until this worker has counted itself out below.
                let f = unsafe { &*job.f };
                if catch_unwind(AssertUnwindSafe(|| f(i))).is_err() {
                    shared.panicked.store(true, Ordering::Relaxed);
                }
            }
        }
        let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        st.running -= 1;
        if st.running == 0 {
            shared.done.notify_all();
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        {
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.shutdown = true;
        }
        self.shared.wake.notify_all();
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn every_task_runs_exactly_once_and_the_sum_does_not_depend_on_the_threads() {
        for workers in [0usize, 1, 3, 7] {
            let pool = Pool::new(workers);
            assert_eq!(pool.threads(), workers + 1);
            for tasks in [0usize, 1, 2, 5, 64, 1000] {
                let hits: Vec<AtomicU64> = (0..tasks).map(|_| AtomicU64::new(0)).collect();
                let total = AtomicU64::new(0);
                pool.run(tasks, &|i| {
                    hits[i].fetch_add(1, Ordering::Relaxed);
                    total.fetch_add(i as u64 * 3 + 1, Ordering::Relaxed);
                });
                assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1), "{workers} workers, {tasks} tasks");
                let expect: u64 = (0..tasks as u64).map(|i| i * 3 + 1).sum();
                assert_eq!(total.load(Ordering::Relaxed), expect);
            }
        }
    }

    #[test]
    fn many_runs_in_a_row_and_borrowed_data() {
        let pool = Pool::new(4);
        let data: Vec<u64> = (0..5000).collect();
        for round in 0..300u64 {
            let sums: Vec<AtomicU64> = (0..8).map(|_| AtomicU64::new(0)).collect();
            pool.run(8, &|k| {
                let part: u64 = data[k * 625..(k + 1) * 625].iter().map(|v| v + round).sum();
                sums[k].store(part, Ordering::Relaxed);
            });
            let got: u64 = sums.iter().map(|s| s.load(Ordering::Relaxed)).sum();
            assert_eq!(got, data.iter().map(|v| v + round).sum::<u64>(), "round {round}");
        }
    }

    #[test]
    fn a_panic_in_a_task_comes_back_to_the_caller_and_the_pool_still_works() {
        let pool = Pool::new(3);
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            pool.run(16, &|i| {
                if i == 11 {
                    panic!("task 11");
                }
            })
        }));
        assert!(r.is_err());
        let n = AtomicU64::new(0);
        pool.run(16, &|_| {
            n.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(n.load(Ordering::Relaxed), 16);
    }
}
