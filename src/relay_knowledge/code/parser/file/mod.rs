use crate::domain::CodeParseStatus;

use super::{
    chunks::{add_file_chunk, chunks_for_symbols},
    dependencies::{collect_dependencies, dependency_manifest_is_facts_only},
    imports::collect_imports,
    manual::collect_manual_nodes,
    records::records_from_captures,
    syntax::{extract_tag_captures_safely, parse_tree_safely},
    text::{count_lines, validate_text_content},
};

use crate::code::{
    CodeIndexError, SnapshotBuild, config_files, generated_detection, languages::detect_language,
    stable_content_hash, stable_id,
};

mod contracts;
mod feature_flag_projection;
mod parse_status;
mod route_projection;
mod text_only;

pub(in crate::code::parser) use contracts::{
    FileParseContext, FileParseOutput, ReferenceDedupKey, SyntaxFileInput,
};

pub(in crate::code) fn parse_indexed_file(
    build: &mut SnapshotBuild,
    path: &str,
    bytes: &[u8],
) -> Result<(), CodeIndexError> {
    let blob_hash = stable_content_hash(bytes);
    let file_id = stable_id(
        "file",
        [&build.repository_id, &build.source_scope, path, &blob_hash],
    );
    let language = detect_language(path);
    let line_count = count_lines(bytes);
    let is_generated = generated_detection::is_generated_file(path, bytes);
    let (parse_status, degraded_reason, content) = validate_text_content(path, bytes, language)?;

    let Some(content) = content else {
        parse_status::record_file_status(
            build,
            parse_status::FileStatusInput {
                path,
                file_id: &file_id,
                language_id: language.map_or("unknown", |spec| spec.id),
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                is_generated,
                degraded_reason,
            },
        );
        return Ok(());
    };
    let Some(language) = language else {
        parse_status::record_file_status(
            build,
            parse_status::FileStatusInput {
                path,
                file_id: &file_id,
                language_id: "unknown",
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                is_generated,
                degraded_reason,
            },
        );
        if dependency_manifest_is_facts_only(path) {
            record_dependencies(build, path, &file_id, &content)?;
            return Ok(());
        }
        add_file_chunk(build, path, &file_id, "unknown", &content)?;
        record_dependencies(build, path, &file_id, &content)?;
        feature_flag_projection::record_feature_flags(
            build, path, &file_id, "unknown", &content, None,
        )?;
        return Ok(());
    };
    if parse_status == CodeParseStatus::TextOnly {
        parse_status::record_file_status(
            build,
            parse_status::FileStatusInput {
                path,
                file_id: &file_id,
                language_id: language.id,
                blob_hash: &blob_hash,
                byte_len: bytes.len(),
                line_count,
                parse_status,
                is_generated,
                degraded_reason,
            },
        );
        text_only::record_topic_symbols(build, path, &file_id, language.id, bytes)?;
        add_file_chunk(build, path, &file_id, language.id, &content)?;
        record_dependencies(build, path, &file_id, &content)?;
        feature_flag_projection::record_feature_flags(
            build,
            path,
            &file_id,
            language.id,
            &content,
            None,
        )?;
        route_projection::record_routes(build, path, &file_id, language.id, &content);
        return Ok(());
    }

    parse_syntax_file(
        build,
        SyntaxFileInput {
            path,
            file_id: &file_id,
            language,
            blob_hash: &blob_hash,
            byte_len: bytes.len(),
            line_count,
            is_generated,
            content: &content,
        },
    )
}

pub(in crate::code::parser) fn parse_syntax_file(
    build: &mut SnapshotBuild,
    input: SyntaxFileInput<'_>,
) -> Result<(), CodeIndexError> {
    let parsed = match parse_tree_safely(input.language, input.content) {
        Ok(parsed) => parsed,
        Err(error) => {
            parse_status::record_tree_sitter_failure(build, &input, "parse", &error);
            feature_flag_projection::record_feature_flags(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
                None,
            )?;
            route_projection::record_routes(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
            );
            return Ok(());
        }
    };
    let root = parsed.root_node();
    let captures = match extract_tag_captures_safely(input.language, root, input.content) {
        Ok(captures) => captures,
        Err(error) => {
            parse_status::record_tree_sitter_failure(build, &input, "query", &error);
            feature_flag_projection::record_feature_flags(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
                None,
            )?;
            route_projection::record_routes(
                build,
                input.path,
                input.file_id,
                input.language.id,
                input.content,
            );
            return Ok(());
        }
    };
    let context = FileParseContext {
        build,
        path: input.path,
        file_id: input.file_id,
        language_id: input.language.id,
        content: input.content,
    };
    let mut output = FileParseOutput::new();
    let (config_definitions, config_references) =
        config_files::structured_facts(input.path, input.language.id, input.content);
    records_from_captures(&context, captures, &mut output)?;
    collect_manual_nodes(
        &context,
        root,
        &config_definitions,
        &config_references,
        &mut output,
    )?;
    let imports = collect_imports(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
        root,
    )?;
    let chunks = chunks_for_symbols(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
        &output.symbols,
    )?;
    let (parse_status, degraded_reason) = parse_status::syntax_parse_status(
        input.language.id,
        root,
        input.content,
        &output,
        &imports,
    );
    parse_status::record_file_status(
        build,
        parse_status::FileStatusInput {
            path: input.path,
            file_id: input.file_id,
            language_id: input.language.id,
            blob_hash: input.blob_hash,
            byte_len: input.byte_len,
            line_count: input.line_count,
            parse_status,
            is_generated: input.is_generated,
            degraded_reason,
        },
    );

    build.symbols.extend(output.symbols);
    build.references.extend(output.references);
    build.imports.extend(imports);
    record_dependencies(build, input.path, input.file_id, input.content)?;
    feature_flag_projection::record_feature_flags(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
        Some(&config_definitions),
    )?;
    build.chunks.extend(chunks);
    route_projection::record_routes(
        build,
        input.path,
        input.file_id,
        input.language.id,
        input.content,
    );

    Ok(())
}

fn record_dependencies(
    build: &mut SnapshotBuild,
    path: &str,
    file_id: &str,
    content: &str,
) -> Result<(), CodeIndexError> {
    let records = collect_dependencies(build, path, file_id, content)?;
    build.dependencies.extend(records);
    Ok(())
}
