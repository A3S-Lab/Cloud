use a3s_cloud_contracts::{
    NodePluginHostAuthorizeTrustRequest, NodePluginHostTrustAuthorized,
};
use a3s_use_core::{
    PluginHostApplyRequest, PluginHostApplyResult, PluginHostCapabilities,
    PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult, PluginHostManager,
    PluginHostObservationRequest, PluginHostObservationResult, PluginHostPlanRequest,
    PluginHostPlanResult, UseResult,
};
use chrono::Utc;
use sha2::{Digest, Sha256};

pub(crate) async fn inspect(manager: &dyn PluginHostManager) -> UseResult<PluginHostCapabilities> {
    let capabilities = manager.capabilities().await?;
    capabilities.validate()?;
    Ok(capabilities)
}

pub(crate) async fn plan(
    manager: &dyn PluginHostManager,
    request: &PluginHostPlanRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostPlanResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.plan(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn apply(
    manager: &dyn PluginHostManager,
    request: &PluginHostApplyRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostApplyResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.apply(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn plan_enablement(
    manager: &dyn PluginHostManager,
    request: &PluginHostEnablementPlanRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostEnablementPlanResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.plan_enablement(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn observe(
    manager: &dyn PluginHostManager,
    request: &PluginHostObservationRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostObservationResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.observe(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

/// Verify downloaded trust-root and policy ACL Artifacts, then admit them to Use.
///
/// Digests in the request are the only authority recorded in the acknowledgement.
/// The Agent:
/// 1. Recomputes content digests of the downloaded bytes
/// 2. Parses the policy only through the canonical A3S ACL path
/// 3. Gives the root bytes to A3S Use via `inspect_bootstrap_root` as TUF
///    bootstrap evidence (no second registry-sync or policy evaluator)
pub(crate) fn authorize_trust(
    request: &NodePluginHostAuthorizeTrustRequest,
    trust_root_bytes: &[u8],
    policy_acl_bytes: &[u8],
) -> Result<NodePluginHostTrustAuthorized, String> {
    request.validate()?;
    let trust_digest = content_digest(trust_root_bytes);
    if trust_digest != request.trust_root.digest {
        return Err(
            "Plugin Host trust-root Artifact digest does not match downloaded bytes".into(),
        );
    }
    let policy_digest = content_digest(policy_acl_bytes);
    if policy_digest != request.policy_acl.digest {
        return Err(
            "Plugin Host policy ACL Artifact digest does not match downloaded bytes".into(),
        );
    }

    let policy_text = std::str::from_utf8(policy_acl_bytes)
        .map_err(|error| format!("Plugin Host policy ACL is not UTF-8: {error}"))?;
    a3s_acl::parse_acl(policy_text).map_err(|error| {
        format!("Plugin Host policy ACL failed canonical A3S ACL parse: {error}")
    })?;

    let pinned = a3s_use_extension::inspect_bootstrap_root(trust_root_bytes).map_err(|error| {
        format!(
            "Plugin Host trust-root failed A3S Use bootstrap inspection ({}): {}",
            error.code, error.message
        )
    })?;
    let inspected_digest = format!("sha256:{}", pinned.root_sha256);
    if inspected_digest != request.trust_root.digest {
        return Err(
            "Plugin Host trust-root Use bootstrap identity drifted from Artifact digest".into(),
        );
    }

    NodePluginHostTrustAuthorized::new(
        request.generation,
        request.trust_root.digest.clone(),
        request.policy_acl.digest.clone(),
        Utc::now(),
    )
}

fn content_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_BOOTSTRAP_ROOT: &[u8] =
        include_bytes!("../../control-plane/fixtures/plugins/bootstrap-root.json");
    const POLICY_ACL: &[u8] = b"default = \"deny\"\nallow.plugin.selftest = true\n";

    #[test]
    fn authorize_trust_admits_use_bootstrap_and_parses_policy_acl() {
        let trust_digest = content_digest(VALID_BOOTSTRAP_ROOT);
        let policy_digest = content_digest(POLICY_ACL);
        let request = NodePluginHostAuthorizeTrustRequest::from_digests(
            7,
            trust_digest.clone(),
            policy_digest.clone(),
        )
        .expect("request");
        let authorized =
            authorize_trust(&request, VALID_BOOTSTRAP_ROOT, POLICY_ACL).expect("authorized");
        authorized.validate_for(&request).expect("match request");
        assert_eq!(authorized.trust_root_digest, trust_digest);
        assert_eq!(authorized.policy_digest, policy_digest);
    }

    #[test]
    fn authorize_trust_rejects_digest_drift_and_invalid_policy() {
        let request = NodePluginHostAuthorizeTrustRequest::from_digests(
            7,
            content_digest(VALID_BOOTSTRAP_ROOT),
            content_digest(POLICY_ACL),
        )
        .expect("request");
        assert!(authorize_trust(&request, b"not-the-root", POLICY_ACL).is_err());
        assert!(authorize_trust(&request, VALID_BOOTSTRAP_ROOT, b"not = acl {{{").is_err());
    }
}
