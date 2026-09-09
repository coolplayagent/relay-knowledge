//! Snapshot-local evidence for the two Python overload provider modules.
//! This is source inventory evidence, never a read of the host Python environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code) enum PythonModuleOrigin {
    StandardCandidate,
    Local,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::code) struct PythonModuleOrigins {
    pub typing: PythonModuleOrigin,
    pub typing_extensions: PythonModuleOrigin,
}

impl Default for PythonModuleOrigins {
    fn default() -> Self {
        Self {
            typing: PythonModuleOrigin::Unknown,
            typing_extensions: PythonModuleOrigin::Unknown,
        }
    }
}

impl PythonModuleOrigins {
    pub(in crate::code) fn is_provider_path(path: &str) -> bool {
        matches!(
            path,
            "typing.py"
                | "typing/__init__.py"
                | "typing_extensions.py"
                | "typing_extensions/__init__.py"
        )
    }
    pub(in crate::code) fn from_authorized_paths<'a>(
        paths: impl IntoIterator<Item = &'a str>,
        path_filters: &[String],
        language_filters: &[String],
    ) -> Self {
        let visible = |name: &str| {
            (language_filters.is_empty() || language_filters.iter().any(|lang| lang == "python"))
                && (path_filters.is_empty()
                    || [format!("{name}.py"), format!("{name}/__init__.py")]
                        .iter()
                        .all(|path| {
                            path_filters.iter().any(|filter| {
                                filter.is_empty()
                                    || path == filter
                                    || path
                                        .strip_prefix(filter)
                                        .is_some_and(|rest| rest.starts_with('/'))
                            })
                        }))
        };
        let mut result = Self {
            typing: if visible("typing") {
                PythonModuleOrigin::StandardCandidate
            } else {
                PythonModuleOrigin::Unknown
            },
            typing_extensions: if visible("typing_extensions") {
                PythonModuleOrigin::StandardCandidate
            } else {
                PythonModuleOrigin::Unknown
            },
        };
        // Callers already own the authorized, bounded snapshot inventory. Inspect only
        // four exact module identities; unrelated path length cannot invalidate evidence.
        // This adds no source I/O, copied paths, or collection proportional to the inventory.
        for path in paths {
            match path {
                "typing.py" | "typing/__init__.py" => result.typing = PythonModuleOrigin::Local,
                "typing_extensions.py" | "typing_extensions/__init__.py" => {
                    result.typing_extensions = PythonModuleOrigin::Local
                }
                _ => {}
            }
        }
        result
    }

    pub(in crate::code) fn permits_standard_module(self, module: &str) -> bool {
        match module {
            "typing" => self.typing == PythonModuleOrigin::StandardCandidate,
            "typing_extensions" => self.typing_extensions == PythonModuleOrigin::StandardCandidate,
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "python_imports_tests.rs"]
mod tests;
