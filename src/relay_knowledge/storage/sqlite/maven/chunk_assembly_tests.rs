use super::*;

#[test]
fn maven_chunk_assembly_checks_byte_ranges_and_utf8_budgets() {
    let connection = Connection::open_in_memory().unwrap();
    create_refresh_schema(&connection);
    let content = format!(
        "<project><groupId>x</groupId><artifactId>a</artifactId><version>1</version><description>{}</description></project>",
        "多字节 ".repeat(2000)
    );
    let split = content
        .char_indices()
        .map(|(offset, _)| offset)
        .find(|offset| *offset >= 8000)
        .unwrap();
    for (id, start, end) in [("z", 0, split), ("a", split, content.len())] {
        connection.execute("INSERT INTO code_repository_chunks VALUES ('repo','scope',?1,'file','pom.xml',?2,?3,?4,1,1,NULL)", params![id, &content[start..end], start, end]).unwrap();
    }
    let loaded = super::super::pom_documents(&connection, "scope").unwrap();
    assert!(!loaded.has_truncated_documents);
    assert_eq!(loaded.documents.len(), 1);
    assert_eq!(loaded.documents[0].content, content);
    assert!(
        !super::super::effective_models(&connection, "scope")
            .unwrap()
            .preserve_existing_facts
    );
    assert!(matches!(
        super::super::pom_documents_with_limits(&connection, "scope", 1, 2, content.len() - 1),
        Err(StorageError::CapacityExceeded(_))
    ));
    for sql in [
        "UPDATE code_repository_chunks SET byte_start=byte_start+1 WHERE chunk_id='a'",
        "UPDATE code_repository_chunks SET byte_start=byte_start-2 WHERE chunk_id='a'",
        "UPDATE code_repository_chunks SET byte_start=byte_start+1,file_id='other' WHERE chunk_id='a'",
        "DELETE FROM code_repository_chunks WHERE chunk_id='z'",
    ] {
        connection.execute(sql, []).unwrap();
        assert!(
            super::super::pom_documents(&connection, "scope")
                .unwrap()
                .has_truncated_documents,
            "{sql}"
        );
    }
}
