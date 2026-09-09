use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct InferenceCredentials => "inference_credentials" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        prefix: String => "prefix",
        verifier_hash: String => "verifier_hash",
        generation: u64 => "generation",
        aggregate_version: u64 => "aggregate_version",
        expires_at: DateTime<Utc> => "expires_at",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        revoked_at: Option<DateTime<Utc>> => "revoked_at",
    }
}

orm_table! {
    pub(super) struct InferenceCredentialDeliveryReceipts => "inference_credential_delivery_receipts" {
        credential_id: Uuid => "credential_id",
        organization_id: Uuid => "organization_id",
        generation: u64 => "generation",
        key_id: String => "key_id",
        ciphertext: String => "ciphertext",
        expires_at: DateTime<Utc> => "expires_at",
        created_at: DateTime<Utc> => "created_at",
    }
}
