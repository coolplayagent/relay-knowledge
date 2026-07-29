fn run_limited(limiter: &Limiter, spec: CommandSpec) -> CommandResult {
    let _permit = limiter.acquire();
    run_command(&spec)
}

fn run_writer_limited(runtime: &EvalRuntime, spec: CommandSpec) -> CommandResult {
    let _permit = runtime.limiter.acquire();
    let _writer = runtime
        .writer_lock
        .lock()
        .expect("writer lock should not be poisoned");
    run_command(&spec)
}

fn parallel_map<T, R, F>(items: Vec<T>, jobs: usize, f: F) -> Vec<R>
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
