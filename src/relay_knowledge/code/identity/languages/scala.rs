use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution};

const SCALA_EXTENSIONS: [&str; 2] = ["scala", "sc"];

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    super::jvm::resolve_import(import, context, "scala", &SCALA_EXTENSIONS)
}
