//! HTTP diagnostic reads use the shared authorized repository service.
use super::*;

#[derive(Deserialize)]
pub(super) struct DiagnosticQuery {
    #[serde(rename = "ref")]
    ref_selector: Option<String>,
    path_filters: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

pub(super) async fn get(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Query(query): Query<DiagnosticQuery>,
) -> Response {
    let paths = match query
        .path_filters
        .map(|p| serde_json::from_str::<Vec<String>>(&p))
        .transpose()
    {
        Ok(paths) => paths.unwrap_or_default(),
        Err(e) => return api_error_response(ApiError::invalid_argument(e.to_string())),
    };
    let repository = match CodeRepositorySelector::new(
        alias,
        query.ref_selector.unwrap_or_else(|| "HEAD".into()),
        paths,
        Vec::new(),
    ) {
        Ok(repository) => repository,
        Err(e) => return api_error_response(ApiError::invalid_argument(e.to_string())),
    };
    match state
        .service
        .code_repository_diagnostics(
            crate::domain::CodeDiagnosticsRequest {
                repository,
                limit: query.limit.unwrap_or(50),
                cursor: query.cursor,
            },
            api_context(&headers),
        )
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => api_error_response(e),
    }
}
