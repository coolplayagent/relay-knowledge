//! Conventional Cargo source roots. Custom manifest roots need explicit evidence.

/// Candidate roots for a source file; callers must prove membership through `mod` facts.
pub(crate) fn roots(path: &str) -> Vec<String> {
    let components = path.split('/').collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate().rev() {
        if (*component == "bin" && index > 0 && components[index - 1] == "src")
            || (matches!(*component, "tests" | "examples") && !components[..index].contains(&"src"))
        {
            let tail = &components[index + 1..];
            if tail.len() == 1 {
                return vec![path.to_owned()];
            }
            if tail.len() > 1 {
                return vec![format!("{}/main.rs", components[..index + 2].join("/"))];
            }
        }
        if *component == "src" {
            let directory = components[..=index].join("/");
            if components.len() == index + 2
                && matches!(components[index + 1], "lib.rs" | "main.rs")
            {
                return vec![path.to_owned()];
            }
            return vec![
                format!("{directory}/lib.rs"),
                format!("{directory}/main.rs"),
            ];
        }
    }
    Vec::new()
}

/// External modules beside a conventional crate root or `mod.rs`, below other modules.
pub(crate) fn module_directory(path: &str) -> Option<&str> {
    let stem = path.strip_suffix(".rs")?;
    if stem.ends_with("/mod") || roots(path) == [path] {
        return Some(path.rsplit_once('/').map_or("", |(directory, _)| directory));
    }
    Some(stem)
}
