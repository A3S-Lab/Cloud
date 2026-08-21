use super::arguments::{DEFAULT_LOG_LIMIT, MAXIMUM_IDEMPOTENCY_KEY_LENGTH, MAXIMUM_LOG_LIMIT};
use crate::modules::applications::{
    APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES, APPLICATION_DESCRIPTION_MAX_CHARS,
    APPLICATION_INVOCATION_INPUT_MAX_BYTES, APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES,
    DEFAULT_APPLICATION_LIST_LIMIT, DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT,
    MAXIMUM_APPLICATION_LIST_LIMIT, MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
};
use crate::modules::connectors::{
    CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES, DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT,
    MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
};
use crate::modules::data::OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES;
use crate::modules::durable_cells::domain::{
    DURABLE_CELL_APPLICATION_MAX_ACL_BYTES, DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES,
    DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES,
};
use crate::modules::durable_cells::{
    DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT, MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
};
use crate::modules::executions::EXECUTION_TEMPLATE_MAX_ACL_BYTES;
use crate::modules::forms::presentation::form_interaction_submission_schema;
use crate::modules::forms::CLOUD_FORM_DOCUMENT_MAX_BYTES;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::resource_access_evaluator;
use crate::modules::notifications::{
    DEFAULT_NOTIFICATION_LIMIT, MAXIMUM_NOTIFICATION_LIMIT,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES,
};
use crate::modules::projects::domain::value_objects::{
    BUSINESS_OWNER_REFERENCE_MAX_CHARS, COST_ATTRIBUTION_CODE_MAX_CHARS,
    PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS, PROJECT_ATTRIBUTION_LABEL_MAX_COUNT,
    PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS,
};
use crate::modules::workflow::{
    WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS, WORKFLOW_RUN_MAX_TIMEOUT_SECONDS,
};
use crate::modules::workloads::presentation::WORKLOAD_MANIFEST_MAX_BYTES;
use a3s_boot::AuthPrincipal;
use a3s_use_extension::{
    plugin_catalog_host_input_schema, plugin_catalog_inspection_input_schema,
    plugin_catalog_search_input_schema,
};
use serde_json::{json, Value};

pub const BUILD_RUNS_GET: &str = "a3s_cloud_build_runs_get";
pub const BUILD_RUNS_LIST: &str = "a3s_cloud_build_runs_list";
pub const BUILD_RUNS_CANCEL: &str = "a3s_cloud_build_runs_cancel";
pub const BUILD_RUNS_RETRY: &str = "a3s_cloud_build_runs_retry";
pub const BUILD_RUN_LOGS_GET: &str = "a3s_cloud_build_run_logs_get";
pub const BUILD_EVIDENCE_GET: &str = "a3s_cloud_build_evidence_get";
pub const DEPLOYMENTS_CANCEL: &str = "a3s_cloud_deployments_cancel";
pub const DEPLOYMENTS_GET: &str = "a3s_cloud_deployments_get";
pub const ENVIRONMENTS_CREATE: &str = "a3s_cloud_environments_create";
pub const ENVIRONMENTS_LIST: &str = "a3s_cloud_environments_list";
pub const APPLICATIONS_CREATE: &str = "a3s_cloud_applications_create";
pub const APPLICATIONS_LIST: &str = "a3s_cloud_applications_list";
pub const APPLICATIONS_GET: &str = "a3s_cloud_applications_get";
pub const APPLICATION_RELEASES_PUBLISH: &str = "a3s_cloud_application_releases_publish";
pub const APPLICATION_RELEASES_LIST: &str = "a3s_cloud_application_releases_list";
pub const APPLICATION_RELEASES_GET: &str = "a3s_cloud_application_releases_get";
pub const APPLICATION_SESSIONS_OPEN: &str = "a3s_cloud_application_sessions_open";
pub const APPLICATION_SESSIONS_GET: &str = "a3s_cloud_application_sessions_get";
pub const APPLICATION_SESSIONS_CLOSE: &str = "a3s_cloud_application_sessions_close";
pub const APPLICATION_SESSIONS_REPLAY: &str = "a3s_cloud_application_sessions_replay";
pub const APPLICATION_INVOCATIONS_REQUEST: &str = "a3s_cloud_application_invocations_request";
pub const APPLICATION_INVOCATIONS_GET: &str = "a3s_cloud_application_invocations_get";
pub const APPLICATION_INVOCATIONS_CANCEL: &str = "a3s_cloud_application_invocations_cancel";
pub const APPLICATION_MESSAGES_LIST: &str = "a3s_cloud_application_messages_list";
pub const CONNECTOR_PROFILES_CREATE: &str = "a3s_cloud_connector_profiles_create";
pub const CONNECTOR_PROFILES_REVISE: &str = "a3s_cloud_connector_profiles_revise";
pub const CONNECTOR_PROFILES_LIST: &str = "a3s_cloud_connector_profiles_list";
pub const CONNECTOR_PROFILES_GET: &str = "a3s_cloud_connector_profiles_get";
pub const CONNECTOR_REVISIONS_LIST: &str = "a3s_cloud_connector_revisions_list";
pub const CONNECTOR_REVISIONS_GET: &str = "a3s_cloud_connector_revisions_get";
pub const DURABLE_CELL_APPLICATIONS_CREATE: &str = "a3s_cloud_durable_cell_applications_create";
pub const DURABLE_CELL_APPLICATIONS_REVISE: &str = "a3s_cloud_durable_cell_applications_revise";
pub const DURABLE_CELL_APPLICATIONS_START: &str = "a3s_cloud_durable_cell_applications_start";
pub const DURABLE_CELL_APPLICATIONS_STOP: &str = "a3s_cloud_durable_cell_applications_stop";
pub const DURABLE_CELL_APPLICATIONS_LIST: &str = "a3s_cloud_durable_cell_applications_list";
pub const DURABLE_CELL_APPLICATIONS_GET: &str = "a3s_cloud_durable_cell_applications_get";
pub const DURABLE_CELL_REVISIONS_LIST: &str = "a3s_cloud_durable_cell_revisions_list";
pub const DURABLE_CELL_REVISIONS_GET: &str = "a3s_cloud_durable_cell_revisions_get";
pub const DURABLE_CELL_DEPLOYMENTS_CREATE: &str = "a3s_cloud_durable_cell_deployments_create";
pub const DURABLE_CELL_ROUTES_PUBLISH: &str = "a3s_cloud_durable_cell_routes_publish";
pub const EXECUTION_TEMPLATES_CREATE: &str = "a3s_cloud_execution_templates_create";
pub const EXECUTION_TEMPLATES_GET: &str = "a3s_cloud_execution_templates_get";
pub const EXECUTION_TEMPLATES_LIST: &str = "a3s_cloud_execution_templates_list";
pub const FORMS_CREATE: &str = "a3s_cloud_forms_create";
pub const FORMS_GET: &str = "a3s_cloud_forms_get";
pub const FORMS_LIST: &str = "a3s_cloud_forms_list";
pub const FORMS_REVISE: &str = "a3s_cloud_forms_revise";
pub const FORM_RELEASES_GET: &str = "a3s_cloud_form_releases_get";
pub const FORM_RELEASES_LIST: &str = "a3s_cloud_form_releases_list";
pub const FORM_RELEASES_PUBLISH: &str = "a3s_cloud_form_releases_publish";
pub const MEMBERSHIPS_LIST: &str = "a3s_cloud_memberships_list";
pub const MEMBERSHIPS_GET: &str = "a3s_cloud_memberships_get";
pub const MEMBERSHIPS_CREATE: &str = "a3s_cloud_memberships_create";
pub const MEMBERSHIPS_CHANGE_ROLE: &str = "a3s_cloud_memberships_change_role";
pub const MEMBERSHIPS_REVOKE: &str = "a3s_cloud_memberships_revoke";
pub const MEMBERSHIP_INVITATIONS_LIST: &str = "a3s_cloud_membership_invitations_list";
pub const MEMBERSHIP_INVITATIONS_GET: &str = "a3s_cloud_membership_invitations_get";
pub const MEMBERSHIP_INVITATIONS_CREATE: &str = "a3s_cloud_membership_invitations_create";
pub const MEMBERSHIP_INVITATIONS_REVOKE: &str = "a3s_cloud_membership_invitations_revoke";
pub const MY_MEMBERSHIP_INVITATIONS_LIST: &str = "a3s_cloud_my_membership_invitations_list";
pub const MEMBERSHIP_INVITATIONS_ACCEPT: &str = "a3s_cloud_membership_invitations_accept";
pub const RESOURCE_GRANTS_LIST: &str = "a3s_cloud_resource_grants_list";
pub const RESOURCE_GRANTS_GET: &str = "a3s_cloud_resource_grants_get";
pub const RESOURCE_GRANTS_CREATE: &str = "a3s_cloud_resource_grants_create";
pub const RESOURCE_GRANTS_REVOKE: &str = "a3s_cloud_resource_grants_revoke";
pub const NODES_GET: &str = "a3s_cloud_nodes_get";
pub const NODES_LIST: &str = "a3s_cloud_nodes_list";
pub const OPERATIONS_LIST: &str = "a3s_cloud_operations_list";
pub const AUDIT_RECORDS_LIST: &str = "a3s_cloud_audit_records_list";
pub const NOTIFICATIONS_LIST: &str = "a3s_cloud_notifications_list";
pub const NOTIFICATIONS_GET: &str = "a3s_cloud_notifications_get";
pub const NOTIFICATIONS_READ: &str = "a3s_cloud_notifications_read";
pub const NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_CREATE: &str =
    "a3s_cloud_notification_outbound_subscriptions_create";
pub const NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_LIST: &str =
    "a3s_cloud_notification_outbound_subscriptions_list";
pub const NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_GET: &str =
    "a3s_cloud_notification_outbound_subscriptions_get";
pub const NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_REVOKE: &str =
    "a3s_cloud_notification_outbound_subscriptions_revoke";
