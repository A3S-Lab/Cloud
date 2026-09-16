//! C0.5-C2: Claim path for Identity-gated audit / SIEM enterprise surface.
//!
//! Freezes the non-invented contract: `enterprise.audit-security` reuses proven
//! shared Audit query routes under `/organizations` (list, export, export
//! manifest, retention). Does not invent a SIEM product, investigation UI,
//! or PII-redaction CMS.

use super::audit_query_controller;
use a3s_boot::QueryBus;
use std::sync::Arc;

fn audit_security_paths() -> Vec<String> {
    let query_bus = Arc::new(QueryBus::new());
    let controller = audit_query_controller(query_bus).expect("audit queries");

    assert_eq!(
        controller.prefix(),
        "/organizations",
        "audit-security claim path must stay on shared Audit management traffic"
    );

    controller
        .routes()
        .iter()
        .map(|route| route.path().to_string())
        .collect()
}

#[test]
fn enterprise_audit_security_exposes_export_manifest_list_and_retention_paths() {
    let paths = audit_security_paths();

    for required in [
        "audit-records",
        "audit-records/export",
        "audit-records/export/manifest",
        "audit-records/retention",
    ] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing audit-security claim path fragment `{required}` in {paths:?}"
        );
    }
}

#[test]
fn enterprise_audit_security_keeps_organization_scoped_export_surface() {
    let paths = audit_security_paths();
    assert!(
        paths
            .iter()
            .any(|path| path.contains("audit-records/export")
                && !path.contains("manifest")),
        "audit export surface required; got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| path.contains("audit-records/export/manifest")),
        "tamper-evident export manifest surface required; got {paths:?}"
    );
}

