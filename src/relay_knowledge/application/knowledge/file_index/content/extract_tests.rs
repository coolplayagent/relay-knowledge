use super::*;

#[test]
fn content_chunks_keep_query_tokens_in_one_chunk() {
    let filler = "x ".repeat((MAX_CONTENT_CHUNK_BYTES - 12) / 2);
    let token = "TOKEN_SAFE_IDENTIFIER_that_crosses_the_soft_chunk_boundary";
    let content = format!("{filler}{token} tail");

    let chunks = content_chunks(&content);
    let token_chunks = chunks
        .iter()
        .filter(|chunk| chunk.content.contains(token))
        .collect::<Vec<_>>();

    assert_eq!(token_chunks.len(), 1);
    assert!(chunks[0].end_byte as usize <= filler.len());
    assert!(token_chunks[0].start_byte as usize >= filler.len());
}

#[test]
fn content_chunks_end_newline_boundary_on_previous_line() {
    let content = format!("{}\nnext line", "a".repeat(MAX_CONTENT_CHUNK_BYTES - 1));

    let chunks = content_chunks(&content);

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].end_line, 1);
    assert_eq!(chunks[1].start_line, 2);
}