pub const PROJECTS_CREATE: &str = "a3s_cloud_projects_create";
pub const PROJECTS_LIST: &str = "a3s_cloud_projects_list";
pub const PROJECT_ATTRIBUTION_GET: &str = "a3s_cloud_project_attribution_get";
pub const PROJECT_ATTRIBUTION_UPDATE: &str = "a3s_cloud_project_attribution_update";
pub const ONTOLOGIES_CREATE: &str = "a3s_cloud_ontologies_create";
pub const ONTOLOGIES_GET: &str = "a3s_cloud_ontologies_get";
pub const ONTOLOGIES_LIST: &str = "a3s_cloud_ontologies_list";
pub const ONTOLOGIES_REVISE: &str = "a3s_cloud_ontologies_revise";
pub const ONTOLOGY_REVISIONS_GET: &str = "a3s_cloud_ontology_revisions_get";
pub const ONTOLOGY_REVISIONS_LIST: &str = "a3s_cloud_ontology_revisions_list";
pub const ONTOLOGY_REVISIONS_DIFF: &str = "a3s_cloud_ontology_revisions_diff";
pub const WORKFLOW_NODE_CATALOG_GET: &str = "a3s_cloud_workflow_node_catalog_get";
pub const WORKFLOW_DEFINITIONS_CREATE: &str = "a3s_cloud_workflow_definitions_create";
pub const WORKFLOW_DEFINITIONS_GET: &str = "a3s_cloud_workflow_definitions_get";
pub const WORKFLOW_DEFINITIONS_LIST: &str = "a3s_cloud_workflow_definitions_list";
pub const WORKFLOW_DEFINITIONS_REVISE: &str = "a3s_cloud_workflow_definitions_revise";
pub const WORKFLOW_REVISIONS_GET: &str = "a3s_cloud_workflow_revisions_get";
pub const WORKFLOW_REVISIONS_LIST: &str = "a3s_cloud_workflow_revisions_list";
pub const WORKFLOW_GOALS_CREATE: &str = "a3s_cloud_workflow_goals_create";
pub const WORKFLOW_GOALS_GET: &str = "a3s_cloud_workflow_goals_get";
pub const WORKFLOW_GOALS_LIST: &str = "a3s_cloud_workflow_goals_list";
pub const WORKFLOW_PLAN_REVISIONS_GET: &str = "a3s_cloud_workflow_plan_revisions_get";
pub const WORKFLOW_RUNS_START: &str = "a3s_cloud_workflow_runs_start";
pub const WORKFLOW_RUNS_CANCEL: &str = "a3s_cloud_workflow_runs_cancel";
pub const WORKFLOW_RUNS_GET: &str = "a3s_cloud_workflow_runs_get";
pub const WORKFLOW_RUNS_LIST: &str = "a3s_cloud_workflow_runs_list";
pub const WORKFLOW_RUNS_WAIT: &str = "a3s_cloud_workflow_runs_wait";
pub const WORKFLOW_RUN_OUTPUT_GET: &str = "a3s_cloud_workflow_run_output_get";
pub const WORKFLOW_RUN_HISTORY_GET: &str = "a3s_cloud_workflow_run_history_get";
pub const WORKFLOW_RUN_VARIABLES_GET: &str = "a3s_cloud_workflow_run_variables_get";
pub const HUMAN_TASKS_CLAIM: &str = "a3s_cloud_human_tasks_claim";
pub const HUMAN_TASKS_GET: &str = "a3s_cloud_human_tasks_get";
pub const HUMAN_TASKS_LIST: &str = "a3s_cloud_human_tasks_list";
pub const HUMAN_TASKS_RELEASE: &str = "a3s_cloud_human_tasks_release";
pub const HUMAN_TASKS_SUBMIT: &str = "a3s_cloud_human_tasks_submit";
pub const ROUTES_GET: &str = "a3s_cloud_routes_get";
pub const ROUTES_LIST: &str = "a3s_cloud_routes_list";
pub const SEARCH: &str = "a3s_cloud_search";
pub const PLUGIN_REGISTRIES_GET: &str = "a3s_cloud_plugin_registries_get";
pub const PLUGIN_REGISTRIES_LIST: &str = "a3s_cloud_plugin_registries_list";
pub const PLUGIN_CATALOG_INSPECT: &str = "a3s_cloud_plugin_catalog_inspect";
pub const PLUGIN_CATALOG_INSPECT_CACHED: &str = "a3s_cloud_plugin_catalog_inspect_cached";
pub const PLUGIN_CATALOG_SEARCH: &str = "a3s_cloud_plugin_catalog_search";
pub const PLUGIN_CATALOG_SEARCH_CACHED: &str = "a3s_cloud_plugin_catalog_search_cached";
pub const WORKLOADS_GET: &str = "a3s_cloud_workloads_get";
pub const WORKLOADS_LIST: &str = "a3s_cloud_workloads_list";
pub const WORKLOADS_ROLLBACK: &str = "a3s_cloud_workloads_rollback";
pub const WORKLOADS_STOP: &str = "a3s_cloud_workloads_stop";
pub const WORKLOAD_LOGS_GET: &str = "a3s_cloud_workload_logs_get";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementTool {
    EnvironmentsCreate,
    EnvironmentsList,
    ApplicationsCreate,
    ApplicationsList,
    ApplicationsGet,
    ApplicationReleasesPublish,
    ApplicationReleasesList,
    ApplicationReleasesGet,
    ApplicationSessionsOpen,
    ApplicationSessionsGet,
    ApplicationSessionsClose,
    ApplicationSessionsReplay,
    ApplicationInvocationsRequest,
    ApplicationInvocationsGet,
    ApplicationInvocationsCancel,
    ApplicationMessagesList,
    ConnectorProfilesCreate,
    ConnectorProfilesRevise,
    ConnectorProfilesList,
    ConnectorProfilesGet,
    ConnectorRevisionsList,
    ConnectorRevisionsGet,
    DurableCellApplicationsCreate,
    DurableCellApplicationsRevise,
    DurableCellApplicationsStart,
    DurableCellApplicationsStop,
    DurableCellApplicationsList,
    DurableCellApplicationsGet,
    DurableCellRevisionsList,
    DurableCellRevisionsGet,
    DurableCellDeploymentsCreate,
    DurableCellRoutesPublish,
    ExecutionTemplatesCreate,
    ExecutionTemplatesGet,
    ExecutionTemplatesList,
    MembershipsList,
    MembershipsGet,
    MembershipsCreate,
    MembershipsChangeRole,
    MembershipsRevoke,
    MembershipInvitationsList,
    MembershipInvitationsGet,
    MembershipInvitationsCreate,
    MembershipInvitationsRevoke,
    MyMembershipInvitationsList,
    MembershipInvitationsAccept,
    ResourceGrantsList,
    ResourceGrantsGet,
    ResourceGrantsCreate,
    ResourceGrantsRevoke,
    ProjectsCreate,
    ProjectsList,
    ProjectAttributionGet,
    ProjectAttributionUpdate,
    FormsCreate,
    FormsGet,
    FormsList,
    FormsRevise,
    FormReleasesGet,
    FormReleasesList,
    FormReleasesPublish,
    OntologiesCreate,
    OntologiesGet,
    OntologiesList,
    OntologiesRevise,
    OntologyRevisionsGet,
    OntologyRevisionsList,
    OntologyRevisionsDiff,
    WorkflowNodeCatalogGet,
    WorkflowDefinitionsCreate,
    WorkflowDefinitionsGet,
    WorkflowDefinitionsList,
    WorkflowDefinitionsRevise,
    WorkflowRevisionsGet,
    WorkflowRevisionsList,
    WorkflowGoalsCreate,
    WorkflowGoalsGet,
    WorkflowGoalsList,
    WorkflowPlanRevisionsGet,
    WorkflowRunsStart,
    WorkflowRunsCancel,
    WorkflowRunsGet,
    WorkflowRunsList,
    WorkflowRunsWait,
    WorkflowRunOutputGet,
    WorkflowRunHistoryGet,
    WorkflowRunVariablesGet,
    HumanTasksClaim,
    HumanTasksGet,
    HumanTasksList,
    HumanTasksRelease,
    HumanTasksSubmit,
    Search,
    PluginRegistriesList,
    PluginRegistriesGet,
    PluginCatalogSearch,
    PluginCatalogSearchCached,
    PluginCatalogInspect,
    PluginCatalogInspectCached,
    NodesList,
    NodesGet,
    OperationsList,
    AuditRecordsList,
    NotificationsList,
    NotificationsGet,
    NotificationsRead,
    NotificationOutboundSubscriptionsCreate,
    NotificationOutboundSubscriptionsList,
    NotificationOutboundSubscriptionsGet,
    NotificationOutboundSubscriptionsRevoke,
    WorkloadsList,
    WorkloadsGet,
    WorkloadLogsGet,
    WorkloadsStop,
    WorkloadsRollback,
    DeploymentsGet,
    DeploymentsCancel,
    RoutesList,
    RoutesGet,
    BuildRunsList,
    BuildRunsGet,
    BuildRunLogsGet,
    BuildEvidenceGet,
    BuildRunsCancel,
    BuildRunsRetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManagementResourceBinding {
    ProjectArgument,
    EnvironmentArguments,
    NodeArgument,
    ProjectOwnedResource,
    ProjectCollection,
    EnvironmentCollection,
    NodeCollection,
    SearchCollection,
    PolymorphicCollection,
    SelfPrincipal,
}

impl ManagementTool {
    const ALL: [Self; 125] = [
        Self::EnvironmentsCreate,
        Self::EnvironmentsList,
        Self::ApplicationsCreate,
        Self::ApplicationsList,
        Self::ApplicationsGet,
        Self::ApplicationReleasesPublish,
        Self::ApplicationReleasesList,
        Self::ApplicationReleasesGet,
        Self::ApplicationSessionsOpen,
        Self::ApplicationSessionsGet,
        Self::ApplicationSessionsClose,
        Self::ApplicationSessionsReplay,
        Self::ApplicationInvocationsRequest,
        Self::ApplicationInvocationsGet,
        Self::ApplicationInvocationsCancel,
        Self::ApplicationMessagesList,
        Self::ConnectorProfilesCreate,
        Self::ConnectorProfilesRevise,
        Self::ConnectorProfilesList,
        Self::ConnectorProfilesGet,
        Self::ConnectorRevisionsList,
        Self::ConnectorRevisionsGet,
        Self::DurableCellApplicationsCreate,
        Self::DurableCellApplicationsRevise,
        Self::DurableCellApplicationsStart,
        Self::DurableCellApplicationsStop,
        Self::DurableCellApplicationsList,
        Self::DurableCellApplicationsGet,
        Self::DurableCellRevisionsList,
        Self::DurableCellRevisionsGet,
        Self::DurableCellDeploymentsCreate,
        Self::DurableCellRoutesPublish,
        Self::ExecutionTemplatesCreate,
        Self::ExecutionTemplatesGet,
        Self::ExecutionTemplatesList,
        Self::MembershipsList,
        Self::MembershipsGet,
        Self::MembershipsCreate,
        Self::MembershipsChangeRole,
        Self::MembershipsRevoke,
        Self::MembershipInvitationsList,
        Self::MembershipInvitationsGet,
        Self::MembershipInvitationsCreate,
        Self::MembershipInvitationsRevoke,
        Self::MyMembershipInvitationsList,
        Self::MembershipInvitationsAccept,
        Self::ResourceGrantsList,
        Self::ResourceGrantsGet,
        Self::ResourceGrantsCreate,
        Self::ResourceGrantsRevoke,
        Self::ProjectsCreate,
        Self::ProjectsList,
        Self::ProjectAttributionGet,
        Self::ProjectAttributionUpdate,
        Self::FormsCreate,
        Self::FormsGet,
        Self::FormsList,
        Self::FormsRevise,
        Self::FormReleasesGet,
        Self::FormReleasesList,
        Self::FormReleasesPublish,
        Self::OntologiesCreate,
        Self::OntologiesGet,
        Self::OntologiesList,
        Self::OntologiesRevise,
        Self::OntologyRevisionsGet,
        Self::OntologyRevisionsList,
        Self::OntologyRevisionsDiff,
        Self::WorkflowNodeCatalogGet,
        Self::WorkflowDefinitionsCreate,
        Self::WorkflowDefinitionsGet,
        Self::WorkflowDefinitionsList,
        Self::WorkflowDefinitionsRevise,
        Self::WorkflowRevisionsGet,
        Self::WorkflowRevisionsList,
        Self::WorkflowGoalsCreate,
        Self::WorkflowGoalsGet,
        Self::WorkflowGoalsList,
        Self::WorkflowPlanRevisionsGet,
        Self::WorkflowRunsStart,
        Self::WorkflowRunsCancel,
        Self::WorkflowRunsGet,
        Self::WorkflowRunsList,
        Self::WorkflowRunsWait,
        Self::WorkflowRunOutputGet,
        Self::WorkflowRunHistoryGet,
        Self::WorkflowRunVariablesGet,
        Self::HumanTasksClaim,
        Self::HumanTasksGet,
        Self::HumanTasksList,
        Self::HumanTasksRelease,
        Self::HumanTasksSubmit,
        Self::Search,
        Self::PluginRegistriesList,
        Self::PluginRegistriesGet,
        Self::PluginCatalogSearch,
        Self::PluginCatalogSearchCached,
        Self::PluginCatalogInspect,
        Self::PluginCatalogInspectCached,
        Self::NodesList,
        Self::NodesGet,
        Self::OperationsList,
        Self::AuditRecordsList,
        Self::NotificationsList,
        Self::NotificationsGet,
        Self::NotificationsRead,
        Self::NotificationOutboundSubscriptionsCreate,
        Self::NotificationOutboundSubscriptionsList,
        Self::NotificationOutboundSubscriptionsGet,
        Self::NotificationOutboundSubscriptionsRevoke,
        Self::WorkloadsList,
        Self::WorkloadsGet,
        Self::WorkloadLogsGet,
        Self::WorkloadsStop,
        Self::WorkloadsRollback,
        Self::DeploymentsGet,
        Self::DeploymentsCancel,
        Self::RoutesList,
        Self::RoutesGet,
        Self::BuildRunsList,
        Self::BuildRunsGet,
        Self::BuildRunLogsGet,
        Self::BuildEvidenceGet,
        Self::BuildRunsCancel,
        Self::BuildRunsRetry,
    ];

    pub fn visible_to(self, principal: &AuthPrincipal) -> bool {
        self.required_scope()
            .is_none_or(|scope| principal.has_scope(scope))
            && (!self.requires_identity_administrator()
                || principal.has_role("platform_admin")
                || principal.has_role("organization_owner")
                || principal.has_role("organization_admin"))
            && self.resource_binding_is_visible(principal)
    }

    pub fn resolve(name: &str, principal: &AuthPrincipal) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|tool| tool.name() == name && tool.visible_to(principal))
    }

    pub fn visible_catalog(principal: &AuthPrincipal) -> Vec<Value> {
        Self::ALL
            .into_iter()
            .filter(|tool| tool.visible_to(principal))
            .map(Self::definition)
            .collect()
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::EnvironmentsCreate => ENVIRONMENTS_CREATE,
            Self::EnvironmentsList => ENVIRONMENTS_LIST,
            Self::ApplicationsCreate => APPLICATIONS_CREATE,
            Self::ApplicationsList => APPLICATIONS_LIST,
            Self::ApplicationsGet => APPLICATIONS_GET,
            Self::ApplicationReleasesPublish => APPLICATION_RELEASES_PUBLISH,
            Self::ApplicationReleasesList => APPLICATION_RELEASES_LIST,
            Self::ApplicationReleasesGet => APPLICATION_RELEASES_GET,
            Self::ApplicationSessionsOpen => APPLICATION_SESSIONS_OPEN,
            Self::ApplicationSessionsGet => APPLICATION_SESSIONS_GET,
            Self::ApplicationSessionsClose => APPLICATION_SESSIONS_CLOSE,
            Self::ApplicationSessionsReplay => APPLICATION_SESSIONS_REPLAY,
            Self::ApplicationInvocationsRequest => APPLICATION_INVOCATIONS_REQUEST,
            Self::ApplicationInvocationsGet => APPLICATION_INVOCATIONS_GET,
            Self::ApplicationInvocationsCancel => APPLICATION_INVOCATIONS_CANCEL,
            Self::ApplicationMessagesList => APPLICATION_MESSAGES_LIST,
            Self::ConnectorProfilesCreate => CONNECTOR_PROFILES_CREATE,
            Self::ConnectorProfilesRevise => CONNECTOR_PROFILES_REVISE,
            Self::ConnectorProfilesList => CONNECTOR_PROFILES_LIST,
            Self::ConnectorProfilesGet => CONNECTOR_PROFILES_GET,
            Self::ConnectorRevisionsList => CONNECTOR_REVISIONS_LIST,
            Self::ConnectorRevisionsGet => CONNECTOR_REVISIONS_GET,
            Self::DurableCellApplicationsCreate => DURABLE_CELL_APPLICATIONS_CREATE,
            Self::DurableCellApplicationsRevise => DURABLE_CELL_APPLICATIONS_REVISE,
            Self::DurableCellApplicationsStart => DURABLE_CELL_APPLICATIONS_START,
            Self::DurableCellApplicationsStop => DURABLE_CELL_APPLICATIONS_STOP,
            Self::DurableCellApplicationsList => DURABLE_CELL_APPLICATIONS_LIST,
            Self::DurableCellApplicationsGet => DURABLE_CELL_APPLICATIONS_GET,
            Self::DurableCellRevisionsList => DURABLE_CELL_REVISIONS_LIST,
            Self::DurableCellRevisionsGet => DURABLE_CELL_REVISIONS_GET,
            Self::DurableCellDeploymentsCreate => DURABLE_CELL_DEPLOYMENTS_CREATE,
            Self::DurableCellRoutesPublish => DURABLE_CELL_ROUTES_PUBLISH,
            Self::ExecutionTemplatesCreate => EXECUTION_TEMPLATES_CREATE,
            Self::ExecutionTemplatesGet => EXECUTION_TEMPLATES_GET,
            Self::ExecutionTemplatesList => EXECUTION_TEMPLATES_LIST,
            Self::MembershipsList => MEMBERSHIPS_LIST,
            Self::MembershipsGet => MEMBERSHIPS_GET,
            Self::MembershipsCreate => MEMBERSHIPS_CREATE,
            Self::MembershipsChangeRole => MEMBERSHIPS_CHANGE_ROLE,
            Self::MembershipsRevoke => MEMBERSHIPS_REVOKE,
            Self::MembershipInvitationsList => MEMBERSHIP_INVITATIONS_LIST,
            Self::MembershipInvitationsGet => MEMBERSHIP_INVITATIONS_GET,
            Self::MembershipInvitationsCreate => MEMBERSHIP_INVITATIONS_CREATE,
            Self::MembershipInvitationsRevoke => MEMBERSHIP_INVITATIONS_REVOKE,
            Self::MyMembershipInvitationsList => MY_MEMBERSHIP_INVITATIONS_LIST,
            Self::MembershipInvitationsAccept => MEMBERSHIP_INVITATIONS_ACCEPT,
            Self::ResourceGrantsList => RESOURCE_GRANTS_LIST,
            Self::ResourceGrantsGet => RESOURCE_GRANTS_GET,
            Self::ResourceGrantsCreate => RESOURCE_GRANTS_CREATE,
            Self::ResourceGrantsRevoke => RESOURCE_GRANTS_REVOKE,
            Self::ProjectsCreate => PROJECTS_CREATE,
            Self::ProjectsList => PROJECTS_LIST,
            Self::ProjectAttributionGet => PROJECT_ATTRIBUTION_GET,
            Self::ProjectAttributionUpdate => PROJECT_ATTRIBUTION_UPDATE,
            Self::FormsCreate => FORMS_CREATE,
            Self::FormsGet => FORMS_GET,
            Self::FormsList => FORMS_LIST,
            Self::FormsRevise => FORMS_REVISE,
            Self::FormReleasesGet => FORM_RELEASES_GET,
            Self::FormReleasesList => FORM_RELEASES_LIST,
            Self::FormReleasesPublish => FORM_RELEASES_PUBLISH,
            Self::OntologiesCreate => ONTOLOGIES_CREATE,
            Self::OntologiesGet => ONTOLOGIES_GET,
            Self::OntologiesList => ONTOLOGIES_LIST,
            Self::OntologiesRevise => ONTOLOGIES_REVISE,
            Self::OntologyRevisionsGet => ONTOLOGY_REVISIONS_GET,
            Self::OntologyRevisionsList => ONTOLOGY_REVISIONS_LIST,
            Self::OntologyRevisionsDiff => ONTOLOGY_REVISIONS_DIFF,
            Self::WorkflowNodeCatalogGet => WORKFLOW_NODE_CATALOG_GET,
            Self::WorkflowDefinitionsCreate => WORKFLOW_DEFINITIONS_CREATE,
            Self::WorkflowDefinitionsGet => WORKFLOW_DEFINITIONS_GET,
            Self::WorkflowDefinitionsList => WORKFLOW_DEFINITIONS_LIST,
            Self::WorkflowDefinitionsRevise => WORKFLOW_DEFINITIONS_REVISE,
            Self::WorkflowRevisionsGet => WORKFLOW_REVISIONS_GET,
            Self::WorkflowRevisionsList => WORKFLOW_REVISIONS_LIST,
            Self::WorkflowGoalsCreate => WORKFLOW_GOALS_CREATE,
            Self::WorkflowGoalsGet => WORKFLOW_GOALS_GET,
            Self::WorkflowGoalsList => WORKFLOW_GOALS_LIST,
            Self::WorkflowPlanRevisionsGet => WORKFLOW_PLAN_REVISIONS_GET,
            Self::WorkflowRunsStart => WORKFLOW_RUNS_START,
            Self::WorkflowRunsCancel => WORKFLOW_RUNS_CANCEL,
            Self::WorkflowRunsGet => WORKFLOW_RUNS_GET,
            Self::WorkflowRunsList => WORKFLOW_RUNS_LIST,
            Self::WorkflowRunsWait => WORKFLOW_RUNS_WAIT,
            Self::WorkflowRunOutputGet => WORKFLOW_RUN_OUTPUT_GET,
            Self::WorkflowRunHistoryGet => WORKFLOW_RUN_HISTORY_GET,
            Self::WorkflowRunVariablesGet => WORKFLOW_RUN_VARIABLES_GET,
            Self::HumanTasksClaim => HUMAN_TASKS_CLAIM,
            Self::HumanTasksGet => HUMAN_TASKS_GET,
            Self::HumanTasksList => HUMAN_TASKS_LIST,
            Self::HumanTasksRelease => HUMAN_TASKS_RELEASE,
            Self::HumanTasksSubmit => HUMAN_TASKS_SUBMIT,
            Self::Search => SEARCH,
            Self::PluginRegistriesList => PLUGIN_REGISTRIES_LIST,
            Self::PluginRegistriesGet => PLUGIN_REGISTRIES_GET,
            Self::PluginCatalogSearch => PLUGIN_CATALOG_SEARCH,
            Self::PluginCatalogSearchCached => PLUGIN_CATALOG_SEARCH_CACHED,
            Self::PluginCatalogInspect => PLUGIN_CATALOG_INSPECT,
            Self::PluginCatalogInspectCached => PLUGIN_CATALOG_INSPECT_CACHED,
            Self::NodesList => NODES_LIST,
            Self::NodesGet => NODES_GET,
            Self::OperationsList => OPERATIONS_LIST,
            Self::AuditRecordsList => AUDIT_RECORDS_LIST,
            Self::NotificationsList => NOTIFICATIONS_LIST,
            Self::NotificationsGet => NOTIFICATIONS_GET,
            Self::NotificationsRead => NOTIFICATIONS_READ,
            Self::NotificationOutboundSubscriptionsCreate => {
                NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_CREATE
            }
            Self::NotificationOutboundSubscriptionsList => NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_LIST,
            Self::NotificationOutboundSubscriptionsGet => NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_GET,
            Self::NotificationOutboundSubscriptionsRevoke => {
                NOTIFICATION_OUTBOUND_SUBSCRIPTIONS_REVOKE
            }
            Self::WorkloadsList => WORKLOADS_LIST,
            Self::WorkloadsGet => WORKLOADS_GET,
            Self::WorkloadLogsGet => WORKLOAD_LOGS_GET,
            Self::WorkloadsStop => WORKLOADS_STOP,
            Self::WorkloadsRollback => WORKLOADS_ROLLBACK,
            Self::DeploymentsGet => DEPLOYMENTS_GET,
            Self::DeploymentsCancel => DEPLOYMENTS_CANCEL,
            Self::RoutesList => ROUTES_LIST,
            Self::RoutesGet => ROUTES_GET,
            Self::BuildRunsList => BUILD_RUNS_LIST,
            Self::BuildRunsGet => BUILD_RUNS_GET,
            Self::BuildRunLogsGet => BUILD_RUN_LOGS_GET,
            Self::BuildEvidenceGet => BUILD_EVIDENCE_GET,
            Self::BuildRunsCancel => BUILD_RUNS_CANCEL,
            Self::BuildRunsRetry => BUILD_RUNS_RETRY,
        }
    }

    const fn required_scope(self) -> Option<&'static str> {
        match self {
            Self::EnvironmentsCreate => Some(ApiTokenScope::ENVIRONMENT_WRITE),
            Self::ApplicationsCreate
            | Self::ApplicationReleasesPublish
            | Self::ApplicationSessionsOpen
            | Self::ApplicationSessionsGet
            | Self::ApplicationSessionsClose
            | Self::ApplicationSessionsReplay
            | Self::ApplicationInvocationsRequest
            | Self::ApplicationInvocationsGet
            | Self::ApplicationInvocationsCancel
            | Self::ApplicationMessagesList => Some(ApiTokenScope::APPLICATION_WRITE),
            Self::ConnectorProfilesCreate | Self::ConnectorProfilesRevise => {
                Some(ApiTokenScope::CONNECTOR_WRITE)
            }
            Self::DurableCellApplicationsCreate
            | Self::DurableCellApplicationsRevise
            | Self::DurableCellApplicationsStart
            | Self::DurableCellApplicationsStop
            | Self::DurableCellDeploymentsCreate => Some(ApiTokenScope::WORKLOAD_WRITE),
            Self::DurableCellRoutesPublish => Some(ApiTokenScope::ROUTE_WRITE),
            Self::ExecutionTemplatesCreate => Some(ApiTokenScope::EXECUTION_WRITE),
            Self::MembershipsList
            | Self::MembershipsGet
            | Self::MembershipsCreate
            | Self::MembershipsChangeRole
            | Self::MembershipsRevoke
            | Self::MembershipInvitationsList
            | Self::MembershipInvitationsGet
            | Self::MembershipInvitationsCreate
            | Self::MembershipInvitationsRevoke
            | Self::ResourceGrantsList
            | Self::ResourceGrantsGet
            | Self::ResourceGrantsCreate
            | Self::ResourceGrantsRevoke => Some(ApiTokenScope::IDENTITY_WRITE),
            Self::ProjectsCreate | Self::ProjectAttributionUpdate => {
                Some(ApiTokenScope::PROJECT_WRITE)
            }
            Self::FormsCreate | Self::FormsRevise | Self::FormReleasesPublish => {
                Some(ApiTokenScope::FORM_WRITE)
            }
            Self::OntologiesCreate | Self::OntologiesRevise => Some(ApiTokenScope::ONTOLOGY_WRITE),
            Self::WorkflowDefinitionsCreate
            | Self::WorkflowDefinitionsRevise
            | Self::WorkflowGoalsCreate
            | Self::WorkflowRunsStart
            | Self::WorkflowRunsCancel
            | Self::HumanTasksClaim
            | Self::HumanTasksRelease
            | Self::HumanTasksSubmit => Some(ApiTokenScope::WORKFLOW_WRITE),
            Self::WorkloadsStop | Self::WorkloadsRollback | Self::DeploymentsCancel => {
                Some(ApiTokenScope::WORKLOAD_WRITE)
            }
            Self::BuildRunsCancel | Self::BuildRunsRetry => Some(ApiTokenScope::BUILD_WRITE),
            Self::MyMembershipInvitationsList
            | Self::AuditRecordsList
            | Self::NotificationsList
            | Self::NotificationsGet
            | Self::NotificationOutboundSubscriptionsList
            | Self::NotificationOutboundSubscriptionsGet
            | Self::ApplicationsList
            | Self::ApplicationsGet
            | Self::ApplicationReleasesList
            | Self::ApplicationReleasesGet
            | Self::ConnectorProfilesList
            | Self::ConnectorProfilesGet
            | Self::ConnectorRevisionsList
            | Self::ConnectorRevisionsGet
            | Self::DurableCellApplicationsList
            | Self::DurableCellApplicationsGet
            | Self::DurableCellRevisionsList
            | Self::DurableCellRevisionsGet => Some(ApiTokenScope::CLOUD_READ),
            Self::NotificationsRead
            | Self::NotificationOutboundSubscriptionsCreate
            | Self::NotificationOutboundSubscriptionsRevoke => {
                Some(ApiTokenScope::NOTIFICATION_WRITE)
            }
            Self::MembershipInvitationsAccept => Some(ApiTokenScope::IDENTITY_WRITE),
            Self::EnvironmentsList
            | Self::ExecutionTemplatesGet
            | Self::ExecutionTemplatesList
            | Self::ProjectsList
            | Self::ProjectAttributionGet
            | Self::FormsGet
            | Self::FormsList
            | Self::FormReleasesGet
            | Self::FormReleasesList
            | Self::OntologiesGet
            | Self::OntologiesList
            | Self::OntologyRevisionsGet
            | Self::OntologyRevisionsList
            | Self::OntologyRevisionsDiff
            | Self::WorkflowNodeCatalogGet
            | Self::WorkflowDefinitionsGet
            | Self::WorkflowDefinitionsList
            | Self::WorkflowRevisionsGet
            | Self::WorkflowRevisionsList
            | Self::WorkflowGoalsGet
            | Self::WorkflowGoalsList
            | Self::WorkflowPlanRevisionsGet
            | Self::WorkflowRunsGet
            | Self::WorkflowRunsList
            | Self::WorkflowRunsWait
            | Self::WorkflowRunOutputGet
            | Self::WorkflowRunHistoryGet
            | Self::WorkflowRunVariablesGet
            | Self::HumanTasksGet
            | Self::HumanTasksList
            | Self::Search
            | Self::PluginRegistriesList
            | Self::PluginRegistriesGet
            | Self::PluginCatalogSearch
            | Self::PluginCatalogSearchCached
            | Self::PluginCatalogInspect
            | Self::PluginCatalogInspectCached
            | Self::NodesList
            | Self::NodesGet
            | Self::OperationsList
            | Self::WorkloadsList
            | Self::WorkloadsGet
            | Self::WorkloadLogsGet
            | Self::DeploymentsGet
            | Self::RoutesList
            | Self::RoutesGet
            | Self::BuildRunsList
            | Self::BuildRunsGet
            | Self::BuildRunLogsGet
            | Self::BuildEvidenceGet => None,
        }
    }

    const fn requires_identity_administrator(self) -> bool {
        matches!(
            self,
            Self::MembershipsList
                | Self::MembershipsGet
                | Self::MembershipsCreate
                | Self::MembershipsChangeRole
                | Self::MembershipsRevoke
                | Self::MembershipInvitationsList
                | Self::MembershipInvitationsGet
                | Self::MembershipInvitationsCreate
                | Self::MembershipInvitationsRevoke
                | Self::AuditRecordsList
                | Self::ResourceGrantsList
                | Self::ResourceGrantsGet
                | Self::ResourceGrantsCreate
                | Self::ResourceGrantsRevoke
        )
    }

    pub(super) const fn resource_binding(self) -> Option<ManagementResourceBinding> {
        match self {
            Self::EnvironmentsCreate
            | Self::ProjectAttributionGet
            | Self::ProjectAttributionUpdate
            | Self::ApplicationsCreate
            | Self::ApplicationsList
            | Self::ExecutionTemplatesCreate
            | Self::ExecutionTemplatesGet
            | Self::ExecutionTemplatesList
            | Self::FormsCreate
            | Self::FormsList
            | Self::OntologiesCreate
            | Self::OntologiesList
            | Self::WorkflowNodeCatalogGet
            | Self::WorkflowDefinitionsCreate
            | Self::WorkflowDefinitionsList
            | Self::WorkflowGoalsCreate
            | Self::WorkflowGoalsList
            | Self::WorkflowRunsStart
            | Self::WorkflowRunsList
            | Self::HumanTasksList => Some(ManagementResourceBinding::ProjectArgument),
            Self::ConnectorProfilesCreate
            | Self::ConnectorProfilesRevise
            | Self::ConnectorProfilesList
            | Self::ConnectorProfilesGet
            | Self::ConnectorRevisionsList
            | Self::ConnectorRevisionsGet
            | Self::DurableCellApplicationsCreate
            | Self::DurableCellApplicationsRevise
            | Self::DurableCellApplicationsStart
            | Self::DurableCellApplicationsStop
            | Self::DurableCellApplicationsList
            | Self::DurableCellApplicationsGet
            | Self::DurableCellRevisionsList
            | Self::DurableCellRevisionsGet
            | Self::DurableCellDeploymentsCreate
            | Self::DurableCellRoutesPublish
            | Self::WorkloadsList
            | Self::RoutesList
            | Self::BuildRunsList => Some(ManagementResourceBinding::EnvironmentArguments),
            Self::WorkloadsGet
            | Self::FormsGet
            | Self::ApplicationsGet
            | Self::ApplicationReleasesPublish
            | Self::ApplicationReleasesList
            | Self::ApplicationReleasesGet
            | Self::ApplicationSessionsOpen
            | Self::ApplicationSessionsGet
            | Self::ApplicationSessionsClose
            | Self::ApplicationSessionsReplay
            | Self::ApplicationInvocationsRequest
            | Self::ApplicationInvocationsGet
            | Self::ApplicationInvocationsCancel
            | Self::ApplicationMessagesList
            | Self::FormsRevise
            | Self::FormReleasesGet
            | Self::FormReleasesList
            | Self::FormReleasesPublish
            | Self::OntologiesGet
            | Self::OntologiesRevise
            | Self::OntologyRevisionsGet
            | Self::OntologyRevisionsList
            | Self::OntologyRevisionsDiff
            | Self::WorkflowDefinitionsGet
            | Self::WorkflowDefinitionsRevise
            | Self::WorkflowRevisionsGet
            | Self::WorkflowRevisionsList
            | Self::WorkflowGoalsGet
            | Self::WorkflowPlanRevisionsGet
            | Self::WorkflowRunsCancel
            | Self::WorkflowRunsGet
            | Self::WorkflowRunsWait
            | Self::WorkflowRunOutputGet
            | Self::WorkflowRunHistoryGet
            | Self::WorkflowRunVariablesGet
            | Self::HumanTasksClaim
            | Self::HumanTasksGet
            | Self::HumanTasksRelease
            | Self::HumanTasksSubmit
            | Self::WorkloadLogsGet
            | Self::WorkloadsStop
            | Self::WorkloadsRollback
            | Self::DeploymentsGet
            | Self::DeploymentsCancel
            | Self::RoutesGet
            | Self::BuildRunsGet
            | Self::BuildRunLogsGet
            | Self::BuildEvidenceGet
            | Self::BuildRunsCancel
            | Self::BuildRunsRetry => Some(ManagementResourceBinding::ProjectOwnedResource),
            Self::NodesGet => Some(ManagementResourceBinding::NodeArgument),
            Self::ProjectsList => Some(ManagementResourceBinding::ProjectCollection),
            Self::EnvironmentsList => Some(ManagementResourceBinding::EnvironmentCollection),
            Self::NodesList => Some(ManagementResourceBinding::NodeCollection),
            Self::Search => Some(ManagementResourceBinding::SearchCollection),
            Self::OperationsList => Some(ManagementResourceBinding::PolymorphicCollection),
            Self::MyMembershipInvitationsList
            | Self::MembershipInvitationsAccept
            | Self::NotificationsList
            | Self::NotificationsGet
            | Self::NotificationsRead
            | Self::NotificationOutboundSubscriptionsCreate
            | Self::NotificationOutboundSubscriptionsList
            | Self::NotificationOutboundSubscriptionsGet
            | Self::NotificationOutboundSubscriptionsRevoke => {
                Some(ManagementResourceBinding::SelfPrincipal)
            }
            _ => None,
        }
    }

    fn resource_binding_is_visible(self, principal: &AuthPrincipal) -> bool {
        if !principal.has_role("organization_restricted") {
            return true;
        }
        let Ok(evaluator) = resource_access_evaluator(principal) else {
            return false;
        };
        match self.resource_binding() {
            Some(ManagementResourceBinding::ProjectArgument) => evaluator.has_project_authority(),
            Some(ManagementResourceBinding::EnvironmentArguments) => {
                evaluator.has_project_visibility()
            }
            Some(ManagementResourceBinding::NodeArgument) => evaluator.has_node_visibility(),
            Some(ManagementResourceBinding::ProjectOwnedResource) => {
                evaluator.has_project_visibility()
            }
            Some(
                ManagementResourceBinding::ProjectCollection
                | ManagementResourceBinding::EnvironmentCollection,
            ) => evaluator.has_project_visibility(),
            Some(ManagementResourceBinding::NodeCollection) => evaluator.has_node_visibility(),
            Some(
                ManagementResourceBinding::SearchCollection
                | ManagementResourceBinding::PolymorphicCollection,
            ) => evaluator.has_any_visible_resource(),
            Some(ManagementResourceBinding::SelfPrincipal) => true,
            None => false,
        }
    }

    fn definition(self) -> Value {
        let (title, description, input_schema, read_only) = match self {
            Self::EnvironmentsCreate => (
                "Create environment",
                "Create an environment in one tenant-authorized project with explicit idempotency.",
                create_environment_schema(),
                false,
            ),
            Self::EnvironmentsList => (
                "List environments",
                "List environments in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::ApplicationsCreate => (
                "Create Application",
                "Create one project-scoped Application and immutable release from canonical A3S ACL with explicit idempotency.",
                create_application_schema(),
                false,
            ),
            Self::ApplicationsList => (
                "List Applications",
                "List a bounded set of Applications in one tenant-authorized project.",
                list_applications_schema(),
                true,
            ),
            Self::ApplicationsGet => (
                "Get Application",
                "Get one exact Application and its current immutable release.",
                application_schema(),
                true,
            ),
            Self::ApplicationReleasesPublish => (
                "Publish Application release",
                "Publish one immutable Application release from canonical A3S ACL using optimistic concurrency and explicit idempotency.",
                publish_application_release_schema(),
                false,
            ),
            Self::ApplicationReleasesList => (
                "List Application releases",
                "List a bounded set of immutable releases for one tenant-authorized Application.",
                list_application_releases_schema(),
                true,
            ),
            Self::ApplicationReleasesGet => (
                "Get Application release",
                "Get one exact immutable Application release and its Workflow revision evidence.",
                application_release_schema(),
                true,
            ),
            Self::ApplicationSessionsOpen => (
                "Open Application session",
                "Open or replay one caller-owned project-member session pinned to an exact Application release.",
                open_application_session_schema(),
                false,
            ),
            Self::ApplicationSessionsGet => (
                "Get Application session",
                "Get one caller-owned release-pinned Application session.",
                application_session_schema(),
                true,
            ),
            Self::ApplicationSessionsClose => (
                "Close Application session",
                "Close or replay one caller-owned Application session using optimistic concurrency and explicit idempotency.",
                close_application_session_schema(),
                false,
            ),
            Self::ApplicationSessionsReplay => (
                "Replay Application session",
                "Read one caller-owned session head, current variable snapshot, and bounded contiguous channel page.",
                list_application_messages_schema(),
                true,
            ),
            Self::ApplicationInvocationsRequest => (
                "Request Application invocation",
                "Persist one idempotent invocation and start or adopt its exact ordinary WorkflowRun.",
                request_application_invocation_schema(),
                false,
            ),
            Self::ApplicationInvocationsGet => (
                "Get Application invocation",
                "Get one caller-owned Application invocation and WorkflowRun correlation.",
                application_invocation_schema(),
                true,
            ),
            Self::ApplicationInvocationsCancel => (
                "Cancel Application invocation",
                "Request or replay cancellation of one caller-owned Application invocation using optimistic concurrency and explicit idempotency.",
                cancel_application_invocation_schema(),
                false,
            ),
            Self::ApplicationMessagesList => (
                "List Application messages",
                "List bounded ordered channel messages after one session sequence.",
                list_application_messages_schema(),
                true,
            ),
            Self::ConnectorProfilesCreate => (
                "Create Connector profile",
                "Create one immutable, environment-scoped Connector profile revision from canonical A3S ACL with explicit idempotency.",
                create_connector_profile_schema(),
                false,
            ),
            Self::ConnectorProfilesRevise => (
                "Revise Connector profile",
                "Publish one immutable Connector profile revision using optimistic concurrency and explicit idempotency.",
                revise_connector_profile_schema(),
                false,
            ),
            Self::ConnectorProfilesList => (
                "List Connector profiles",
                "List a bounded set of Connector profiles in one tenant-authorized environment.",
                list_connector_profiles_schema(),
                true,
            ),
            Self::ConnectorProfilesGet => (
                "Get Connector profile",
                "Get one exact Connector profile and its current immutable revision without resolving referenced Secrets.",
                connector_profile_schema(),
                true,
            ),
            Self::ConnectorRevisionsList => (
                "List Connector revisions",
                "List a bounded set of immutable revisions for one tenant-authorized Connector profile.",
                list_connector_revisions_schema(),
                true,
            ),
            Self::ConnectorRevisionsGet => (
                "Get Connector revision",
                "Get one exact immutable Connector revision and canonical A3S ACL without resolving referenced Secrets.",
                connector_revision_schema(),
                true,
            ),
            Self::DurableCellApplicationsCreate => (
                "Create Durable Cell application",
                "Create one environment-scoped Durable Cell application and immutable revision from canonical A3S ACL through the existing application authority.",
                create_durable_cell_application_schema(),
                false,
            ),
            Self::DurableCellApplicationsRevise => (
                "Revise Durable Cell application",
                "Publish one immutable Durable Cell application revision with optimistic concurrency and explicit idempotency.",
                revise_durable_cell_application_schema(),
                false,
            ),
            Self::DurableCellApplicationsStart => (
                "Start Durable Cell application",
                "Set one Durable Cell application to running through its existing optimistic-concurrency command.",
                durable_cell_application_state_schema(),
                false,
            ),
            Self::DurableCellApplicationsStop => (
                "Stop Durable Cell application",
                "Set one Durable Cell application to stopped while leaving provider-owned state under its retention policy.",
                durable_cell_application_state_schema(),
                false,
            ),
            Self::DurableCellApplicationsList => (
                "List Durable Cell applications",
                "List a bounded set of Durable Cell application heads in one tenant-authorized environment.",
                list_durable_cell_applications_schema(),
                true,
            ),
            Self::DurableCellApplicationsGet => (
                "Get Durable Cell application",
                "Get one exact Durable Cell application and its current immutable canonical-ACL revision.",
                durable_cell_application_schema(),
                true,
            ),
            Self::DurableCellRevisionsList => (
                "List Durable Cell revisions",
                "List a bounded set of immutable revisions for one tenant-authorized Durable Cell application.",
                list_durable_cell_revisions_schema(),
                true,
            ),
            Self::DurableCellRevisionsGet => (
                "Get Durable Cell revision",
                "Get one exact immutable Durable Cell application revision and its canonical A3S ACL.",
                durable_cell_revision_schema(),
                true,
            ),
            Self::DurableCellDeploymentsCreate => (
                "Deploy Durable Cell revision",
                "Project one exact running revision through the existing S0, Secrets, Workloads, Operation, Outbox, and Fleet authorities from bounded canonical A3S ACL inputs.",
                deploy_durable_cell_application_schema(),
                false,
            ),
            Self::DurableCellRoutesPublish => (
                "Publish Durable Cell route",
                "Publish only the Service-profile-selected public port of one exact deployed revision through Edge's existing verified route authority.",
                publish_durable_cell_route_schema(),
                false,
            ),
            Self::ExecutionTemplatesCreate => (
                "Create ExecutionTemplate",
                "Publish one immutable project-scoped ExecutionTemplate revision from canonical A3S ACL with explicit idempotency.",
                create_execution_template_schema(),
                false,
            ),
            Self::ExecutionTemplatesGet => (
                "Get ExecutionTemplate revision",
                "Get one exact immutable tenant-authorized ExecutionTemplate revision and its canonical A3S ACL.",
                get_execution_template_schema(),
                true,
            ),
            Self::ExecutionTemplatesList => (
                "List ExecutionTemplate revisions",
                "List a bounded set of immutable ExecutionTemplate revisions in one tenant-authorized project.",
                list_execution_templates_schema(),
                true,
            ),
            Self::MembershipsList => (
                "List memberships",
                "List organization memberships from the shared Cloud identity authority.",
                empty_schema(),
                true,
            ),
            Self::MembershipsGet => (
                "Get membership",
                "Get one organization membership and its bound principal.",
                uuid_id_schema("membershipId"),
                true,
            ),
            Self::MembershipsCreate => (
                "Create membership",
                "Create one human or service Principal and organization Membership atomically with explicit idempotency.",
                create_membership_schema(),
                false,
            ),
            Self::MembershipsChangeRole => (
                "Change membership role",
                "Change one membership role with optimistic concurrency and explicit idempotency.",
                change_membership_role_schema(),
                false,
            ),
            Self::MembershipsRevoke => (
                "Revoke membership",
                "Revoke one membership with last-owner protection, optimistic concurrency, and explicit idempotency.",
                revoke_membership_schema(),
                false,
            ),
            Self::MembershipInvitationsList => (
                "List membership invitations",
                "List organization membership invitation history from the shared Identity authority.",
                empty_schema(),
                true,
            ),
            Self::MembershipInvitationsGet => (
                "Get membership invitation",
                "Get one organization membership invitation.",
                uuid_id_schema("invitationId"),
                true,
            ),
            Self::MembershipInvitationsCreate => (
                "Create membership invitation",
                "Invite one exact existing Principal to an organization with a bounded expiry and explicit idempotency.",
                create_membership_invitation_schema(),
                false,
            ),
            Self::MembershipInvitationsRevoke => (
                "Revoke membership invitation",
                "Revoke one pending membership invitation with optimistic concurrency and explicit idempotency.",
                membership_invitation_mutation_schema(),
                false,
            ),
            Self::MyMembershipInvitationsList => (
                "List my membership invitations",
                "List membership invitations bound exactly to the authenticated Principal.",
                empty_schema(),
                true,
            ),
            Self::MembershipInvitationsAccept => (
                "Accept membership invitation",
                "Accept one invitation bound exactly to the authenticated Principal and create the ordinary Membership atomically.",
                membership_invitation_mutation_schema(),
                false,
            ),
            Self::ResourceGrantsList => (
                "List Resource Grants",
                "List Resource Grant history for one restricted organization membership.",
                uuid_id_schema("membershipId"),
                true,
            ),
            Self::ResourceGrantsGet => (
                "Get Resource Grant",
                "Get one Resource Grant from the shared Cloud identity authority.",
                uuid_id_schema("resourceGrantId"),
                true,
            ),
            Self::ResourceGrantsCreate => (
                "Create Resource Grant",
                "Grant one closed project, environment, or node scope to a restricted membership with explicit idempotency.",
                create_resource_grant_schema(),
                false,
            ),
            Self::ResourceGrantsRevoke => (
                "Revoke Resource Grant",
                "Revoke one Resource Grant with optimistic concurrency and explicit idempotency.",
                revoke_resource_grant_schema(),
                false,
            ),
            Self::ProjectsCreate => (
                "Create project",
                "Create a project in the authenticated organization with explicit idempotency.",
                create_project_schema(),
                false,
            ),
            Self::ProjectsList => (
                "List projects",
                "List projects in the authenticated organization.",
                empty_schema(),
                true,
            ),
            Self::ProjectAttributionGet => (
                "Get project attribution",
                "Get the current or one exact immutable attribution profile for a tenant-authorized project.",
                get_project_attribution_schema(),
                true,
            ),
            Self::ProjectAttributionUpdate => (
                "Update project attribution",
                "Create one immutable non-monetary attribution profile and move the project pointer with optimistic concurrency.",
                update_project_attribution_schema(),
                false,
            ),
            Self::FormsCreate => (
                "Create Form draft",
                "Create one project-scoped native A3S Form draft with explicit idempotency.",
                create_form_draft_schema(),
                false,
            ),
            Self::FormsGet => (
                "Get Form draft",
                "Get one tenant-authorized native A3S Form draft and its latest release identity.",
                uuid_id_schema("formId"),
                true,
            ),
            Self::FormsList => (
                "List Form drafts",
                "List native A3S Form drafts in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::FormsRevise => (
                "Revise Form draft",
                "Create one immutable Form draft revision with optimistic concurrency and explicit idempotency.",
                revise_form_draft_schema(),
                false,
            ),
            Self::FormReleasesGet => (
                "Get Form release",
                "Get one immutable native A3S Form release including its owner-compiled plan.",
                form_release_schema(),
                true,
            ),
            Self::FormReleasesList => (
                "List Form releases",
                "List immutable releases for one tenant-authorized native A3S Form.",
                uuid_id_schema("formId"),
                true,
            ),
            Self::FormReleasesPublish => (
                "Publish Form release",
                "Compile and publish one immutable native A3S Form release with optimistic concurrency and explicit idempotency.",
                publish_form_release_schema(),
                false,
            ),
            Self::OntologiesCreate => (
                "Create Ontology",
                "Create one project-scoped Ontology from canonical A3S ACL with explicit idempotency.",
                create_ontology_schema(),
                false,
            ),
            Self::OntologiesGet => (
                "Get Ontology",
                "Get one tenant-authorized Ontology aggregate and its current revision identity.",
                uuid_id_schema("ontologyId"),
                true,
            ),
            Self::OntologiesList => (
                "List Ontologies",
                "List Ontology aggregates in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::OntologiesRevise => (
                "Revise Ontology",
                "Publish one immutable Ontology revision with optimistic concurrency and migration-rule enforcement.",
                revise_ontology_schema(),
                false,
            ),
            Self::OntologyRevisionsGet => (
                "Get Ontology revision",
                "Get one immutable Ontology revision including its canonical A3S ACL.",
                ontology_revision_schema(),
                true,
            ),
            Self::OntologyRevisionsList => (
                "List Ontology revisions",
                "List immutable revision summaries for one tenant-authorized Ontology.",
                uuid_id_schema("ontologyId"),
                true,
            ),
            Self::OntologyRevisionsDiff => (
                "Diff Ontology revisions",
                "Compute the deterministic structural and compatibility diff between two Ontology revisions.",
                ontology_diff_schema(),
                true,
            ),
            Self::WorkflowNodeCatalogGet => (
                "Get Workflow node catalog",
                "Discover the frozen built-in Workflow node catalog for one tenant-authorized project. Catalog visibility does not admit descriptors or confer execution authority.",
                project_id_schema(),
                true,
            ),
            Self::WorkflowDefinitionsCreate => (
                "Create Workflow definition",
                "Publish one project-scoped WorkflowDefinition and its exact canonical ACL payloads with explicit idempotency.",
                create_workflow_definition_schema(),
                false,
            ),
            Self::WorkflowDefinitionsGet => (
                "Get Workflow definition",
                "Get one tenant-authorized WorkflowDefinition aggregate and current revision identity.",
                uuid_id_schema("workflowDefinitionId"),
                true,
            ),
            Self::WorkflowDefinitionsList => (
                "List Workflow definitions",
                "List WorkflowDefinition aggregates in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::WorkflowDefinitionsRevise => (
                "Revise Workflow definition",
                "Publish one immutable Workflow revision and exact ACL payload set with optimistic concurrency.",
                revise_workflow_definition_schema(),
                false,
            ),
            Self::WorkflowRevisionsGet => (
                "Get Workflow revision",
                "Get one immutable Workflow revision including canonical definition and payload ACLs.",
                workflow_revision_schema(),
                true,
            ),
            Self::WorkflowRevisionsList => (
                "List Workflow revisions",
                "List immutable revision summaries for one tenant-authorized WorkflowDefinition.",
                uuid_id_schema("workflowDefinitionId"),
                true,
            ),
            Self::WorkflowGoalsCreate => (
                "Create Workflow goal",
                "Compile one exact WorkflowGoal ACL into a deterministic immutable PlanRevision with explicit idempotency.",
                create_workflow_goal_schema(),
                false,
            ),
            Self::WorkflowGoalsGet => (
                "Get Workflow goal",
                "Get one immutable WorkflowGoal and its exact authority and plan bindings.",
                uuid_id_schema("workflowGoalId"),
                true,
            ),
            Self::WorkflowGoalsList => (
                "List Workflow goals",
                "List immutable compiled WorkflowGoals in one tenant-authorized project.",
                project_id_schema(),
                true,
            ),
            Self::WorkflowPlanRevisionsGet => (
                "Get Workflow plan revision",
                "Get one immutable deterministic PlanRevision for an exact WorkflowGoal.",
                workflow_plan_revision_schema(),
                true,
            ),
            Self::WorkflowRunsStart => (
                "Start Workflow run",
                "Start one exact immutable PlanRevision through the shared Operation and A3S Flow authority.",
                start_workflow_run_schema(),
                false,
            ),
            Self::WorkflowRunsCancel => (
                "Cancel Workflow run",
                "Request cancellation of one tenant-authorized WorkflowRun with explicit idempotency.",
                cancel_workflow_run_schema(),
                false,
            ),
            Self::WorkflowRunsGet => (
                "Get Workflow run",
                "Get one WorkflowRun and its current semantic step projections.",
                uuid_id_schema("workflowRunId"),
                true,
            ),
            Self::WorkflowRunsList => (
                "List Workflow runs",
                "List a bounded set of WorkflowRuns in one tenant-authorized project.",
                list_workflow_runs_schema(),
                true,
            ),
            Self::WorkflowRunsWait => (
                "Wait for Workflow run",
                "Wait for a bounded interval and return the latest WorkflowRun state.",
                wait_workflow_run_schema(),
                true,
            ),
            Self::WorkflowRunOutputGet => (
                "Get Workflow run output",
                "Get the bounded output and digest of one completed WorkflowRun.",
                uuid_id_schema("workflowRunId"),
                true,
            ),
            Self::WorkflowRunHistoryGet => (
                "Get Workflow run history",
                "Get one bounded, redacted page of the correlated A3S Flow history.",
                workflow_run_history_schema(),
                true,
            ),
            Self::WorkflowRunVariablesGet => (
                "Inspect Workflow run variables",
                "Inspect bounded typed values reconstructed from immutable WorkflowRun input and the correlated A3S Flow history.",
                uuid_id_schema("workflowRunId"),
                true,
            ),
            Self::HumanTasksGet => (
                "Get HumanTask",
                "Get one tenant-authorized HumanTask; only its current claimant receives the request-bound A3S Form interaction.",
                uuid_id_schema("humanTaskId"),
                true,
            ),
            Self::HumanTasksClaim => (
                "Claim HumanTask",
                "Claim one ready tenant-authorized HumanTask with optimistic concurrency and explicit idempotency.",
                human_task_mutation_schema(),
                false,
            ),
            Self::HumanTasksList => (
                "List HumanTasks",
                "List bounded HumanTask summaries in one tenant-authorized project without interaction payloads.",
                list_human_tasks_schema(),
                true,
            ),
            Self::HumanTasksRelease => (
                "Release HumanTask",
                "Release one claimed tenant-authorized HumanTask as its current claimant with optimistic concurrency and explicit idempotency.",
                human_task_mutation_schema(),
                false,
            ),
            Self::HumanTasksSubmit => (
                "Submit HumanTask",
                "Submit the exact request-bound native A3S Form interaction as the current claimant; the submission owns task version and idempotency.",
                human_task_submission_schema(),
                false,
            ),
            Self::Search => (
                "Search Cloud resources",
                "Search bounded tenant-authorized resource projections in the authenticated organization.",
                search_schema(),
                true,
            ),
            Self::PluginRegistriesList => (
                "List plugin registries",
                "List trusted A3S Use Registry references enrolled in the authenticated organization.",
                empty_schema(),
                true,
            ),
            Self::PluginRegistriesGet => (
                "Get plugin registry",
                "Get one tenant-authorized A3S Use Registry reference and pinned root evidence.",
                uuid_id_schema("registryId"),
                true,
            ),
            Self::PluginCatalogSearch => (
                "Search plugin catalog",
                "Refresh and search one signed A3S Use catalog without downloading package bytes.",
                plugin_catalog_search_schema(),
                true,
            ),
            Self::PluginCatalogSearchCached => (
                "Search cached plugin catalog",
                "Search one already verified A3S Use catalog cache without network fallback or package download.",
                plugin_catalog_search_schema(),
                true,
            ),
            Self::PluginCatalogInspect => (
                "Inspect plugin catalog release",
                "Refresh and inspect one exact compatible signed A3S Use catalog release without downloading package bytes.",
                plugin_catalog_inspection_schema(),
                true,
            ),
            Self::PluginCatalogInspectCached => (
                "Inspect cached plugin catalog release",
                "Inspect one exact compatible release from an already verified A3S Use catalog cache without network fallback.",
                plugin_catalog_inspection_schema(),
                true,
            ),
            Self::NodesList => (
                "List nodes",
                "List node inventory and current availability in the authenticated organization.",
                empty_schema(),
                true,
            ),
            Self::NodesGet => (
                "Get node",
                "Get one tenant-authorized node and its current availability.",
                uuid_id_schema("nodeId"),
                true,
            ),
            Self::OperationsList => (
                "List operations",
                "List a bounded snapshot of recent operations in the authenticated organization.",
                bounded_limit_schema(),
                true,
            ),
            Self::AuditRecordsList => (
                "List audit records",
                "List one bounded, redacted page from the shared append-only organization audit history.",
                audit_record_list_schema(),
                true,
            ),
            Self::NotificationsList => (
                "List my notifications",
                "List one bounded, Resource-Grant-filtered page from the authenticated Principal's in-app inbox.",
                notification_list_schema(),
                true,
            ),
            Self::NotificationsGet => (
                "Get my notification",
                "Get one exact notification addressed to the authenticated Principal when its resource scope remains authorized.",
                uuid_id_schema("notificationId"),
                true,
            ),
            Self::NotificationsRead => (
                "Mark my notification read",
                "Mark one exact notification addressed to the authenticated Principal read with optimistic concurrency and idempotency.",
                mark_notification_read_schema(),
                false,
            ),
            Self::NotificationOutboundSubscriptionsCreate => (
                "Create my outbound notification subscription",
                "Create one immutable, recipient-bound outbound notification subscription from canonical A3S ACL, an exact Connector revision, and the ACL-pinned provider-attempt budget.",
                create_notification_outbound_subscription_schema(),
                false,
            ),
            Self::NotificationOutboundSubscriptionsList => (
                "List my outbound notification subscriptions",
                "List one bounded, Resource-Grant-filtered page of the authenticated Principal's outbound notification subscriptions.",
                notification_outbound_subscription_list_schema(),
                true,
            ),
            Self::NotificationOutboundSubscriptionsGet => (
                "Get my outbound notification subscription",
                "Get one exact recipient-bound outbound notification subscription and immutable provider-attempt budget without resolving its Connector endpoint or credentials.",
                uuid_id_schema("subscriptionId"),
                true,
            ),
            Self::NotificationOutboundSubscriptionsRevoke => (
                "Revoke my outbound notification subscription",
                "Revoke one exact recipient-bound outbound notification subscription with optimistic concurrency and idempotency.",
                revoke_notification_outbound_subscription_schema(),
                false,
            ),
            Self::WorkloadsList => (
                "List workloads",
                "List workloads in one tenant-authorized environment.",
                environment_scope_schema(),
                true,
            ),
            Self::WorkloadsGet => (
                "Get workload",
                "Get one tenant-authorized workload and its deployment state.",
                uuid_id_schema("workloadId"),
                true,
            ),
            Self::WorkloadLogsGet => (
                "Get workload logs",
                "Get one bounded page of retained logs for a tenant-authorized Workload revision.",
                workload_logs_schema(),
                true,
            ),
            Self::WorkloadsStop => (
                "Stop workload",
                "Stop one tenant-authorized Workload with explicit idempotency.",
                idempotent_uuid_id_schema("workloadId"),
                false,
            ),
            Self::WorkloadsRollback => (
                "Roll back workload",
                "Roll back one tenant-authorized Workload to an existing revision with explicit idempotency.",
                rollback_workload_schema(),
                false,
            ),
            Self::DeploymentsGet => (
                "Get deployment",
                "Get one tenant-authorized deployment and its observed operation state.",
                uuid_id_schema("deploymentId"),
                true,
            ),
            Self::DeploymentsCancel => (
                "Cancel deployment",
                "Cancel one tenant-authorized Deployment with explicit idempotency.",
                idempotent_uuid_id_schema("deploymentId"),
                false,
            ),
            Self::RoutesList => (
                "List routes",
                "List routes in one tenant-authorized environment.",
                environment_scope_schema(),
                true,
            ),
            Self::RoutesGet => (
                "Get route",
                "Get one tenant-authorized route and its Gateway publication state.",
                uuid_id_schema("routeId"),
                true,
            ),
            Self::BuildRunsList => (
                "List build runs",
                "List a bounded set of BuildRuns in one tenant-authorized environment.",
                build_run_list_schema(),
                true,
            ),
            Self::BuildRunsGet => (
                "Get build run",
                "Get one tenant-authorized BuildRun and its publication summary.",
                uuid_id_schema("buildRunId"),
                true,
            ),
            Self::BuildRunLogsGet => (
                "Get build run logs",
                "Get one bounded page of retained logs for a tenant-authorized BuildRun.",
                build_run_logs_schema(),
                true,
            ),
            Self::BuildEvidenceGet => (
                "Get build evidence",
                "Get the signed evidence projection for a tenant-authorized BuildRun.",
                uuid_id_schema("buildRunId"),
                true,
            ),
            Self::BuildRunsCancel => (
                "Cancel build run",
                "Cancel one tenant-authorized BuildRun with explicit idempotency.",
                idempotent_uuid_id_schema("buildRunId"),
                false,
            ),
            Self::BuildRunsRetry => (
                "Retry build run",
                "Retry one tenant-authorized BuildRun with explicit idempotency.",
                idempotent_uuid_id_schema("buildRunId"),
                false,
            ),
        };
        let destructive = matches!(
            self,
            Self::MembershipsRevoke
                | Self::MembershipInvitationsRevoke
                | Self::ResourceGrantsRevoke
                | Self::ApplicationSessionsClose
                | Self::ApplicationInvocationsCancel
                | Self::WorkloadsStop
                | Self::DeploymentsCancel
                | Self::BuildRunsCancel
                | Self::WorkflowRunsCancel
        );
        json!({
            "name": self.name(),
            "title": title,
            "description": description,
            "inputSchema": input_schema,
            "annotations": {
                "readOnlyHint": read_only,
                "destructiveHint": destructive,
                "idempotentHint": true,
                "openWorldHint": false
            }
        })
    }
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn plugin_catalog_search_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["registryId", "host", "search"],
        "properties": {
            "registryId": {"type": "string", "format": "uuid"},
            "host": plugin_catalog_host_input_schema(),
            "search": plugin_catalog_search_input_schema()
        }
    })
}

