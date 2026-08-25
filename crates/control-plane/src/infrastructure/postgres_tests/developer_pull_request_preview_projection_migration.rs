const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/157_developer_pull_request_preview_projections.sql"
));

#[test]
fn migration_157_persists_one_local_preview_projection_without_another_delivery_rail() {
    for required in [
        "create table developer_pull_request_previews",
        "create table developer_pull_request_change_projections",
        "developer_preview_policy_effective_at_idx",
        "developer_preview_policy_projection_reference_key",
        "developer_pull_request_previews_validate_mutation",
        "developer_pull_request_change_projections_immutable",
        "pull-request Preview mutation changed immutable authority or skipped CAS",
        "pull-request change projection receipts are immutable",
        "not another Inbox, queue, or worker",
    ] {
        assert!(
            MIGRATION.contains(required),
            "migration 157 lost required invariant {required}"
        );
    }
    for forbidden in [
        "create table developer_pull_request_preview_inbox",
        "create table developer_pull_request_preview_queue",
        "create table developer_pull_request_preview_jobs",
        "create table developer_pull_request_preview_retries",
    ] {
        assert!(
            !MIGRATION.contains(forbidden),
            "migration 157 introduced duplicate delivery state {forbidden}"
        );
    }
}
