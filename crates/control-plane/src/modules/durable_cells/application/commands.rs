use super::build_run_access::validate_definition_build_run;
use super::managed_replica_lifecycle::converge_current_managed_replicas;
use super::resource_access::{application_not_found, environment, environment_not_found};
use super::DurableCellApplicationMutationResult;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, DurableCellApplication, DurableCellApplicationChanged,
    DurableCellApplicationDefinition, DurableCellApplicationDesiredState,
    DurableCellApplicationRecord, DurableCellApplicationRevision,
    IDurableCellApplicationRepository, RequestDurableCellApplicationStateWrite,
    ReviseDurableCellApplicationWrite,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    OrganizationId, PrincipalId, ProjectId, RepositoryError, ResourceName,
};
use crate::modules::workloads::IWorkloadRepository;
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: String,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateDurableCellApplication {
    type Output = ApplicationResult<DurableCellApplicationMutationResult>;
}

pub struct CreateDurableCellApplicationHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    applications: Arc<dyn IDurableCellApplicationRepository>,
    builds: Arc<dyn IBuildRunRepository>,
}

impl CreateDurableCellApplicationHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        applications: Arc<dyn IDurableCellApplicationRepository>,
        builds: Arc<dyn IBuildRunRepository>,
    ) -> Self {
        Self {
            environments,
            applications,
            builds,
        }
    }
}

