use crate::modules::identity::application::RESOURCE_GRANT_SCOPES_CLAIM;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
use a3s_boot::{AuthPrincipal, BootError, Result};

pub fn resource_access_evaluator(principal: &AuthPrincipal) -> Result<ResourceAccessEvaluator> {
    if principal.has_role("platform_admin") {
        return Ok(ResourceAccessEvaluator::organization_wide());
    }
    let role = principal
        .claim("organization_role")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            BootError::Forbidden(
                "authenticated principal has no active organization membership".into(),
            )
        })
        .and_then(|role| {
            MembershipRole::parse(role).map_err(|error| {
                BootError::Internal(format!(
                    "authenticated organization role claim is invalid: {error}"
                ))
            })
        })?;
    if role != MembershipRole::Restricted {
        return Ok(ResourceAccessEvaluator::organization_wide());
    }
    let scopes = principal
        .claim(RESOURCE_GRANT_SCOPES_CLAIM)
        .cloned()
        .map(serde_json::from_value::<Vec<ResourceGrantScope>>)
        .transpose()
        .map_err(|error| {
            BootError::Internal(format!(
                "authenticated Resource Grant claim is invalid: {error}"
            ))
        })?
        .unwrap_or_default();
    Ok(ResourceAccessEvaluator::for_membership(role, scopes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{NodeId, ProjectId};

    #[test]
    fn restricted_principal_uses_server_issued_grant_claim() {
        let project_id = ProjectId::new();
        let principal = AuthPrincipal::new("principal")
            .with_claim("organization_role", "restricted")
            .expect("role")
            .with_claim(
                RESOURCE_GRANT_SCOPES_CLAIM,
                [ResourceGrantScope::Project { project_id }],
            )
            .expect("grants");
        let evaluator = resource_access_evaluator(&principal).expect("evaluator");
        assert!(evaluator.allows(ResourceGrantScope::Project { project_id }));
        assert!(!evaluator.allows(ResourceGrantScope::Node {
            node_id: NodeId::new(),
        }));
    }

    #[test]
    fn missing_restricted_claim_fails_closed() {
        let principal = AuthPrincipal::new("principal")
            .with_claim("organization_role", "restricted")
            .expect("role");
        let evaluator = resource_access_evaluator(&principal).expect("evaluator");
        assert!(!evaluator.has_any_visible_resource());
    }

    #[test]
    fn platform_administrator_does_not_require_membership_claims() {
        let evaluator =
            resource_access_evaluator(&AuthPrincipal::new("principal").with_role("platform_admin"))
                .expect("evaluator");
        assert!(evaluator.is_organization_wide());
    }
}
