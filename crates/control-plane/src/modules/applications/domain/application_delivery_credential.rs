//! Applications-owned delivery credential binding for anonymous admission.
//!
//! `APP0.2-C16` freezes the credential projection Applications needs before
//! Identity issues material (`APP0.3`) or public routes exist. Plaintext never
//! enters this aggregate; Secrets owns the exact version reference.

use super::{ApplicationAudience, ApplicationEndUser, ApplicationRelease};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationDeliveryCredentialId, ApplicationEndUserId, ApplicationId,
    OrganizationId, PrincipalId, ProjectId, SecretVersionReference,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const LOOKUP_KEY_MAX_CHARS: usize = 64;
const MAX_SAFE_GENERATION: u64 = 9_007_199_254_740_991;

/// Lifecycle of one Applications-owned delivery credential binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationDeliveryCredentialStatus {
    Active,
    Disabled,
    Revoked,
}

/// One Application-scoped delivery credential binding.
///
/// The opaque `lookup_key` is path-safe public identity within the Application
/// scope. The signing/material secret remains an exact Secrets version
/// reference. Generation fences disable/enable/revoke without rewriting
/// identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub id: ApplicationDeliveryCredentialId,
    pub audience: ApplicationAudience,
    pub lookup_key: String,
    pub secret: SecretVersionReference,
    pub generation: u64,
    pub status: ApplicationDeliveryCredentialStatus,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl ApplicationDeliveryCredential {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: ApplicationDeliveryCredentialId,
        release: &ApplicationRelease,
        lookup_key: impl Into<String>,
        secret: SecretVersionReference,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        release.validate()?;
        if release.contract.spec().audience != ApplicationAudience::Anonymous {
            return Err(
                "Application delivery credentials require an anonymous-audience release".into(),
            );
        }
        let created_at = canonical_timestamp(created_at);
        Self::restore(
            release.organization_id,
            release.project_id,
            release.application_id,
            id,
            ApplicationAudience::Anonymous,
            lookup_key,
            secret,
            1,
            ApplicationDeliveryCredentialStatus::Active,
            created_by,
            created_at,
            created_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        id: ApplicationDeliveryCredentialId,
        audience: ApplicationAudience,
        lookup_key: impl Into<String>,
        secret: SecretVersionReference,
        generation: u64,
        status: ApplicationDeliveryCredentialStatus,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            application_id,
            id,
            audience,
            lookup_key: lookup_key.into(),
            secret,
            generation,
            status,
            created_by,
            created_at: canonical_timestamp(created_at),
            updated_at: canonical_timestamp(updated_at),
            revoked_at: revoked_at.map(canonical_timestamp),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate_lookup_key(value: &str) -> Result<(), String> {
        if value.is_empty()
            || value.len() > LOOKUP_KEY_MAX_CHARS
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || value.starts_with('-')
            || value.ends_with('-')
            || value.contains("--")
        {
            return Err("Application delivery credential lookup key is invalid".into());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.generation == 0
            || self.generation > MAX_SAFE_GENERATION
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
        {
            return Err("stored Application delivery credential identity is invalid".into());
        }
        Self::validate_lookup_key(&self.lookup_key)?;
        self.secret.validate()?;
        if self.audience != ApplicationAudience::Anonymous {
            return Err(
                "APP0.2-C16 Application delivery credentials admit only anonymous audience".into(),
            );
        }
        match (self.status, self.revoked_at) {
            (ApplicationDeliveryCredentialStatus::Revoked, None) => {
                Err("revoked Application delivery credential requires revoked_at".into())
            }
            (ApplicationDeliveryCredentialStatus::Revoked, Some(revoked_at)) => {
                if revoked_at != canonical_timestamp(revoked_at) || revoked_at < self.updated_at {
                    return Err("Application delivery credential revoked_at is invalid".into());
                }
                Ok(())
            }
            (_, Some(_)) => {
                Err("non-revoked Application delivery credential cannot carry revoked_at".into())
            }
            _ => Ok(()),
        }
    }

    pub fn validate_release(&self, release: &ApplicationRelease) -> Result<(), String> {
        self.validate()?;
        release.validate()?;
        if self.organization_id != release.organization_id
            || self.project_id != release.project_id
            || self.application_id != release.application_id
            || release.contract.spec().audience != ApplicationAudience::Anonymous
        {
            return Err(
                "Application delivery credential is outside the exact anonymous release".into(),
            );
        }
        Ok(())
    }

    /// Stable anonymous end-user identity derived from the credential only.
    pub fn anonymous_end_user_id(&self) -> Result<ApplicationEndUserId, String> {
        self.validate()?;
        ApplicationEndUser::anonymous_credential_id(self.application_id, self.id)
    }

    /// Admit one Principal-free anonymous ApplicationEndUser for an active
    /// credential against an exact anonymous release.
    pub fn admit_anonymous_end_user(
        &self,
        release: &ApplicationRelease,
        created_at: DateTime<Utc>,
    ) -> Result<ApplicationEndUser, String> {
        self.validate_release(release)?;
        if self.status != ApplicationDeliveryCredentialStatus::Active {
            return Err("inactive Application delivery credential cannot admit end users".into());
        }
        ApplicationEndUser::anonymous_credential(
            self.anonymous_end_user_id()?,
            release,
            self.created_by,
            created_at,
        )
    }

    pub fn disable(
        &mut self,
        expected_generation: u64,
        updated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.transition(
            expected_generation,
            ApplicationDeliveryCredentialStatus::Disabled,
            updated_at,
            None,
        )
    }

    pub fn enable(
        &mut self,
        expected_generation: u64,
        updated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status == ApplicationDeliveryCredentialStatus::Revoked {
            return Err("revoked Application delivery credential cannot be enabled".into());
        }
        self.transition(
            expected_generation,
            ApplicationDeliveryCredentialStatus::Active,
            updated_at,
            None,
        )
    }

    pub fn revoke(
        &mut self,
        expected_generation: u64,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.transition(
            expected_generation,
            ApplicationDeliveryCredentialStatus::Revoked,
            revoked_at,
            Some(revoked_at),
        )
    }

    fn transition(
        &mut self,
        expected_generation: u64,
        status: ApplicationDeliveryCredentialStatus,
        updated_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<(), String> {
        self.validate()?;
        if expected_generation != self.generation {
            return Err("Application delivery credential generation is stale".into());
        }
        if self.status == ApplicationDeliveryCredentialStatus::Revoked {
            return Err("revoked Application delivery credential is immutable".into());
        }
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("Application delivery credential update timestamp is invalid".into());
        }
        let generation = self
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= MAX_SAFE_GENERATION)
            .ok_or_else(|| "Application delivery credential generation exhausted".to_string())?;
        self.generation = generation;
        self.status = status;
        self.updated_at = updated_at;
        self.revoked_at = revoked_at.map(canonical_timestamp);
        self.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationDeliveryPolicy, ApplicationExperience, ApplicationReleaseContract,
        ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationWorkflowBinding,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationReleaseId, SecretId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::TimeZone;
    use uuid::Uuid;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn anonymous_release() -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatbot,
            audience: ApplicationAudience::Anonymous,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationExperience::Chatbot.interaction_mode(),
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::from_uuid(
                    Uuid::parse_str("018f0000-0000-7000-8000-000000000101").expect("UUID"),
                ),
                workflow_revision_id: WorkflowRevisionId::from_uuid(
                    Uuid::parse_str("018f0000-0000-7000-8000-000000000102").expect("UUID"),
                ),
                workflow_contract_digest: digest('a'),
                workflow_payload_set_digest: digest('b'),
                workflow_semantic_contract_set_digest: digest('c'),
                input_schema_digest: digest('d'),
                output_schema_digest: digest('e'),
            },
            presentation_digest: digest('f'),
        })
        .expect("contract");
        ApplicationRelease::initial(
            OrganizationId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000001").expect("UUID"),
            ),
            ProjectId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000002").expect("UUID"),
            ),
            ApplicationId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000003").expect("UUID"),
            ),
            ApplicationReleaseId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000004").expect("UUID"),
            ),
            contract,
            PrincipalId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000005").expect("UUID"),
            ),
            Utc.with_ymd_and_hms(2026, 9, 14, 8, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    fn secret() -> SecretVersionReference {
        SecretVersionReference::new(
            SecretId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000010").expect("UUID"),
            ),
            1,
        )
        .expect("secret")
    }

    fn issued() -> ApplicationDeliveryCredential {
        let release = anonymous_release();
        ApplicationDeliveryCredential::issue(
            ApplicationDeliveryCredentialId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000020").expect("UUID"),
            ),
            &release,
            "public-embed-key",
            secret(),
            release.created_by,
            Utc.with_ymd_and_hms(2026, 9, 14, 9, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("credential")
    }

    #[test]
    fn issue_admits_stable_anonymous_end_user_without_principal() {
        let release = anonymous_release();
        let credential = issued();
        let end_user = credential
            .admit_anonymous_end_user(
                &release,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 30, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("admit");
        assert_eq!(end_user.audience, ApplicationAudience::Anonymous);
        assert_eq!(end_user.linked_principal_id, None);
        assert_eq!(
            end_user.id,
            credential.anonymous_end_user_id().expect("end user id")
        );
        let again = credential
            .admit_anonymous_end_user(
                &release,
                Utc.with_ymd_and_hms(2026, 9, 14, 10, 0, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("readmit");
        assert_eq!(again.id, end_user.id);
    }

    #[test]
    fn generation_fences_disable_enable_and_revoke() {
        let mut credential = issued();
        credential
            .disable(
                1,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 5, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("disable");
        assert_eq!(credential.generation, 2);
        assert_eq!(
            credential.status,
            ApplicationDeliveryCredentialStatus::Disabled
        );
        assert!(credential
            .admit_anonymous_end_user(
                &anonymous_release(),
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 6, 0)
                    .single()
                    .expect("timestamp"),
            )
            .is_err());
        assert!(credential
            .enable(
                1,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 7, 0)
                    .single()
                    .expect("timestamp"),
            )
            .is_err());
        credential
            .enable(
                2,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 7, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("enable");
        credential
            .revoke(
                3,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 8, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("revoke");
        assert_eq!(
            credential.status,
            ApplicationDeliveryCredentialStatus::Revoked
        );
        assert!(credential
            .enable(
                4,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 9, 0)
                    .single()
                    .expect("timestamp"),
            )
            .is_err());
    }

    #[test]
    fn lookup_key_and_project_member_release_fail_closed() {
        assert!(ApplicationDeliveryCredential::validate_lookup_key("").is_err());
        assert!(ApplicationDeliveryCredential::validate_lookup_key("-bad").is_err());
        assert!(ApplicationDeliveryCredential::validate_lookup_key("bad--key").is_err());
        let mut release = anonymous_release();
        // Force project-member audience through restore path by issuing against
        // anonymous then checking validate_release with a mismatched release.
        let credential = issued();
        let member_contract =
            ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
                experience: ApplicationExperience::Chatbot,
                audience: ApplicationAudience::ProjectMembers,
                delivery: ApplicationDeliveryPolicy {
                    interaction_mode: ApplicationExperience::Chatbot.interaction_mode(),
                    response_modes: vec![ApplicationResponseMode::Blocking],
                },
                workflow: release.contract.spec().workflow.clone(),
                presentation_digest: digest('9'),
            })
            .expect("member contract");
        release = ApplicationRelease::initial(
            release.organization_id,
            release.project_id,
            release.application_id,
            ApplicationReleaseId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000044").expect("UUID"),
            ),
            member_contract,
            release.created_by,
            release.created_at,
        )
        .expect("member release");
        assert!(credential.validate_release(&release).is_err());
    }
}
