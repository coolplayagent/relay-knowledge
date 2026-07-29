//! Owns durable JSON writes for model provider configuration.

use std::path::PathBuf;

use serde::Serialize;
use tokio::fs;

use super::ModelProviderError;

pub(super) async fn write_json<T: Serialize>(
    path: PathBuf,
    value: &T,
) -> Result<(), ModelProviderError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let body = serde_json::to_vec_pretty(value)?;
    fs::write(path, body)
        .await
        .map_err(ModelProviderError::from)
}