fn plugin_catalog_inspection_schema() -> Value {
    let mut schema = plugin_catalog_inspection_input_schema();
    let Some(object) = schema.as_object_mut() else {
        return schema;
    };
    let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) else {
        return schema;
    };
    properties.insert(
        "registryId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    properties.insert("host".into(), plugin_catalog_host_input_schema());
    let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) else {
        return schema;
    };
    required.insert(0, json!("host"));
    required.insert(0, json!("registryId"));
    schema
}

fn project_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn get_project_attribution_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "attributionProfileId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn update_project_attribution_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "businessOwnerReference": {
                "type": "string",
                "minLength": 1,
                "maxLength": BUSINESS_OWNER_REFERENCE_MAX_CHARS
            },
            "costAttributionCode": {
                "type": "string",
                "minLength": 1,
                "maxLength": COST_ATTRIBUTION_CODE_MAX_CHARS
            },
            "labels": {
                "type": "object",
                "maxProperties": PROJECT_ATTRIBUTION_LABEL_MAX_COUNT,
                "propertyNames": {
                    "maxLength": PROJECT_ATTRIBUTION_LABEL_KEY_MAX_CHARS,
                    "pattern": "^[a-z][a-z0-9._-]*$"
                },
                "additionalProperties": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": PROJECT_ATTRIBUTION_LABEL_VALUE_MAX_CHARS
                }
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "expectedVersion",
            "businessOwnerReference",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn environment_scope_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId"],
        "additionalProperties": false
    })
}

