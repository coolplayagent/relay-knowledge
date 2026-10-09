use super::*;

#[test]
fn rejects_cross_platform_escape_paths() {
    for path in [
        "../outside",
        "/outside",
        "a/../outside",
        "a\\outside",
        "C:/outside",
        "./file",
    ] {
        assert!(confined_components(Path::new(path)).is_err(), "{path}");
    }
    assert_eq!(
        confined_components(Path::new("sources/file"))
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn rejects_symlink_special_files_and_file_budgets() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!("relay-research-reader-{}", digest(&nonce)));
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("file"), b"bytes").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("file"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("nested"), root.join("dirlink")).unwrap();
    }
    run(root.clone(), |reader| {
        assert_eq!(reader.read(Path::new("file"), 5).unwrap(), b"bytes");
        assert!(reader.read(Path::new("file"), 4).is_err());
        assert!(reader.read(Path::new("nested"), 100).is_err());
        assert!(reader.read(Path::new("missing"), 100).is_err());
        assert!(reader.read(Path::new(""), 100).is_err());
        #[cfg(unix)]
        {
            assert!(reader.read(Path::new("link"), 100).is_err());
            assert!(reader.read(Path::new("dirlink/file"), 100).is_err());
        }
        reader.remaining_bytes = 1;
        assert!(reader.read(Path::new("file"), 100).is_err());
        reader.remaining_bytes = 100;
        reader.cancelled.store(true, Ordering::Relaxed);
        assert!(reader.read(Path::new("file"), 100).is_err());
        Ok(())
    })
    .await
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn saturated_workers_reject_admission_and_cancelled_callers_stop_workers() {
    let _guard = super::super::test_support::TEST_LOCK.lock().await;
    let held = Arc::clone(&PERMITS).acquire_many_owned(4).await.unwrap();
    assert!(
        compute(|_| Ok(()))
            .await
            .unwrap_err()
            .message
            .contains("budget")
    );
    drop(held);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(compute(move |cancelled| {
        started_tx.send(()).unwrap();
        while !cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }
        stopped_tx.send(()).unwrap();
        Ok(())
    }));
    started_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(2), stopped_rx)
        .await
        .unwrap()
        .unwrap();
    let permits = tokio::time::timeout(
        Duration::from_secs(2),
        Arc::clone(&PERMITS).acquire_many_owned(4),
    )
    .await
    .unwrap()
    .unwrap();
    drop(permits);
    assert!(
        run(PathBuf::from("/nonexistent-research-root"), |_| Ok(()))
            .await
            .is_err()
    );
}
