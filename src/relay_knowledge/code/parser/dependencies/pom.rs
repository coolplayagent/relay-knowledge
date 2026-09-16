//! Streaming, bounded raw POM dependencies; effective models remain in storage.
use quick_xml::{Reader, events::Event};

use super::{CodeIndexError, DependencySeed, SeedInput, push_seed};

const FIELDS: [&str; 5] = ["groupId", "artifactId", "version", "scope", "type"];
const MAX_DEPTH: usize = 64;
const MAX_TEXT: usize = 65_536;
const MAX_EVENTS: usize = 1_000_000;
const MAX_DEPENDENCIES: usize = 16_384;

struct Dependency {
    depth: usize,
    line: usize,
    managed: bool,
    values: [Option<String>; 5],
    seen: [bool; 5],
}

pub(super) fn parse(
    content: &str,
    records: &mut Vec<DependencySeed>,
) -> Result<(), CodeIndexError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().expand_empty_elements = true;
    let mut stack = Vec::<String>::new();
    let mut current = None::<Dependency>;
    let mut value = String::new();
    let mut line = 1usize;
    let mut dependencies = 0usize;
    for _ in 0..MAX_EVENTS {
        let offset = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|error| invalid(&error.to_string()))?;
        let event_line = line;
        line += content.as_bytes()[offset..reader.buffer_position() as usize]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        if matches!(event, Event::Start(_) | Event::Empty(_))
            && current
                .as_ref()
                .is_some_and(|dependency| stack.len() == dependency.depth + 1)
            && stack
                .last()
                .is_some_and(|name| FIELDS.contains(&name.as_str()))
        {
            return Err(invalid("nested element in dependency coordinate"));
        }
        match event {
            Event::Start(event) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(invalid("XML depth budget exceeded"));
                }
                let name = std::str::from_utf8(event.local_name().as_ref())
                    .map_err(|_| invalid("invalid element name"))?
                    .to_owned();
                if name == "dependency" && dependency_context(&stack) {
                    dependencies += 1;
                    if current.is_some() || dependencies > MAX_DEPENDENCIES {
                        return Err(invalid("nested dependency or dependency budget exceeded"));
                    }
                    current = Some(Dependency {
                        depth: stack.len() + 1,
                        line: event_line,
                        managed: stack.iter().any(|name| name == "dependencyManagement"),
                        values: Default::default(),
                        seen: [false; 5],
                    });
                }
                stack.push(name);
                value.clear();
            }
            Event::End(_) => {
                if let Some(dependency) = current.as_mut() {
                    if stack.len() == dependency.depth + 1 {
                        if let Some(field) = stack
                            .last()
                            .and_then(|name| FIELDS.iter().position(|field| field == name))
                        {
                            let text = value.trim();
                            if dependency.seen[field] {
                                return Err(invalid("duplicate dependency coordinate"));
                            }
                            dependency.seen[field] = true;
                            if !text.is_empty() {
                                dependency.values[field] = Some(text.to_owned());
                            }
                        }
                    } else if stack.len() == dependency.depth {
                        current.take().expect("checked dependency").emit(records);
                    }
                }
                stack
                    .pop()
                    .ok_or_else(|| invalid("unmatched closing element"))?;
                value.clear();
            }
            Event::Eof => {
                return if stack.is_empty() {
                    Ok(())
                } else {
                    Err(invalid("unclosed XML element"))
                };
            }
            event
                if current
                    .as_ref()
                    .is_some_and(|dependency| stack.len() == dependency.depth + 1)
                    && stack
                        .last()
                        .is_some_and(|name| FIELDS.contains(&name.as_str())) =>
            {
                let text = match event {
                    Event::Text(event) => event
                        .decode()
                        .map_err(|_| invalid("invalid XML text"))?
                        .into_owned(),
                    Event::CData(event) => event
                        .decode()
                        .map_err(|_| invalid("invalid CDATA"))?
                        .into_owned(),
                    Event::GeneralRef(event) => quick_xml::escape::unescape(&format!(
                        "&{};",
                        event
                            .decode()
                            .map_err(|_| invalid("invalid XML reference"))?
                    ))
                    .map_err(|_| invalid("unknown XML reference"))?
                    .into_owned(),
                    _ => continue,
                };
                if value.len() + text.len() > MAX_TEXT {
                    return Err(invalid("coordinate text budget exceeded"));
                }
                value.push_str(&text);
            }
            _ => {}
        }
    }
    Err(invalid("XML event budget exceeded"))
}

fn dependency_context(stack: &[String]) -> bool {
    // Plugin configuration and properties are arbitrary XML, not dependency
    // declarations. Fragment roots support the existing raw-manifest contract.
    const PATHS: &[&[&str]] = &[
        &["dependencies"],
        &["dependencyManagement", "dependencies"],
        &["project", "dependencies"],
        &["project", "dependencyManagement", "dependencies"],
        &["project", "profiles", "profile", "dependencies"],
        &[
            "project",
            "profiles",
            "profile",
            "dependencyManagement",
            "dependencies",
        ],
    ];
    PATHS
        .iter()
        .any(|path| stack.iter().map(String::as_str).eq(path.iter().copied()))
}

impl Dependency {
    fn emit(self, records: &mut Vec<DependencySeed>) {
        let [Some(group), Some(artifact), version, scope, kind] = self.values else {
            return;
        };
        let bom =
            self.managed && kind.as_deref() == Some("pom") && scope.as_deref() == Some("import");
        if self.managed && !bom {
            return;
        }
        let package = format!("{group}:{artifact}");
        push_seed(
            records,
            SeedInput::new(
                "maven",
                "java",
                package.clone(),
                version,
                if bom {
                    "bom"
                } else {
                    scope.as_deref().unwrap_or("compile")
                },
                "pom.xml",
                false,
            )
            .line(self.line)
            .excerpt(package),
        );
    }
}

fn invalid(reason: &str) -> CodeIndexError {
    CodeIndexError::InvalidInput(format!("POM dependency analysis incomplete: {reason}"))
}

#[cfg(test)]
#[path = "pom_tests.rs"]
mod tests;
