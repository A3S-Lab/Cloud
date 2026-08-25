use crate::modules::agents::{IAgentRepository, PostgresAgentRepository};
use crate::modules::applications::{
    IApplicationRepository, IApplicationSessionRepository, PostgresApplicationRepository,
    PostgresApplicationSessionRepository,
};
use crate::modules::artifacts::{
    IBuildCandidateProjectionPort, IBuildRunRepository, PostgresBuildRunRepository,
};
use crate::modules::assets::{
    IAssetGitRepositoryControl, IAssetRepository, IMcpServiceProfileRepository,
    PostgresAssetRepository,
};
use crate::modules::audit::{IAuditRecordRepository, PostgresAuditRecordRepository};
use crate::modules::connectors::{
    IConnectorExecutionAttemptRepository, IConnectorExecutionAttemptResolutionRepository,
    IConnectorProfileRepository, IConnectorRevisionRevocationRepository,
    PostgresConnectorExecutionAttemptRepository, PostgresConnectorProfileRepository,
};
use crate::modules::durable_cells::{
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    PostgresDurableCellApplicationRepository, PostgresDurableCellDeploymentRepository,
};
use crate::modules::edge::{
    IEdgeRepository, IMcpCredentialLifecycleRepository, IMcpCredentialRepository,
    IMcpGatewaySnapshotRepository, IMcpRoutePolicyRepository, PostgresEdgeRepository,
};
use crate::modules::executions::{
    IExecutionRepository, IExecutionTemplateRepository, PostgresExecutionRepository,
    PostgresExecutionTemplateRepository,
};
use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, INodeAvailabilityRepository, INodeControlRepository,
    INodeDrainRepository, INodePoolRepository, INodeRepository, INodeSchedulingRepository,
};
use crate::modules::fleet::PostgresNodeRepository;
use crate::modules::forms::{IFormRepository, PostgresFormRepository};
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IMembershipInvitationRepository, IMembershipRepository,
    IOidcIdentityRepository, IOrganizationRepository, IRecipientContactRepository,
    IRecipientContactVerificationDeliveryRepository, IResourceAuthorizationDecisionRepository,
    IResourceGrantRepository,
};
use crate::modules::identity::PostgresIdentityRepository;
use crate::modules::integration_events::{IOutboxRepository, PostgresOutboxRepository};
use crate::modules::notifications::{
    INotificationAlertPolicyRepository, INotificationRepository,
    IOutboundNotificationDeliveryRepository, IOutboundNotificationRepository,
    IOutboundNotificationSmtpAttemptRepository, PostgresNotificationRepository,
};
use crate::modules::operations::{IOperationRepository, PostgresOperationRepository};
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::IPluginRegistryEnrollmentAuthorizer;
use crate::modules::plugins::PostgresPluginRegistryRepository;
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::PostgresProjectsRepository;
use crate::modules::search::{ISearchRepository, PostgresSearchRepository};
use crate::modules::secrets::{ISecretRepository, PostgresSecretRepository};
use crate::modules::security::{
    IGatewayRoutePolicyTimelineRepository, PostgresGatewayRoutePolicyTimelineRepository,
};
use crate::modules::sources::domain::{
    IGithubConnectionRepository, ISourceRevisionRepository, ISourceSubscriptionRepository,
    ISourceWebhookRepository,
};
use crate::modules::sources::{
    PostgresGithubConnectionRepository, PostgresSourceRevisionRepository,
    PostgresSourceSubscriptionRepository,
};
use crate::modules::workflow::{
    IHumanTaskRepository, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, PostgresHumanTaskRepository,
    PostgresOntologyRepository, PostgresWorkflowDefinitionRepository,
    PostgresWorkflowGoalRepository, PostgresWorkflowRunRepository,
};
use crate::modules::workloads::{
    IDeploymentFlowWorkloadRepository, IResourceClaimRepository, ISecretRotationRestartRepository,
    IWorkloadReplicaDeploymentRepository, IWorkloadReplicaEvacuationRepository,
    IWorkloadReplicaRetirementRepository, IWorkloadRepository, IWorkloadRuntimeControl,
    IWorkloadRuntimeTargetRepository, IWorkloadWriterFenceRepository,
    PostgresResourceClaimRepository, PostgresWorkloadRepository,
};
use a3s_orm::PostgresExecutor;
use std::sync::Arc;

