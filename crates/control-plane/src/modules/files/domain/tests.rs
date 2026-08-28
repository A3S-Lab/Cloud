use super::*;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, Sha256Digest, UserFileId, UserFileUploadId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const FIXTURE: &str = include_str!("../../../../../../contracts/k0.1/user-file.acl");

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn identifier<T>(value: &str, constructor: impl FnOnce(Uuid) -> T) -> T {
    constructor(Uuid::parse_str(value).expect("UUID"))
}

fn fixture_contract() -> UserFileAdmissionContract {
    UserFileAdmissionContract::from_spec(UserFileAdmissionContractSpec {
        original_name: "report.pdf".into(),
        upload_expires_at: timestamp("2026-08-21T01:00:00Z"),
        retention_until: timestamp("2027-08-21T00:00:00Z"),
        scan_policy: UserFileScanPolicy::Required,
        content: UserFileContentReference::new(
            identifier(
                "018f0000-0000-7000-8000-000000000201",
                OrganizationId::from_uuid,
            ),
            identifier("018f0000-0000-7000-8000-000000000202", ProjectId::from_uuid),
            identifier(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            identifier(
                "018f0000-0000-7000-8000-000000000204",
                UserFileUploadId::from_uuid,
            ),
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            4_096,
            "application/pdf",
        )
        .expect("content reference"),
    })
    .expect("contract")
}

fn reserved_file() -> UserFile {
    UserFile::reserve(
        fixture_contract(),
        PrincipalId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000205").expect("principal"),
        ),
        timestamp("2026-08-21T00:00:00Z"),
    )
    .expect("reserved file")
}

fn stored_write(file: &UserFile) -> UserFileObjectWrite {
    UserFileObjectWrite::stored(file.contract.spec().content.clone(), false)
}

fn scan_receipt(file: &UserFile, decision: UserFileScanDecision) -> UserFileScanReceipt {
    UserFileScanReceipt::new(
        file.contract.spec().content.clone(),
        Sha256Digest::from_bytes(b"scanner evidence"),
        decision,
    )
    .expect("scan receipt")
}

#[test]
fn canonical_contract_matches_checked_in_acl_and_round_trips() {
    let contract = fixture_contract();
    assert_eq!(contract.canonical_acl(), FIXTURE);
    assert_eq!(
        UserFileAdmissionContract::parse_acl(FIXTURE).expect("fixture"),
        contract
    );
    assert_eq!(
        UserFileAdmissionContract::restore(FIXTURE, contract.digest().as_str())
            .expect("restored fixture"),
        contract
    );
    assert_eq!(contract.spec().content.size_bytes, 4_096);
    assert!(contract
        .spec()
        .content
        .object_ref
        .starts_with("organizations/"));
}

#[test]
fn contract_rejects_noncanonical_unknown_or_identity_drifted_acl() {
    let contract = fixture_contract();
    assert!(UserFileAdmissionContract::parse_acl(FIXTURE.trim_end()).is_err());
    assert!(UserFileAdmissionContract::parse_acl(&FIXTURE.replacen(
        "  content {",
        "  unexpected = true\n  content {",
        1
    ))
    .is_err());
    assert!(
        UserFileAdmissionContract::parse_acl(&FIXTURE.replacen("/content\"", "/foreign\"", 1))
            .is_err()
    );
    assert!(UserFileAdmissionContract::restore(
        FIXTURE,
        Sha256Digest::from_bytes(b"different contract").as_str()
    )
    .is_err());

    let mut reference = contract.spec().content.clone();
    reference.project_id = ProjectId::new();
    assert!(reference.validate().is_err());
}

#[test]
fn contract_rejects_unsafe_names_media_sizes_and_nil_scope() {
    let base = fixture_contract().spec().clone();
    for original_name in ["", " ../report.pdf", "folder/report.pdf", "a\r.pdf"] {
        let mut spec = base.clone();
        spec.original_name = original_name.into();
        assert!(UserFileAdmissionContract::from_spec(spec).is_err());
    }
    for media_type in [
        "",
        "application",
        "application/pdf; charset=utf-8",
        "text/\nplain",
    ] {
        let mut spec = base.clone();
        spec.content.media_type = media_type.into();
        assert!(UserFileAdmissionContract::from_spec(spec).is_err());
    }
    for size in [0, USER_FILE_MAX_BYTES + 1] {
        let mut spec = base.clone();
        spec.content.size_bytes = size;
        assert!(UserFileAdmissionContract::from_spec(spec).is_err());
    }
    let mut spec = base;
    spec.content.organization_id = OrganizationId::from_uuid(Uuid::nil());
    assert!(UserFileAdmissionContract::from_spec(spec).is_err());
}

