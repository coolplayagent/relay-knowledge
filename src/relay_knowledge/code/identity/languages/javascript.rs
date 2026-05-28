use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution};

const MODULE_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs"];

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    super::typescript::resolve_import_with_extensions(import, context, MODULE_EXTENSIONS, false)
}