fn uuid_id_schema(property: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(property.into(), json!({"type": "string", "format": "uuid"}));
    json!({
        "type": "object",
        "properties": properties,
        "required": [property],
        "additionalProperties": false
    })
}

fn idempotent_uuid_id_schema(property: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(property.into(), json!({"type": "string", "format": "uuid"}));
    properties.insert("idempotencyKey".into(), idempotency_key_schema());
    json!({
        "type": "object",
        "properties": properties,
        "required": [property, "idempotencyKey"],
        "additionalProperties": false
    })
}

fn rollback_workload_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workloadId": {"type": "string", "format": "uuid"},
            "sourceRevisionId": {"type": "string", "format": "uuid"},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["workloadId", "sourceRevisionId", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn idempotency_key_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAXIMUM_IDEMPOTENCY_KEY_LENGTH
    })
}

fn bounded_limit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
                "default": DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT
            }
        },
        "additionalProperties": false
    })
}

fn audit_record_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "actorPrincipalId": {"type": "string", "format": "uuid"},
            "action": {
                "type": "string",
                "minLength": 1,
                "maxLength": 255,
                "pattern": "^[a-z-]+(?:\\.[a-z-]+){2,}$"
            },
            "aggregateId": {"type": "string", "format": "uuid"},
            "requestId": {"type": "string", "format": "uuid"},
            "from": {"type": "string", "format": "date-time"},
            "to": {"type": "string", "format": "date-time"},
            "cursor": {"type": "string", "minLength": 1, "maxLength": 128},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT,
                "default": DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT
            }
        },
        "additionalProperties": false
    })
}

