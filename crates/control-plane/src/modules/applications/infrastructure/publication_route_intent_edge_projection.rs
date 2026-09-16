//! Pure ACA mapper from Applications publication route intent to Edge desired state.
//!
//! `APP0.3-C15` projects `ApplicationPublicationRouteIntent` into
//! [`ApplicationPublicationRouteIntentEdgeProjection`] without calling
//! `PublishRouteHandler`, compiling a Gateway snapshot, or installing nodes.
//! Rate shaping remains declare-only.

use crate::modules::applications::application::ApplicationPublicationRouteIntentEdgeProjection;
use crate::modules::applications::domain::ApplicationPublicationRouteIntent;

/// Project one validated publication route intent into Edge-facing desired state.
///
/// The mapper is the only Applications infrastructure surface that defines how
/// intent fields become the Edge projection vocabulary. It does not import Edge
/// command or aggregate types.
pub fn project_application_publication_route_intent_edge(
    intent: &ApplicationPublicationRouteIntent,
) -> Result<ApplicationPublicationRouteIntentEdgeProjection, String> {
    intent.validate()?;
    let projection = ApplicationPublicationRouteIntentEdgeProjection {
        organization_id: intent.organization_id,
        project_id: intent.project_id,
        application_id: intent.application_id,
        application_release_id: intent.application_release_id,
        application_release_digest: intent.application_release_digest.clone(),
        publication_route_intent_id: intent.id,
        channels: intent
            .channels
            .iter()
            .map(|channel| channel.as_str().to_owned())
            .collect(),
        embed_origin_allowlist: intent.embed_origin_allowlist.clone(),
        rate_shaping_profile_id: intent.rate_shaping_policy.profile_id.clone(),
        rate_shaping_policy_revision_digest: intent
            .rate_shaping_policy
            .policy_revision_digest
            .clone(),
    };
    projection.validate()?;
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
        ApplicationInteractionMode, ApplicationPublicationChannel,
        ApplicationPublicationRateShapingPolicyRef, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationWorkflowBinding,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
        WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::{TimeZone, Utc};

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn rate_policy() -> ApplicationPublicationRateShapingPolicyRef {
        ApplicationPublicationRateShapingPolicyRef::create("public-api-default", digest('a'))
            .expect("rate policy")
    }

    fn release() -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatflow,
            audience: ApplicationAudience::ProjectMembers,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationInteractionMode::Conversation,
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::new(),
                workflow_revision_id: WorkflowRevisionId::new(),
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
            OrganizationId::new(),
            ProjectId::new(),
            ApplicationId::new(),
            ApplicationReleaseId::new(),
            contract,
            PrincipalId::new(),
            Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    #[test]
    fn projects_channels_origins_and_rate_policy_without_edge_types() {
        let release = release();
        let intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![
                ApplicationPublicationChannel::Web,
                ApplicationPublicationChannel::ApiBlocking,
                ApplicationPublicationChannel::Embed,
            ],
            vec![
                "https://docs.example.com".into(),
                "https://app.example.com/".into(),
            ],
            rate_policy(),
        )
        .expect("create");

        let projection =
            project_application_publication_route_intent_edge(&intent).expect("project");

        assert_eq!(projection.organization_id, intent.organization_id);
        assert_eq!(projection.project_id, intent.project_id);
        assert_eq!(projection.application_id, intent.application_id);
        assert_eq!(
            projection.application_release_id,
            intent.application_release_id
        );
        assert_eq!(
            projection.application_release_digest,
            intent.application_release_digest
        );
        assert_eq!(projection.publication_route_intent_id, intent.id);
        assert_eq!(
            projection.channels,
            vec![
                "api_blocking".to_owned(),
                "embed".to_owned(),
                "web".to_owned(),
            ]
        );
        assert_eq!(
            projection.embed_origin_allowlist,
            intent.embed_origin_allowlist
        );
        assert_eq!(
            projection.rate_shaping_profile_id,
            intent.rate_shaping_policy.profile_id
        );
        assert_eq!(
            projection.rate_shaping_policy_revision_digest,
            intent.rate_shaping_policy.policy_revision_digest
        );
        projection.validate().expect("projection validate");
    }

    #[test]
    fn rejects_drifted_intent_before_projection() {
        let release = release();
        let mut intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![ApplicationPublicationChannel::Internal],
            Vec::new(),
            rate_policy(),
        )
        .expect("create");
        intent.rate_shaping_policy.profile_id = "mutated".into();

        let error = project_application_publication_route_intent_edge(&intent)
            .expect_err("drifted intent");
        assert!(
            error.contains("identity drifted") || error.contains("not canonical"),
            "unexpected error: {error}"
        );
    }
}