/// The sole constructor boundary between process composition and PostgreSQL
/// repository implementations.
///
/// Creating the factory performs no I/O and grants no capability. A process
/// receives a repository only by selecting the corresponding typed family in
/// its existing role-gated composition branch.
pub(super) struct PostgresAdapterFactory {
    executor: PostgresExecutor,
}

impl PostgresAdapterFactory {
    pub(super) const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }

    pub(super) fn api_worker(&self) -> ApiWorkerPostgresAdapters {
        let artifacts = ArtifactPostgresAdapters::new(self.executor.clone());
        ApiWorkerPostgresAdapters {
            identity: IdentityPostgresAdapters::new(self.executor.clone()),
            projects: ProjectPostgresAdapters::new(self.executor.clone()),
            workflow: WorkflowPostgresAdapters::new(self.executor.clone()),
            notifications: NotificationPostgresAdapters::new(self.executor.clone()),
            plugins: PluginPostgresAdapters::new(self.executor.clone()),
            fleet: FleetPostgresAdapters::new(self.executor.clone()),
            workloads: WorkloadPostgresAdapters::new(self.executor.clone()),
            edge: EdgePostgresAdapters::new(self.executor.clone()),
            assets: AssetPostgresAdapters::new(self.executor.clone()),
            sources: SourcePostgresAdapters::new(self.executor.clone()),
            search: Arc::new(PostgresSearchRepository::new(self.executor.clone())),
            audit_records: Arc::new(PostgresAuditRecordRepository::new(self.executor.clone())),
            security_investigations: Arc::new(PostgresGatewayRoutePolicyTimelineRepository::new(
                self.executor.clone(),
            )),
            builds: artifacts.builds,
            build_candidates: artifacts.build_candidates,
            executions: Arc::new(PostgresExecutionRepository::new(self.executor.clone())),
            execution_templates: Arc::new(PostgresExecutionTemplateRepository::new(
                self.executor.clone(),
            )),
            agents: Arc::new(PostgresAgentRepository::new(self.executor.clone())),
            secrets: Arc::new(PostgresSecretRepository::new(self.executor.clone())),
            connector_profiles: Arc::new(PostgresConnectorProfileRepository::new(
                self.executor.clone(),
            )),
            applications: Arc::new(PostgresApplicationRepository::new(self.executor.clone())),
            application_sessions: Arc::new(PostgresApplicationSessionRepository::new(
                self.executor.clone(),
            )),
            durable_cell_applications: Arc::new(PostgresDurableCellApplicationRepository::new(
                self.executor.clone(),
            )),
            durable_cell_deployments: Arc::new(PostgresDurableCellDeploymentRepository::new(
                self.executor.clone(),
            )),
            operations: Arc::new(PostgresOperationRepository::new(self.executor.clone())),
        }
    }

    pub(super) fn relay(&self) -> RelayPostgresAdapters {
        let identity = IdentityPostgresAdapters::new(self.executor.clone());
        let notifications = NotificationPostgresAdapters::new(self.executor.clone());
        let assets = AssetPostgresAdapters::new(self.executor.clone());
        let artifacts = ArtifactPostgresAdapters::new(self.executor.clone());
        RelayPostgresAdapters {
            memberships: identity.memberships,
            resource_grants: identity.resource_grants,
            notifications: notifications.notifications,
            alert_policies: notifications.alert_policies,
            assets: assets.assets,
            build_candidates: artifacts.build_candidates,
            outbox: self.outbox(),
        }
    }

    pub(super) fn connector_execution(&self) -> ConnectorExecutionPostgresAdapters {
        ConnectorExecutionPostgresAdapters::new(self.executor.clone())
    }

    pub(super) fn outbox(&self) -> Arc<dyn IOutboxRepository> {
        Arc::new(PostgresOutboxRepository::new(self.executor.clone()))
    }
}

