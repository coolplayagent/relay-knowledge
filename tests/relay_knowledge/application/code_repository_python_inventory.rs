use super::*;
use std::{io::Write, process::Stdio};

fn tree(repo: &FixtureRepo, entries: &str) -> String {
    let mut command = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo.path)
        .arg("mktree")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    command
        .stdin
        .take()
        .unwrap()
        .write_all(entries.as_bytes())
        .unwrap();
    let output = command.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn complete_large_path_inventory_preserves_python_overload_identity() {
    let repo = FixtureRepo::create("python-large-path-inventory");
    repo.write("app.py", APP);
    repo.write("empty.txt", "");
    let app = repo.git_text(["hash-object", "-w", "app.py"]);
    let empty = repo.git_text(["hash-object", "-w", "empty.txt"]);
    let entries = (0..4096)
        .map(|index| format!("100644 blob {empty}\t{index}.txt\n"))
        .collect::<String>();
    let mut nested = tree(&repo, &entries);
    let components = (0..21)
        .map(|index| format!("segment{index}{}", "x".repeat(190)))
        .collect::<Vec<_>>();
    for component in components.iter().skip(1).rev() {
        nested = tree(&repo, &format!("040000 tree {nested}\t{component}\n"));
    }
    let root = tree(
        &repo,
        &format!(
            "100644 blob {app}\tapp.py\n040000 tree {nested}\t{}\n",
            components[0]
        ),
    );
    let commit = repo.git_text(["commit-tree", &root, "-m", "Complete long path inventory"]);
    repo.git(["update-ref", "HEAD", &commit]);
    let paths = repo.git_text(["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(paths.lines().map(str::len).sum::<usize>() > 16 * 1024 * 1024);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec![]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    assert_origin_call(&service, "HEAD", true).await;
}
