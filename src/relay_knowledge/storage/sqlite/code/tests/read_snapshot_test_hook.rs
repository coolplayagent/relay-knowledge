use std::sync::{Arc, Condvar, Mutex, mpsc::Sender};

type Barrier = (Sender<()>, Arc<(Mutex<bool>, Condvar)>);

static BARRIER: Mutex<Option<Barrier>> = Mutex::new(None);

pub(super) fn install(reached: Sender<()>, release: Arc<(Mutex<bool>, Condvar)>) {
    *BARRIER.lock().expect("read snapshot test barrier lock") = Some((reached, release));
}

pub(super) fn after_retiring_check() {
    let barrier = BARRIER
        .lock()
        .expect("read snapshot test barrier lock")
        .take();
    let Some((reached, release)) = barrier else {
        return;
    };
    let _ = reached.send(());
    let (released, signal) = &*release;
    if let Ok(released) = released.lock() {
        drop(
            signal.wait_timeout_while(released, std::time::Duration::from_secs(5), |released| {
                !*released
            }),
        );
    }
}
