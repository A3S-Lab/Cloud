use super::applications_support::{digest, release_contract};
use a3s_cloud_control_plane::modules::applications::{
    Application, ApplicationRecord, ApplicationRelease, ApplicationReleasePublished,
    CreateApplicationWrite, IApplicationRepository, PostgresApplicationRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, OntologyId, OntologyRevisionId,
    OrganizationId, PrincipalId, ProjectId, ResourceName, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId, WorkflowRunId,
};
use a3s_orm::{
    sql_query, DatabaseError, Executor, PostgresDialect, PostgresError, PostgresExecutor,
    PostgresTransaction, Query,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) struct SeededWorkflowRun {
    pub run_id: WorkflowRunId,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn persist_application_release(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    workflow_definition_id: WorkflowDefinitionId,
    workflow_revision_id: WorkflowRevisionId,
    workflow_contract_digest: Sha256Digest,
    workflow_payload_set_digest: Sha256Digest,
    created_at: DateTime<Utc>,
) -> Result<(Application, ApplicationRelease), Box<dyn std::error::Error>> {
    let application_id = ApplicationId::new();
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        release_contract(
            workflow_definition_id,
            workflow_revision_id,
            workflow_contract_digest,
            workflow_payload_set_digest,
            'f',
        )?,
        actor,
        created_at,
    )?;
    let application = Application::create(
        application_id,
        ResourceName::parse("Persistent session application")?,
        "Release-pinned Application session persistence".into(),
        &release,
    )?;
    let record = ApplicationRecord::new(application.clone(), release.clone())?;
    let request_id = Uuid::now_v7();
    PostgresApplicationRepository::new(executor.clone())
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(&application, &release, request_id)?,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-session-persistence",
                "create-application",
                release.contract.canonical_acl().as_bytes(),
            )?,
            record,
        })
        .await?;
    Ok((application, release))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn seed_workflow_run(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    workflow_definition_id: WorkflowDefinitionId,
    workflow_revision_id: WorkflowRevisionId,
    created_at: DateTime<Utc>,
) -> Result<SeededWorkflowRun, Box<dyn std::error::Error>> {
    let ontology_id = Uuid::now_v7();
    let ontology_revision_id = Uuid::now_v7();
    let goal_id = Uuid::now_v7();
    let plan_revision_id = Uuid::now_v7();
    let run_id = WorkflowRunId::new();
    let ontology_digest = digest('1');
    let persisted_ontology_digest = ontology_digest.clone();
    let goal_digest = digest('2');
    let input_digest = digest('3');
    let workflow_digest = digest('a');
    let plan_digest = digest('4');
    let execution_input_digest = digest('5');
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = SeedTransaction::new(transaction);
                database
                    .execute(
                        sql_query::<()>("insert into ontologies (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(ontology_id)
                            .append(", 'Application session ontology', 'application-session-ontology', '', ")
                            .bind(ontology_revision_id)
                            .append(", 1, ")
                            .bind(ontology_digest.as_str())
                            .append(", 1, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into ontology_revisions (organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, migration_policy, migration_rule_id, migration_digest, created_by, created_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(ontology_id)
                            .append(", ")
                            .bind(ontology_revision_id)
                            .append(", 1, null, null, 'cloud.workflow.ontology.v1', 1, 'ontology \"application_session_fixture\" {}', ")
                            .bind(ontology_digest.as_str())
                            .append(", 'initial', null, null, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_goals (organization_id, project_id, id, name, contract_schema, canonical_acl, contract_digest, input_digest, workflow_definition_id, workflow_revision_id, workflow_digest, ontology_id, ontology_revision_id, ontology_digest, environment_id, plan_revision_id, plan_digest, created_by, created_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(goal_id)
                            .append(", 'Application session goal', 'cloud.workflow.goal.v1', 'goal \"application_session_fixture\" {}', ")
                            .bind(goal_digest.as_str())
                            .append(", ")
                            .bind(input_digest.as_str())
                            .append(", ")
                            .bind(workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(workflow_revision_id.as_uuid())
                            .append(", ")
                            .bind(workflow_digest.as_str())
                            .append(", ")
                            .bind(ontology_id)
                            .append(", ")
                            .bind(ontology_revision_id)
                            .append(", ")
                            .bind(ontology_digest.as_str())
                            .append(", null, ")
                            .bind(plan_revision_id)
                            .append(", ")
                            .bind(plan_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_plan_revisions (organization_id, project_id, workflow_goal_id, id, plan_schema, compiler_revision, canonical_plan, plan_digest, created_by, created_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(goal_id)
                            .append(", ")
                            .bind(plan_revision_id)
                            .append(", 'cloud.workflow.plan.v1', 'cloud.workflow.plan-compiler.v1', 'plan \"application_session_fixture\" {}', ")
                            .bind(plan_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (")
                            .bind(run_id.as_uuid())
                            .append(", ")
                            .bind(organization_id.as_uuid())
                            .append(", 'workflow_run', ")
                            .bind(run_id.as_uuid())
                            .append(", 'cloud.workflow.run', '1', '{}'::jsonb, ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_runs (organization_id, project_id, id, workflow_goal_id, plan_revision_id, plan_digest, operation_id, flow_run_id, flow_runtime_build_id, execution_input, execution_input_digest, status, last_flow_sequence, output, output_digest, error, aggregate_version, requested_by, requested_at, updated_at, started_at, cancellation_requested_at, cancellation_reason, finished_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(run_id.as_uuid())
                            .append(", ")
                            .bind(goal_id)
                            .append(", ")
                            .bind(plan_revision_id)
                            .append(", ")
                            .bind(plan_digest.as_str())
                            .append(", ")
                            .bind(run_id.as_uuid())
                            .append(", ")
                            .bind(run_id.to_string())
                            .append(", null, '{}', ")
                            .bind(execution_input_digest.as_str())
                            .append(", 'pending', 0, null, null, null, 1, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(", null, null, null, null)"),
                    )
                    .await?;
                Ok::<(), DatabaseError<PostgresError>>(())
            })
        })
        .await?;
    Ok(SeededWorkflowRun {
        run_id,
        ontology_id: OntologyId::from_uuid(ontology_id),
        ontology_revision_id: OntologyRevisionId::from_uuid(ontology_revision_id),
        ontology_digest: persisted_ontology_digest,
    })
}

struct SeedTransaction<'a> {
    transaction: &'a PostgresTransaction,
}

impl<'a> SeedTransaction<'a> {
    const fn new(transaction: &'a PostgresTransaction) -> Self {
        Self { transaction }
    }

    async fn execute<Q>(&self, query: Q) -> Result<(), DatabaseError<PostgresError>>
    where
        Q: Query,
    {
        let query = query
            .compile(&PostgresDialect)
            .map_err(DatabaseError::Build)?;
        self.transaction
            .execute(&query)
            .await
            .map_err(DatabaseError::Execute)?;
        Ok(())
    }
}
