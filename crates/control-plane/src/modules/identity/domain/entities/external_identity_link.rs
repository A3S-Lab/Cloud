use crate::modules::identity::domain::entities::{IdentityPrincipal, IdentityPrincipalKind};
use crate::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ExternalIdentityLinkId, PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIdentityLink {
    pub id: ExternalIdentityLinkId,
    pub provider_key: OidcProviderKey,
    pub issuer: OidcIssuer,
    pub subject: ExternalIdentitySubject,
    pub principal_id: PrincipalId,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub last_verified_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl ExternalIdentityLink {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: ExternalIdentityLinkId,
        provider_key: OidcProviderKey,
        issuer: OidcIssuer,
        subject: ExternalIdentitySubject,
        principal: &IdentityPrincipal,
        verified_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if principal.kind != IdentityPrincipalKind::Human || !principal.is_active() {
            return Err("external identity links require one active human principal".into());
        }
        let verified_at = canonical_timestamp(verified_at);
        Ok(Self {
            id,
            provider_key,
            issuer,
            subject,
            principal_id: principal.id,
            aggregate_version: 1,
            created_at: verified_at,
            last_verified_at: verified_at,
            revoked_at: None,
        })
    }

    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    pub fn record_verification(&mut self, verified_at: DateTime<Utc>) -> Result<bool, String> {
        if !self.is_active() {
            return Err("revoked external identity link cannot be verified".into());
        }
        let verified_at = canonical_timestamp(verified_at);
        if verified_at < self.last_verified_at {
            return Err("external identity verification time cannot move backwards".into());
        }
        if verified_at == self.last_verified_at {
            return Ok(false);
        }
        self.last_verified_at = verified_at;
        self.aggregate_version += 1;
        Ok(true)
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> bool {
        if !self.is_active() {
            return false;
        }
        let revoked_at = canonical_timestamp(revoked_at).max(self.last_verified_at);
        self.revoked_at = Some(revoked_at);
        self.aggregate_version += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::ResourceName;
    use chrono::Duration;

    fn human(now: DateTime<Utc>) -> IdentityPrincipal {
        IdentityPrincipal::create_human(
            PrincipalId::new(),
            ResourceName::parse("Human operator").expect("name"),
            now,
        )
    }

    #[test]
    fn external_subject_link_is_human_only_and_revocation_is_terminal() {
        let now = Utc::now();
        let provider = OidcProviderKey::parse("workforce").expect("provider");
        let issuer = OidcIssuer::parse("https://identity.example.com").expect("issuer");
        let subject = ExternalIdentitySubject::parse("subject-42").expect("subject");
        let service = IdentityPrincipal::create_service(
            PrincipalId::new(),
            ResourceName::parse("automation").expect("name"),
            now,
        );
        assert!(ExternalIdentityLink::create(
            ExternalIdentityLinkId::new(),
            provider.clone(),
            issuer.clone(),
            subject.clone(),
            &service,
            now,
        )
        .is_err());

        let mut link = ExternalIdentityLink::create(
            ExternalIdentityLinkId::new(),
            provider,
            issuer,
            subject,
            &human(now),
            now,
        )
        .expect("link");
        assert!(link
            .record_verification(now + Duration::minutes(1))
            .expect("verify"));
        assert!(link.revoke(now + Duration::minutes(2)));
        assert!(!link.revoke(now + Duration::minutes(3)));
        assert!(link
            .record_verification(now + Duration::minutes(3))
            .is_err());
    }
}
