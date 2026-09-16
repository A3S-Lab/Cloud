//! C0.5-C1: Claim path for Identity-owned OIDC federation enterprise surface.
//!
//! Freezes the non-invented contract: `enterprise.saml-oidc-scim` reuses proven
//! Identity OIDC login/callback and organization link routes. Does not invent
//! SAML IdP federation, SCIM directory provisioning, or a second SSO product.

use super::{oidc_link_controller, oidc_public_controller};
use a3s_boot::CommandBus;
use std::sync::Arc;

fn oidc_federation_paths() -> Vec<(String, String)> {
    let commands = Arc::new(CommandBus::new());
    let controllers = vec![
        oidc_public_controller(Arc::clone(&commands)).expect("oidc public"),
        oidc_link_controller(Arc::clone(&commands)).expect("oidc link"),
    ];

    controllers
        .iter()
        .flat_map(|controller| {
            let prefix = controller.prefix().to_string();
            controller
                .routes()
                .iter()
                .map(move |route| (prefix.clone(), route.path().to_string()))
        })
        .collect()
}

#[test]
fn enterprise_saml_oidc_scim_exposes_oidc_login_callback_and_link_paths() {
    let paths = oidc_federation_paths();
    let joined: Vec<String> = paths
        .iter()
        .map(|(prefix, path)| format!("{prefix}{path}"))
        .collect();

    for required in [
        "/identity/oidc/{provider_key}/login",
        "/identity/oidc/{provider_key}/callback",
        "/organizations/{organization_id}/identity/oidc/{provider_key}/link",
    ] {
        assert!(
            joined.iter().any(|path| path.contains(required)),
            "missing OIDC federation claim path `{required}` in {joined:?}"
        );
    }
}

#[test]
fn enterprise_saml_oidc_scim_keeps_public_login_and_tenant_link_owners() {
    let paths = oidc_federation_paths();
    assert!(
        paths
            .iter()
            .any(|(prefix, path)| prefix == "/identity/oidc" && path.contains("login")),
        "OIDC public login owner required; got {paths:?}"
    );
    assert!(
        paths.iter().any(|(prefix, path)| prefix == "/organizations"
            && path.contains("identity/oidc")
            && path.contains("link")),
        "OIDC tenant link owner required; got {paths:?}"
    );
}