fn notification_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "unreadOnly": {"type": "boolean", "default": false},
            "cursor": {"type": "string", "minLength": 1, "maxLength": 128},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "additionalProperties": false
    })
}

fn mark_notification_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "notificationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["notificationId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn notification_outbound_subscription_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cursor": {"type": "string", "minLength": 1, "maxLength": 128},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_NOTIFICATION_LIMIT,
                "default": DEFAULT_NOTIFICATION_LIMIT
            }
        },
        "additionalProperties": false
    })
}

fn create_notification_outbound_subscription_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["definitionAcl", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revoke_notification_outbound_subscription_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subscriptionId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["subscriptionId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn build_run_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "required": ["projectId", "environmentId"],
        "additionalProperties": false
    })
}

fn workload_logs_schema() -> Value {
    let mut properties = log_page_properties();
    properties.insert(
        "workloadId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    properties.insert(
        "revisionId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    json!({
        "type": "object",
        "properties": properties,
        "required": ["workloadId", "revisionId"],
        "additionalProperties": false
    })
}

fn build_run_logs_schema() -> Value {
    let mut properties = log_page_properties();
    properties.insert(
        "buildRunId".into(),
        json!({"type": "string", "format": "uuid"}),
    );
    json!({
        "type": "object",
        "properties": properties,
        "required": ["buildRunId"],
        "additionalProperties": false
    })
}

