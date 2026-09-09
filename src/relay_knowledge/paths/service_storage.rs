//! Resolve and validate storage selected by a restored Windows service definition.

use super::*;
use crate::env::{EnvironmentConfig, RELAY_KNOWLEDGE_HOME};
use quick_xml::{Reader, events::Event};

impl RuntimePaths {
    /// Rejects unusable data directories before service execution. Windows also
    /// rejects links; operator-managed ACLs are never changed.
    pub(crate) async fn ensure_privileged_service_storage(
        &self,
        access: StorageDirectoryAccess,
    ) -> Result<(), PathError> {
        #[cfg(windows)]
        {
            windows_storage::validate_service_database_path(&self.database_file(), access).await
        }
        #[cfg(not(windows))]
        {
            let _ = access;
            let result = tokio::time::timeout(
                DATA_DIRECTORY_PROBE_TIMEOUT,
                tokio::fs::metadata(&self.data_dir),
            )
            .await
            .unwrap_or_else(|_| {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "directory probe timed out",
                ))
            });
            match result {
                Ok(metadata) if metadata.is_dir() => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => Err(PathError {
                    purpose: PathPurpose::Data,
                    kind: PathErrorKind::DataDirectoryProbe {
                        path: self.data_dir.clone(),
                        reason: match result {
                            Err(error) => error.to_string(),
                            Ok(_) => "data path is not a directory".to_owned(),
                        },
                    },
                }),
            }
        }
    }

    /// Resolves only the storage roots pinned by a restored service definition.
    /// Other runtime roots remain owned by the current lifecycle process.
    pub(crate) fn with_service_storage_overrides(
        &self,
        overrides: &PathEnvOverrides,
    ) -> Result<Self, PathError> {
        let data_dir = match (&overrides.data_dir, &overrides.home) {
            (Some(path), _) => path.clone(),
            (None, Some(root)) => runtime_home_defaults(root)?.data_dir,
            (None, None) => {
                return Err(PathError::missing_base(
                    PathPurpose::Data,
                    RELAY_KNOWLEDGE_DATA_DIR,
                ));
            }
        };
        validate_path(PathPurpose::Data, &data_dir)?;
        let mut selected = self.clone();
        selected.windows_data_sid = windows_data_sid_from_path(&data_dir)?;
        selected.data_dir = data_dir;
        Ok(selected)
    }

    /// Validates the pinned storage of a restored Windows service without provisioning.
    pub(crate) async fn validate_restored_service_storage(
        &self,
        definition: &str,
    ) -> Result<(), String> {
        let restored = self
            .with_service_storage_overrides(&storage_overrides(definition)?)
            .map_err(|error| error.to_string())?;
        restored
            .ensure_storage_access(StorageDirectoryAccess::ExistingOnly)
            .await
            .map_err(|error| error.to_string())?;
        restored
            .ensure_privileged_service_storage(StorageDirectoryAccess::ExistingOnly)
            .await
            .map_err(|error| error.to_string())?;
        if !restored
            .database_file_exists()
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(format!(
                "checkpointed service database is missing: {}",
                restored.database_file().display()
            ));
        }
        Ok(())
    }
}

fn storage_overrides(definition: &str) -> Result<PathEnvOverrides, String> {
    let mut reader = Reader::from_str(definition);
    reader.config_mut().check_comments = true;
    let mut depth = 0_usize;
    let mut root_seen = false;
    let mut values = Vec::new();
    loop {
        let event = reader.read_event().map_err(|error| error.to_string())?;
        let starts = matches!(&event, Event::Start(_));
        match event {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"env" && depth == 1 =>
            {
                let mut name = None;
                let mut value = None;
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| error.to_string())?;
                    let text = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|error| error.to_string())?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = Some(text.to_ascii_uppercase()),
                        b"value" => value = Some(text),
                        _ => {}
                    }
                }
                if let Some(name) = name.filter(|name| {
                    matches!(
                        name.as_str(),
                        RELAY_KNOWLEDGE_DATA_DIR | RELAY_KNOWLEDGE_HOME
                    )
                }) {
                    if values.iter().any(|(key, _)| key == &name) {
                        return Err("service definition repeats a storage override".to_owned());
                    }
                    values.push((
                        name,
                        value.ok_or_else(|| "service storage override has no value".to_owned())?,
                    ));
                }
                if starts {
                    depth += 1;
                }
            }
            Event::Start(element) => {
                if depth == 0 {
                    if root_seen || element.name().as_ref() != b"service" {
                        return Err("invalid Windows service definition root".to_owned());
                    }
                    root_seen = true;
                }
                depth += 1;
                if depth > 32 {
                    return Err("service definition nesting exceeds 32 levels".to_owned());
                }
            }
            Event::Empty(_) if depth == 0 => {
                return Err("service definition contains an element outside its root".to_owned());
            }
            Event::Text(text)
                if depth == 0 && !text.as_ref().iter().all(u8::is_ascii_whitespace) =>
            {
                return Err("service definition contains text outside its root".to_owned());
            }
            Event::CData(_) | Event::GeneralRef(_) if depth == 0 => {
                return Err("service definition contains content outside its root".to_owned());
            }
            Event::Decl(_) if root_seen => {
                return Err("service definition contains a misplaced XML declaration".to_owned());
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "invalid service definition nesting".to_owned())?;
            }
            Event::DocType(_) => return Err("service definition DTD is unsupported".to_owned()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 || values.is_empty() {
        return Err("restored service must pin a DATA_DIR or HOME storage override".to_owned());
    }
    EnvironmentConfig::from_pairs(PlatformKind::Windows, values)
        .map(|environment| environment.paths)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "service_storage_tests.rs"]
mod tests;
