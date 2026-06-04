use super::code_query_excerpts::{call_excerpt, callee_excerpt, reference_excerpt};

#[test]
fn call_excerpt_prefers_actual_call_line_over_type_mentions() {
    let excerpt = call_excerpt(
        Some(
            r#"
const params = {} satisfies Parameters<typeof generateObject>[0]
return yield* Effect.promise(() => generateObject(params).then((r) => r.object))
"#,
        ),
        "generate",
        "generateObject",
    );

    assert_eq!(
        excerpt,
        "generate calls generateObject: return yield* Effect.promise(() => generateObject(params).then((r) => r.object))"
    );
}

#[test]
fn call_excerpt_requires_identifier_boundaries_for_call_sites() {
    let excerpt = call_excerpt(
        Some(
            r#"
int rk_dispatch_read(
    const struct rk_driver_ops *ops)
{
    int result = ops->read(dev, buffer, length);
}
"#,
        ),
        "rk_dispatch_read",
        "read",
    );

    assert_eq!(
        excerpt,
        "rk_dispatch_read calls read: int result = ops->read(dev, buffer, length);"
    );
}

#[test]
fn call_excerpt_prefers_local_callable_declaration_over_later_invocation() {
    let excerpt = call_excerpt(
        Some(
            r#"
auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
    cache.Insert(event.key);
    return pipeline(event);
};
total += append_event(event);
"#,
        ),
        "RunPipeline",
        "append_event",
    );

    assert_eq!(
        excerpt,
        "RunPipeline calls append_event: auto append_event = [&cache, &pipeline](const PipelineEvent& event) {"
    );
}

#[test]
fn callee_excerpt_includes_inline_callable_body_context() {
    let excerpt = callee_excerpt(
        Some(
            r#"
auto append_event = [&cache, &pipeline](const PipelineEvent& event) {
    cache.Insert(event.key);
    return pipeline(event);
};
total += append_event(event);
"#,
        ),
        None,
        "RunPipeline",
        "append_event",
    );

    assert_eq!(
        excerpt,
        "RunPipeline calls append_event: auto append_event = [&cache, &pipeline](const PipelineEvent& event) { cache.Insert(event.key); return pipeline(event); };"
    );
    assert!(!excerpt.contains("total += append_event"));
}

#[test]
fn callee_excerpt_includes_bounded_execution_context_after_call_site() {
    let excerpt = callee_excerpt(
        Some(
            r#"
int rk_dispatch_read(struct rk_driver_ops *ops) {
    if (!rk_validate_device(dev)) {
        return -EINVAL;
    }
    if (ops->open(dev) < 0) {
        return -EIO;
    }
    if (rk_lock_device(dev) < 0) {
        return -EBUSY;
    }
    int result = ops->read(dev, buffer, length);
    rk_unlock_device(dev);
    return result;
}
"#,
        ),
        None,
        "rk_dispatch_read",
        "rk_validate_device",
    );

    assert!(excerpt.contains("rk_validate_device(dev)"));
    assert!(excerpt.contains("ops->open(dev)"));
    assert!(excerpt.contains("ops->read(dev, buffer, length)"));
    assert!(!excerpt.contains("rk_unlock_device(dev)"));
    assert!(!excerpt.contains("return result"));
}

#[test]
fn callee_excerpt_includes_preceding_control_context() {
    let excerpt = callee_excerpt(
        Some(
            r#"
rk_install_main() {
  local command="${1:-install}"
  case "$command" in
    install) rk_runtime_dispatch "install" ;;
    doctor) rk_runtime_dispatch "doctor" ;;
    *) rk_missing_command "$command" ;;
  esac
}
"#,
        ),
        None,
        "rk_install_main",
        "rk_runtime_dispatch",
    );

    assert!(excerpt.contains("case \"$command\" in"), "{excerpt}");
    assert!(
        excerpt.contains("rk_runtime_dispatch \"install\""),
        "{excerpt}"
    );
    assert!(
        excerpt.contains("rk_missing_command \"$command\""),
        "{excerpt}"
    );
}

#[test]
fn callee_excerpt_appends_resolved_callee_body_context() {
    let excerpt = callee_excerpt(
        Some(
            r#"
public String dispatch(String value) {
    Function<String, String> transformer = ignored -> create().handle(value);
    return transformer.apply(value);
}
"#,
        ),
        Some(
            r#"
public String handle(String value) {
    return normalize(value).trim();
}
"#,
        ),
        "dispatch",
        "handle",
    );

    assert!(excerpt.contains("create().handle(value)"), "{excerpt}");
    assert!(excerpt.contains("normalize(value).trim()"), "{excerpt}");
}

#[test]
fn callee_excerpt_keeps_deeper_resolved_body_calls_within_bound() {
    let excerpt = callee_excerpt(
        Some(
            r#"
Status InternalGet(const Slice& key) {
    return BlockReader(table, key);
}
"#,
        ),
        Some(
            r#"
Iterator* BlockReader(void* arg, const ReadOptions& options, const Slice& index_value) {
    Table* table = reinterpret_cast<Table*>(arg);
    BlockContents contents;
    Slice handle = index_value;
    bool may_cache = options.fill_cache;
    Cache::Handle* cache_handle = nullptr;
    Block* block = nullptr;
    Status s;
    if (cache_handle == nullptr) {
        s = ReadBlock(table->rep_->file.get(), options, handle, &contents);
    }
    if (s.ok()) {
        block = new Block(contents);
    }
    return block->NewIterator(table->rep_->options.comparator);
    int padding_one = 1;
    int padding_two = 2;
    int padding_three = 3;
    int padding_four = 4;
    int padding_five = 5;
    int padding_six = 6;
    int padding_seven = 7;
    int padding_eight = 8;
    int padding_nine = 9;
    int padding_ten = 10;
    int padding_eleven = 11;
    int padding_twelve = 12;
    int padding_thirteen = 13;
    int padding_fourteen = 14;
    int padding_fifteen = 15;
    int padding_sixteen = 16;
    int padding_seventeen = 17;
    unreachable_cleanup();
    return nullptr;
}
"#,
        ),
        "Table::InternalGet",
        "BlockReader",
    );

    assert!(excerpt.contains("ReadBlock("), "{excerpt}");
    assert!(!excerpt.contains("unreachable_cleanup"), "{excerpt}");
}

#[test]
fn reference_excerpt_includes_matching_source_line() {
    let excerpt = reference_excerpt(
        Some(
            r#"
int rk_driver_read(struct rk_device *dev)
{
    buffer[0] = (char)RK_TRACE_VALUE(dev->fd);
}
"#,
        ),
        "call",
        "RK_TRACE_VALUE",
    );

    assert_eq!(
        excerpt,
        "call reference to RK_TRACE_VALUE: buffer[0] = (char)RK_TRACE_VALUE(dev->fd);"
    );
}