fn log_page_properties() -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([
        (
            "cursor".into(),
            json!({"type": "string", "pattern": "^v1:[0-9]+$"}),
        ),
        (
            "limit".into(),
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_LOG_LIMIT,
                "default": DEFAULT_LOG_LIMIT
            }),
        ),
        (
            "stream".into(),
            json!({"type": "string", "enum": ["stdout", "stderr"]}),
        ),
    ])
}

fn create_project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "minLength": 1, "maxLength": 100},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["name", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn create_environment_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 100},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "name", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn create_application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 63},
            "description": {
                "type": "string",
                "maxLength": APPLICATION_DESCRIPTION_MAX_CHARS,
                "default": ""
            },
            "releaseAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "name", "releaseAcl", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn publish_application_release_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "releaseAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "applicationId",
            "expectedVersion",
            "releaseAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn list_applications_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_APPLICATION_LIST_LIMIT
            }
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "applicationId"],
        "additionalProperties": false
    })
}

fn list_application_releases_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_APPLICATION_LIST_LIMIT
            }
        },
        "required": ["projectId", "applicationId"],
        "additionalProperties": false
    })
}

fn application_release_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "releaseId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "applicationId", "releaseId"],
        "additionalProperties": false
    })
}

fn open_application_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "releaseId": {"type": "string", "format": "uuid"},
            "initialVariables": {
                "type": "object",
                "x-a3s-max-canonical-bytes": APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES,
                "default": {}
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "applicationId", "releaseId", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn application_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "applicationId", "sessionId"],
        "additionalProperties": false
    })
}

