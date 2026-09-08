use super::*;

#[test]
fn windows_sid_validation_accepts_accounts_and_rejects_path_injection() {
    for sid in ["S-1-5-18", "S-1-5-21-4294967295-2-3-1001", "S-1-12-1-2-3-4"] {
        validate_sid(sid).unwrap();
    }
    for sid in [
        "",
        "S-2-5-18",
        "s-1-5-18",
        "S-1-5",
        "S-1-5-",
        "S-1-5-01",
        "S-1-5-4294967296",
        "S-1-281474976710656-1",
        "S-1-5-18/../other",
        "S-1-5-18'; exit 0; '",
        "S-1-5-18\n",
        "S-1-5--1",
        "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
    ] {
        assert!(validate_sid(sid).is_err(), "accepted {sid:?}");
    }
}

#[tokio::test]
async fn windows_security_rejects_invalid_inputs_before_launching_a_process() {
    let error = prepare_private_directory(
        Path::new("/data"),
        "../account",
        StorageDirectoryAccess::OpenOrCreate,
        None,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("invalid account SID"));
    for path in ["/bad\0path".to_owned(), "x".repeat(4097)] {
        assert!(
            prepare_private_directory(
                Path::new(&path),
                "S-1-5-18",
                StorageDirectoryAccess::OpenOrCreate,
                None
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("4096 bytes")
        );
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn windows_identity_requires_a_native_host_without_an_environment_fallback() {
    assert!(
        current_sid()
            .await
            .unwrap_err()
            .to_string()
            .contains("Windows host")
    );
    assert!(
        prepare_private_directory(
            Path::new("/quoted'path"),
            "S-1-5-18",
            StorageDirectoryAccess::ExistingOnly,
            None
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("Windows host")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn windows_command_boundary_checks_exit_status_encoding_and_output_budget() {
    use std::time::Duration;
    for (script, expected) in [
        ("printf ' S-1-5-18\\n'", Ok("S-1-5-18")),
        ("printf 'access denied'; exit 7", Err("access denied")),
        ("printf '\\377'", Err("not UTF-8")),
        ("head -c 4097 /dev/zero", Err("exceeds 4096 bytes")),
    ] {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", script]);
        let result = bounded_command(&mut command, Duration::from_secs(2)).await;
        match expected {
            Ok(text) => assert_eq!(result.unwrap(), text),
            Err(text) => assert!(result.unwrap_err().to_string().contains(text)),
        }
    }
    let mut missing = tokio::process::Command::new("/absent/relay-windows-security-test");
    assert!(
        bounded_command(&mut missing, Duration::from_secs(1))
            .await
            .is_err()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn windows_command_timeout_and_cancellation_terminate_the_child() {
    use std::time::Duration;
    let mut command = tokio::process::Command::new("sleep");
    command.arg("10");
    assert!(
        bounded_command(&mut command, Duration::from_millis(20))
            .await
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );

    let pid_file =
        std::env::temp_dir().join(format!("relay-security-child-{}", std::process::id()));
    let output_path = pid_file.clone();
    let worker = tokio::spawn(async move {
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "echo $$ > \"$1\"; exec sleep 10", "sh"])
            .arg(output_path);
        bounded_command(&mut command, Duration::from_secs(15)).await
    });
    let pid = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&pid_file).await {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(2), async {
        while tokio::fs::try_exists(format!("/proc/{pid}")).await.unwrap() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled child must exit and be reaped");
    tokio::fs::remove_file(pid_file).await.unwrap();
}

#[cfg(windows)]
#[test]
fn windows_probe_timeout_does_not_delay_runtime_shutdown() {
    let started = std::time::Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(run_security_script(
            "Start-Sleep -Seconds 30",
            Duration::from_millis(200),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    drop(runtime);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "timed-out probe must not leave blocking I/O in the runtime"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_native_sid_is_stable_across_repeated_token_reads() {
    let first = current_sid().await.unwrap();
    assert_eq!(first, current_sid().await.unwrap());
    validate_sid(&first).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_native_ignores_a_counterfeit_system_root() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("relay-fake-system-root-{nonce}"));
    let shell = root.join("System32/WindowsPowerShell/v1.0");
    tokio::fs::create_dir_all(&shell).await.unwrap();
    tokio::fs::write(
        shell.join("powershell.exe"),
        b"counterfeit executable must never run",
    )
    .await
    .unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
    child.args(["--exact", "paths::windows_storage::tests::windows_native_sid_is_stable_across_repeated_token_reads"])
        .env("SystemRoot", &root)
        .env("WINDIR", &root)
        .env("PSModulePath", &root)
        .env("COR_ENABLE_PROFILING", "1")
        .env("COR_PROFILER_PATH", root.join("fake-profiler.dll"));
    let output = bounded_command(&mut child, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    assert!(output.contains("1 passed"));
    tokio::fs::remove_dir_all(root).await.unwrap();
}
