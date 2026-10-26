//! Work on other threads for what a streamed world needs and the picture does not have to wait for: a pool of worker threads, and a handle (`Pending`) on a result that is on its way. The renderers
//! start the preparation of a mesh or a texture (interleaving, clusters, levels of detail, mip chains, block compression) when it first shows up, draw the picture without it, and take it when it
//! is ready: a streamed cell costs the picture nothing but the copy to the card. The pool is shared (`LUNARIS_BG_THREADS` sets how many threads: by default the cores but two, from one to four).
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

type Job = Box<dyn FnOnce() + Send>;

/// Let the calling thread give way to the one that draws the pictures: a worker that is preparing a cell of the world (all of them busy at once, on a machine with few cores) must not take the processor away from the next
/// picture. Linux: a nice value of 10 for this thread (Linux keeps the nice value per thread); Windows: below normal. Nothing elsewhere.
pub fn background_priority() {
    #[cfg(target_os = "linux")]
    {
        extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }
        // SAFETY: a plain system call with integers; `who` 0 is the calling thread.
        let _ = unsafe { setpriority(0, 0, 10) };
    }
    #[cfg(windows)]
    {
        extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        }
        // SAFETY: the pseudo handle of the calling thread and THREAD_PRIORITY_BELOW_NORMAL.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), -1) };
    }
}

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
}

/// The pool: threads that take jobs from a queue, oldest first.
pub struct Pool {
    queue: Arc<Queue>,
}

impl Pool {
    fn start(threads: usize) -> Pool {
        let queue = Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), wake: Condvar::new() });
        for n in 0..threads.max(1) {
            let q = queue.clone();
            let _ = std::thread::Builder::new().name(format!("lunaris-bg-{n}")).spawn(move || {
                background_priority();
                loop {
                    let job = {
                        let mut jobs = q.jobs.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            if let Some(j) = jobs.pop_front() {
                                break j;
                            }
                            jobs = q.wake.wait(jobs).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    // a job that panics must not take the thread down (its `Pending` stays empty: whoever waits for it is told)
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                }
            });
        }
        Pool { queue }
    }

    /// The pool of this process.
    pub fn shared() -> &'static Pool {
        static POOL: OnceLock<Pool> = OnceLock::new();
        POOL.get_or_init(|| {
            let n = std::env::var("LUNARIS_BG_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2).clamp(1, 4)));
            Pool::start(n)
        })
    }

    /// Run `f` on a pool thread; the result comes through the handle.
    pub fn spawn<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
        let slot = Arc::new(Slot { value: Mutex::new(State::Waiting), done: Condvar::new() });
        let s = slot.clone();
        let job: Job = Box::new(move || {
            // a job that panics leaves a `Failed` behind: whoever waits for it is told, and the thread goes on
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            *s.value.lock().unwrap_or_else(|e| e.into_inner()) = match r {
                Ok(v) => State::Done(v),
                Err(_) => State::Failed,
            };
            s.done.notify_all();
        });
        self.queue.jobs.lock().unwrap_or_else(|e| e.into_inner()).push_back(job);
        self.queue.wake.notify_one();
        Pending { slot }
    }
}

enum State<T> {
    Waiting,
    Done(T),
    Taken,
    Failed,
}

struct Slot<T> {
    value: Mutex<State<T>>,
    done: Condvar,
}

/// A result that is on its way from a pool thread.
pub struct Pending<T> {
    slot: Arc<Slot<T>>,
}

impl<T> Pending<T> {
    /// Whether the result is here (and not taken yet).
    pub fn is_ready(&self) -> bool {
        matches!(&*self.slot.value.lock().unwrap_or_else(|e| e.into_inner()), State::Done(_))
    }

    /// Whether the job panicked: there will never be a result.
    pub fn failed(&self) -> bool {
        matches!(&*self.slot.value.lock().unwrap_or_else(|e| e.into_inner()), State::Failed)
    }

    /// Take the result if it is here.
    pub fn try_take(&self) -> Option<T> {
        let mut v = self.slot.value.lock().unwrap_or_else(|e| e.into_inner());
        match std::mem::replace(&mut *v, State::Taken) {
            State::Done(r) => Some(r),
            other => {
                *v = other;
                None
            }
        }
    }

    /// Wait for the result and take it (`None` when the job panicked or the result was taken already).
    pub fn wait(&self) -> Option<T> {
        let mut v = self.slot.value.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match std::mem::replace(&mut *v, State::Taken) {
                State::Done(r) => return Some(r),
                State::Waiting => {
                    *v = State::Waiting;
                    v = self.slot.done.wait(v).unwrap_or_else(|e| e.into_inner());
                }
                State::Taken => return None,
                State::Failed => {
                    *v = State::Failed;
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_on_another_thread_and_its_result_comes_back() {
        let p = Pool::shared().spawn(|| (1..=10u64).product::<u64>());
        assert_eq!(p.wait(), Some(3_628_800));
        assert!(p.try_take().is_none(), "taken once");
    }

    #[test]
    fn many_jobs_all_finish() {
        let jobs: Vec<_> = (0..40u64).map(|i| Pool::shared().spawn(move || i * i)).collect();
        let sum: u64 = jobs.iter().map(|j| j.wait().expect("done")).sum();
        assert_eq!(sum, (0..40u64).map(|i| i * i).sum());
    }
}
