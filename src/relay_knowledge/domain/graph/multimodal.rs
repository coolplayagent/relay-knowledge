use serde::{Deserialize, Serialize};

use super::{DomainError, error::required_text};

/// Evidence media unit tracked by multimodal ingestion and retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceModality {
    TextSpan,
    ImageAsset,
    OcrText,
    Caption,
    ImageEmbedding,
    Table,
    LayoutRegion,
}

impl EvidenceModality {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TextSpan => "text_span",
            Self::ImageAsset => "image_asset",
            Self::OcrText => "ocr_text",
            Self::Caption => "caption",
            Self::ImageEmbedding => "image_embedding",
            Self::Table => "table",
            Self::LayoutRegion => "layout_region",
        }
    }

    /// Returns whether this modality is derived from a parent evidence item.
    pub const fn requires_parent(self) -> bool {
        matches!(
            self,
            Self::OcrText | Self::Caption | Self::ImageEmbedding | Self::LayoutRegion
        )
    }
}

/// Page-space rectangle for table and layout evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutRegion {
    pub page_number: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl LayoutRegion {
    /// Validates a one-based page coordinate and non-empty rectangle.
    pub fn new(
        page_number: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<Self, DomainError> {
        if page_number == 0 {
            return Err(DomainError::invalid(
                "layout_region",
                "page number must be one-based",
            ));
        }
        if width == 0 || height == 0 {
            return Err(DomainError::invalid(
                "layout_region",
                "width and height must be greater than zero",
            ));
        }

        Ok(Self {
            page_number,
            x,
            y,
            width,
            height,
        })
    }
}

/// Extraction outcome recorded when multimodal workers degrade or fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    Succeeded,
    Degraded,
    Failed,
}

impl ExtractionStatus {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

/// Diagnostic emitted by an extractor without blocking other modalities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionDiagnostic {
    pub status: ExtractionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ExtractionDiagnostic {
    /// Validates status and requires a useful message for degraded/failed work.
    pub fn new(status: ExtractionStatus, message: Option<String>) -> Result<Self, DomainError> {
        let message = message
            .map(|value| required_text("extraction_diagnostic", value))
            .transpose()?;
        if status != ExtractionStatus::Succeeded && message.is_none() {
            return Err(DomainError::invalid(
                "extraction_diagnostic",
                "degraded or failed extraction requires a diagnostic message",
            ));
        }

        Ok(Self { status, message })
    }
}

/// Metadata shared by text, image, OCR, caption, table, layout, and embedding evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceExtractionMetadata {
    pub modality: EvidenceModality,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extractor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extractor_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_evidence_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_region: Option<LayoutRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_dimension: Option<u16>,
    pub diagnostic: ExtractionDiagnostic,
}

impl EvidenceExtractionMetadata {
    /// Creates validated default text evidence metadata.
    pub fn text_span() -> Self {
        Self {
            modality: EvidenceModality::TextSpan,
            source_uri: None,
            source_hash: None,
            media_hash: None,
            extractor: None,
            extractor_version: None,
            observed_at: None,
            parent_evidence_id: None,
            layout_region: None,
            embedding_model: None,
            embedding_dimension: None,
            diagnostic: ExtractionDiagnostic {
                status: ExtractionStatus::Succeeded,
                message: None,
            },
        }
    }

