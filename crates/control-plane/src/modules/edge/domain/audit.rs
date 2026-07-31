use super::McpCredential;
use crate::modules::operations::AuditRecord;
use crate::modules::shared_kernel::domain::AuditId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const ISSUE_ACTION: &str = "edge.mcp-credential.issue";
const ROTATE_ACTION: &str = "edge.mcp-credential.rotate";
const REVOKE_ACTION: &str = "edge.mcp-credential.revoke";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct McpCredentialAuditDetails {
    project_id: Uuid,
    environment_id: Uuid,
    generation: u64,
    aggregate_version: u64,
    expires_at: DateTime<Utc>,
    revoked: bool,
    result: String,
}

pub(crate) fn mcp_credential_audit_record(
    credential: &McpCredential,
    expected_aggregate_version: Option<u64>,
    actor_id: Uuid,
    request_id: Uuid,
) -> Result<AuditRecord, String> {
    AuditRecord::new(
        AuditId::new(),
        credential.organization_id,
        Some(actor_id),
        audit_action(credential, expected_aggregate_version),
        credential.id.as_uuid(),
        credential.updated_at(),
        request_id,
        serde_json::to_value(expected_details(credential))
            .map_err(|_| "MCP credential audit details could not be serialized".to_string())?,
    )
}

pub(crate) fn validate_mcp_credential_audit(
    audit: &AuditRecord,
    credential: &McpCredential,
    expected_aggregate_version: Option<u64>,
) -> Result<(), String> {
    audit.validate()?;
    let details = serde_json::from_value::<McpCredentialAuditDetails>(audit.details.clone())
        .map_err(|_| "MCP credential audit details are invalid".to_string())?;
    if audit.organization_id != credential.organization_id
        || audit.actor_id.is_none()
        || audit.action != audit_action(credential, expected_aggregate_version)
        || audit.aggregate_id != credential.id.as_uuid()
        || audit.occurred_at != credential.updated_at()
        || details != expected_details(credential)
    {
        return Err("MCP credential audit record is inconsistent".into());
    }
    Ok(())
}

fn audit_action(
    credential: &McpCredential,
    expected_aggregate_version: Option<u64>,
) -> &'static str {
    if credential.revoked_at().is_some() {
        REVOKE_ACTION
    } else if expected_aggregate_version.is_none() {
        ISSUE_ACTION
    } else {
        ROTATE_ACTION
    }
}

fn expected_details(credential: &McpCredential) -> McpCredentialAuditDetails {
    McpCredentialAuditDetails {
        project_id: credential.project_id.as_uuid(),
        environment_id: credential.environment_id.as_uuid(),
        generation: credential.generation(),
        aggregate_version: credential.aggregate_version(),
        expires_at: credential.expires_at(),
        revoked: credential.revoked_at().is_some(),
        result: "accepted".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
    };
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn credential() -> McpCredential {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("timestamp");
        McpCredential::issue(
            McpCredentialId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now + Duration::days(30),
            now,
        )
        .expect("credential")
    }

    #[test]
    fn creates_closed_secret_free_audit_details() {
        let credential = credential();
        let audit = mcp_credential_audit_record(&credential, None, Uuid::new_v4(), Uuid::new_v4())
            .expect("audit");

        validate_mcp_credential_audit(&audit, &credential, None).expect("valid audit");
        assert_eq!(audit.action, ISSUE_ACTION);
        let rendered = audit.details.to_string().to_ascii_lowercase();
        for forbidden in [
            "prefix",
            "secret",
            "verifier",
            "ciphertext",
            "keyid",
            "delivery",
        ] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn rejects_added_or_mismatched_audit_fields() {
        let credential = credential();
        let mut audit =
            mcp_credential_audit_record(&credential, None, Uuid::new_v4(), Uuid::new_v4())
                .expect("audit");
        audit.details["secret"] = json!("must-not-persist");
        assert!(validate_mcp_credential_audit(&audit, &credential, None).is_err());

        let mut audit =
            mcp_credential_audit_record(&credential, None, Uuid::new_v4(), Uuid::new_v4())
                .expect("audit");
        audit.action = ROTATE_ACTION.into();
        assert!(validate_mcp_credential_audit(&audit, &credential, None).is_err());
    }
}
