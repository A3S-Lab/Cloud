const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/155_connector_execution_attempt_resolutions.sql"
));

#[test]
fn migration_155_closes_indeterminate_attempts_without_provider_replay() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table connector_execution_attempt_resolutions",
        "outcome in ('accepted', 'retryable', 'rejected', 'indeterminate')",
        "stored_state <> 'dispatching'",
        "new.resolved_at < stored_outcome_deadline_at",
        "for update",
        "connector execution attempt resolutions are immutable",
        "indeterminate connector execution evidence requires exact resolution authority",
        "deferrable initially deferred",
        "without authorizing provider replay",
    ] {
        assert!(
            lower.contains(expected),
            "migration 155 is missing {expected}"
        );
    }
    for forbidden in [
        "update connector_revisions",
        "delete from connector_execution_attempts",
        "provider_response",
        "retry_count",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 155 contains forbidden authority {forbidden}"
        );
    }
}