#[test]
fn organization_quota_is_revisioned_and_json_safe() {
    let organization_id = OrganizationId::new();
    assert!(UserFileQuota::empty(organization_id, USER_FILE_PUBLIC_INTEGER_MAX + 1).is_err());
    assert!(UserFileQuota::restore(
        organization_id,
        1,
        0,
        USER_FILE_PUBLIC_INTEGER_MAX + 1,
        Some(timestamp("2026-08-21T00:00:00Z")),
    )
    .is_err());

    let quota = UserFileQuota::empty(organization_id, USER_FILE_PUBLIC_INTEGER_MAX)
        .expect("maximum JSON-safe quota");
    let reserved = quota
        .reserve(
            USER_FILE_PUBLIC_INTEGER_MAX,
            timestamp("2026-08-21T00:00:00Z"),
        )
        .expect("full quota reservation");
    assert_eq!(reserved.available_bytes(), 0);
    assert_eq!(reserved.revision, 1);
    let released = reserved
        .release(
            USER_FILE_PUBLIC_INTEGER_MAX,
            timestamp("2026-08-21T00:00:01Z"),
        )
        .expect("full quota release");
    assert_eq!(released.allocated_bytes, 0);
    assert_eq!(released.revision, 2);
}

#[test]
fn lifecycle_requires_upload_and_scan_before_admission() {
    let reserved = reserved_file();
    assert_eq!(reserved.state, UserFileState::AwaitingUpload);
    assert_eq!(reserved.aggregate_version, 1);
    assert!(reserved.admitted_reference().is_err());

    let write = stored_write(&reserved);
    let uploaded = reserved
        .record_upload(1, &write, timestamp("2026-08-21T00:30:00Z"))
        .expect("uploaded");
    assert_eq!(uploaded.state, UserFileState::AwaitingScan);
    assert_eq!(uploaded.aggregate_version, 2);
    assert!(uploaded
        .record_upload(2, &write, uploaded.updated_at)
        .is_err());
    assert!(reserved
        .record_upload(7, &write, timestamp("2026-08-21T00:30:00Z"))
        .is_err());
    let foreign = UserFileObjectWrite::stored(
        UserFileContentReference::new(
            reserved.organization_id,
            reserved.project_id,
            UserFileId::new(),
            UserFileUploadId::new(),
            Sha256Digest::from_bytes(b"foreign"),
            7,
            "application/octet-stream",
        )
        .expect("foreign reference"),
        false,
    );
    assert!(reserved
        .record_upload(1, &foreign, timestamp("2026-08-21T00:30:00Z"))
        .is_err());

    let evidence = Sha256Digest::from_bytes(b"scanner evidence");
    let receipt = scan_receipt(&uploaded, UserFileScanDecision::Admitted);
    let admitted = uploaded
        .record_scan(2, &receipt, timestamp("2026-08-21T00:31:00Z"))
        .expect("admitted");
    assert_eq!(admitted.state, UserFileState::Admitted);
    assert_eq!(admitted.scan_evidence_digest, Some(evidence));
    assert_eq!(
        admitted.admitted_reference().expect("reference"),
        &admitted.contract.spec().content
    );

    let foreign_receipt = UserFileScanReceipt::new(
        UserFileContentReference::new(
            uploaded.organization_id,
            uploaded.project_id,
            UserFileId::new(),
            UserFileUploadId::new(),
            Sha256Digest::from_bytes(b"foreign scan target"),
            19,
            "application/octet-stream",
        )
        .expect("foreign scan reference"),
        Sha256Digest::from_bytes(b"scanner evidence"),
        UserFileScanDecision::Admitted,
    )
    .expect("foreign scan receipt");
    assert!(uploaded
        .record_scan(2, &foreign_receipt, timestamp("2026-08-21T00:31:00Z"))
        .is_err());

    let tombstoned = admitted
        .tombstone(3, timestamp("2026-08-22T00:00:00Z"))
        .expect("tombstone");
    assert_eq!(tombstoned.state, UserFileState::Tombstoned);
    assert_eq!(tombstoned.tombstoned_from, Some(UserFileState::Admitted));
    assert_eq!(tombstoned.aggregate_version, 4);
    assert!(tombstoned.admitted_reference().is_err());
}

