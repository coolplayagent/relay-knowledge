use std::sync::{Arc, Condvar, Mutex};

use crate::command::{CommandResult, CommandSpec, run_command};

use super::contracts::{EvalRuntime, Limiter};

struct Permit {
    inner: Arc<(Mutex<usize>, Condvar)>,
}

impl Limiter {
    pub(in crate::evaluator) fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new((Mutex::new(limit.max(1)), Condvar::new())),
        }
    }

    fn acquire(&self) -> Permit {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        while *available == 0 {
            available = condvar
                .wait(available)
                .expect("limiter lock should not be poisoned");
        }
        *available -= 1;
        Permit {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.inner;
        let mut available = lock.lock().expect("limiter lock should not be poisoned");
        *available += 1;
        condvar.notify_one();
    }
}

pub(in crate::evaluator) fn run_limited(limiter: &Limiter, spec: CommandSpec) -> CommandResult {
    let _permit = limiter.acquire();
    run_command(&spec)
}

pub(in crate::evaluator) fn run_writer_limited(
    runtime: &EvalRuntime,
    spec: CommandSpec,
) -> CommandResult {
    let _permit = runtime.limiter.acquire();
    let _writer = runtime
        .writer_lock
        .lock()
        .expect("writer lock should not be poisoned");
    run_command(&spec)
}

pub(in crate::evaluator) fn parallel_map<T, R, F>(items: Vec<T>, jobs: usize, f: F) -> Vec<R>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> R + Send + Sync + 'static,
{
    if items.is_empty() {
        return Vec::new();
    }
    let queue = Arc::new(Mutex::new(items.into_iter().collect::<Vec<_>>()));
    let output = Arc::new(Mutex::new(Vec::new()));
    let function = Arc::new(f);
    let workers = jobs.max(1).min(queue.lock().expect("queue").len());
    let mut handles = Vec::new();
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let output = Arc::clone(&output);
        let function = Arc::clone(&function);
        handles.push(std::thread::spawn(move || {
            loop {
                let item = queue.lock().expect("queue").pop();
                let Some(item) = item else {
                    break;
                };
                let result = function(item);
                output.lock().expect("output").push(result);
            }
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }
    match Arc::try_unwrap(output) {
        Ok(output) => output.into_inner().expect("output should not be poisoned"),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
#[path = "concurrency_tests.rs"]
mod tests;