fn close_application_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "applicationId",
            "sessionId",
            "expectedVersion",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn request_application_invocation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"},
            "ontologyId": {"type": "string", "format": "uuid"},
            "ontologyRevisionId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "responseMode": {
                "type": "string",
                "enum": ["asynchronous", "blocking", "streaming"]
            },
            "input": {
                "type": "object",
                "x-a3s-max-canonical-bytes": APPLICATION_INVOCATION_INPUT_MAX_BYTES
            },
            "timeoutSeconds": {
                "type": "integer",
                "minimum": 1,
                "maximum": WORKFLOW_RUN_MAX_TIMEOUT_SECONDS,
                "default": WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "applicationId",
            "sessionId",
            "ontologyId",
            "ontologyRevisionId",
            "responseMode",
            "input",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn application_invocation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"},
            "invocationId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "applicationId", "sessionId", "invocationId"],
        "additionalProperties": false
    })
}

fn cancel_application_invocation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"},
            "invocationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "applicationId",
            "sessionId",
            "invocationId",
            "expectedVersion",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn list_application_messages_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "sessionId": {"type": "string", "format": "uuid"},
            "afterSequence": {"type": "integer", "minimum": 0, "default": 0},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_APPLICATION_MESSAGE_REPLAY_LIMIT,
                "default": DEFAULT_APPLICATION_MESSAGE_REPLAY_LIMIT
            }
        },
        "required": ["projectId", "applicationId", "sessionId"],
        "additionalProperties": false
    })
}

fn create_connector_profile_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 63},
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "name",
            "definitionAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn revise_connector_profile_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "profileId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "profileId",
            "expectedVersion",
            "definitionAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn list_connector_profiles_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "required": ["projectId", "environmentId"],
        "additionalProperties": false
    })
}

fn connector_profile_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "profileId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId", "profileId"],
        "additionalProperties": false
    })
}

fn list_connector_revisions_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "profileId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "required": ["projectId", "environmentId", "profileId"],
        "additionalProperties": false
    })
}

fn connector_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "profileId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId", "profileId", "revisionId"],
        "additionalProperties": false
    })
}

fn create_durable_cell_application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 63},
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": DURABLE_CELL_APPLICATION_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "name",
            "definitionAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn revise_durable_cell_application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": DURABLE_CELL_APPLICATION_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "applicationId",
            "expectedVersion",
            "definitionAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn durable_cell_application_state_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "applicationId",
            "expectedVersion",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn list_durable_cell_applications_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT
            }
        },
        "required": ["projectId", "environmentId"],
        "additionalProperties": false
    })
}

fn durable_cell_application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId", "applicationId"],
        "additionalProperties": false
    })
}

fn list_durable_cell_revisions_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT,
                "default": DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT
            }
        },
        "required": ["projectId", "environmentId", "applicationId"],
        "additionalProperties": false
    })
}

fn durable_cell_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "environmentId", "applicationId", "revisionId"],
        "additionalProperties": false
    })
}

fn deploy_durable_cell_application_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"},
            "serviceProfileAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES
            },
            "storageProviderProfileAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES
            },
            "providerWorkloadAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": WORKLOAD_MANIFEST_MAX_BYTES
            },
            "storageBindingAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "applicationId",
            "revisionId",
            "serviceProfileAcl",
            "providerWorkloadAcl",
            "storageBindingAcl",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn publish_durable_cell_route_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "environmentId": {"type": "string", "format": "uuid"},
            "applicationId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"},
            "serviceProfileAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES
            },
            "gatewayScopeId": {"type": "string", "format": "uuid"},
            "domainClaimId": {"type": "string", "format": "uuid"},
            "hostname": {"type": "string", "minLength": 1, "maxLength": 253},
            "pathPrefix": {"type": "string", "minLength": 1, "maxLength": 2048},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "projectId",
            "environmentId",
            "applicationId",
            "revisionId",
            "serviceProfileAcl",
            "gatewayScopeId",
            "domainClaimId",
            "hostname",
            "pathPrefix",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn create_execution_template_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "definitionAcl": {
                "type": "string",
                "minLength": 1,
                "maxLength": EXECUTION_TEMPLATE_MAX_ACL_BYTES
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "definitionAcl", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn get_execution_template_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "templateId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["projectId", "templateId", "revisionId"],
        "additionalProperties": false
    })
}

fn list_execution_templates_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn form_document_schema() -> Value {
    json!({
        "type": "object",
        "description": "A native A3S Form document. Canonicalization and semantic validation remain owned by A3S Form.",
        "x-a3s-max-canonical-bytes": CLOUD_FORM_DOCUMENT_MAX_BYTES
    })
}

fn create_form_draft_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 120},
            "description": {"type": "string", "maxLength": 4096, "default": ""},
            "document": form_document_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "name", "document", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revise_form_draft_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "formId": {"type": "string", "format": "uuid"},
            "name": {"type": "string", "minLength": 1, "maxLength": 120},
            "description": {"type": "string", "maxLength": 4096, "default": ""},
            "document": form_document_schema(),
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "formId",
            "name",
            "document",
            "expectedVersion",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn form_release_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "formId": {"type": "string", "format": "uuid"},
            "releaseId": {"type": "string", "format": "uuid"}
        },
        "required": ["formId", "releaseId"],
        "additionalProperties": false
    })
}

fn publish_form_release_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "formId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["formId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn create_ontology_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "acl": {"type": "string", "minLength": 1, "maxLength": 1048576},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "acl", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revise_ontology_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ontologyId": {"type": "string", "format": "uuid"},
            "acl": {"type": "string", "minLength": 1, "maxLength": 1048576},
            "expectedVersion": expected_version_schema(),
            "migrationRuleId": {
                "type": "string",
                "minLength": 1,
                "maxLength": 96,
                "pattern": "^[A-Za-z0-9_-]+$"
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["ontologyId", "acl", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn ontology_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ontologyId": {"type": "string", "format": "uuid"},
            "revisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["ontologyId", "revisionId"],
        "additionalProperties": false
    })
}

fn ontology_diff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ontologyId": {"type": "string", "format": "uuid"},
            "fromRevisionId": {"type": "string", "format": "uuid"},
            "toRevisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["ontologyId", "fromRevisionId", "toRevisionId"],
        "additionalProperties": false
    })
}

fn workflow_payloads_schema() -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "maxItems": 2048,
        "items": {
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["configuration", "data_schema", "policy"]},
                "acl": {"type": "string", "minLength": 1, "maxLength": 262144}
            },
            "required": ["kind", "acl"],
            "additionalProperties": false
        }
    })
}

fn workflow_semantic_contracts_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "descriptorBindingsAcl": {"type": "string", "minLength": 1, "maxLength": 524288},
            "descriptorRegistryAcl": {"type": "string", "minLength": 1, "maxLength": 4194304},
            "variableContractAcl": {"type": "string", "minLength": 1, "maxLength": 2097152},
            "variableDefaultsAcl": {"type": "string", "minLength": 1, "maxLength": 2097152},
            "compositeRegionsAcl": {"type": "string", "minLength": 1, "maxLength": 524288}
        },
        "required": ["descriptorBindingsAcl", "descriptorRegistryAcl", "variableContractAcl"],
        "additionalProperties": false
    })
}

fn create_workflow_definition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "definitionAcl": {"type": "string", "minLength": 1, "maxLength": 1048576},
            "payloads": workflow_payloads_schema(),
            "semanticContracts": workflow_semantic_contracts_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "definitionAcl", "payloads", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revise_workflow_definition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowDefinitionId": {"type": "string", "format": "uuid"},
            "definitionAcl": {"type": "string", "minLength": 1, "maxLength": 1048576},
            "payloads": workflow_payloads_schema(),
            "semanticContracts": workflow_semantic_contracts_schema(),
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": [
            "workflowDefinitionId",
            "definitionAcl",
            "payloads",
            "expectedVersion",
            "idempotencyKey"
        ],
        "additionalProperties": false
    })
}

fn workflow_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowDefinitionId": {"type": "string", "format": "uuid"},
            "workflowRevisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["workflowDefinitionId", "workflowRevisionId"],
        "additionalProperties": false
    })
}

fn create_workflow_goal_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "acl": {"type": "string", "minLength": 1, "maxLength": 262144},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "acl", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn workflow_plan_revision_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowGoalId": {"type": "string", "format": "uuid"},
            "planRevisionId": {"type": "string", "format": "uuid"}
        },
        "required": ["workflowGoalId", "planRevisionId"],
        "additionalProperties": false
    })
}

fn start_workflow_run_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "workflowGoalId": {"type": "string", "format": "uuid"},
            "planRevisionId": {"type": "string", "format": "uuid"},
            "timeoutSeconds": {
                "type": "integer",
                "minimum": 1,
                "maximum": 2592000,
                "default": 86400
            },
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["projectId", "workflowGoalId", "planRevisionId", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn cancel_workflow_run_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowRunId": {"type": "string", "format": "uuid"},
            "reason": {"type": "string", "minLength": 1, "maxLength": 4096},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["workflowRunId", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn list_workflow_runs_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 100}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn wait_workflow_run_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowRunId": {"type": "string", "format": "uuid"},
            "timeoutSeconds": {"type": "integer", "minimum": 0, "maximum": 30, "default": 30}
        },
        "required": ["workflowRunId"],
        "additionalProperties": false
    })
}

fn workflow_run_history_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "workflowRunId": {"type": "string", "format": "uuid"},
            "afterSequence": {"type": "integer", "minimum": 0, "default": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 100}
        },
        "required": ["workflowRunId"],
        "additionalProperties": false
    })
}

fn list_human_tasks_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {"type": "string", "format": "uuid"},
            "status": {
                "type": "string",
                "enum": [
                    "pending_activation",
                    "ready",
                    "claimed",
                    "completed",
                    "expired",
                    "cancelled"
                ]
            },
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 100}
        },
        "required": ["projectId"],
        "additionalProperties": false
    })
}

fn human_task_mutation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "humanTaskId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["humanTaskId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn human_task_submission_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "humanTaskId": {"type": "string", "format": "uuid"},
            "submission": form_interaction_submission_schema()
        },
        "required": ["humanTaskId", "submission"],
        "additionalProperties": false
    })
}

fn membership_role_schema() -> Value {
    json!({"type": "string", "enum": ["owner", "admin", "member", "restricted"]})
}

fn expected_version_schema() -> Value {
    json!({"type": "integer", "minimum": 1})
}