#[test]
fn rejection_and_upload_expiry_are_distinct_terminal_states() {
    let reserved = reserved_file();
    assert!(reserved
        .expire_upload(1, timestamp("2026-08-21T00:59:59Z"))
        .is_err());
    let expired = reserved
        .expire_upload(1, timestamp("2026-08-21T01:00:00Z"))
        .expect("expired");
    assert_eq!(expired.state, UserFileState::Expired);
    assert!(expired.scan_evidence_digest.is_none());
    assert!(expired.rejection_reason_code.is_none());

    let uploaded = reserved
        .record_upload(
            1,
            &stored_write(&reserved),
            timestamp("2026-08-21T00:20:00Z"),
        )
        .expect("uploaded");
    assert!(UserFileScanReceipt::new(
        uploaded.contract.spec().content.clone(),
        Sha256Digest::from_bytes(b"scan"),
        UserFileScanDecision::Rejected {
            reason_code: "Contains Malware".into(),
        },
    )
    .is_err());
    let rejection = UserFileScanReceipt::new(
        uploaded.contract.spec().content.clone(),
        Sha256Digest::from_bytes(b"scan"),
        UserFileScanDecision::Rejected {
            reason_code: "malware_detected".into(),
        },
    )
    .expect("rejection receipt");
    let rejected = uploaded
        .record_scan(2, &rejection, timestamp("2026-08-21T00:21:00Z"))
        .expect("rejected");
    assert_eq!(rejected.state, UserFileState::Rejected);
    assert_eq!(
        rejected.rejection_reason_code.as_deref(),
        Some("malware_detected")
    );
    assert!(rejected.admitted_reference().is_err());
}

#[test]
fn reservation_enforces_upload_and_retention_bounds() {
    let mut spec = fixture_contract().spec().clone();
    spec.retention_until = spec.upload_expires_at;
    assert!(UserFileAdmissionContract::from_spec(spec).is_err());

    let mut spec = fixture_contract().spec().clone();
    spec.upload_expires_at = timestamp("2026-08-22T00:00:01Z");
    spec.retention_until = timestamp("2027-08-22T00:00:00Z");
    let contract = UserFileAdmissionContract::from_spec(spec).expect("contract");
    assert!(UserFile::reserve(
        contract,
        PrincipalId::new(),
        timestamp("2026-08-21T00:00:00Z")
    )
    .is_err());

    let mut spec = fixture_contract().spec().clone();
    spec.retention_until = timestamp("2036-08-22T00:00:00Z");
    let contract = UserFileAdmissionContract::from_spec(spec).expect("contract");
    assert!(UserFile::reserve(
        contract,
        PrincipalId::new(),
        timestamp("2026-08-21T00:00:00Z")
    )
    .is_err());
}

#[test]
fn lifecycle_event_is_bounded_metadata_only() {
    let file = reserved_file();
    let correlation_id = Uuid::now_v7();
    let envelope =
        UserFileLifecycleChanged::changed(&file, correlation_id, None).expect("lifecycle event");
    assert_eq!(envelope.event_key, "user-file.lifecycle.changed");
    assert_eq!(envelope.aggregate_version, 1);
    assert_eq!(envelope.correlation_id, correlation_id);
    assert_eq!(envelope.payload["schema"], USER_FILE_LIFECYCLE_EVENT_SCHEMA);
    assert!(envelope.payload["cleanupDueAt"].is_null());

    let uploaded = file
        .record_upload(
            file.aggregate_version,
            &stored_write(&file),
            timestamp("2026-08-21T00:30:00Z"),
        )
        .expect("uploaded file");
    let cleanup_event =
        UserFileLifecycleChanged::changed(&uploaded, Uuid::now_v7(), Some(envelope.event_id))
            .expect("cleanup-bearing lifecycle event");
    assert_eq!(
        cleanup_event.payload["cleanupDueAt"],
        serde_json::json!(uploaded.contract.spec().retention_until)
    );

    let serialized = serde_json::to_string(&envelope).expect("event JSON");
    for forbidden in ["provider", "bucket", "credential", "localPath", "bytes"] {
        assert!(!serialized.contains(forbidden));
    }
    assert!(UserFileLifecycleChanged::changed(&file, Uuid::nil(), None).is_err());
}

#[test]
fn restored_rows_reject_scope_and_lifecycle_drift() {
    let mut file = reserved_file();
    file.organization_id = OrganizationId::new();
    assert!(file.validate().is_err());

    let mut file = reserved_file();
    file.aggregate_version = 2;
    assert!(file.validate().is_err());

    let mut file = reserved_file();
    file.scan_evidence_digest = Some(Sha256Digest::from_bytes(b"impossible scan"));
    assert!(file.validate().is_err());
}
