//! H0.5-C1: Claim path for enterprise HA / disaster-recovery surface.
//!
//! Freezes the non-invented contract: `enterprise.ha-disaster-recovery`
//! reuses proven Data object-namespace recovery Flow v2 seal / restore /
//! delete workflows. Does not invent a second HA control plane, ops console,
//! SIEM-backed failover product, or BYOK residency CMS.

use super::infrastructure::object_namespace_recovery_flow_workflow_identities;
use super::{
    OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME, OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
    OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME, OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
};

fn object_namespace_recovery_claim_identities() -> Vec<(String, String)> {
    object_namespace_recovery_flow_workflow_identities()
        .map(|(name, version)| (name.to_string(), version.to_string()))
        .collect()
}

#[test]
fn enterprise_ha_disaster_recovery_exposes_object_namespace_recovery_v2_claim_paths() {
    let identities = object_namespace_recovery_claim_identities();

    for required_name in [
        OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
        OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
        OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
    ] {
        assert!(
            identities.iter().any(|(name, version)| {
                name == required_name && version == OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION
            }),
            "missing HA-DR claim path `{required_name}@{OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION}` in {identities:?}"
        );
    }
}

#[test]
fn enterprise_ha_disaster_recovery_keeps_seal_restore_delete_on_one_recovery_version() {
    let identities = object_namespace_recovery_claim_identities();
    let v2: Vec<_> = identities
        .iter()
        .filter(|(_, version)| version == OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION)
        .collect();

    assert!(
        v2.len() >= 3,
        "object-namespace recovery v2 must expose seal/restore/delete; got {v2:?}"
    );
    for required in [
        OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
        OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
        OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
    ] {
        assert!(
            v2.iter().any(|(name, _)| name.as_str() == required),
            "v2 recovery missing `{required}` in {v2:?}"
        );
    }
}
