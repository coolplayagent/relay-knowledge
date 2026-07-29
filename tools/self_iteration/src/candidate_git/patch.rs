pub fn capture_patch(
    workspace: &Path,
    paths: &HistoryPaths,
    run_id: &str,
    base_ref: &str,
) -> Result<PatchSnapshot, String> {
    paths.ensure()?;
    let untracked = git_checked(
        workspace,
        &["ls-files", "--others", "--exclude-standard"],
        60,
    )?
    .stdout
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if !untracked.is_empty() {
        let mut args = vec!["add".to_owned(), "-N".to_owned(), "--".to_owned()];
        args.extend(untracked);
        git_dynamic(workspace, &args, 60, false)?;
    }
    let diff = git_checked(workspace, &["diff", "--binary", base_ref], 120)?.stdout;
    let _ = git_checked(workspace, &["reset", "--mixed", "HEAD"], 120)?;
    let patch_path = paths.patches.join(format!("{run_id}.patch"));
    std::fs::write(&patch_path, &diff)
        .map_err(|error| format!("failed to write {}: {error}", patch_path.display()))?;
    let sha256 = format!("{:x}", Sha256::digest(diff.as_bytes()));
    Ok(PatchSnapshot {
        path: patch_path,
        diff,
        sha256,
        base_ref: base_ref.to_owned(),
    })
}

pub fn changed_paths_from_diff(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| line.strip_prefix("diff --git a/"))
        .filter_map(|rest| rest.split(" b/").next())
        .map(ToOwned::to_owned)
        .collect()
}
