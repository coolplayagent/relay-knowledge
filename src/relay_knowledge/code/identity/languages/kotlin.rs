use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution};

const KOTLIN_EXTENSIONS: [&str; 2] = ["kt", "kts"];

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    super::jvm::resolve_import(import, context, "kotlin", &KOTLIN_EXTENSIONS)
}
