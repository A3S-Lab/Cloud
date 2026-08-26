use crate::modules::artifacts::application::IExternalSourceBuildOutcomeQueryPort;
use crate::modules::artifacts::published::ExternalSourceBuildOutcome;
use crate::modules::developer_workflows::application::{
    IWorkloadBuildOutcomePort, VerifiedOciArtifact, VerifiedWorkloadBuildOutcome,
    WORKLOAD_BUILD_OUTCOME_SCHEMA,
};
use crate::modules::developer_workflows::domain::{AcceptedBuildPlan, IBuildPlanRepository};
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

/// Developer Workflows anti-corruption adapter for an Artifacts-owned,
/// successful external-source BuildRun fact. The adapter enriches that fact
/// only with the exact accepted BuildPlan from this bounded context.
pub struct ArtifactsWorkloadBuildOutcomeAdapter {
    outcomes: Arc<dyn IExternalSourceBuildOutcomeQueryPort>,
    build_plans: Arc<dyn IBuildPlanRepository>,
}

impl ArtifactsWorkloadBuildOutcomeAdapter {
    pub fn new(
        outcomes: Arc<dyn IExternalSourceBuildOutcomeQueryPort>,
        build_plans: Arc<dyn IBuildPlanRepository>,
    ) -> Self {
        Self {
            outcomes,
            build_plans,
        }
    }
}

