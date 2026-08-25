use super::*;

fn c_language() -> LanguageSpec {
    LanguageSpec {
        id: "c",
        language: || tree_sitter_c::LANGUAGE.into(),
        tags_query: "",
    }
}

fn rust_language() -> LanguageSpec {
    LanguageSpec {
        id: "rust",
        language: || tree_sitter_rust::LANGUAGE.into(),
        tags_query: tree_sitter_rust::TAGS_QUERY,
    }
}

fn python_language() -> LanguageSpec {
    LanguageSpec {
        id: "python",
        language: || tree_sitter_python::LANGUAGE.into(),
        tags_query: tree_sitter_python::TAGS_QUERY,
    }
}

fn clear_syntax_parser_cache() {
    SYNTAX_PARSERS.with(|parsers| parsers.borrow_mut().parsers.clear());
}

#[test]
fn syntax_parsers_are_reused_per_worker_and_isolated_by_language() {
    clear_syntax_parser_cache();

    let first = with_syntax_parser(rust_language(), |parser| std::ptr::from_mut(parser).addr())
        .expect("Rust parser should be configured");
    let second = with_syntax_parser(rust_language(), |parser| std::ptr::from_mut(parser).addr())
        .expect("Rust parser should be reused");
    let other = with_syntax_parser(python_language(), |parser| {
        std::ptr::from_mut(parser).addr()
    })
    .expect("Python parser should be configured independently");

    assert_eq!(first, second);
    assert_ne!(first, other);
}

#[test]
fn syntax_parser_cache_stops_at_its_hard_capacity() {
    let mut cache = SyntaxParserCache::default();
    let mut creations = 0usize;

    for address in 0..=MAX_CACHED_SYNTAX_PARSERS {
        let parser = cache
            .get_or_try_insert(
                SyntaxParserCacheKey {
                    language_id: "syntax-parser-cap-test",
                    language_factory_address: address,
                },
                || {
                    creations += 1;
                    configured_syntax_parser(c_language())
                },
            )
            .expect("cache insertion should not fail");
        assert_eq!(parser.is_some(), address < MAX_CACHED_SYNTAX_PARSERS);
    }

    assert_eq!(cache.parsers.len(), MAX_CACHED_SYNTAX_PARSERS);
    assert_eq!(creations, MAX_CACHED_SYNTAX_PARSERS);
}

#[test]
fn compiled_tag_queries_are_reused_per_static_language() {
    let first = compiled_tag_query(rust_language()).expect("Rust query should compile");
    let second = compiled_tag_query(rust_language()).expect("Rust query should be cached");
    let other = compiled_tag_query(python_language()).expect("Python query should compile");

    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first, &other));
}

#[test]
fn invalid_tag_queries_are_not_cached() {
    let language = LanguageSpec {
        id: "invalid-tag-query-cache-test",
        language: || tree_sitter_rust::LANGUAGE.into(),
        tags_query: "(",
    };
    let key = tag_query_cache_key(language);

    assert!(compiled_tag_query(language).is_err());
    assert!(compiled_tag_query(language).is_err());
    assert!(!lock_compiled_tag_queries().contains_key(&key));
}

#[test]
fn tag_query_cache_identity_includes_the_language_factory() {
    let c = LanguageSpec {
        id: "shared-query-cache-language",
        language: || tree_sitter_c::LANGUAGE.into(),
        tags_query: "",
    };
    let rust = LanguageSpec {
        id: "shared-query-cache-language",
        language: || tree_sitter_rust::LANGUAGE.into(),
        tags_query: "",
    };

    assert!(tag_query_cache_key(c) != tag_query_cache_key(rust));
    let c_query = compiled_tag_query(c).expect("C query should compile");
    let rust_query = compiled_tag_query(rust).expect("Rust query should compile separately");
    assert!(!Arc::ptr_eq(&c_query, &rust_query));
}

#[test]
fn query_compilation_panic_does_not_poison_other_languages() {
    let panicking = LanguageSpec {
        id: "panicking-query-cache-language",
        language: || panic!("query language factory panic"),
        tags_query: "",
    };

    let panic = std::panic::catch_unwind(|| compiled_tag_query(panicking));
    assert!(panic.is_err());
    assert!(
        !COMPILED_TAG_QUERIES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .is_poisoned()
    );
    compiled_tag_query(rust_language())
        .expect("an unrelated language should still use the compiled-query cache");
}

#[test]
fn syntax_work_budget_scales_with_content_and_stays_bounded() {
    assert_eq!(syntax_stage_work_quanta(0), SYNTAX_BASE_WORK_QUANTA);
    assert!(syntax_stage_work_quanta(64 * 1_024) > SYNTAX_BASE_WORK_QUANTA);
    assert_eq!(syntax_stage_work_quanta(usize::MAX), SYNTAX_MAX_WORK_QUANTA);
}

#[test]
fn callback_work_budget_exhaustion_is_deterministic() {
    let mut budget = SyntaxCallbackWorkBudget::new(2);

    assert_eq!(budget.consume(), ControlFlow::Continue(()));
    assert_eq!(budget.consume(), ControlFlow::Continue(()));
    assert_eq!(budget.consume(), ControlFlow::Break(()));
    assert!(budget.exhausted);
    assert_eq!(budget.consume(), ControlFlow::Break(()));
}

#[test]
fn parser_cancels_pathological_error_recovery_at_the_budget() {
    clear_syntax_parser_cache();
    let fragment = "(".repeat(64 * 1_024);

    let error = parse_tree_with_budget(c_language(), &fragment, 0)
        .expect_err("the progress callback should cancel pathological recovery");

    assert!(
        error
            .to_string()
            .contains("exceeded bounded syntax budget of 0 callback work quanta")
    );
}

#[test]
fn parser_reset_discards_cancelled_parse_state_before_reuse() {
    clear_syntax_parser_cache();
    let fragment = "(".repeat(64 * 1_024);

    parse_tree_with_budget(c_language(), &fragment, 0)
        .expect_err("the first parse should be cancelled at zero work quanta");
    let tree = parse_tree(c_language(), "int main(void) { return 0; }\n")
        .expect("the reused parser should start the next document from the beginning");

    assert!(!tree.root_node().has_error());
}

#[test]
fn parser_rejects_repeated_top_level_initializer_fragments_before_grammar_recovery() {
    let mut fragment = String::new();
    for index in 0..MIN_REPEATED_INITIALIZER_FRAGMENT_LINES {
        fragment.push_str(&format!("{{ .flag = {index}, .value = 1 }},\n"));
    }

    let error = parse_tree(c_language(), &fragment)
        .expect_err("a repeated declaration-free initializer fragment should be bounded");

    assert!(
        error
            .to_string()
            .contains("top-level designated initializer fragment")
    );
}

#[test]
fn parser_keeps_designated_initializers_inside_a_declaration() {
    let mut declaration = String::from("static const struct item values[] = {\n");
    for index in 0..MIN_REPEATED_INITIALIZER_FRAGMENT_LINES {
        declaration.push_str(&format!("    {{ .flag = {index}, .value = 1 }},\n"));
    }
    declaration.push_str("};\n");

    parse_tree(c_language(), &declaration)
        .expect("a declared initializer table remains eligible for structured parsing");
}