impl CommandHandler<CreateDurableCellApplication> for CreateDurableCellApplicationHandler {
    fn execute(
        &self,
        command: CreateDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationMutationResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let applications = Arc::clone(&self.applications);
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let definition =
                match DurableCellApplicationDefinition::parse_acl(&command.definition_acl) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let canonical = serde_json::to_vec(&CanonicalCreateDurableCellApplication {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                name: name.as_str(),
                definition_digest: definition.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/durable-cell-applications",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match applications.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !create_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        &name,
                        &definition,
                    ) {
                        return Err(BootError::Internal(
                            "Durable Cell create replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(DurableCellApplicationMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(environment_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = validate_definition_build_run(
                builds.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                &definition,
            )
            .await
            {
                return Ok(Err(error));
            }
            let application_id = DurableCellApplicationId::new();
            let revision = match DurableCellApplicationRevision::initial(
                command.organization_id,
                command.project_id,
                command.environment_id,
                application_id,
                DurableCellApplicationRevisionId::new(),
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let application = match DurableCellApplication::create(application_id, name, &revision)
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event =
                DurableCellApplicationChanged::created(&application, &revision, command.request_id)
                    .map_err(BootError::Internal)?;
            let record = DurableCellApplicationRecord::new(application, revision)
                .map_err(BootError::Internal)?;
            match applications
                .create(CreateDurableCellApplicationWrite {
                    record,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(DurableCellApplicationMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ReviseDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub expected_version: u64,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReviseDurableCellApplication {
    type Output = ApplicationResult<DurableCellApplicationMutationResult>;
}

pub struct ReviseDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    builds: Arc<dyn IBuildRunRepository>,
}

impl ReviseDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        builds: Arc<dyn IBuildRunRepository>,
    ) -> Self {
        Self {
            applications,
            builds,
        }
    }
}

impl CommandHandler<ReviseDurableCellApplication> for ReviseDurableCellApplicationHandler {
    fn execute(
        &self,
        command: ReviseDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let builds = Arc::clone(&self.builds);
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Durable Cell application version must be positive".into(),
                )));
            }
            let definition =
                match DurableCellApplicationDefinition::parse_acl(&command.definition_acl) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let canonical = serde_json::to_vec(&CanonicalReviseDurableCellApplication {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                application_id: command.application_id,
                expected_version: command.expected_version,
                definition_digest: definition.digest().as_str(),
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/durable-cell-applications/{}/revisions",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match applications.replay_write(&idempotency).await {
                Ok(Some(record)) => {
                    if !revise_replay_matches(
                        &record,
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.application_id,
                        command.expected_version,
                        &definition,
                    ) {
                        return Err(BootError::Internal(
                            "Durable Cell revision replay reference is inconsistent".into(),
                        ));
                    }
                    return Ok(Ok(DurableCellApplicationMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let current = match applications
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(application_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if current.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "Durable Cell application was revised from a stale aggregate version".into(),
                )));
            }
            let current_revision =
                match load_current_revision(applications.as_ref(), &current).await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            if let Err(error) = validate_definition_build_run(
                builds.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                &definition,
            )
            .await
            {
                return Ok(Err(error));
            }
            let revision = match DurableCellApplicationRevision::successor(
                &current_revision,
                DurableCellApplicationRevisionId::new(),
                definition,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let application = match current.advance(command.expected_version, &revision) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                DurableCellApplicationChanged::revised(&application, &revision, command.request_id)
                    .map_err(BootError::Internal)?;
            let record = DurableCellApplicationRecord::new(application, revision)
                .map_err(BootError::Internal)?;
            match applications
                .revise(ReviseDurableCellApplicationWrite {
                    record,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(DurableCellApplicationMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct StartDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for StartDurableCellApplication {
    type Output = ApplicationResult<DurableCellApplicationMutationResult>;
}

pub struct StartDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
}

impl StartDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
    ) -> Self {
        Self {
            applications,
            workloads,
        }
    }
}

impl CommandHandler<StartDurableCellApplication> for StartDurableCellApplicationHandler {
    fn execute(
        &self,
        command: StartDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationMutationResult>>,
    > {
        execute_state(
            Arc::clone(&self.applications),
            Arc::clone(&self.workloads),
            DurableCellStateCommand {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                application_id: command.application_id,
                expected_version: command.expected_version,
                actor_principal_id: command.actor_principal_id,
                resource_access: command.resource_access,
                idempotency_key: command.idempotency_key,
                request_id: command.request_id,
            },
            DurableCellApplicationDesiredState::Running,
        )
    }
}

#[derive(Debug, Clone)]
pub struct StopDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for StopDurableCellApplication {
    type Output = ApplicationResult<DurableCellApplicationMutationResult>;
}

pub struct StopDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
}

impl StopDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
    ) -> Self {
        Self {
            applications,
            workloads,
        }
    }
}

impl CommandHandler<StopDurableCellApplication> for StopDurableCellApplicationHandler {
    fn execute(
        &self,
        command: StopDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationMutationResult>>,
    > {
        execute_state(
            Arc::clone(&self.applications),
            Arc::clone(&self.workloads),
            DurableCellStateCommand {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                application_id: command.application_id,
                expected_version: command.expected_version,
                actor_principal_id: command.actor_principal_id,
                resource_access: command.resource_access,
                idempotency_key: command.idempotency_key,
                request_id: command.request_id,
            },
            DurableCellApplicationDesiredState::Stopped,
        )
    }
}

struct DurableCellStateCommand {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    actor_principal_id: PrincipalId,
    resource_access: ResourceAccessEvaluator,
    idempotency_key: String,
    request_id: Uuid,
}

fn execute_state(
    applications: Arc<dyn IDurableCellApplicationRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    command: DurableCellStateCommand,
    desired_state: DurableCellApplicationDesiredState,
) -> a3s_boot::BoxFuture<
    'static,
    a3s_boot::Result<ApplicationResult<DurableCellApplicationMutationResult>>,
> {
    Box::pin(async move {
        if let Err(error) = environment(
            command.project_id,
            command.environment_id,
            &command.resource_access,
        ) {
            return Ok(Err(error));
        }
        if command.expected_version == 0 {
            return Ok(Err(ApplicationError::Invalid(
                "expected Durable Cell application version must be positive".into(),
            )));
        }
        let canonical = serde_json::to_vec(&CanonicalRequestDurableCellApplicationState {
            organization_id: command.organization_id,
            project_id: command.project_id,
            environment_id: command.environment_id,
            application_id: command.application_id,
            expected_version: command.expected_version,
            desired_state: desired_state.as_str(),
        })
        .map_err(|error| BootError::Internal(error.to_string()))?;
        let idempotency = match IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/durable-cell-applications/{}/desired-state",
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.application_id
            ),
            command.idempotency_key,
            &canonical,
        ) {
            Ok(value) => value,
            Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
        };
        match applications.replay_write(&idempotency).await {
            Ok(Some(record)) => {
                if !state_replay_matches(
                    &record,
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                    command.expected_version,
                    desired_state,
                ) {
                    return Err(BootError::Internal(
                        "Durable Cell state replay reference is inconsistent".into(),
                    ));
                }
                let mutation = DurableCellApplicationMutationResult {
                    record,
                    replayed: true,
                };
                if let Err(error) = converge_current_managed_replicas(
                    applications.as_ref(),
                    workloads.as_ref(),
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                )
                .await
                {
                    return Ok(Err(error));
                }
                return Ok(Ok(mutation));
            }
            Ok(None) => {}
            Err(error) => return Ok(Err(error.into())),
        }
        let current = match applications
            .find(
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.application_id,
            )
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(application_not_found())),
            Err(error) => return Ok(Err(error.into())),
        };
        if current.aggregate_version != command.expected_version {
            return Ok(Err(ApplicationError::Conflict(
                "Durable Cell desired state was requested from a stale aggregate version".into(),
            )));
        }
        if current.desired_state == desired_state {
            return Ok(Err(ApplicationError::Conflict(format!(
                "Durable Cell application is already {}",
                desired_state.as_str()
            ))));
        }
        let revision = match load_current_revision(applications.as_ref(), &current).await {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        let application =
            match current.request_state(command.expected_version, desired_state, Utc::now()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
        let event = DurableCellApplicationChanged::state_requested(
            &application,
            &revision,
            command.request_id,
        )
        .map_err(BootError::Internal)?;
        let record = DurableCellApplicationRecord::new(application, revision)
            .map_err(BootError::Internal)?;
        match applications
            .request_state(RequestDurableCellApplicationStateWrite {
                record,
                expected_version: command.expected_version,
                event,
                actor_principal_id: command.actor_principal_id,
                request_id: command.request_id,
                idempotency,
            })
            .await
        {
            Ok(result) => {
                let mutation = DurableCellApplicationMutationResult {
                    record: result.value,
                    replayed: result.replayed,
                };
                if let Err(error) = converge_current_managed_replicas(
                    applications.as_ref(),
                    workloads.as_ref(),
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                )
                .await
                {
                    return Ok(Err(error));
                }
                Ok(Ok(mutation))
            }
            Err(error) => Ok(Err(error.into())),
        }
    })
}

async fn load_current_revision(
    applications: &dyn IDurableCellApplicationRepository,
    application: &DurableCellApplication,
) -> ApplicationResult<DurableCellApplicationRevision> {
    match applications
        .find_revision(
            application.organization_id,
            application.project_id,
            application.environment_id,
            application.id,
            application.current_revision_id,
        )
        .await
    {
        Ok(Some(value)) => Ok(value),
        Ok(None) | Err(RepositoryError::NotFound) => Err(ApplicationError::Internal(
            "Durable Cell application current revision is missing".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

fn create_replay_matches(
    record: &DurableCellApplicationRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &ResourceName,
    definition: &DurableCellApplicationDefinition,
) -> bool {
    record.application.organization_id == organization_id
        && record.application.project_id == project_id
        && record.application.environment_id == environment_id
        && &record.application.name == name
        && record.revision.definition.digest() == definition.digest()
}

fn revise_replay_matches(
    record: &DurableCellApplicationRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    definition: &DurableCellApplicationDefinition,
) -> bool {
    record.application.organization_id == organization_id
        && record.application.project_id == project_id
        && record.application.environment_id == environment_id
        && record.application.id == application_id
        && record.application.aggregate_version == expected_version.saturating_add(1)
        && record.revision.definition.digest() == definition.digest()
}

#[allow(clippy::too_many_arguments)]
fn state_replay_matches(
    record: &DurableCellApplicationRecord,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    desired_state: DurableCellApplicationDesiredState,
) -> bool {
    record.application.organization_id == organization_id
        && record.application.project_id == project_id
        && record.application.environment_id == environment_id
        && record.application.id == application_id
        && record.application.aggregate_version == expected_version.saturating_add(1)
        && record.application.desired_state == desired_state
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalCreateDurableCellApplication<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &'a str,
    definition_digest: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalReviseDurableCellApplication<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    definition_digest: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalRequestDurableCellApplicationState<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    expected_version: u64,
    desired_state: &'a str,
}
