use super::applications::{
    ApplicationArguments, ApplicationInvocationArguments, ApplicationReleaseArguments,
    ApplicationSessionArguments, CancelApplicationInvocationArguments,
    CloseApplicationSessionArguments, CreateApplicationArguments, ListApplicationMessagesArguments,
    ListApplicationReleasesArguments, ListApplicationsArguments, OpenApplicationSessionArguments,
    PublishApplicationReleaseArguments, RequestApplicationInvocationArguments,
};
use super::arguments::{
    self, BuildRunArguments, BuildRunListArguments, BuildRunLogArguments, DeploymentArguments,
    EmptyArguments, EnvironmentScopeArguments, NodeArguments, OperationListArguments,
    RouteArguments, WorkloadArguments, WorkloadLogArguments,
};
use super::artifacts::BuildRunMutationArguments;
use super::audit::{
    AuditRecordExportArguments, AuditRecordListArguments, AuditRecordManifestExportArguments,
};
use super::catalog::ManagementTool;
use super::connectors::{
    ConnectorProfileArguments, ConnectorRevisionArguments, CreateConnectorProfileArguments,
    ListConnectorProfilesArguments, ListConnectorRevisionsArguments,
    ReviseConnectorProfileArguments,
};
use super::developer_workflows::{
    AcceptBuildPlanArguments, AcceptPullRequestPreviewPolicyArguments,
    AcceptWorkloadProfileArguments, DetectBuildPlansArguments, GetAcceptedBuildPlanArguments,
    GetAcceptedPullRequestPreviewPolicyRevisionArguments,
    GetAcceptedWorkloadProfileRevisionArguments,
    GetCurrentAcceptedPullRequestPreviewPolicyRevisionArguments,
    GetCurrentAcceptedWorkloadProfileRevisionArguments, GetPullRequestPreviewArguments,
    ListAcceptedBuildPlansArguments, ListAcceptedPullRequestPreviewPolicyRevisionsArguments,
    ListAcceptedWorkloadProfileRevisionsArguments,
};
use super::durable_cells::{
    CreateDurableCellApplicationArguments, DeployDurableCellApplicationArguments,
    DurableCellApplicationArguments, DurableCellApplicationRevisionArguments,
    ListDurableCellApplicationRevisionsArguments, ListDurableCellApplicationsArguments,
    PublishDurableCellApplicationRouteArguments, ReviseDurableCellApplicationArguments,
    SetDurableCellApplicationStateArguments,
};
use super::execution_templates::{
    CreateExecutionTemplateArguments, GetExecutionTemplateArguments,
    ListExecutionTemplatesArguments,
};
use super::files::{
    ListUserFilesArguments, ReserveUserFileArguments, TombstoneUserFileArguments, UserFileArguments,
};
use super::forms::{
    CreateFormDraftArguments, FormDraftArguments, FormReleaseArguments, ListFormDraftsArguments,
    PublishFormReleaseArguments, ReviseFormDraftArguments,
};
use super::identity::{
    ChangeMembershipRoleArguments, CreateMembershipArguments, CreateMembershipInvitationArguments,
    CreateResourceGrantArguments, ListResourceGrantsArguments, MembershipArguments,
    MembershipInvitationArguments, MembershipInvitationMutationArguments,
    RecipientContactArguments, ResourceGrantArguments, RevokeMembershipArguments,
    RevokeRecipientContactArguments, RevokeResourceGrantArguments,
};
use super::notifications::{
    CreateNotificationAlertPolicyArguments, CreateOutboundNotificationSubscriptionArguments,
    MarkNotificationReadArguments, NotificationAlertPolicyArguments,
    NotificationAlertPolicyListArguments, NotificationArguments, NotificationListArguments,
    OutboundNotificationSubscriptionArguments, OutboundNotificationSubscriptionListArguments,
    RevokeNotificationAlertPolicyArguments, RevokeOutboundNotificationSubscriptionArguments,
};
use super::ontology::{
    CreateOntologyArguments, ListOntologiesArguments, OntologyArguments, OntologyDiffArguments,
    OntologyRevisionArguments, ReviseOntologyArguments,
};
use super::plugins::{
    PluginCatalogInspectArguments, PluginCatalogSearchArguments, PluginRegistryArguments,
};
use super::privileged_management::{
    AcceptPlatformRolePolicyArguments, AcceptTrustDomainRevisionArguments,
    AcceptWorkloadIdentityPolicyRevisionArguments, ApproveTenantSupportGrantArguments,
    ChangePlatformRoleBindingArguments, CreatePlatformRoleBindingArguments,
    PlatformRoleBindingArguments, PlatformRolePolicyRevisionArguments,
    PrincipalPlatformRoleBindingArguments, ProposeTenantSupportGrantArguments,
    RevokePlatformRoleBindingArguments, RevokeTenantSupportGrantArguments,
    TenantSupportGrantArguments, TrustDomainArguments, TrustDomainRevisionArguments,
    TrustDomainRevisionListArguments, WorkloadIdentityPolicyArguments,
    WorkloadIdentityPolicyForWorkloadArguments, WorkloadIdentityPolicyRevisionArguments,
    WorkloadIdentityPolicyRevisionListArguments,
};
use super::projects::{
    CreateEnvironmentArguments, CreateProjectArguments, GetProjectAttributionArguments,
    ProjectArguments, UpdateProjectAttributionArguments,
};
use super::search::SearchArguments;
use super::security::SecurityGatewayRoutePolicyTimelineArguments;
use super::sources::{
    GithubInstallationRepositoriesArguments, GithubRepositoryReferencesArguments,
};
use super::workflow::{
    CancelWorkflowRunArguments, CreateWorkflowDefinitionArguments, CreateWorkflowGoalArguments,
    HumanTaskArguments, HumanTaskMutationArguments, HumanTaskSubmissionArguments,
    ListHumanTasksArguments, ListProjectWorkflowArguments, ListWorkflowRunsArguments,
    ReviseWorkflowDefinitionArguments, StartWorkflowRunArguments, WaitWorkflowRunArguments,
    WorkflowDefinitionArguments, WorkflowGoalArguments, WorkflowPlanRevisionArguments,
    WorkflowRevisionArguments, WorkflowRunArguments, WorkflowRunHistoryArguments,
};
use super::workloads::{
    CancelDeploymentArguments, RollbackWorkloadArguments, StopWorkloadArguments,
};
use super::{
    applications, artifacts, audit, connectors, developer_workflows, durable_cells, edge,
    execution_templates, files, forms, identity, nodes, notifications, ontology, operations,
    plugins, privileged_management, projects, search, security, sources, workflow, workloads,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{ApiTokenId, OrganizationId, PrincipalId};
use crate::modules::workflow::HumanTaskAssignmentAction;
use a3s_boot::{CommandBus, QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) struct ManagementExecutionContext {
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    request_id: Uuid,
    resource_access: ResourceAccessEvaluator,
}

impl ManagementExecutionContext {
    pub(super) fn new(
        organization_id: OrganizationId,
        actor_principal_id: PrincipalId,
        credential_id: ApiTokenId,
        request_id: Uuid,
        resource_access: ResourceAccessEvaluator,
    ) -> Self {
        Self {
            organization_id,
            actor_principal_id,
            credential_id,
            request_id,
            resource_access,
        }
    }
}

pub async fn execute(
    tool: ManagementTool,
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
    context: ManagementExecutionContext,
    arguments: Value,
) -> Option<Result<Value>> {
    let ManagementExecutionContext {
        organization_id,
        actor_principal_id,
        credential_id,
        request_id,
        resource_access,
    } = context;
    let result = match tool {
        ManagementTool::EnvironmentsCreate => {
            let arguments = arguments::parse::<CreateEnvironmentArguments>(arguments).ok()?;
            projects::create_environment(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::EnvironmentsList => {
            let arguments = arguments::parse::<ProjectArguments>(arguments).ok()?;
            projects::list_environments(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationsCreate => {
            let arguments = arguments::parse::<CreateApplicationArguments>(arguments).ok()?;
            applications::create(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationReleasesPublish => {
            let arguments =
                arguments::parse::<PublishApplicationReleaseArguments>(arguments).ok()?;
            applications::publish_release(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationsList => {
            let arguments = arguments::parse::<ListApplicationsArguments>(arguments).ok()?;
            applications::list(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationsGet => {
            let arguments = arguments::parse::<ApplicationArguments>(arguments).ok()?;
            applications::get(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationReleasesList => {
            let arguments = arguments::parse::<ListApplicationReleasesArguments>(arguments).ok()?;
            applications::list_releases(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationReleasesGet => {
            let arguments = arguments::parse::<ApplicationReleaseArguments>(arguments).ok()?;
            applications::get_release(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationSessionsOpen => {
            let arguments = arguments::parse::<OpenApplicationSessionArguments>(arguments).ok()?;
            applications::open_session(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationSessionsGet => {
            let arguments = arguments::parse::<ApplicationSessionArguments>(arguments).ok()?;
            applications::get_session(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationSessionsClose => {
            let arguments = arguments::parse::<CloseApplicationSessionArguments>(arguments).ok()?;
            applications::close_session(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationSessionsReplay => {
            let arguments = arguments::parse::<ListApplicationMessagesArguments>(arguments).ok()?;
            applications::replay_session(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationInvocationsRequest => {
            let arguments =
                arguments::parse::<RequestApplicationInvocationArguments>(arguments).ok()?;
            applications::request_invocation(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationInvocationsGet => {
            let arguments = arguments::parse::<ApplicationInvocationArguments>(arguments).ok()?;
            applications::get_invocation(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationInvocationsCancel => {
            let arguments =
                arguments::parse::<CancelApplicationInvocationArguments>(arguments).ok()?;
            applications::cancel_invocation(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ApplicationMessagesList => {
            let arguments = arguments::parse::<ListApplicationMessagesArguments>(arguments).ok()?;
            applications::list_messages(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ExecutionTemplatesCreate => {
            let arguments = arguments::parse::<CreateExecutionTemplateArguments>(arguments).ok()?;
            execution_templates::create(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::ExecutionTemplatesList => {
            let arguments = arguments::parse::<ListExecutionTemplatesArguments>(arguments).ok()?;
            execution_templates::list(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ExecutionTemplatesGet => {
            let arguments = arguments::parse::<GetExecutionTemplateArguments>(arguments).ok()?;
            execution_templates::get(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ConnectorProfilesCreate => {
            let arguments = arguments::parse::<CreateConnectorProfileArguments>(arguments).ok()?;
            connectors::create_profile(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ConnectorProfilesRevise => {
            let arguments = arguments::parse::<ReviseConnectorProfileArguments>(arguments).ok()?;
            connectors::revise_profile(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ConnectorProfilesList => {
            let arguments = arguments::parse::<ListConnectorProfilesArguments>(arguments).ok()?;
            connectors::list_profiles(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ConnectorProfilesGet => {
            let arguments = arguments::parse::<ConnectorProfileArguments>(arguments).ok()?;
            connectors::get_profile(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ConnectorRevisionsList => {
            let arguments = arguments::parse::<ListConnectorRevisionsArguments>(arguments).ok()?;
            connectors::list_revisions(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ConnectorRevisionsGet => {
            let arguments = arguments::parse::<ConnectorRevisionArguments>(arguments).ok()?;
            connectors::get_revision(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsCreate => {
            let arguments =
                arguments::parse::<CreateDurableCellApplicationArguments>(arguments).ok()?;
            durable_cells::create_application(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsRevise => {
            let arguments =
                arguments::parse::<ReviseDurableCellApplicationArguments>(arguments).ok()?;
            durable_cells::revise_application(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsStart => {
            let arguments =
                arguments::parse::<SetDurableCellApplicationStateArguments>(arguments).ok()?;
            durable_cells::set_application_state(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
                true,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsStop => {
            let arguments =
                arguments::parse::<SetDurableCellApplicationStateArguments>(arguments).ok()?;
            durable_cells::set_application_state(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
                false,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsList => {
            let arguments =
                arguments::parse::<ListDurableCellApplicationsArguments>(arguments).ok()?;
            durable_cells::list_applications(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellApplicationsGet => {
            let arguments = arguments::parse::<DurableCellApplicationArguments>(arguments).ok()?;
            durable_cells::get_application(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellRevisionsList => {
            let arguments =
                arguments::parse::<ListDurableCellApplicationRevisionsArguments>(arguments).ok()?;
            durable_cells::list_revisions(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellRevisionsGet => {
            let arguments =
                arguments::parse::<DurableCellApplicationRevisionArguments>(arguments).ok()?;
            durable_cells::get_revision(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellDeploymentsCreate => {
            let arguments =
                arguments::parse::<DeployDurableCellApplicationArguments>(arguments).ok()?;
            durable_cells::deploy_application(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DurableCellRoutesPublish => {
            let arguments =
                arguments::parse::<PublishDurableCellApplicationRouteArguments>(arguments).ok()?;
            durable_cells::publish_route(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRolePolicyCurrentGet => {
            arguments::parse::<EmptyArguments>(arguments).ok()?;
            privileged_management::get_current_platform_role_policy(
                query_bus,
                actor_principal_id,
                credential_id,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRolePolicyRevisionsGet => {
            let arguments =
                arguments::parse::<PlatformRolePolicyRevisionArguments>(arguments).ok()?;
            privileged_management::get_platform_role_policy_revision(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRolePolicyRevisionsAccept => {
            let arguments =
                arguments::parse::<AcceptPlatformRolePolicyArguments>(arguments).ok()?;
            privileged_management::accept_platform_role_policy(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRoleBindingsGet => {
            let arguments = arguments::parse::<PlatformRoleBindingArguments>(arguments).ok()?;
            privileged_management::get_platform_role_binding(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PrincipalPlatformRoleBindingGet => {
            let arguments =
                arguments::parse::<PrincipalPlatformRoleBindingArguments>(arguments).ok()?;
            privileged_management::get_principal_platform_role_binding(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRoleBindingsCreate => {
            let arguments =
                arguments::parse::<CreatePlatformRoleBindingArguments>(arguments).ok()?;
            privileged_management::create_platform_role_binding(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRoleBindingsChangeRole => {
            let arguments =
                arguments::parse::<ChangePlatformRoleBindingArguments>(arguments).ok()?;
            privileged_management::change_platform_role_binding(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PlatformRoleBindingsRevoke => {
            let arguments =
                arguments::parse::<RevokePlatformRoleBindingArguments>(arguments).ok()?;
            privileged_management::revoke_platform_role_binding(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TenantSupportGrantsGet => {
            let arguments = arguments::parse::<TenantSupportGrantArguments>(arguments).ok()?;
            privileged_management::get_tenant_support_grant(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TenantSupportGrantsPropose => {
            let arguments =
                arguments::parse::<ProposeTenantSupportGrantArguments>(arguments).ok()?;
            privileged_management::propose_tenant_support_grant(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TenantSupportGrantsApprove => {
            let arguments =
                arguments::parse::<ApproveTenantSupportGrantArguments>(arguments).ok()?;
            privileged_management::approve_tenant_support_grant(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TenantSupportGrantsRevoke => {
            let arguments =
                arguments::parse::<RevokeTenantSupportGrantArguments>(arguments).ok()?;
            privileged_management::revoke_tenant_support_grant(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TrustDomainsCurrentGet => {
            let arguments = arguments::parse::<TrustDomainArguments>(arguments).ok()?;
            privileged_management::get_current_trust_domain(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TrustDomainProviderInspect => {
            let arguments = arguments::parse::<TrustDomainArguments>(arguments).ok()?;
            privileged_management::inspect_current_trust_domain_provider(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TrustDomainRevisionsList => {
            let arguments = arguments::parse::<TrustDomainRevisionListArguments>(arguments).ok()?;
            privileged_management::list_trust_domain_revisions(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TrustDomainRevisionsGet => {
            let arguments = arguments::parse::<TrustDomainRevisionArguments>(arguments).ok()?;
            privileged_management::get_trust_domain_revision(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::TrustDomainRevisionsAccept => {
            let arguments =
                arguments::parse::<AcceptTrustDomainRevisionArguments>(arguments).ok()?;
            privileged_management::accept_trust_domain_revision(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadIdentityPoliciesCurrentGet => {
            let arguments = arguments::parse::<WorkloadIdentityPolicyArguments>(arguments).ok()?;
            privileged_management::get_current_workload_identity_policy(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadIdentityPolicyRevisionsList => {
            let arguments =
                arguments::parse::<WorkloadIdentityPolicyRevisionListArguments>(arguments).ok()?;
            privileged_management::list_workload_identity_policy_revisions(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadIdentityPolicyRevisionsGet => {
            let arguments =
                arguments::parse::<WorkloadIdentityPolicyRevisionArguments>(arguments).ok()?;
            privileged_management::get_workload_identity_policy_revision(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadIdentityPolicyRevisionsAccept => {
            let arguments =
                arguments::parse::<AcceptWorkloadIdentityPolicyRevisionArguments>(arguments)
                    .ok()?;
            privileged_management::accept_workload_identity_policy_revision(
                command_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadIdentityPolicyForWorkloadGet => {
            let arguments =
                arguments::parse::<WorkloadIdentityPolicyForWorkloadArguments>(arguments).ok()?;
            privileged_management::get_current_workload_identity_policy_for_workload(
                query_bus,
                actor_principal_id,
                credential_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            identity::list_memberships(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::MembershipsGet => {
            let arguments = arguments::parse::<MembershipArguments>(arguments).ok()?;
            identity::get_membership(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::MembershipsCreate => {
            let arguments = arguments::parse::<CreateMembershipArguments>(arguments).ok()?;
            identity::create_membership(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipsChangeRole => {
            let arguments = arguments::parse::<ChangeMembershipRoleArguments>(arguments).ok()?;
            identity::change_membership_role(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipsRevoke => {
            let arguments = arguments::parse::<RevokeMembershipArguments>(arguments).ok()?;
            identity::revoke_membership(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipInvitationsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            identity::list_membership_invitations(query_bus, organization_id, arguments, request_id)
                .await
        }
        ManagementTool::MembershipInvitationsGet => {
            let arguments = arguments::parse::<MembershipInvitationArguments>(arguments).ok()?;
            identity::get_membership_invitation(query_bus, organization_id, arguments, request_id)
                .await
        }
        ManagementTool::MembershipInvitationsCreate => {
            let arguments =
                arguments::parse::<CreateMembershipInvitationArguments>(arguments).ok()?;
            identity::create_membership_invitation(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipInvitationsRevoke => {
            let arguments =
                arguments::parse::<MembershipInvitationMutationArguments>(arguments).ok()?;
            identity::revoke_membership_invitation(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MyMembershipInvitationsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            identity::list_my_membership_invitations(
                query_bus,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipInvitationsAccept => {
            let arguments =
                arguments::parse::<MembershipInvitationMutationArguments>(arguments).ok()?;
            identity::accept_membership_invitation(
                command_bus,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::ResourceGrantsList => {
            let arguments = arguments::parse::<ListResourceGrantsArguments>(arguments).ok()?;
            identity::list_resource_grants(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ResourceGrantsGet => {
            let arguments = arguments::parse::<ResourceGrantArguments>(arguments).ok()?;
            identity::get_resource_grant(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ResourceGrantsCreate => {
            let arguments = arguments::parse::<CreateResourceGrantArguments>(arguments).ok()?;
            identity::create_resource_grant(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::ResourceGrantsRevoke => {
            let arguments = arguments::parse::<RevokeResourceGrantArguments>(arguments).ok()?;
            identity::revoke_resource_grant(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::RecipientContactsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            identity::list_recipient_contacts(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::RecipientContactsGet => {
            let arguments = arguments::parse::<RecipientContactArguments>(arguments).ok()?;
            identity::get_recipient_contact(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::RecipientContactsRevoke => {
            let arguments = arguments::parse::<RevokeRecipientContactArguments>(arguments).ok()?;
            identity::revoke_recipient_contact(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::ProjectsCreate => {
            let arguments = arguments::parse::<CreateProjectArguments>(arguments).ok()?;
            projects::create_project(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ProjectsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            projects::list_projects(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ProjectAttributionGet => {
            let arguments = arguments::parse::<GetProjectAttributionArguments>(arguments).ok()?;
            projects::get_project_attribution(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::ProjectAttributionUpdate => {
            let arguments =
                arguments::parse::<UpdateProjectAttributionArguments>(arguments).ok()?;
            projects::update_project_attribution(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::FormsCreate => {
            let arguments = arguments::parse::<CreateFormDraftArguments>(arguments).ok()?;
            forms::create_draft(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::FormsRevise => {
            let arguments = arguments::parse::<ReviseFormDraftArguments>(arguments).ok()?;
            forms::revise_draft(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::FormsList => {
            let arguments = arguments::parse::<ListFormDraftsArguments>(arguments).ok()?;
            forms::list_drafts(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormsGet => {
            let arguments = arguments::parse::<FormDraftArguments>(arguments).ok()?;
            forms::get_draft(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::FormReleasesList => {
            let arguments = arguments::parse::<FormDraftArguments>(arguments).ok()?;
            forms::list_releases(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::FormReleasesGet => {
            let arguments = arguments::parse::<FormReleaseArguments>(arguments).ok()?;
            forms::get_release(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::FormReleasesPublish => {
            let arguments = arguments::parse::<PublishFormReleaseArguments>(arguments).ok()?;
            forms::publish_release(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesCreate => {
            let arguments = arguments::parse::<CreateOntologyArguments>(arguments).ok()?;
            ontology::create_ontology(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesRevise => {
            let arguments = arguments::parse::<ReviseOntologyArguments>(arguments).ok()?;
            ontology::revise_ontology(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesList => {
            let arguments = arguments::parse::<ListOntologiesArguments>(arguments).ok()?;
            ontology::list_ontologies(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OntologiesGet => {
            let arguments = arguments::parse::<OntologyArguments>(arguments).ok()?;
            ontology::get_ontology(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::OntologyRevisionsList => {
            let arguments = arguments::parse::<OntologyArguments>(arguments).ok()?;
            ontology::list_revisions(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::OntologyRevisionsGet => {
            let arguments = arguments::parse::<OntologyRevisionArguments>(arguments).ok()?;
            ontology::get_revision(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::OntologyRevisionsDiff => {
            let arguments = arguments::parse::<OntologyDiffArguments>(arguments).ok()?;
            ontology::diff_revisions(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowDefinitionsCreate => {
            let arguments =
                arguments::parse::<CreateWorkflowDefinitionArguments>(arguments).ok()?;
            workflow::create_definition(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowNodeCatalogGet => {
            let arguments = arguments::parse::<ListProjectWorkflowArguments>(arguments).ok()?;
            workflow::get_node_catalog(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowDefinitionsRevise => {
            let arguments =
                arguments::parse::<ReviseWorkflowDefinitionArguments>(arguments).ok()?;
            workflow::revise_definition(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowDefinitionsList => {
            let arguments = arguments::parse::<ListProjectWorkflowArguments>(arguments).ok()?;
            workflow::list_definitions(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowDefinitionsGet => {
            let arguments = arguments::parse::<WorkflowDefinitionArguments>(arguments).ok()?;
            workflow::get_definition(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRevisionsList => {
            let arguments = arguments::parse::<WorkflowDefinitionArguments>(arguments).ok()?;
            workflow::list_revisions(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRevisionsGet => {
            let arguments = arguments::parse::<WorkflowRevisionArguments>(arguments).ok()?;
            workflow::get_revision(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowGoalsCreate => {
            let arguments = arguments::parse::<CreateWorkflowGoalArguments>(arguments).ok()?;
            workflow::create_goal(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowGoalsList => {
            let arguments = arguments::parse::<ListProjectWorkflowArguments>(arguments).ok()?;
            workflow::list_goals(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowGoalsGet => {
            let arguments = arguments::parse::<WorkflowGoalArguments>(arguments).ok()?;
            workflow::get_goal(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowPlanRevisionsGet => {
            let arguments = arguments::parse::<WorkflowPlanRevisionArguments>(arguments).ok()?;
            workflow::get_plan_revision(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunsStart => {
            let arguments = arguments::parse::<StartWorkflowRunArguments>(arguments).ok()?;
            workflow::start_run(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunsCancel => {
            let arguments = arguments::parse::<CancelWorkflowRunArguments>(arguments).ok()?;
            workflow::cancel_run(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunsList => {
            let arguments = arguments::parse::<ListWorkflowRunsArguments>(arguments).ok()?;
            workflow::list_runs(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowRunsGet => {
            let arguments = arguments::parse::<WorkflowRunArguments>(arguments).ok()?;
            workflow::get_run(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunsWait => {
            let arguments = arguments::parse::<WaitWorkflowRunArguments>(arguments).ok()?;
            workflow::wait_run(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunOutputGet => {
            let arguments = arguments::parse::<WorkflowRunArguments>(arguments).ok()?;
            workflow::get_run_output(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunDiagnosticsGet => {
            let arguments = arguments::parse::<WorkflowRunArguments>(arguments).ok()?;
            workflow::get_run_diagnostics(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunHistoryGet => {
            let arguments = arguments::parse::<WorkflowRunHistoryArguments>(arguments).ok()?;
            workflow::get_run_history(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowRunVariablesGet => {
            let arguments = arguments::parse::<WorkflowRunArguments>(arguments).ok()?;
            workflow::get_run_variables(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::HumanTasksGet => {
            let arguments = arguments::parse::<HumanTaskArguments>(arguments).ok()?;
            workflow::get_human_task(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::HumanTasksClaim => {
            let arguments = arguments::parse::<HumanTaskMutationArguments>(arguments).ok()?;
            workflow::change_human_task_assignment(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                HumanTaskAssignmentAction::Claim,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::HumanTasksRelease => {
            let arguments = arguments::parse::<HumanTaskMutationArguments>(arguments).ok()?;
            workflow::change_human_task_assignment(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                HumanTaskAssignmentAction::Release,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::HumanTasksSubmit => {
            let arguments = arguments::parse::<HumanTaskSubmissionArguments>(arguments).ok()?;
            workflow::submit_human_task(
                command_bus,
                organization_id,
                actor_principal_id,
                credential_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::HumanTasksList => {
            let arguments = arguments::parse::<ListHumanTasksArguments>(arguments).ok()?;
            workflow::list_human_tasks(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::Search => {
            let arguments = arguments::parse::<SearchArguments>(arguments).ok()?;
            search::search(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::GithubInstallationRepositoriesList => {
            let arguments =
                arguments::parse::<GithubInstallationRepositoriesArguments>(arguments).ok()?;
            sources::list_installation_repositories(
                query_bus,
                organization_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::GithubRepositoryReferencesList => {
            let arguments =
                arguments::parse::<GithubRepositoryReferencesArguments>(arguments).ok()?;
            sources::list_repository_references(query_bus, organization_id, arguments, request_id)
                .await
        }
        ManagementTool::UserFilesReserve => {
            let arguments = arguments::parse::<ReserveUserFileArguments>(arguments).ok()?;
            files::reserve(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::UserFilesList => {
            let arguments = arguments::parse::<ListUserFilesArguments>(arguments).ok()?;
            files::list(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::UserFilesGet => {
            let arguments = arguments::parse::<UserFileArguments>(arguments).ok()?;
            files::get(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::UserFilesTombstone => {
            let arguments = arguments::parse::<TombstoneUserFileArguments>(arguments).ok()?;
            files::tombstone(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::UserFileQuotaGet => {
            let _arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            files::quota(query_bus, organization_id, resource_access, request_id).await
        }
        ManagementTool::PluginRegistriesList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            plugins::list_registries(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::PluginRegistriesGet => {
            let arguments = arguments::parse::<PluginRegistryArguments>(arguments).ok()?;
            plugins::get_registry(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::PluginCatalogSearch => {
            let arguments = arguments::parse::<PluginCatalogSearchArguments>(arguments).ok()?;
            plugins::search_catalog(query_bus, organization_id, arguments, request_id, false).await
        }
        ManagementTool::PluginCatalogSearchCached => {
            let arguments = arguments::parse::<PluginCatalogSearchArguments>(arguments).ok()?;
            plugins::search_catalog(query_bus, organization_id, arguments, request_id, true).await
        }
        ManagementTool::PluginCatalogInspect => {
            let arguments = arguments::parse::<PluginCatalogInspectArguments>(arguments).ok()?;
            plugins::inspect_catalog(query_bus, organization_id, arguments, request_id, false).await
        }
        ManagementTool::PluginCatalogInspectCached => {
            let arguments = arguments::parse::<PluginCatalogInspectArguments>(arguments).ok()?;
            plugins::inspect_catalog(query_bus, organization_id, arguments, request_id, true).await
        }
        ManagementTool::NodesList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            nodes::list_nodes(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NodesGet => {
            let arguments = arguments::parse::<NodeArguments>(arguments).ok()?;
            nodes::get_node(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OperationsList => {
            let arguments = arguments::parse::<OperationListArguments>(arguments).ok()?;
            operations::list_operations(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::AuditRecordsList => {
            let arguments = arguments::parse::<AuditRecordListArguments>(arguments).ok()?;
            audit::list_audit_records(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::AuditRecordsExport => {
            let arguments = arguments::parse::<AuditRecordExportArguments>(arguments).ok()?;
            audit::export_audit_records(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::AuditRecordsExportManifest => {
            let arguments =
                arguments::parse::<AuditRecordManifestExportArguments>(arguments).ok()?;
            audit::export_audit_manifest(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::AuditRetentionGet => {
            arguments::parse::<EmptyArguments>(arguments).ok()?;
            audit::get_audit_retention_status(query_bus, organization_id, request_id).await
        }
        ManagementTool::SecurityGatewayRoutePolicyTimelineList => {
            let arguments =
                arguments::parse::<SecurityGatewayRoutePolicyTimelineArguments>(arguments).ok()?;
            security::list_gateway_route_policy_timeline(
                query_bus,
                organization_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationsList => {
            let arguments = arguments::parse::<NotificationListArguments>(arguments).ok()?;
            notifications::list(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationsGet => {
            let arguments = arguments::parse::<NotificationArguments>(arguments).ok()?;
            notifications::get(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationsRead => {
            let arguments = arguments::parse::<MarkNotificationReadArguments>(arguments).ok()?;
            notifications::mark_read(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationAlertPoliciesList => {
            let arguments =
                arguments::parse::<NotificationAlertPolicyListArguments>(arguments).ok()?;
            notifications::list_alert_policies(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationAlertPoliciesGet => {
            let arguments = arguments::parse::<NotificationAlertPolicyArguments>(arguments).ok()?;
            notifications::get_alert_policy(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationAlertPoliciesCreate => {
            let arguments =
                arguments::parse::<CreateNotificationAlertPolicyArguments>(arguments).ok()?;
            notifications::create_alert_policy(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationAlertPoliciesRevoke => {
            let arguments =
                arguments::parse::<RevokeNotificationAlertPolicyArguments>(arguments).ok()?;
            notifications::revoke_alert_policy(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationOutboundSubscriptionsList => {
            let arguments =
                arguments::parse::<OutboundNotificationSubscriptionListArguments>(arguments)
                    .ok()?;
            notifications::list_outbound_subscriptions(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationOutboundSubscriptionsGet => {
            let arguments =
                arguments::parse::<OutboundNotificationSubscriptionArguments>(arguments).ok()?;
            notifications::get_outbound_subscription(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationOutboundSubscriptionsCreate => {
            let arguments =
                arguments::parse::<CreateOutboundNotificationSubscriptionArguments>(arguments)
                    .ok()?;
            notifications::create_outbound_subscription(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::NotificationOutboundSubscriptionsRevoke => {
            let arguments =
                arguments::parse::<RevokeOutboundNotificationSubscriptionArguments>(arguments)
                    .ok()?;
            notifications::revoke_outbound_subscription(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadsList => {
            let arguments = arguments::parse::<EnvironmentScopeArguments>(arguments).ok()?;
            workloads::list_workloads(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadsGet => {
            let arguments = arguments::parse::<WorkloadArguments>(arguments).ok()?;
            workloads::get_workload(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadLogsGet => {
            let arguments = arguments::parse::<WorkloadLogArguments>(arguments).ok()?;
            workloads::get_workload_logs(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadsStop => {
            let arguments = arguments::parse::<StopWorkloadArguments>(arguments).ok()?;
            workloads::stop_workload(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadsRollback => {
            let arguments = arguments::parse::<RollbackWorkloadArguments>(arguments).ok()?;
            workloads::rollback_workload(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DeploymentsGet => {
            let arguments = arguments::parse::<DeploymentArguments>(arguments).ok()?;
            workloads::get_deployment(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::DeploymentsCancel => {
            let arguments = arguments::parse::<CancelDeploymentArguments>(arguments).ok()?;
            workloads::cancel_deployment(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::RoutesList => {
            let arguments = arguments::parse::<EnvironmentScopeArguments>(arguments).ok()?;
            edge::list_routes(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::RoutesGet => {
            let arguments = arguments::parse::<RouteArguments>(arguments).ok()?;
            edge::get_route(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::BuildPlanDetectionsCreate => {
            let arguments = arguments::parse::<DetectBuildPlansArguments>(arguments).ok()?;
            developer_workflows::detect_build_plans(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::BuildPlansAccept => {
            let arguments = arguments::parse::<AcceptBuildPlanArguments>(arguments).ok()?;
            developer_workflows::accept_build_plan(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::BuildPlansList => {
            let arguments = arguments::parse::<ListAcceptedBuildPlansArguments>(arguments).ok()?;
            developer_workflows::list_build_plans(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::BuildPlansGet => {
            let arguments = arguments::parse::<GetAcceptedBuildPlanArguments>(arguments).ok()?;
            developer_workflows::get_build_plan(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadProfilesAccept => {
            let arguments = arguments::parse::<AcceptWorkloadProfileArguments>(arguments).ok()?;
            developer_workflows::accept_workload_profile(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadProfilesGet => {
            let arguments =
                arguments::parse::<GetCurrentAcceptedWorkloadProfileRevisionArguments>(arguments)
                    .ok()?;
            developer_workflows::get_current_workload_profile_revision(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadProfileRevisionsList => {
            let arguments =
                arguments::parse::<ListAcceptedWorkloadProfileRevisionsArguments>(arguments)
                    .ok()?;
            developer_workflows::list_workload_profile_revisions(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkloadProfileRevisionsGet => {
            let arguments =
                arguments::parse::<GetAcceptedWorkloadProfileRevisionArguments>(arguments).ok()?;
            developer_workflows::get_workload_profile_revision(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PullRequestPreviewPoliciesAccept => {
            let arguments =
                arguments::parse::<AcceptPullRequestPreviewPolicyArguments>(arguments).ok()?;
            developer_workflows::accept_pull_request_preview_policy(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PullRequestPreviewPoliciesGet => {
            let arguments = arguments::parse::<
                GetCurrentAcceptedPullRequestPreviewPolicyRevisionArguments,
            >(arguments)
            .ok()?;
            developer_workflows::get_current_pull_request_preview_policy_revision(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PullRequestPreviewPolicyRevisionsList => {
            let arguments = arguments::parse::<
                ListAcceptedPullRequestPreviewPolicyRevisionsArguments,
            >(arguments)
            .ok()?;
            developer_workflows::list_pull_request_preview_policy_revisions(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PullRequestPreviewPolicyRevisionsGet => {
            let arguments =
                arguments::parse::<GetAcceptedPullRequestPreviewPolicyRevisionArguments>(arguments)
                    .ok()?;
            developer_workflows::get_pull_request_preview_policy_revision(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::PullRequestPreviewsGet => {
            let arguments = arguments::parse::<GetPullRequestPreviewArguments>(arguments).ok()?;
            developer_workflows::get_pull_request_preview(
                query_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::BuildRunsList => {
            let arguments = arguments::parse::<BuildRunListArguments>(arguments).ok()?;
            artifacts::list_build_runs(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunsGet => {
            let arguments = arguments::parse::<BuildRunArguments>(arguments).ok()?;
            artifacts::get_build_run(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::BuildRunLogsGet => {
            let arguments = arguments::parse::<BuildRunLogArguments>(arguments).ok()?;
            artifacts::get_build_run_logs(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::BuildEvidenceGet => {
            let arguments = arguments::parse::<BuildRunArguments>(arguments).ok()?;
            artifacts::get_build_evidence(
                query_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::BuildRunsCancel => {
            let arguments = arguments::parse::<BuildRunMutationArguments>(arguments).ok()?;
            artifacts::cancel_build_run(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
        ManagementTool::BuildRunsRetry => {
            let arguments = arguments::parse::<BuildRunMutationArguments>(arguments).ok()?;
            artifacts::retry_build_run(
                command_bus,
                organization_id,
                arguments,
                resource_access,
                request_id,
            )
            .await
        }
    };
    Some(result)
}
