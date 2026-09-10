//! Bounded, snapshot-only reactor discovery; effective dependency resolution lives in storage.

use super::WorkspaceSource;
use crate::domain::CodeWorkspaceMember;
use quick_xml::{Reader, events::Event};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_MEMBERS: usize = 8_192;
const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn detect(source: &dyn WorkspaceSource) -> Option<Vec<CodeWorkspaceMember>> {
    let mut queue = VecDeque::from(["pom.xml".to_owned()]);
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    let mut bytes = 0usize;
    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if seen.len() > MAX_MEMBERS {
            return None;
        }
        let Some(content) = source.read_to_string(&path) else {
            continue;
        };
        bytes = bytes.checked_add(content.len())?;
        if bytes > MAX_BYTES {
            return None;
        }
        let pom = summary(&content)?;
        let directory = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        if let Some(name) = pom.coordinate() {
            members.push(CodeWorkspaceMember {
                package_name: name,
                relative_path: if directory.is_empty() {
                    ".".into()
                } else {
                    directory.into()
                },
            });
        }
        for member in &pom.modules {
            if queue.len() + seen.len() >= MAX_MEMBERS {
                return None;
            }
            if let Some(path) = child_pom(directory, member) {
                queue.push_back(path);
            }
        }
    }
    (members.len() >= 2).then_some(members)
}

#[derive(Default)]
struct PomSummary {
    values: BTreeMap<String, String>,
    modules: Vec<String>,
}

impl PomSummary {
    fn coordinate(&self) -> Option<String> {
        let group = self
            .values
            .get("project/groupId")
            .or_else(|| self.values.get("project/parent/groupId"))?;
        let artifact = self.values.get("project/artifactId")?;
        if group.is_empty()
            || artifact.is_empty()
            || group.contains("${")
            || artifact.contains("${")
        {
            return None;
        }
        Some(format!("{group}:{artifact}"))
    }
}

fn summary(content: &str) -> Option<PomSummary> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut stack = Vec::new();
    let mut result = PomSummary::default();
    loop {
        match reader.read_event().ok()? {
            Event::Start(event) => {
                if stack.len() >= 64 {
                    return None;
                }
                stack.push(String::from_utf8_lossy(event.local_name().as_ref()).into_owned());
            }
            Event::End(_) => {
                stack.pop()?;
            }
            Event::Text(event) => {
                let text = quick_xml::escape::unescape(&event.decode().ok()?)
                    .ok()?
                    .into_owned();
                let key = stack.join("/");
                if key == "project/modules/module" {
                    if result.modules.len() >= MAX_MEMBERS {
                        return None;
                    }
                    result.modules.push(text);
                } else if matches!(
                    key.as_str(),
                    "project/groupId" | "project/artifactId" | "project/parent/groupId"
                ) {
                    result.values.entry(key).or_default().push_str(&text);
                }
            }
            Event::Eof => return stack.is_empty().then_some(result),
            _ => {}
        }
    }
}

fn child_pom(directory: &str, member: &str) -> Option<String> {
    let member = member.trim().replace('\\', "/");
    if member.starts_with('/') || member.contains(':') || member.contains("${") {
        return None;
    }
    let mut parts = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for part in member.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    if parts.last().copied() != Some("pom.xml") {
        parts.push("pom.xml");
    }
    Some(parts.join("/"))
}

#[cfg(test)]
#[path = "maven_tests.rs"]
mod tests;
