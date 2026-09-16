//! Content evidence for ambiguous headers and extensionless interpreter scripts.
use super::*;

pub(in crate::code) fn detect_source_language(path: &str, bytes: &[u8]) -> Option<LanguageSpec> {
    let language = detect_language(path);
    if let Some(spec) = language.filter(|spec| matches!(spec.id, "javascript" | "jsx")) {
        if flow_pragma(bytes) {
            // Typed JSX retains declarations in Flow sources; unsupported
            // syntax continues to produce ordinary partial diagnostics.
            return Some(LanguageSpec {
                id: spec.id,
                language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
                tags_query: tree_sitter_typescript::TAGS_QUERY,
            });
        }
    }
    if Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("h"))
        && cpp_header_syntax(bytes)
    {
        return language_for_extension("cpp");
    }
    if language.is_some() || Path::new(path).extension().is_some() {
        return language;
    }
    let line = bytes.split(|byte| *byte == b'\n').next()?;
    if line.len() > 256 {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?.strip_prefix("#!")?.trim();
    let mut words = line.split_whitespace();
    let interpreter = words.next()?.rsplit('/').next()?;
    let interpreter = if interpreter == "env" {
        let next = words.next()?;
        if next == "-S" { words.next()? } else { next }
    } else {
        interpreter
    };
    match interpreter {
        "bash" | "sh" | "dash" => Some(bash()),
        "python" | "python3" => language_for_extension("py"),
        "ruby" => Some(ruby()),
        "node" => language_for_extension("js"),
        "php" => language_for_extension("php"),
        _ => None,
    }
}

fn flow_pragma(bytes: &[u8]) -> bool {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let prefix = prefix.trim_start();
    let comment = if let Some(comment) = prefix.strip_prefix("//") {
        comment.lines().next().unwrap_or_default()
    } else if let Some(comment) = prefix.strip_prefix("/*") {
        let Some((comment, _)) = comment.split_once("*/") else {
            return false;
        };
        comment
    } else {
        return false;
    };
    comment.split_whitespace().any(|word| word == "@flow")
}

fn cpp_header_syntax(bytes: &[u8]) -> bool {
    // One lexical pass; strings and comments cannot select the C++ grammar.
    let mut at = 0;
    let mut previous = &b""[..];
    let mut typedef = false;
    let mut c_keywords = std::collections::BTreeSet::<&[u8]>::new();
    let mut declaration_keyword = false;
    while at < bytes.len() {
        if bytes[at] == b'#' {
            let start = at;
            at = logical_line_end(bytes, at);
            let words = bytes[start..at]
                .split(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
                .filter(|word| !word.is_empty())
                .take(3)
                .collect::<Vec<_>>();
            if let [b"define", name, ..] = words.as_slice()
                && matches!(*name, b"class" | b"namespace")
            {
                c_keywords.insert(name);
            }
            previous = b"";
            declaration_keyword = false;
        } else if bytes.get(at..at + 2) == Some(b"//") {
            at += 2;
            at = logical_line_end(bytes, at);
        } else if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            while at < bytes.len() && bytes.get(at..at + 2) != Some(b"*/") {
                at += 1;
            }
            at = at.saturating_add(2);
        } else if matches!(bytes[at], b'\'' | b'"') {
            let quote = bytes[at];
            at += 1;
            while at < bytes.len() {
                if bytes[at] == b'\\' {
                    at = at.saturating_add(2);
                    continue;
                }
                let end = bytes[at] == quote;
                at += 1;
                if end {
                    break;
                }
            }
            previous = b"";
        } else if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
            let start = at;
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            if declaration_keyword {
                return true;
            }
            let word = &bytes[start..at];
            if word == b"typedef" {
                typedef = true;
            }
            if typedef && matches!(word, b"namespace" | b"class") {
                c_keywords.insert(word);
            }
            declaration_keyword = matches!(word, b"namespace" | b"class")
                && !typedef
                && !c_keywords.contains(word)
                && !matches!(previous, b"struct" | b"union" | b"enum");
            previous = word;
        } else if bytes.get(at..at + 2) == Some(b"::")
            || (bytes[at] == b'<' && previous == b"template")
        {
            return true;
        } else {
            if !bytes[at].is_ascii_whitespace() {
                previous = b"";
                declaration_keyword = false;
                if bytes[at] == b';' {
                    typedef = false;
                }
            }
            at += 1;
        }
    }
    false
}

// Translation-phase line splicing also applies to directives and // comments.
fn logical_line_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        if bytes[at] == b'\n' {
            let previous = if at > 0 && bytes[at - 1] == b'\r' {
                at - 1
            } else {
                at
            };
            if previous == 0 || bytes[previous - 1] != b'\\' {
                break;
            }
        }
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extensionless_scripts_require_bounded_interpreter_evidence() {
        for line in [
            "#!/bin/bash\necho $FLAG",
            "#!/usr/bin/env bash\necho $FLAG",
            "#!/usr/bin/env -S bash -e\necho $FLAG",
        ] {
            assert_eq!(
                detect_source_language("libexec/task", line.as_bytes())
                    .unwrap()
                    .id,
                "bash"
            );
        }
        for source in [
            "echo '#!/bin/bash'",
            "#!/bin/unknown",
            "#!/usr/bin/env bashful",
        ] {
            assert!(detect_source_language("libexec/task", source.as_bytes()).is_none());
        }
    }
    #[test]
    fn header_dialect_ignores_comments_and_strings() {
        assert_eq!(
            detect_source_language("Widget.H", b"class Widget {};")
                .unwrap()
                .id,
            "cpp"
        );
        for source in [
            "typedef int class; class value;",
            "typedef int namespace; namespace value;",
            "struct class value;",
            "#define class int\nclass value;",
            "#define UNUSED \\\n class Example\nint run(void);",
            "// note \\\r\nclass Example {};\nint run(void);",
        ] {
            assert_eq!(
                detect_source_language("api.h", source.as_bytes())
                    .unwrap()
                    .id,
                "c",
                "{source}"
            );
        }
        assert_eq!(
            detect_source_language(
                "entry.h",
                b"// class Foo\nconst char *s = \"std::string\";\nint run();"
            )
            .unwrap()
            .id,
            "c"
        );
        for source in [
            "namespace details { class Worker {}; }",
            "inline bool Worker::enabled() {return flag.load();}",
            "template<class T> struct Value {};",
        ] {
            assert_eq!(
                detect_source_language("entry.h", source.as_bytes())
                    .unwrap()
                    .id,
                "cpp"
            );
        }
    }

    #[test]
    fn flow_pragma_requires_the_first_bounded_comment() {
        let boundary = format!("// @flow\n{}\u{4e2d}", " ".repeat(4086));
        assert!(flow_pragma(boundary.as_bytes()));
        for source in [
            "// @flow strict-local\nclass Widget {}",
            "/*\n * @flow\n */\nclass Widget {}",
        ] {
            assert!(flow_pragma(source.as_bytes()));
        }
        for source in [
            "const a='@flow';",
            "const a=0;\n// @flow",
            "/* example */\n// @flow",
        ] {
            assert!(!flow_pragma(source.as_bytes()));
        }
    }
}
