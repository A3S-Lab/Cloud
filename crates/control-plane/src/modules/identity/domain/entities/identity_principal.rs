use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, ResourceName};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityPrincipalKind {
    Human,
    Service,
}

impl IdentityPrincipalKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "human" => Ok(Self::Human),
            "service" => Ok(Self::Service),
            _ => Err("identity principal kind must be human or service".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Service => "service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityPrincipal {
    pub id: PrincipalId,
    pub kind: IdentityPrincipalKind,
    pub name: ResourceName,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
}

impl IdentityPrincipal {
    pub fn create(
        id: PrincipalId,
        kind: IdentityPrincipalKind,
        name: ResourceName,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            kind,
            name,
            aggregate_version: 1,
            created_at: canonical_timestamp(created_at),
            disabled_at: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.disabled_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_principal_is_stable_and_active() {
        let created_at = Utc::now();
        let principal = IdentityPrincipal::create(
            PrincipalId::new(),
            IdentityPrincipalKind::Service,
            ResourceName::parse("release automation").expect("name"),
            created_at,
        );
        assert_eq!(principal.kind, IdentityPrincipalKind::Service);
        assert_eq!(principal.aggregate_version, 1);
        assert!(principal.is_active());
    }

    #[test]
    fn human_principal_uses_the_same_stable_identity_authority() {
        let principal = IdentityPrincipal::create(
            PrincipalId::new(),
            IdentityPrincipalKind::Human,
            ResourceName::parse("Human operator").expect("name"),
            Utc::now(),
        );
        assert_eq!(principal.kind, IdentityPrincipalKind::Human);
        assert!(principal.is_active());
    }
}