pub(super) struct ConnectorExecutionPostgresAdapters {
    pub(super) attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    pub(super) resolutions: Arc<dyn IConnectorExecutionAttemptResolutionRepository>,
    pub(super) revocations: Arc<dyn IConnectorRevisionRevocationRepository>,
}

impl ConnectorExecutionPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresConnectorExecutionAttemptRepository::new(executor));
        Self {
            attempts: repository.clone(),
            resolutions: repository.clone(),
            revocations: repository,
        }
    }
}

pub(super) struct ApiWorkerPostgresAdapters {
    pub(super) identity: IdentityPostgresAdapters,
    pub(super) projects: ProjectPostgresAdapters,
    pub(super) workflow: WorkflowPostgresAdapters,
    pub(super) notifications: NotificationPostgresAdapters,
    pub(super) plugins: PluginPostgresAdapters,
    pub(super) fleet: FleetPostgresAdapters,
    pub(super) workloads: WorkloadPostgresAdapters,
    pub(super) edge: EdgePostgresAdapters,
    pub(super) assets: AssetPostgresAdapters,
    pub(super) sources: SourcePostgresAdapters,
    pub(super) search: Arc<dyn ISearchRepository>,
    pub(super) audit_records: Arc<dyn IAuditRecordRepository>,
    pub(super) security_investigations: Arc<dyn IGatewayRoutePolicyTimelineRepository>,
    pub(super) builds: Arc<dyn IBuildRunRepository>,
    pub(super) build_candidates: Arc<dyn IBuildCandidateProjectionPort>,
    pub(super) executions: Arc<dyn IExecutionRepository>,
    pub(super) execution_templates: Arc<dyn IExecutionTemplateRepository>,
    pub(super) agents: Arc<dyn IAgentRepository>,
    pub(super) secrets: Arc<dyn ISecretRepository>,
    pub(super) connector_profiles: Arc<dyn IConnectorProfileRepository>,
    pub(super) applications: Arc<dyn IApplicationRepository>,
    pub(super) application_sessions: Arc<dyn IApplicationSessionRepository>,
    pub(super) durable_cell_applications: Arc<dyn IDurableCellApplicationRepository>,
    pub(super) durable_cell_deployments: Arc<dyn IDurableCellDeploymentRepository>,
    pub(super) operations: Arc<dyn IOperationRepository>,
}

pub(super) struct RelayPostgresAdapters {
    pub(super) memberships: Arc<dyn IMembershipRepository>,
    pub(super) resource_grants: Arc<dyn IResourceGrantRepository>,
    pub(super) notifications: Arc<dyn INotificationRepository>,
    pub(super) alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
    pub(super) assets: Arc<dyn IAssetRepository>,
    pub(super) build_candidates: Arc<dyn IBuildCandidateProjectionPort>,
    pub(super) outbox: Arc<dyn IOutboxRepository>,
}

struct ArtifactPostgresAdapters {
    builds: Arc<dyn IBuildRunRepository>,
    build_candidates: Arc<dyn IBuildCandidateProjectionPort>,
}

impl ArtifactPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresBuildRunRepository::new(executor));
        Self {
            builds: repository.clone(),
            build_candidates: repository,
        }
    }
}

pub(super) struct IdentityPostgresAdapters {
    pub(super) organizations: Arc<dyn IOrganizationRepository>,
    pub(super) api_tokens: Arc<dyn IApiTokenRepository>,
    pub(super) memberships: Arc<dyn IMembershipRepository>,
    pub(super) membership_invitations: Arc<dyn IMembershipInvitationRepository>,
    pub(super) resource_grants: Arc<dyn IResourceGrantRepository>,
    pub(super) oidc_identity: Arc<dyn IOidcIdentityRepository>,
    pub(super) recipient_contacts: Arc<dyn IRecipientContactRepository>,
    pub(super) recipient_contact_verification_deliveries:
        Arc<dyn IRecipientContactVerificationDeliveryRepository>,
    pub(super) resource_authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
}

