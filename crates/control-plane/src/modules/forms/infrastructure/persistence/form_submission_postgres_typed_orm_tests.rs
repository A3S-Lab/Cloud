#[test]
fn postgres_form_submission_persistence_uses_the_typed_a3s_orm_api() {
    let source = include_str!("form_submission_postgres.rs");
    for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
        assert!(
            !source.contains(forbidden),
            "FormSubmission persistence must not contain {forbidden}"
        );
    }
    assert!(source.contains("select_from::<FormSubmissions>()"));
    assert!(source.contains("insert_into::<FormSubmissions>()"));

    let schema = include_str!("form_submission_postgres/schema.rs");
    assert!(schema.contains("struct FormSubmissions"));
}
