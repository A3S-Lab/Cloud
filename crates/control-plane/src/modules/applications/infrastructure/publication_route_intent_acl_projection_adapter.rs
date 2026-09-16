//! Repository-backed Applications ACL projection port for Gateway snapshot compile.
//!
//! `APP0.3-C17` loads persisted publication route intents, maps them through the
//! C15 Edge projection, then into C16 contract ACL projections. Edge still
//! consumes this port only through one ACA adapter.

use crate::modules::applications::application::{
    ApplicationPublicationRouteIntentAclScope, IApplicationPublicationRouteIntentAclProjectionPort,
};
use crate::modules::applications::domain::IApplicationPublicationRouteIntentRepository;
use crate::modules::applications::infrastructure::{
    project_application_publication_route_intent_acl,
    project_application_publication_route_intent_edge,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Loads org/project-scoped publication route intent ACL projections from storage.
#[derive(Clone)]
pub struct ApplicationPublicationRouteIntentAclProjectionAdapter {
    intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
}

impl ApplicationPublicationRouteIntentAclProjectionAdapter {
    pub fn new(intents: Arc<dyn IApplicationPublicationRouteIntentRepository>) -> Self {
        Self { intents }
    }
}

#[async_trait]
impl IApplicationPublicationRouteIntentAclProjectionPort
    for ApplicationPublicationRouteIntentAclProjectionAdapter
{
    async fn list_publication_route_intent_acl_projections(
        &self,
        scopes: &[ApplicationPublicationRouteIntentAclScope],
    ) -> Result<Vec<ApplicationPublicationRouteIntentAclProjection>, RepositoryError> {
        let unique = scopes.iter().copied().collect::<BTreeSet<_>>();
        let mut projections = Vec::new();
        for scope in unique {
            let intents = self
                .intents
                .list_intents_by_project(scope.organization_id(), scope.project_id())
                .await?;
            for intent in intents {
                let edge = project_application_publication_route_intent_edge(&intent)
                    .map_err(RepositoryError::Storage)?;
                let acl = project_application_publication_route_intent_acl(&edge)
                    .map_err(RepositoryError::Storage)?;
                projections.push(acl);
            }
        }
        projections.sort_by_key(|projection| projection.publication_route_intent_id);
        Ok(projections)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
        ApplicationInteractionMode, ApplicationPublicationChannel,
        ApplicationPublicationRateShapingPolicyRef, ApplicationPublicationRouteIntent,
        ApplicationRelease, ApplicationReleaseContract, ApplicationReleaseContractSpec,
        ApplicationResponseMode, ApplicationWorkflowBinding,
    };
    use crate::modules::applications::infrastructure::InMemoryApplicationPublicationRouteIntentRepository;
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

    fn release(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> ApplicationRelease {
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
            organization_id,
            project_id,
            application_id,
            ApplicationReleaseId::new(),
            contract,
            PrincipalId::new(),
            Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    #[tokio::test]
    async fn empty_repository_returns_no_acl_projections() {
        let adapter = ApplicationPublicationRouteIntentAclProjectionAdapter::new(Arc::new(
            InMemoryApplicationPublicationRouteIntentRepository::new(),
        ));
        let scope = ApplicationPublicationRouteIntentAclScope::new(
            OrganizationId::new(),
            ProjectId::new(),
        )
        .expect("scope");
        let projections = adapter
            .list_publication_route_intent_acl_projections(&[scope])
            .await
            .expect("list");
        assert!(projections.is_empty());
    }

    #[tokio::test]
    async fn maps_persisted_intent_through_c15_into_c16_acl_projection() {
        let intents = Arc::new(InMemoryApplicationPublicationRouteIntentRepository::new());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let application_id = ApplicationId::new();
        let release = release(organization_id, project_id, application_id);
        let intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![
                ApplicationPublicationChannel::ApiBlocking,
                ApplicationPublicationChannel::Embed,
            ],
            vec!["https://app.example.com".into()],
            rate_policy(),
        )
        .expect("create");
        intents
            .create_intent(intent.clone())
            .await
            .expect("persist");

        let adapter = ApplicationPublicationRouteIntentAclProjectionAdapter::new(intents);
        let scope =
            ApplicationPublicationRouteIntentAclScope::new(organization_id, project_id).expect("scope");
        let projections = adapter
            .list_publication_route_intent_acl_projections(&[scope, scope])
            .await
            .expect("list");
        assert_eq!(projections.len(), 1);
        assert_eq!(
            projections[0].publication_route_intent_id,
            intent.id.as_uuid()
        );
        assert_eq!(
            projections[0].channels,
            vec!["api_blocking".to_owned(), "embed".to_owned()]
        );
        let rendered =
            a3s_cloud_contracts::render_application_publication_route_intent_acl_blocks(&projections)
                .expect("render");
        assert!(rendered.contains("application_publication_route_intents {"));
        assert!(rendered.contains("api_blocking"));
    }
}