fn create_membership_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "principalKind": {"type": "string", "enum": ["human", "service"]},
            "name": {"type": "string", "minLength": 1, "maxLength": 63},
            "role": membership_role_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["principalKind", "name", "role", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn change_membership_role_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "membershipId": {"type": "string", "format": "uuid"},
            "role": membership_role_schema(),
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["membershipId", "role", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revoke_membership_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "membershipId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["membershipId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn create_membership_invitation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "principalId": {"type": "string", "format": "uuid"},
            "role": membership_role_schema(),
            "expiresAt": {"type": "string", "format": "date-time"},
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["principalId", "role", "expiresAt", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn membership_invitation_mutation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "invitationId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["invitationId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn resource_grant_scope_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["project"]},
                    "projectId": {"type": "string", "format": "uuid"}
                },
                "required": ["kind", "projectId"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["environment"]},
                    "projectId": {"type": "string", "format": "uuid"},
                    "environmentId": {"type": "string", "format": "uuid"}
                },
                "required": ["kind", "projectId", "environmentId"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["node"]},
                    "nodeId": {"type": "string", "format": "uuid"}
                },
                "required": ["kind", "nodeId"],
                "additionalProperties": false
            }
        ]
    })
}

fn create_resource_grant_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "membershipId": {"type": "string", "format": "uuid"},
            "scope": resource_grant_scope_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["membershipId", "scope", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn revoke_resource_grant_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "resourceGrantId": {"type": "string", "format": "uuid"},
            "expectedVersion": expected_version_schema(),
            "idempotencyKey": idempotency_key_schema()
        },
        "required": ["resourceGrantId", "expectedVersion", "idempotencyKey"],
        "additionalProperties": false
    })
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "minLength": 1, "maxLength": 128},
            "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 20}
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::application::RESOURCE_GRANT_SCOPES_CLAIM;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::shared_kernel::domain::{NodeId, ProjectId};

    fn restricted_principal(scope: ResourceGrantScope) -> AuthPrincipal {
        AuthPrincipal::new("principal")
            .with_role("organization_restricted")
            .with_scope(ApiTokenScope::WORKLOAD_WRITE)
            .with_scope(ApiTokenScope::BUILD_WRITE)
            .with_scope(ApiTokenScope::ROUTE_WRITE)
            .with_scope(ApiTokenScope::FORM_WRITE)
            .with_scope(ApiTokenScope::ONTOLOGY_WRITE)
            .with_scope(ApiTokenScope::WORKFLOW_WRITE)
            .with_scope(ApiTokenScope::EXECUTION_WRITE)
            .with_scope(ApiTokenScope::CONNECTOR_WRITE)
            .with_scope(ApiTokenScope::APPLICATION_WRITE)
            .with_scope(ApiTokenScope::CLOUD_READ)
            .with_claim("organization_role", "restricted")
            .expect("role")
            .with_claim(RESOURCE_GRANT_SCOPES_CLAIM, [scope])
            .expect("grants")
    }

    #[test]
    fn application_delivery_annotations_match_their_effects() {
        for (tool, read_only, destructive) in [
            (ManagementTool::ApplicationSessionsClose, false, true),
            (ManagementTool::ApplicationSessionsReplay, true, false),
            (ManagementTool::ApplicationInvocationsCancel, false, true),
        ] {
            let definition = tool.definition();
            assert_eq!(
                definition["annotations"]["readOnlyHint"].as_bool(),
                Some(read_only),
                "{} read-only annotation",
                tool.name()
            );
            assert_eq!(
                definition["annotations"]["destructiveHint"].as_bool(),
                Some(destructive),
                "{} destructive annotation",
                tool.name()
            );
        }
    }

    #[test]
    fn restricted_catalog_exposes_direct_and_filtered_collection_tools() {
        let principal = restricted_principal(ResourceGrantScope::Project {
            project_id: ProjectId::new(),
        });
        assert!(ManagementTool::EnvironmentsList.visible_to(&principal));
        assert!(ManagementTool::ExecutionTemplatesCreate.visible_to(&principal));
        assert!(ManagementTool::ExecutionTemplatesGet.visible_to(&principal));
        assert!(ManagementTool::ExecutionTemplatesList.visible_to(&principal));
        assert!(ManagementTool::ApplicationsCreate.visible_to(&principal));
        assert!(ManagementTool::ApplicationsList.visible_to(&principal));
        assert!(ManagementTool::ApplicationsGet.visible_to(&principal));
        assert!(ManagementTool::ApplicationReleasesPublish.visible_to(&principal));
        assert!(ManagementTool::ApplicationReleasesList.visible_to(&principal));
        assert!(ManagementTool::ApplicationReleasesGet.visible_to(&principal));
        assert!(ManagementTool::ConnectorProfilesCreate.visible_to(&principal));
        assert!(ManagementTool::ConnectorProfilesRevise.visible_to(&principal));
        assert!(ManagementTool::ConnectorProfilesList.visible_to(&principal));
        assert!(ManagementTool::ConnectorProfilesGet.visible_to(&principal));
        assert!(ManagementTool::ConnectorRevisionsList.visible_to(&principal));
        assert!(ManagementTool::ConnectorRevisionsGet.visible_to(&principal));
        assert!(ManagementTool::FormsList.visible_to(&principal));
        assert!(ManagementTool::FormsGet.visible_to(&principal));
        assert!(ManagementTool::FormsRevise.visible_to(&principal));
        assert!(ManagementTool::FormReleasesGet.visible_to(&principal));
        assert!(ManagementTool::FormReleasesList.visible_to(&principal));
        assert!(ManagementTool::FormReleasesPublish.visible_to(&principal));
        assert!(!ManagementTool::NodesGet.visible_to(&principal));
        assert!(ManagementTool::ProjectsList.visible_to(&principal));
        assert!(ManagementTool::Search.visible_to(&principal));
        assert!(ManagementTool::WorkloadsGet.visible_to(&principal));
        assert!(ManagementTool::WorkloadLogsGet.visible_to(&principal));
        assert!(ManagementTool::WorkloadsStop.visible_to(&principal));
        assert!(ManagementTool::WorkloadsRollback.visible_to(&principal));
        assert!(ManagementTool::DeploymentsGet.visible_to(&principal));
        assert!(ManagementTool::DeploymentsCancel.visible_to(&principal));
        assert!(ManagementTool::RoutesGet.visible_to(&principal));
        assert!(ManagementTool::BuildRunsGet.visible_to(&principal));
        assert!(ManagementTool::BuildRunLogsGet.visible_to(&principal));
        assert!(ManagementTool::BuildEvidenceGet.visible_to(&principal));
        assert!(ManagementTool::BuildRunsCancel.visible_to(&principal));
        assert!(ManagementTool::BuildRunsRetry.visible_to(&principal));
        for tool in [
            ManagementTool::OntologiesCreate,
            ManagementTool::OntologiesGet,
            ManagementTool::OntologiesList,
            ManagementTool::OntologiesRevise,
            ManagementTool::OntologyRevisionsGet,
            ManagementTool::OntologyRevisionsList,
            ManagementTool::OntologyRevisionsDiff,
            ManagementTool::WorkflowNodeCatalogGet,
            ManagementTool::WorkflowDefinitionsCreate,
            ManagementTool::WorkflowDefinitionsGet,
            ManagementTool::WorkflowDefinitionsList,
            ManagementTool::WorkflowDefinitionsRevise,
            ManagementTool::WorkflowRevisionsGet,
            ManagementTool::WorkflowRevisionsList,
            ManagementTool::WorkflowGoalsCreate,
            ManagementTool::WorkflowGoalsGet,
            ManagementTool::WorkflowGoalsList,
            ManagementTool::WorkflowPlanRevisionsGet,
            ManagementTool::WorkflowRunsStart,
            ManagementTool::WorkflowRunsCancel,
            ManagementTool::WorkflowRunsGet,
            ManagementTool::WorkflowRunsList,
            ManagementTool::WorkflowRunsWait,
            ManagementTool::WorkflowRunOutputGet,
            ManagementTool::WorkflowRunHistoryGet,
            ManagementTool::WorkflowRunVariablesGet,
            ManagementTool::HumanTasksClaim,
            ManagementTool::HumanTasksGet,
            ManagementTool::HumanTasksList,
            ManagementTool::HumanTasksRelease,
            ManagementTool::HumanTasksSubmit,
        ] {
            assert!(tool.visible_to(&principal), "{}", tool.name());
        }
    }

    #[test]
    fn node_grant_does_not_expose_project_or_environment_tools() {
        let principal = restricted_principal(ResourceGrantScope::Node {
            node_id: NodeId::new(),
        });
        assert!(ManagementTool::NodesGet.visible_to(&principal));
        assert!(ManagementTool::NodesList.visible_to(&principal));
        assert!(ManagementTool::Search.visible_to(&principal));
        assert!(!ManagementTool::EnvironmentsList.visible_to(&principal));
        assert!(!ManagementTool::ExecutionTemplatesCreate.visible_to(&principal));
        assert!(!ManagementTool::ExecutionTemplatesGet.visible_to(&principal));
        assert!(!ManagementTool::ExecutionTemplatesList.visible_to(&principal));
        assert!(!ManagementTool::ApplicationsCreate.visible_to(&principal));
        assert!(!ManagementTool::ApplicationsList.visible_to(&principal));
        assert!(!ManagementTool::ApplicationsGet.visible_to(&principal));
        assert!(!ManagementTool::ApplicationReleasesPublish.visible_to(&principal));
        assert!(!ManagementTool::ApplicationReleasesList.visible_to(&principal));
        assert!(!ManagementTool::ApplicationReleasesGet.visible_to(&principal));
        assert!(!ManagementTool::ConnectorProfilesCreate.visible_to(&principal));
        assert!(!ManagementTool::ConnectorProfilesRevise.visible_to(&principal));
        assert!(!ManagementTool::ConnectorProfilesList.visible_to(&principal));
        assert!(!ManagementTool::ConnectorProfilesGet.visible_to(&principal));
        assert!(!ManagementTool::ConnectorRevisionsList.visible_to(&principal));
        assert!(!ManagementTool::ConnectorRevisionsGet.visible_to(&principal));
        assert!(!ManagementTool::ProjectsList.visible_to(&principal));
        assert!(!ManagementTool::FormsList.visible_to(&principal));
        assert!(!ManagementTool::FormsGet.visible_to(&principal));
        assert!(!ManagementTool::FormsRevise.visible_to(&principal));
        assert!(!ManagementTool::FormReleasesGet.visible_to(&principal));
        assert!(!ManagementTool::FormReleasesList.visible_to(&principal));
        assert!(!ManagementTool::FormReleasesPublish.visible_to(&principal));
        assert!(!ManagementTool::WorkloadsGet.visible_to(&principal));
        assert!(!ManagementTool::WorkloadsStop.visible_to(&principal));
        assert!(!ManagementTool::DeploymentsGet.visible_to(&principal));
        assert!(!ManagementTool::DeploymentsCancel.visible_to(&principal));
        assert!(!ManagementTool::RoutesGet.visible_to(&principal));
        assert!(!ManagementTool::BuildRunsGet.visible_to(&principal));
        assert!(!ManagementTool::BuildRunsCancel.visible_to(&principal));
        for tool in [
            ManagementTool::OntologiesCreate,
            ManagementTool::OntologiesGet,
            ManagementTool::OntologiesList,
            ManagementTool::OntologiesRevise,
            ManagementTool::OntologyRevisionsGet,
            ManagementTool::OntologyRevisionsList,
            ManagementTool::OntologyRevisionsDiff,
            ManagementTool::WorkflowNodeCatalogGet,
            ManagementTool::WorkflowDefinitionsCreate,
            ManagementTool::WorkflowDefinitionsGet,
            ManagementTool::WorkflowDefinitionsList,
            ManagementTool::WorkflowDefinitionsRevise,
            ManagementTool::WorkflowRevisionsGet,
            ManagementTool::WorkflowRevisionsList,
            ManagementTool::WorkflowGoalsCreate,
            ManagementTool::WorkflowGoalsGet,
            ManagementTool::WorkflowGoalsList,
            ManagementTool::WorkflowPlanRevisionsGet,
            ManagementTool::WorkflowRunsStart,
            ManagementTool::WorkflowRunsCancel,
            ManagementTool::WorkflowRunsGet,
            ManagementTool::WorkflowRunsList,
            ManagementTool::WorkflowRunsWait,
            ManagementTool::WorkflowRunOutputGet,
            ManagementTool::WorkflowRunHistoryGet,
            ManagementTool::WorkflowRunVariablesGet,
            ManagementTool::HumanTasksClaim,
            ManagementTool::HumanTasksGet,
            ManagementTool::HumanTasksList,
            ManagementTool::HumanTasksRelease,
            ManagementTool::HumanTasksSubmit,
        ] {
            assert!(!tool.visible_to(&principal), "{}", tool.name());
        }
    }
}