#[async_trait]
impl IWorkloadBuildOutcomePort for ArtifactsWorkloadBuildOutcomeAdapter {
    async fn verified_outcome(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<VerifiedWorkloadBuildOutcome>, RepositoryError> {
        let Some(owner_outcome) = self
            .outcomes
            .find_external_source_build_outcome(organization_id, build_run_id)
            .await?
        else {
            return Ok(None);
        };
        owner_outcome.validate().map_err(projection_error)?;
        if owner_outcome.organization_id() != organization_id
            || owner_outcome.build_run_id() != build_run_id
        {
            return Err(projection_error(
                "Artifacts build outcome changed the requested organization or BuildRun identity"
                    .into(),
            ));
        }

        let build_plan_id = AcceptedBuildPlan::id_for(
            owner_outcome.organization_id(),
            owner_outcome.source_revision_id(),
            owner_outcome.recipe().context_path(),
        );
        let Some(build_plan) = self
            .build_plans
            .find(
                owner_outcome.organization_id(),
                owner_outcome.project_id(),
                owner_outcome.environment_id(),
                build_plan_id,
            )
            .await?
        else {
            return Ok(None);
        };
        validate_plan_binding(&build_plan, &owner_outcome).map_err(projection_error)?;

        let artifact = owner_outcome.artifact();
        let outcome = VerifiedWorkloadBuildOutcome {
            schema: WORKLOAD_BUILD_OUTCOME_SCHEMA.into(),
            organization_id: owner_outcome.organization_id(),
            project_id: owner_outcome.project_id(),
            environment_id: owner_outcome.environment_id(),
            build_plan_id: build_plan.id,
            build_plan_digest: build_plan.contract.digest().clone(),
            source_revision_id: owner_outcome.source_revision_id(),
            build_run_id: owner_outcome.build_run_id(),
            source_commit_sha: owner_outcome.commit_sha().clone(),
            source_content_digest: owner_outcome.source_content_digest().clone(),
            recipe: owner_outcome.recipe().clone(),
            artifact: VerifiedOciArtifact {
                uri: artifact.uri().into(),
                digest: artifact.digest().clone(),
                media_type: artifact.media_type().into(),
            },
            requested_at: owner_outcome.requested_at(),
            attested_at: owner_outcome.attested_at(),
            completed_at: owner_outcome.completed_at(),
        };
        outcome.validate().map_err(projection_error)?;
        Ok(Some(outcome))
    }
}

fn validate_plan_binding(
    build_plan: &AcceptedBuildPlan,
    outcome: &ExternalSourceBuildOutcome,
) -> Result<(), String> {
    build_plan.validate()?;
    let proposal = build_plan.contract.spec().proposal.spec();
    if build_plan.organization_id != outcome.organization_id()
        || build_plan.project_id != outcome.project_id()
        || build_plan.environment_id != outcome.environment_id()
        || build_plan.source_revision_id != outcome.source_revision_id()
        || build_plan.id
            != AcceptedBuildPlan::id_for(
                outcome.organization_id(),
                outcome.source_revision_id(),
                outcome.recipe().context_path(),
            )
        || proposal.source.commit_sha != *outcome.commit_sha()
        || proposal.source.content_digest != *outcome.source_content_digest()
        || proposal.recipe != *outcome.recipe()
        || build_plan.accepted_at > outcome.requested_at()
    {
        return Err(
            "Artifacts build outcome does not match the accepted BuildPlan authority".into(),
        );
    }
    Ok(())
}

fn projection_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "invalid Developer Workflows build outcome projection: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::application::ExternalSourceBuildOutcomeQueryService;
    use crate::modules::artifacts::domain::test_support::{
        succeeded_external_build_with_output, typed_build_output,
    };
    use crate::modules::artifacts::domain::BuildRun;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::developer_workflows::domain::{
        AcceptBuildPlanWrite, AcceptedBuildPlanContract, BuildPlanAccepted, BuildPlanDetectorKind,
        BuildPlanProposal, BuildPlanProposalSpec, SourceLayoutIdentity,
    };
    use crate::modules::developer_workflows::infrastructure::InMemoryBuildPlanRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, GitCommitSha, IdempotencyRequest, PrincipalId, ProjectId, Sha256Digest,
        SourceRevisionId,
    };
    use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    #[tokio::test]
    async fn adapter_projects_exact_owner_fact_and_local_plan_authority() {
        let fixture = fixture(false).await;

        let outcome = fixture
            .adapter
            .verified_outcome(fixture.build.organization_id, fixture.build.id)
            .await
            .expect("verified outcome query")
            .expect("verified outcome");

        outcome.validate().expect("valid consumer outcome");
        assert_eq!(outcome.organization_id, fixture.build.organization_id);
        assert_eq!(outcome.project_id, fixture.plan.project_id);
        assert_eq!(outcome.environment_id, fixture.plan.environment_id);
        assert_eq!(outcome.source_revision_id, fixture.plan.source_revision_id);
        assert_eq!(outcome.build_run_id, fixture.build.id);
        assert_eq!(outcome.build_plan_id, fixture.plan.id);
        assert_eq!(outcome.build_plan_digest, *fixture.plan.contract.digest());
        assert_eq!(
            outcome.artifact.digest.as_str(),
            fixture
                .build
                .published_artifact
                .as_ref()
                .expect("published artifact")
                .digest
        );
    }

    #[tokio::test]
    async fn adapter_returns_none_without_local_plan_and_rejects_plan_drift() {
        let missing = fixture_without_plan().await;
        assert_eq!(
            missing
                .adapter
                .verified_outcome(missing.build.organization_id, missing.build.id)
                .await
                .expect("missing plan outcome"),
            None
        );

        let drifted = fixture(true).await;
        assert!(matches!(
            drifted
                .adapter
                .verified_outcome(drifted.build.organization_id, drifted.build.id)
                .await,
            Err(RepositoryError::Storage(message))
                if message.contains("does not match the accepted BuildPlan")
        ));
    }

    #[tokio::test]
    async fn adapter_rejects_owner_query_identity_substitution() {
        let fixture = fixture_without_plan().await;
        let outcomes: Arc<dyn IExternalSourceBuildOutcomeQueryPort> =
            Arc::new(FixedOutcomeQueryPort {
                outcome: fixture.owner_outcome,
            });
        let build_plans: Arc<dyn IBuildPlanRepository> = fixture.plans;
        let adapter = ArtifactsWorkloadBuildOutcomeAdapter::new(outcomes, build_plans);

        for (organization_id, build_run_id) in [
            (OrganizationId::new(), fixture.build.id),
            (fixture.build.organization_id, BuildRunId::new()),
        ] {
            assert!(matches!(
                adapter.verified_outcome(organization_id, build_run_id).await,
                Err(RepositoryError::Storage(message))
                    if message.contains("changed the requested organization or BuildRun identity")
            ));
        }
    }

    struct Fixture {
        adapter: ArtifactsWorkloadBuildOutcomeAdapter,
        build: BuildRun,
        plan: AcceptedBuildPlan,
    }

    async fn fixture(change_plan_content: bool) -> Fixture {
        let missing = fixture_without_plan().await;
        let plan = accepted_plan_for(&missing.build, change_plan_content);
        persist_plan(&missing.plans, plan.clone()).await;
        Fixture {
            adapter: missing.adapter,
            build: missing.build,
            plan,
        }
    }

    struct MissingPlanFixture {
        adapter: ArtifactsWorkloadBuildOutcomeAdapter,
        build: BuildRun,
        plans: Arc<InMemoryBuildPlanRepository>,
        owner_outcome: ExternalSourceBuildOutcome,
    }

    async fn fixture_without_plan() -> MissingPlanFixture {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let plans = Arc::new(InMemoryBuildPlanRepository::new());
        let build = succeeded_external_build_with_output(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            typed_build_output(
                &format!("sha256:{}", "d".repeat(64)),
                DURABLE_CELL_BUNDLE_MEDIA_TYPE,
                512,
            ),
            Utc::now(),
        );
        builds.seed_build(build.clone()).await;
        let outcome_service = ExternalSourceBuildOutcomeQueryService::new(builds);
        let owner_outcome = outcome_service
            .find_external_source_build_outcome(build.organization_id, build.id)
            .await
            .expect("owner outcome query")
            .expect("successful owner outcome");
        let outcomes: Arc<dyn IExternalSourceBuildOutcomeQueryPort> = Arc::new(outcome_service);
        let build_plans: Arc<dyn IBuildPlanRepository> = plans.clone();
        MissingPlanFixture {
            adapter: ArtifactsWorkloadBuildOutcomeAdapter::new(outcomes, build_plans),
            build,
            plans,
            owner_outcome,
        }
    }

    struct FixedOutcomeQueryPort {
        outcome: ExternalSourceBuildOutcome,
    }

    #[async_trait]
    impl IExternalSourceBuildOutcomeQueryPort for FixedOutcomeQueryPort {
        async fn find_external_source_build_outcome(
            &self,
            _organization_id: OrganizationId,
            _build_run_id: BuildRunId,
        ) -> Result<Option<ExternalSourceBuildOutcome>, RepositoryError> {
            Ok(Some(self.outcome.clone()))
        }
    }

    fn accepted_plan_for(build: &BuildRun, change_content: bool) -> AcceptedBuildPlan {
        let evidence = build.evidence.as_deref().expect("build evidence");
        let content_digest = if change_content {
            Sha256Digest::parse(format!("sha256:{}", "9".repeat(64)))
                .expect("changed content digest")
        } else {
            Sha256Digest::parse(&evidence.source_content_digest).expect("source content digest")
        };
        let proposal = BuildPlanProposal::from_spec(BuildPlanProposalSpec {
            source: SourceLayoutIdentity::new(
                Sha256Digest::parse(format!("sha256:{}", "8".repeat(64)))
                    .expect("source identity digest"),
                GitCommitSha::parse(&evidence.commit_sha).expect("commit SHA"),
                content_digest,
            )
            .expect("source identity"),
            detector: BuildPlanDetectorKind::Dockerfile,
            detector_revision:
                crate::modules::developer_workflows::domain::BUILD_PLAN_DETECTOR_REVISION.into(),
            project_root: evidence.recipe.context_path().into(),
            evidence_path: evidence.recipe.dockerfile_path().into(),
            evidence_digest: Sha256Digest::parse(format!("sha256:{}", "7".repeat(64)))
                .expect("evidence digest"),
            recipe: evidence.recipe.clone(),
        })
        .expect("BuildPlan proposal");
        let contract = AcceptedBuildPlanContract::from_proposal(
            build.source_revision_id().expect("source revision"),
            proposal,
        )
        .expect("accepted BuildPlan contract");
        AcceptedBuildPlan::accept(
            build.organization_id,
            build.project_id().expect("project"),
            build.environment_id().expect("environment"),
            contract,
            PrincipalId::new(),
            build.requested_at - Duration::milliseconds(1),
        )
        .expect("accepted BuildPlan")
    }

    async fn persist_plan(repository: &InMemoryBuildPlanRepository, plan: AcceptedBuildPlan) {
        let request_id = Uuid::now_v7();
        let event = BuildPlanAccepted::envelope(&plan, request_id).expect("acceptance event");
        let idempotency = IdempotencyRequest::new(
            "developer-workflows-build-outcome-test",
            plan.id.to_string(),
            plan.contract.canonical_acl().as_bytes(),
        )
        .expect("idempotency");
        repository
            .accept(AcceptBuildPlanWrite {
                actor_principal_id: plan.accepted_by,
                request_id,
                idempotency,
                event,
                plan,
            })
            .await
            .expect("persist accepted BuildPlan");
    }
}
