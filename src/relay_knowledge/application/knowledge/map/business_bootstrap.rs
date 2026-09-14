//! Owns safe glossary creation and Knowledge-only initialization guidance.

use tokio::fs;

use super::{
    KnowledgeMapMutationResponse, KnowledgeMapService, KnowledgeMapServiceError,
    artifact::{ensure_regular_file_within, serialize_yaml},
    fs_contract::{ensure_owned_directory, temporary_path},
};
use crate::{
    api::RequestContext,
    domain::{BusinessGlossary, RepositoryMapType},
};

impl KnowledgeMapService {
    pub(super) fn initialization_response(
        &self,
        context: &RequestContext,
        map_version: u64,
        summary: String,
    ) -> KnowledgeMapMutationResponse {
        let mut response = self.mutation_response(context, map_version, summary);
        if self.map_type == RepositoryMapType::Knowledge {
            response.business_bootstrap = Some(crate::api::BusinessKnowledgeBootstrap::default());
        }
        response
    }

    pub(super) async fn ensure_default_business_glossary(
        &self,
    ) -> Result<bool, KnowledgeMapServiceError> {
        let contract = self.repository_root.join(self.contract_dir_name());
        let owned_contract = ensure_owned_directory(&self.repository_root, &contract).await?;
        let path = self.business_glossary_path();
        if fs::try_exists(&path).await? {
            ensure_regular_file_within(&path, &owned_contract).await?;
            let content = fs::read(&path).await?;
            BusinessGlossary::parse(&content)?;
            return Ok(false);
        }
        let example = crate::api::BUSINESS_GLOSSARY_EXAMPLE
            .lines()
            .map(|line| format!("# {line}\n"))
            .collect::<String>();
        let yaml = format!(
            "# Author reviewed business facts, commit the map and glossary, then run repo index.\n# Code indexing does not infer business terms. Example only (replace with your domain):\n{example}{}",
            serialize_yaml(&BusinessGlossary::empty_v1())?
        );
        let temp = temporary_path(&path);
        if let Err(error) = fs::write(&temp, yaml.as_bytes()).await {
            let _ = fs::remove_file(&temp).await;
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temp, &path).await {
            let _ = fs::remove_file(temp).await;
            return Err(error.into());
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "business_bootstrap_tests.rs"]
mod tests;
