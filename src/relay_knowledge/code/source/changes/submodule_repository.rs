use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use super::super::{
    CodeIndexError,
    git::{git_bytes, git_dir_bytes, resolve_git_root},
};

pub(in crate::code) fn submodule_worktree_root(
    root: &Path,
    path: &str,
) -> Result<PathBuf, CodeIndexError> {
    let worktree = root.join(path);
    if !fs::symlink_metadata(&worktree)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
    {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule worktree for path {path} is unavailable"
        )));
    }

    let resolved = resolve_git_root(&worktree)?;
    let worktree_root = worktree.canonicalize().unwrap_or(worktree);
    let resolved_root = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if resolved_root != worktree_root {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule worktree for path {path} resolves to parent repository"
        )));
    }

    Ok(resolved)
}

pub(in crate::code) fn submodule_git_dir(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    submodule_commit: Option<&str>,
) -> Result<PathBuf, CodeIndexError> {
    for name in submodule_names_for_path(root, path, parent_commit) {
        if let Ok(git_dir) = submodule_git_dir_for_name(root, &name)
            && submodule_git_dir_matches_commit(&git_dir, submodule_commit)
        {
            return Ok(git_dir);
        }
    }
    if let Ok(git_dir) = submodule_git_dir_for_name(root, path.trim_matches('/'))
        && submodule_git_dir_matches_commit(&git_dir, submodule_commit)
    {
        return Ok(git_dir);
    }
    Err(CodeIndexError::InvalidInput(format!(
        "submodule git dir for path {path} is unavailable"
    )))
}

pub(in crate::code) fn submodule_git_dir_from_git_dir(
    git_dir: &Path,
    path: &str,
    parent_commit: Option<&str>,
    submodule_commit: Option<&str>,
) -> Result<PathBuf, CodeIndexError> {
    for name in submodule_names_for_path_from_git_dir(git_dir, path, parent_commit) {
        if let Ok(submodule_git_dir) = submodule_git_dir_for_name_from_git_dir(git_dir, &name)
            && submodule_git_dir_matches_commit(&submodule_git_dir, submodule_commit)
        {
            return Ok(submodule_git_dir);
        }
    }
    if let Ok(submodule_git_dir) =
        submodule_git_dir_for_name_from_git_dir(git_dir, path.trim_matches('/'))
        && submodule_git_dir_matches_commit(&submodule_git_dir, submodule_commit)
    {
        return Ok(submodule_git_dir);
    }
    Err(CodeIndexError::InvalidInput(format!(
        "nested submodule git dir for path {path} is unavailable"
    )))
}

fn submodule_names_for_path(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_submodule_names_from_config(
        git_bytes(root, ["config", "--get-regexp", "^submodule\\..*\\.path$"])
            .ok()
            .as_deref(),
        path,
        &mut names,
    );
    collect_submodule_names_from_config(
        git_bytes(
            root,
            [
                "config",
                "--file",
                ".gitmodules",
                "--get-regexp",
                "^submodule\\..*\\.path$",
            ],
        )
        .ok()
        .as_deref(),
        path,
        &mut names,
    );
    if let Some(parent_commit) = parent_commit {
        let object = format!("{parent_commit}:.gitmodules");
        collect_submodule_names_from_gitmodules(
            git_bytes(root, ["show", &object]).ok().as_deref(),
            path,
            &mut names,
        );
    }

    names
}

fn submodule_names_for_path_from_git_dir(
    git_dir: &Path,
    path: &str,
    parent_commit: Option<&str>,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_submodule_names_from_config(
        git_dir_bytes(
            git_dir,
            &["config", "--get-regexp", "^submodule\\..*\\.path$"],
        )
        .ok()
        .as_deref(),
        path,
        &mut names,
    );
    if let Some(parent_commit) = parent_commit {
        let object = format!("{parent_commit}:.gitmodules");
        collect_submodule_names_from_gitmodules(
            git_dir_bytes(git_dir, &["show", &object]).ok().as_deref(),
            path,
            &mut names,
        );
    }

    names
}

fn collect_submodule_names_from_config(
    bytes: Option<&[u8]>,
    path: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(bytes) = bytes else {
        return;
    };
    for line in String::from_utf8_lossy(bytes).lines() {
        let Some((key, value)) = split_config_key_value(line) else {
            continue;
        };
        if value.trim() != path {
            continue;
        }
        let Some(name) = key
            .strip_prefix("submodule.")
            .and_then(|value| value.strip_suffix(".path"))
        else {
            continue;
        };
        names.insert(name.to_owned());
    }
}

fn collect_submodule_names_from_gitmodules(
    bytes: Option<&[u8]>,
    path: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(bytes) = bytes else {
        return;
    };
    let mut current_name = None::<String>;
    for raw_line in String::from_utf8_lossy(bytes).lines() {
        let line = raw_line.trim();
        if let Some(name) = gitmodules_section_name(line) {
            current_name = Some(name);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "path"
            && value.trim() == path
            && let Some(name) = &current_name
        {
            names.insert(name.clone());
        }
    }
}

fn split_config_key_value(line: &str) -> Option<(&str, &str)> {
    let split_at = line.find(char::is_whitespace)?;
    Some((&line[..split_at], line[split_at..].trim()))
}

fn gitmodules_section_name(line: &str) -> Option<String> {
    line.strip_prefix("[submodule \"")
        .and_then(|value| value.strip_suffix("\"]"))
        .map(ToOwned::to_owned)
}

fn submodule_git_dir_for_name(root: &Path, name: &str) -> Result<PathBuf, CodeIndexError> {
    validate_submodule_name(name)?;
    let git_path = format!("modules/{name}");
    let bytes = git_bytes(
        root,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &git_path,
        ],
    )?;
    let git_dir = PathBuf::from(String::from_utf8_lossy(&bytes).trim().to_owned());
    if git_dir.exists() {
        return Ok(git_dir);
    }

    Err(CodeIndexError::InvalidInput(format!(
        "submodule git dir for name {name} is unavailable"
    )))
}

fn submodule_git_dir_for_name_from_git_dir(
    git_dir: &Path,
    name: &str,
) -> Result<PathBuf, CodeIndexError> {
    validate_submodule_name(name)?;
    let git_path = format!("modules/{name}");
    let bytes = git_dir_bytes(
        git_dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &git_path,
        ],
    )?;
    let submodule_git_dir = PathBuf::from(String::from_utf8_lossy(&bytes).trim().to_owned());
    if submodule_git_dir.exists() {
        return Ok(submodule_git_dir);
    }

    Err(CodeIndexError::InvalidInput(format!(
        "nested submodule git dir for name {name} is unavailable"
    )))
}

fn validate_submodule_name(name: &str) -> Result<(), CodeIndexError> {
    if name.is_empty() {
        return Err(CodeIndexError::InvalidInput(
            "submodule name is empty".to_owned(),
        ));
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(CodeIndexError::InvalidInput(format!(
            "submodule name '{name}' cannot escape the repository git modules directory"
        )));
    }

    Ok(())
}

fn submodule_git_dir_matches_commit(git_dir: &Path, commit: Option<&str>) -> bool {
    commit.is_none_or(|commit| submodule_git_dir_has_commit(git_dir, commit))
}

fn submodule_git_dir_has_commit(git_dir: &Path, commit: &str) -> bool {
    if !git_dir.join("HEAD").exists() || !git_dir.join("objects").is_dir() {
        return false;
    }
    git_dir_bytes(
        git_dir,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
    )
    .is_ok()
}

#[cfg(test)]
#[path = "submodule_repository_tests.rs"]
mod tests;