impl IdentityPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresIdentityRepository::new(executor));
        Self {
            organizations: repository.clone(),
            api_tokens: repository.clone(),
            memberships: repository.clone(),
            membership_invitations: repository.clone(),
            resource_grants: repository.clone(),
            oidc_identity: repository.clone(),
            recipient_contacts: repository.clone(),
            recipient_contact_verification_deliveries: repository.clone(),
            resource_authorization_decisions: repository,
        }
    }
}

pub(super) struct ProjectPostgresAdapters {
    pub(super) projects: Arc<dyn IProjectRepository>,
    pub(super) environments: Arc<dyn IEnvironmentRepository>,
}

impl ProjectPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresProjectsRepository::new(executor));
        Self {
            projects: repository.clone(),
            environments: repository,
        }
    }
}

pub(super) struct WorkflowPostgresAdapters {
    pub(super) ontologies: Arc<dyn IOntologyRepository>,
    pub(super) workflow_definitions: Arc<dyn IWorkflowDefinitionRepository>,
    pub(super) workflow_goals: Arc<dyn IWorkflowGoalRepository>,
    pub(super) workflow_runs: Arc<dyn IWorkflowRunRepository>,
    pub(super) forms: Arc<dyn IFormRepository>,
    pub(super) human_tasks: Arc<dyn IHumanTaskRepository>,
}

impl WorkflowPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        Self {
            ontologies: Arc::new(PostgresOntologyRepository::new(executor.clone())),
            workflow_definitions: Arc::new(PostgresWorkflowDefinitionRepository::new(
                executor.clone(),
            )),
            workflow_goals: Arc::new(PostgresWorkflowGoalRepository::new(executor.clone())),
            workflow_runs: Arc::new(PostgresWorkflowRunRepository::new(executor.clone())),
            forms: Arc::new(PostgresFormRepository::new(executor.clone())),
            human_tasks: Arc::new(PostgresHumanTaskRepository::new(executor)),
        }
    }
}

pub(super) struct NotificationPostgresAdapters {
    pub(super) notifications: Arc<dyn INotificationRepository>,
    pub(super) alert_policies: Arc<dyn INotificationAlertPolicyRepository>,
    pub(super) outbound_notifications: Arc<dyn IOutboundNotificationRepository>,
    pub(super) outbound_deliveries: Arc<dyn IOutboundNotificationDeliveryRepository>,
    pub(super) outbound_smtp_attempts: Arc<dyn IOutboundNotificationSmtpAttemptRepository>,
}

impl NotificationPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresNotificationRepository::new(executor));
        Self {
            notifications: repository.clone(),
            alert_policies: repository.clone(),
            outbound_notifications: repository.clone(),
            outbound_deliveries: repository.clone(),
            outbound_smtp_attempts: repository,
        }
    }
}

pub(super) struct PluginPostgresAdapters {
    pub(super) registries: Arc<dyn IPluginRegistryRepository>,
    pub(super) enrollment_authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer>,
}

impl PluginPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresPluginRegistryRepository::new(executor));
        Self {
            registries: repository.clone(),
            enrollment_authorizer: repository,
        }
    }
}

pub(super) struct FleetPostgresAdapters {
    pub(super) nodes: Arc<dyn INodeRepository>,
    pub(super) node_availability: Arc<dyn INodeAvailabilityRepository>,
    pub(super) scheduling_nodes: Arc<dyn INodeSchedulingRepository>,
    pub(super) node_pools: Arc<dyn INodePoolRepository>,
    pub(super) draining_nodes: Arc<dyn INodeDrainRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) log_retention: Arc<dyn ILogRetentionRepository>,
    pub(super) workload_runtime_control: Arc<dyn IWorkloadRuntimeControl>,
}

