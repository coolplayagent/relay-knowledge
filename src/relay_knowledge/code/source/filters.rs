pub(in crate::code) const DEFAULT_EXCLUDED_EXTENSIONS: &[&str] = &[
    "7z", "avif", "bmp", "bz2", "class", "eot", "gif", "gz", "ico", "jar", "jpeg", "jpg", "jsonl",
    "lockb", "map", "mov", "mp4", "otf", "pdf", "png", "svg", "tar", "tgz", "ttf", "wasm", "webm",
    "woff", "woff2", "zip", "zst",
];
pub(in crate::code) const DEFAULT_EXCLUDED_FILENAMES: &[&str] = &["uv.lock"];
pub(in crate::code) const FILESYSTEM_BROAD_SEGMENTS: &[&str] = &[
    ".cache",
    ".git",
    ".next",
    ".nuxt",
    ".parcel-cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "target",
    "third_party",
    "vendor",
    "venv",
];
pub(in crate::code) const FILESYSTEM_DEFAULT_SOURCE_ROOTS: &[&str] = &[
    "app",
    "config",
    "configs",
    "docs",
    "extensions",
    "include",
    "lib",
    "modules",
    "packages",
    "plugins",
    "source",
    "Sources",
    "src",
];
pub(in crate::code) const FILESYSTEM_AUTO_DISCOVERY_FILTERS: &[&str] =
    &["src", "include", "lib", "Sources"];
