//! Map C15 Edge projections into contracts Published Language ACL projections.

use crate::modules::applications::application::ApplicationPublicationRouteIntentEdgeProjection;
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;

/// Convert one validated C15 Edge projection into a contract ACL projection.
pub fn project_application_publication_route_intent_acl(
    projection: &ApplicationPublicationRouteIntentEdgeProjection,
) -> Result<ApplicationPublicationRouteIntentAclProjection, String> {
    projection.validate()?;
    let acl = ApplicationPublicationRouteIntentAclProjection {
        organization_id: projection.organization_id.as_uuid(),
        project_id: projection.project_id.as_uuid(),
        application_id: projection.application_id.as_uuid(),
        application_release_id: projection.application_release_id.as_uuid(),
        application_release_digest: projection.application_release_digest.as_str().to_owned(),
        publication_route_intent_id: projection.publication_route_intent_id.as_uuid(),
        channels: projection.channels.clone(),
        embed_origin_allowlist: projection.embed_origin_allowlist.clone(),
        rate_shaping_profile_id: projection.rate_shaping_profile_id.clone(),
        rate_shaping_policy_revision_digest: projection
            .rate_shaping_policy_revision_digest
            .as_str()
            .to_owned(),
    };
    acl.validate()?;
    Ok(acl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, OrganizationId,
        ProjectId, Sha256Digest,
    };

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn edge_projection() -> ApplicationPublicationRouteIntentEdgeProjection {
        ApplicationPublicationRouteIntentEdgeProjection {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            application_id: ApplicationId::new(),
            application_release_id: ApplicationReleaseId::new(),
            application_release_digest: digest('a'),
            publication_route_intent_id: ApplicationPublicationRouteIntentId::new(),
            channels: vec!["api_blocking".into(), "embed".into()],
            embed_origin_allowlist: vec!["https://app.example.com".into()],
            rate_shaping_profile_id: "public-api-default".into(),
            rate_shaping_policy_revision_digest: digest('b'),
        }
    }

    #[test]
    fn maps_c15_edge_projection_into_contract_acl_projection() {
        let edge = edge_projection();
        let acl = project_application_publication_route_intent_acl(&edge).expect("map");
        assert_eq!(acl.organization_id, edge.organization_id.as_uuid());
        assert_eq!(acl.channels, edge.channels);
        assert_eq!(acl.rate_shaping_profile_id, edge.rate_shaping_profile_id);
        assert_eq!(
            acl.rate_shaping_policy_revision_digest,
            edge.rate_shaping_policy_revision_digest.as_str()
        );
        assert!(a3s_cloud_contracts::render_application_publication_route_intent_acl_blocks(
            &[acl]
        )
        .is_ok());
    }
}
