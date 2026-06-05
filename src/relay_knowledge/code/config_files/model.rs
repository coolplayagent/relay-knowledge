#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfigFact {
    pub(in crate::code) name: String,
    pub(in crate::code) kind: &'static str,
    pub(in crate::code) value_kind: ConfigValueKind,
    pub(in crate::code) range: ConfigRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ConfigValueKind {
    #[default]
    Unknown,
    Boolean,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::code) struct ConfigReference {
    pub(in crate::code) name: String,
    pub(in crate::code) kind: &'static str,
    pub(in crate::code) range: ConfigRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::code) struct ConfigImport {
    pub(in crate::code) module: String,
    pub(in crate::code) range: ConfigRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::code) struct ConfigRange {
    pub(in crate::code) byte_start: usize,
    pub(in crate::code) byte_end: usize,
    pub(in crate::code) line_start: usize,
    pub(in crate::code) line_end: usize,
}
