//! Language parsing, syntax recovery, and code graph record extraction.

mod chunks;
mod dependencies;
mod file;
mod imports;
mod languages;
mod manual;
mod nodes;
mod records;
mod recovery;
mod routes;
mod syntax;
mod text;
pub(in crate::code) mod workspace;

pub(in crate::code) use dependencies::{
    dependency_manifest_language_ids, dependency_manifest_overrides_default_exclusion,
};
pub(in crate::code) use file::parse_indexed_file;
pub(in crate::code::parser) use file::{FileParseContext, FileParseOutput, ReferenceDedupKey};

#[cfg(test)]
use {
    super::{
        SnapshotBuild, config_files as configuration,
        languages::{LanguageSpec, detect_language},
    },
    file::{SyntaxFileInput, parse_syntax_file},
    manual::{collect_manual_nodes, manual_definitions},
    nodes::push_children_reverse,
    recovery::{
        c_family_typedef_like_function_signature, recoverable_c_family_error_line,
        recoverable_decorated_function_error_text, recoverable_decorated_type_error_text,
    },
    syntax::parse_tree,
    text::{MAX_TEXT_FILE_BYTES, validate_text_content},
};

#[cfg(test)]
#[path = "tests/general.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/configuration.rs"]
mod configuration_tests;

#[cfg(test)]
#[path = "tests/configuration_review.rs"]
mod configuration_review_tests;

#[cfg(test)]
#[path = "tests/configuration_documents.rs"]
mod configuration_document_tests;

#[cfg(test)]
#[path = "tests/configuration_paths.rs"]
mod configuration_path_tests;

#[cfg(test)]
#[path = "tests/exported_value.rs"]
mod exported_value_tests;

#[cfg(test)]
#[path = "tests/identity.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "tests/go.rs"]
mod go_tests;

#[cfg(test)]
#[path = "languages/c/tests.rs"]
mod c_tests;

#[cfg(test)]
#[path = "tests/enum_symbols.rs"]
mod enum_tests;

#[cfg(test)]
#[path = "tests/review.rs"]
mod review_tests;

#[cfg(test)]
#[path = "tests/sql.rs"]
mod sql_tests;

#[cfg(test)]
#[path = "languages/c/gcc_recovery_tests.rs"]
mod gcc_recovery_tests;

#[cfg(test)]
#[path = "languages/cpp/tests.rs"]
mod cpp_tests;

#[cfg(test)]
#[path = "tests/manual.rs"]
mod manual_tests;

#[cfg(test)]
#[path = "tests/source_surface.rs"]
mod source_surface_tests;

#[cfg(test)]
#[path = "tests/knowledge_map.rs"]
mod knowledge_map_tests;

#[cfg(test)]
#[path = "tests/text_only_topics.rs"]
mod text_only_topic_tests;

#[cfg(test)]
#[path = "tests/type_references.rs"]
mod type_reference_tests;