impl FleetPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresNodeRepository::new(executor));
        Self {
            nodes: repository.clone(),
            node_availability: repository.clone(),
            scheduling_nodes: repository.clone(),
            node_pools: repository.clone(),
            draining_nodes: repository.clone(),
            node_control: repository.clone(),
            log_retention: repository.clone(),
            workload_runtime_control: repository,
        }
    }
}

pub(super) struct WorkloadPostgresAdapters {
    pub(super) workloads: Arc<dyn IWorkloadRepository>,
    pub(super) deployment_workloads: Arc<dyn IDeploymentFlowWorkloadRepository>,
    pub(super) replica_deployments: Arc<dyn IWorkloadReplicaDeploymentRepository>,
    pub(super) replica_evacuations: Arc<dyn IWorkloadReplicaEvacuationRepository>,
    pub(super) replica_retirements: Arc<dyn IWorkloadReplicaRetirementRepository>,
    pub(super) writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
    pub(super) workload_targets: Arc<dyn IWorkloadRuntimeTargetRepository>,
    pub(super) secret_rotation_restarts: Arc<dyn ISecretRotationRestartRepository>,
    pub(super) resource_claims: Arc<dyn IResourceClaimRepository>,
}

impl WorkloadPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
        Self {
            workloads: repository.clone(),
            deployment_workloads: repository.clone(),
            replica_deployments: repository.clone(),
            replica_evacuations: repository.clone(),
            replica_retirements: repository.clone(),
            writer_fences: repository.clone(),
            workload_targets: repository.clone(),
            secret_rotation_restarts: repository,
            resource_claims: Arc::new(PostgresResourceClaimRepository::new(executor)),
        }
    }
}

pub(super) struct EdgePostgresAdapters {
    pub(super) routes: Arc<dyn IEdgeRepository>,
    pub(super) mcp_credentials: Arc<dyn IMcpCredentialLifecycleRepository>,
    pub(super) mcp_credential_reader: Arc<dyn IMcpCredentialRepository>,
    pub(super) mcp_route_policies: Arc<dyn IMcpRoutePolicyRepository>,
    pub(super) mcp_gateway_snapshots: Arc<dyn IMcpGatewaySnapshotRepository>,
}

impl EdgePostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresEdgeRepository::new(executor));
        Self {
            routes: repository.clone(),
            mcp_credentials: repository.clone(),
            mcp_credential_reader: repository.clone(),
            mcp_route_policies: repository.clone(),
            mcp_gateway_snapshots: repository,
        }
    }
}

pub(super) struct AssetPostgresAdapters {
    pub(super) assets: Arc<dyn IAssetRepository>,
    pub(super) controls: Arc<dyn IAssetGitRepositoryControl>,
    pub(super) mcp_profiles: Arc<dyn IMcpServiceProfileRepository>,
}

impl AssetPostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let repository = Arc::new(PostgresAssetRepository::new(executor));
        Self {
            assets: repository.clone(),
            controls: repository.clone(),
            mcp_profiles: repository,
        }
    }
}

pub(super) struct SourcePostgresAdapters {
    pub(super) sources: Arc<dyn ISourceRevisionRepository>,
    pub(super) webhooks: Arc<dyn ISourceWebhookRepository>,
    pub(super) subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    pub(super) github_connections: Arc<dyn IGithubConnectionRepository>,
}

impl SourcePostgresAdapters {
    fn new(executor: PostgresExecutor) -> Self {
        let revisions = Arc::new(PostgresSourceRevisionRepository::new(executor.clone()));
        Self {
            sources: revisions.clone(),
            webhooks: revisions,
            subscriptions: Arc::new(PostgresSourceSubscriptionRepository::new(executor.clone())),
            github_connections: Arc::new(PostgresGithubConnectionRepository::new(executor)),
        }
    }
}