    /// Validates extractor metadata and cross-field modality invariants.
    pub fn validate(mut self) -> Result<Self, DomainError> {
        self.source_uri = normalize_optional_text("source_uri", self.source_uri)?;
        self.source_hash = normalize_optional_text("source_hash", self.source_hash)?;
        self.media_hash = normalize_optional_text("media_hash", self.media_hash)?;
        self.extractor = normalize_optional_text("extractor", self.extractor)?;
        self.extractor_version =
            normalize_optional_text("extractor_version", self.extractor_version)?;
        self.observed_at = normalize_optional_text("observed_at", self.observed_at)?;
        self.parent_evidence_id =
            normalize_optional_text("parent_evidence_id", self.parent_evidence_id)?;
        self.embedding_model = normalize_optional_text("embedding_model", self.embedding_model)?;
        self.diagnostic =
            ExtractionDiagnostic::new(self.diagnostic.status, self.diagnostic.message)?;
        if let Some(region) = self.layout_region {
            self.layout_region = Some(LayoutRegion::new(
                region.page_number,
                region.x,
                region.y,
                region.width,
                region.height,
            )?);
        }

        if self.modality.requires_parent() && self.parent_evidence_id.is_none() {
            return Err(DomainError::invalid(
                "parent_evidence_id",
                "derived multimodal evidence must reference a parent evidence item",
            ));
        }
        if self.modality == EvidenceModality::ImageEmbedding
            && (self.embedding_model.is_none()
                || self.embedding_dimension.is_none()
                || self.embedding_dimension == Some(0))
        {
            return Err(DomainError::invalid(
                "embedding_model",
                "image embedding evidence requires model and positive dimension metadata",
            ));
        }
        if self.modality == EvidenceModality::LayoutRegion && self.layout_region.is_none() {
            return Err(DomainError::invalid(
                "layout_region",
                "layout region evidence requires coordinates",
            ));
        }
        if self.modality == EvidenceModality::ImageAsset
            && self.media_hash.is_none()
            && self.source_hash.is_none()
        {
            return Err(DomainError::invalid(
                "media_hash",
                "image evidence requires a media hash or source hash",
            ));
        }

        Ok(self)
    }
}

fn normalize_optional_text(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, DomainError> {
    value.map(|inner| required_text(field, inner)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_derived_multimodal_metadata() {
        let metadata = EvidenceExtractionMetadata {
            modality: EvidenceModality::OcrText,
            parent_evidence_id: Some(" image-1 ".to_owned()),
            extractor: Some(" tesseract ".to_owned()),
            extractor_version: Some(" 5.4 ".to_owned()),
            observed_at: Some(" 2026-05-13T00:00:00Z ".to_owned()),
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect("metadata should validate");

        assert_eq!(metadata.parent_evidence_id.as_deref(), Some("image-1"));
        assert_eq!(metadata.extractor.as_deref(), Some("tesseract"));
    }

    #[test]
    fn rejects_incomplete_multimodal_metadata() {
        let derived = EvidenceExtractionMetadata {
            modality: EvidenceModality::Caption,
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect_err("derived evidence needs a parent");
        let failed =
            ExtractionDiagnostic::new(ExtractionStatus::Failed, None).expect_err("message needed");
        let layout = EvidenceExtractionMetadata {
            modality: EvidenceModality::LayoutRegion,
            parent_evidence_id: Some("page-1".to_owned()),
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect_err("layout coordinates needed");
        let invalid_region = EvidenceExtractionMetadata {
            modality: EvidenceModality::LayoutRegion,
            parent_evidence_id: Some("page-1".to_owned()),
            layout_region: Some(LayoutRegion {
                page_number: 0,
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            }),
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect_err("layout region invariants should be rechecked");
        let empty_embedding = EvidenceExtractionMetadata {
            modality: EvidenceModality::ImageEmbedding,
            parent_evidence_id: Some("image-1".to_owned()),
            embedding_model: Some("clip".to_owned()),
            embedding_dimension: Some(0),
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect_err("zero dimension should be rejected");
        let degraded_without_message = EvidenceExtractionMetadata {
            diagnostic: ExtractionDiagnostic {
                status: ExtractionStatus::Degraded,
                message: Some(" ".to_owned()),
            },
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect_err("diagnostic message invariant should be rechecked");

        assert_eq!(derived.field, "parent_evidence_id");
        assert_eq!(failed.field, "extraction_diagnostic");
        assert_eq!(layout.field, "layout_region");
        assert_eq!(invalid_region.field, "layout_region");
        assert_eq!(empty_embedding.field, "embedding_model");
        assert_eq!(degraded_without_message.field, "extraction_diagnostic");
    }

    #[test]
    fn validates_layout_and_image_embedding_contracts() {
        let region = LayoutRegion::new(1, 10, 20, 300, 120).expect("region should validate");
        let embedding = EvidenceExtractionMetadata {
            modality: EvidenceModality::ImageEmbedding,
            parent_evidence_id: Some("image-1".to_owned()),
            embedding_model: Some("clip-local-hash-v1".to_owned()),
            embedding_dimension: Some(16),
            ..EvidenceExtractionMetadata::text_span()
        }
        .validate()
        .expect("embedding metadata should validate");

        assert_eq!(region.width, 300);
        assert_eq!(embedding.modality.as_str(), "image_embedding");
    }
}
