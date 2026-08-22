import {
  type Application,
  type ApplicationExpectedVersionInput,
  type ApplicationInvocation,
  type ApplicationInvocationCancellationResult,
  type ApplicationInvocationMutationResult,
  type ApplicationMessage,
  type ApplicationMutationResult,
  type ApplicationRelease,
  type ApplicationSession,
  type ApplicationSessionMutationResult,
  type ApplicationSessionReplay,
  type CreateApplicationInput,
  DEFAULT_APPLICATION_LIST_LIMIT,
  DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT,
  type OpenApplicationSessionInput,
  type PublishApplicationReleaseInput,
  type RequestApplicationInvocationInput,
  validateApplicationDescription,
  validateApplicationExpectedVersion,
  validateApplicationInitialVariables,
  validateApplicationInvocationInput,
  validateApplicationInvocationTimeout,
  validateApplicationListLimit,
  validateApplicationMessageList,
  validateApplicationName,
  validateApplicationReleaseAcl,
  validateApplicationResponseMode,
} from './applications';
import { type AuditRecordPage, type AuditRecordQuery, encodeAuditRecordQuery } from './audit';
import {
  type ConnectorProfile,
  type ConnectorProfileMutationResult,
  type ConnectorProfileRecord,
  type ConnectorRevision,
  type CreateConnectorProfileInput,
  DEFAULT_CONNECTOR_LIST_LIMIT,
  type ReviseConnectorProfileInput,
  validateConnectorDefinitionAcl,
  validateConnectorExpectedVersion,
  validateConnectorListLimit,
  validateConnectorProfileName,
} from './connectors';
import type { CloudDiagnostics, CloudHealthReport, CloudPlatformInfo } from './diagnostics';
import {
  type CreateDurableCellApplicationInput,
  DEFAULT_DURABLE_CELL_LIST_LIMIT,
  type DeployDurableCellApplicationInput,
  type DurableCellApplication,
  type DurableCellApplicationMutationResult,
  type DurableCellApplicationRecord,
  type DurableCellApplicationRevision,
  type DurableCellDeploymentResult,
  type DurableCellRoutePublicationResult,
  type PublishDurableCellApplicationRouteInput,
  type ReviseDurableCellApplicationInput,
  validateDeployDurableCellApplicationInput,
  validateDurableCellApplicationAcl,
  validateDurableCellApplicationName,
  validateDurableCellExpectedVersion,
  validateDurableCellListLimit,
  validatePublishDurableCellApplicationRouteInput,
} from './durable-cells';
import { CloudApiError } from './error';
import { type CloudLogQuery, encodeLogQuery } from './log-query';
import {
  encodeNotificationAlertPolicyQuery,
  encodeNotificationQuery,
  encodeOutboundNotificationSubscriptionQuery,
  type Notification,
  type NotificationAlertPolicy,
  type NotificationAlertPolicyMutationResult,
  type NotificationAlertPolicyPage,
  type NotificationAlertPolicyQuery,
  type NotificationMutationResult,
  type NotificationPage,
  type NotificationQuery,
  type OutboundNotificationSubscription,
  type OutboundNotificationSubscriptionMutationResult,
  type OutboundNotificationSubscriptionPage,
  type OutboundNotificationSubscriptionQuery,
  validateExpectedNotificationAlertPolicyVersion,
  validateExpectedNotificationVersion,
  validateExpectedOutboundNotificationSubscriptionVersion,
  validateNotificationAlertPolicyAcl,
  validateNotificationAlertPolicyId,
  validateNotificationId,
  validateOutboundNotificationSubscriptionAcl,
  validateOutboundNotificationSubscriptionId,
} from './notifications';
import { readHealthResponse, readResponse } from './response';
import { DEFAULT_SEARCH_LIMIT, validateSearchRequest } from './search';
import { type CloudSequenceQuery, encodeQueryParameters, encodeSequenceQuery } from './sequence-query';
import type {
  AddNodePoolMembersInput,
  AgentConversation,
  AgentConversationMutationResult,
  AgentExecution,
  AgentExecutionChangeSet,
  AgentExecutionEventsPage,
  AgentExecutionMutationResult,
  ApiToken,
  ApiTokenMutationResult,
  Asset,
  AssetMutationResult,
  AssetRelease,
  AssetReleaseMutationResult,
  BuildEvidence,
  BuildRun,
  BuildRunLogsPage,
  CancelBuildRunResult,
  CancelDeploymentResult,
  CancelNodePoolMaintenanceInput,
  CancelWorkflowRunInput,
  CompleteRecipientContactVerificationInput,
  CreateApiTokenInput,
  CreateAssetInput,
  CreateAssetReleaseInput,
  CreateExecutionInput,
  CreateExecutionTemplateInput,
  CreateGatewayScopeInput,
  CreateGithubRepositorySubscriptionInput,
  CreateMcpCredentialInput,
  CreateMembershipInput,
  CreateMembershipInvitationInput,
  CreateNodePoolInput,
  CreateResourceGrantInput,
  Deployment,
  DomainClaim,
  DomainClaimMutationResult,
  EnrollmentToken,
  Environment,
  EnvironmentMutationResult,
  Execution,
  ExecutionMutationResult,
  ExecutionTemplateMutationResult,
  ExecutionTemplateRevision,
  FormDraft,
  FormDraftInput,
  FormDraftMutationResult,
  FormPublicationMutationResult,
  FormRelease,
  GatewayCertificate,
  GatewayScope,
  GatewayScopeMutationResult,
  GithubConnection,
  GithubConnectionInstall,
  GithubRepositorySubscription,
  GithubRepositorySubscriptionMutationResult,
  HumanTask,
  HumanTaskInteractionSubmission,
  HumanTaskMutationResult,
  HumanTaskStatus,
  HumanTaskSummary,
  IssueEnrollmentTokenInput,
  ListHumanTasksOptions,
  ListWorkflowRunsOptions,
  McpCredential,
  McpCredentialDeliveryResult,
  McpCredentialMutationResult,
  McpRoutePolicy,
  McpRoutePolicyMutationResult,
  McpServiceProfile,
  McpServiceProfileMutationResult,
  Membership,
  MembershipInvitation,
  MembershipInvitationAcceptanceResult,
  MembershipInvitationMutationResult,
  MembershipMutationResult,
  MembershipRole,
  Node,
  NodePool,
  OidcAuthorizationStart,
  Ontology,
  OntologyDiff,
  OntologyMutationResult,
  OntologyRevision,
  OntologyRevisionSummary,
  Operation,
  Organization,
  OrganizationMutationResult,
  PluginCatalogInspection,
  PluginCatalogInspectRequest,
  PluginCatalogPage,
  PluginCatalogSearchRequest,
  PluginRegistry,
  Project,
  ProjectAttributionMutationResult,
  ProjectAttributionProfile,
  ProjectMutationResult,
  PublishFormReleaseOptions,
  PublishRouteInput,
  PublishWorkflowDefinitionInput,
  RecipientContact,
  RecipientContactMutationResult,
  RequestNodePoolMemberRemovalInput,
  RequestRecipientContactVerificationInput,
  ResolveSourceRevisionInput,
  ResourceGrant,
  ResourceGrantMutationResult,
  RetryBuildRunResult,
  ReviseFormDraftOptions,
  ReviseOntologyOptions,
  ReviseWorkflowDefinitionOptions,
  RevokeMcpCredentialInput,
  RotateMcpCredentialInput,
  Route,
  RoutePublicationResult,
  ScheduleNodePoolMaintenanceInput,
  SearchResult,
  Secret,
  SecretDetails,
  SecretMutationResult,
  ServiceTemplate,
  SourceRevision,
  SourceRevisionMutationResult,
  SourceWorkloadTemplate,
  StartAgentExecutionInput,
  StartWorkflowRunInput,
  StopWorkloadResult,
  UpdateProjectAttributionInput,
  WaitWorkflowRunOptions,
  WorkflowDefinition,
  WorkflowDefinitionMutationResult,
  WorkflowGoal,
  WorkflowGoalMutationResult,
  WorkflowNodeCatalog,
  WorkflowPlanRevision,
  WorkflowRevision,
  WorkflowRevisionSummary,
  WorkflowRun,
  WorkflowRunHistoryOptions,
  WorkflowRunHistoryPage,
  WorkflowRunMutationResult,
  WorkflowRunOutput,
  WorkflowRunVariableInspection,
  Workload,
  WorkloadDeploymentResult,
  WorkloadLogStreamFilter,
  WorkloadLogsPage,
} from './types';
import {
  validateExpectedRecipientContactVersion,
  validateRecipientContactAddress,
  validateRecipientContactProof,
} from './identity';
import {
  validateApiTokenInput,
  validateEnrollmentTokenInput,
  validateExecutionTemplateAcl,
  validateExpectedHumanTaskVersion,
  validateExpectedMcpCredentialVersion,
  validateExpectedMembershipInvitationVersion,
  validateExpectedMembershipVersion,
  validateExpectedNodeVersion,
  validateExpectedProjectVersion,
  validateExpectedResourceGrantVersion,
  validateFormDraftInput,
  validateFormVersionControl,
  validateMcpCredentialExpiry,
  validateMcpRoutePolicyAcl,
  validateMcpServiceProfileAcl,
  validateMembershipInput,
  validateMembershipInvitationInput,
  validateMembershipRole,
  validateOntologyAcl,
  validateOntologyRevisionControl,
  validateProjectAttributionInput,
  validateResourceGrantInput,
  validateSecretValue,
  validateWorkflowDefinitionPublication,
  validateWorkflowGoalAcl,
  validateWorkflowRevisionControl,
  validateWorkloadAcl,
} from './validation';

export { CloudApiError } from './error';

export type CloudFetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export interface CloudApiClientOptions {
  fetch?: CloudFetch;
  requestTimeoutMs?: number;
}

const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;
const MAX_REQUEST_TIMEOUT_MS = 300_000;
export const CLOUD_API_MAJOR_VERSION = 1;
export const CLOUD_API_CONTRACT_VERSION = '1.52.0';
export const DEFAULT_CLOUD_API_BASE_PATH = `/api/v${CLOUD_API_MAJOR_VERSION}`;
export const A3S_ACL_MEDIA_TYPE = 'application/vnd.a3s.acl';
export const MAX_WORKFLOW_RUN_TIMEOUT_SECONDS = 2_592_000;
export const MAX_WORKFLOW_RUN_LIST_LIMIT = 200;
export const MAX_WORKFLOW_RUN_HISTORY_LIMIT = 100;
export const MAX_WORKFLOW_RUN_WAIT_SECONDS = 30;
export const MAX_HUMAN_TASK_LIST_LIMIT = 200;
export const MAX_EXECUTION_TEMPLATE_LIST_LIMIT = 200;
export const DEFAULT_WORKFLOW_RUN_WAIT_SECONDS = 25;
const HUMAN_TASK_STATUSES: ReadonlySet<HumanTaskStatus> = new Set([
  'pending_activation',
  'ready',
  'claimed',
  'completed',
  'expired',
  'cancelled',
]);

export type { CloudLogQuery } from './log-query';
export type { CloudSequenceQuery } from './sequence-query';
export {
  MAX_ACL_DOCUMENT_BYTES,
  MAX_EXECUTION_TEMPLATE_ACL_BYTES,
  MAX_FORM_DOCUMENT_BYTES,
  MAX_MCP_ROUTE_POLICY_ACL_BYTES,
  MAX_MCP_SERVICE_PROFILE_ACL_BYTES,
  MAX_ONTOLOGY_ACL_BYTES,
  MAX_SECRET_VALUE_BYTES,
  MAX_WORKFLOW_COMPOSITE_REGIONS_ACL_BYTES,
  MAX_WORKFLOW_DEFINITION_ACL_BYTES,
  MAX_WORKFLOW_GOAL_ACL_BYTES,
  MAX_WORKFLOW_PAYLOAD_ACL_BYTES,
  MAX_WORKFLOW_REVISION_PAYLOAD_BYTES,
  MAX_WORKFLOW_REVISION_PAYLOADS,
  MAX_WORKFLOW_STEP_DESCRIPTOR_BINDINGS_ACL_BYTES,
  MAX_WORKFLOW_STEP_DESCRIPTOR_REGISTRY_ACL_BYTES,
  MAX_WORKFLOW_VARIABLE_CONTRACT_ACL_BYTES,
  MAX_WORKFLOW_VARIABLE_DEFAULTS_ACL_BYTES,
  MAX_WORKLOAD_ACL_BYTES,
  validateExecutionTemplateAcl,
  validateFormDraftInput,
  validateFormVersionControl,
} from './validation';

export function isValidIdempotencyKey(value: string): boolean {
  return /^[A-Za-z0-9._~:/-]{1,255}$/.test(value);
}

export class CloudApi {
  readonly baseUrl: string;
  private readonly token: string | undefined;
  private readonly fetcher: CloudFetch;
  private readonly requestTimeoutMs: number;

  constructor(
    token: string | undefined,
    baseUrl = DEFAULT_CLOUD_API_BASE_PATH,
    options: CloudApiClientOptions = {}
  ) {
    const normalizedBaseUrl = baseUrl.replace(/\/+$/, '');
    if (!normalizedBaseUrl) {
      throw new TypeError('baseUrl must not be empty');
    }
    const requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    if (
      !Number.isSafeInteger(requestTimeoutMs) ||
      requestTimeoutMs < 1 ||
      requestTimeoutMs > MAX_REQUEST_TIMEOUT_MS
    ) {
      throw new RangeError(`requestTimeoutMs must be between 1 and ${MAX_REQUEST_TIMEOUT_MS}`);
    }
    this.token = token;
    this.baseUrl = normalizedBaseUrl;
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.requestTimeoutMs = requestTimeoutMs;
  }

  getPlatform(signal?: AbortSignal): Promise<CloudPlatformInfo> {
    return this.get('/platform', signal);
  }

  getLiveness(signal?: AbortSignal): Promise<CloudHealthReport> {
    return this.getHealth('/health/live', signal);
  }

  getReadiness(signal?: AbortSignal): Promise<CloudHealthReport> {
    return this.getHealth('/health/ready', signal);
  }

  async getDiagnostics(signal?: AbortSignal): Promise<CloudDiagnostics> {
    const [platform, liveness, readiness] = await Promise.all([
      this.getPlatform(signal),
      this.getLiveness(signal),
      this.getReadiness(signal),
    ]);
    return { platform, liveness, readiness };
  }

  listOrganizations(signal?: AbortSignal): Promise<Organization[]> {
    return this.get('/organizations', signal);
  }

  oidcLoginUrl(organizationId: string, providerKey: string): string {
    validateOidcProviderKey(providerKey);
    const query = new URLSearchParams({ organization_id: organizationId });
    return `${this.baseUrl}/identity/oidc/${encodeURIComponent(providerKey)}/login?${query.toString()}`;
  }

  beginOidcLink(
    organizationId: string,
    providerKey: string,
    signal?: AbortSignal
  ): Promise<OidcAuthorizationStart> {
    validateOidcProviderKey(providerKey);
    return this.request(
      'POST',
      `/organizations/${encodeURIComponent(organizationId)}/identity/oidc/${encodeURIComponent(providerKey)}/link`,
      { credentials: 'include', signal }
    );
  }

  createOrganization(
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OrganizationMutationResult> {
    return this.postJson('/organizations', idempotencyKey, { name }, signal);
  }

  listApiTokens(organizationId: string, signal?: AbortSignal): Promise<ApiToken[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/api-tokens`, signal);
  }

  getApiToken(organizationId: string, tokenId: string, signal?: AbortSignal): Promise<ApiToken> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens/${encodeURIComponent(tokenId)}`,
      signal
    );
  }

  createApiToken(
    organizationId: string,
    input: CreateApiTokenInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApiTokenMutationResult> {
    validateApiTokenInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens`,
      idempotencyKey,
      input,
      signal
    );
  }

  revokeApiToken(
    organizationId: string,
    tokenId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApiTokenMutationResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens/${encodeURIComponent(tokenId)}`,
      idempotencyKey,
      signal
    );
  }

  listRecipientContacts(organizationId: string, signal?: AbortSignal): Promise<RecipientContact[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/recipient-contacts`, signal);
  }

  getRecipientContact(
    organizationId: string,
    recipientContactId: string,
    signal?: AbortSignal
  ): Promise<RecipientContact> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/recipient-contacts/${encodeURIComponent(recipientContactId)}`,
      signal
    );
  }

  requestRecipientContactVerification(
    organizationId: string,
    input: RequestRecipientContactVerificationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RecipientContactMutationResult> {
    validateRecipientContactAddress(input?.address);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/recipient-contacts`,
      idempotencyKey,
      { address: input.address },
      signal
    );
  }

  completeRecipientContactVerification(
    organizationId: string,
    recipientContactId: string,
    input: CompleteRecipientContactVerificationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RecipientContactMutationResult> {
    validateRecipientContactProof(input?.proof);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/recipient-contacts/${encodeURIComponent(recipientContactId)}/verification`,
      idempotencyKey,
      { proof: input.proof },
      signal
    );
  }

  revokeRecipientContact(
    organizationId: string,
    recipientContactId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RecipientContactMutationResult> {
    validateExpectedRecipientContactVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/recipient-contacts/${encodeURIComponent(recipientContactId)}/revocation`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listMemberships(organizationId: string, signal?: AbortSignal): Promise<Membership[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/memberships`, signal);
  }

  getMembership(organizationId: string, membershipId: string, signal?: AbortSignal): Promise<Membership> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/memberships/${encodeURIComponent(membershipId)}`,
      signal
    );
  }

  createMembership(
    organizationId: string,
    input: CreateMembershipInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipMutationResult> {
    validateMembershipInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/memberships`,
      idempotencyKey,
      input,
      signal
    );
  }

  changeMembershipRole(
    organizationId: string,
    membershipId: string,
    role: MembershipRole,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipMutationResult> {
    validateMembershipRole(role);
    validateExpectedMembershipVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/memberships/${encodeURIComponent(membershipId)}/role`,
      idempotencyKey,
      { role, expectedVersion },
      signal
    );
  }

  revokeMembership(
    organizationId: string,
    membershipId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipMutationResult> {
    validateExpectedMembershipVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/memberships/${encodeURIComponent(membershipId)}/revocation`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listMembershipInvitations(organizationId: string, signal?: AbortSignal): Promise<MembershipInvitation[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/membership-invitations`, signal);
  }

  getMembershipInvitation(
    organizationId: string,
    invitationId: string,
    signal?: AbortSignal
  ): Promise<MembershipInvitation> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/membership-invitations/${encodeURIComponent(invitationId)}`,
      signal
    );
  }

  listMyMembershipInvitations(signal?: AbortSignal): Promise<MembershipInvitation[]> {
    return this.get('/membership-invitations', signal);
  }

  createMembershipInvitation(
    organizationId: string,
    input: CreateMembershipInvitationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipInvitationMutationResult> {
    validateMembershipInvitationInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/membership-invitations`,
      idempotencyKey,
      input,
      signal
    );
  }

  acceptMembershipInvitation(
    invitationId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipInvitationAcceptanceResult> {
    validateExpectedMembershipInvitationVersion(expectedVersion);
    return this.postJson(
      `/membership-invitations/${encodeURIComponent(invitationId)}/acceptance`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  revokeMembershipInvitation(
    organizationId: string,
    invitationId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<MembershipInvitationMutationResult> {
    validateExpectedMembershipInvitationVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/membership-invitations/${encodeURIComponent(invitationId)}/revocation`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listResourceGrants(
    organizationId: string,
    membershipId: string,
    signal?: AbortSignal
  ): Promise<ResourceGrant[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/memberships/${encodeURIComponent(membershipId)}/resource-grants`,
      signal
    );
  }

  getResourceGrant(
    organizationId: string,
    resourceGrantId: string,
    signal?: AbortSignal
  ): Promise<ResourceGrant> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/resource-grants/${encodeURIComponent(resourceGrantId)}`,
      signal
    );
  }

  createResourceGrant(
    organizationId: string,
    membershipId: string,
    input: CreateResourceGrantInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ResourceGrantMutationResult> {
    validateResourceGrantInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/memberships/${encodeURIComponent(membershipId)}/resource-grants`,
      idempotencyKey,
      input,
      signal
    );
  }

  revokeResourceGrant(
    organizationId: string,
    resourceGrantId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ResourceGrantMutationResult> {
    validateExpectedResourceGrantVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/resource-grants/${encodeURIComponent(resourceGrantId)}/revocation`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listProjects(organizationId: string, signal?: AbortSignal): Promise<Project[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/projects`, signal);
  }

  createProject(
    organizationId: string,
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ProjectMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/projects`,
      idempotencyKey,
      { name },
      signal
    );
  }

  getProjectAttribution(
    organizationId: string,
    projectId: string,
    signal?: AbortSignal
  ): Promise<ProjectAttributionProfile> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/attribution-profile`,
      signal
    );
  }

  getProjectAttributionRevision(
    organizationId: string,
    projectId: string,
    attributionProfileId: string,
    signal?: AbortSignal
  ): Promise<ProjectAttributionProfile> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/attribution-profiles/${encodeURIComponent(attributionProfileId)}`,
      signal
    );
  }

  updateProjectAttribution(
    organizationId: string,
    projectId: string,
    input: UpdateProjectAttributionInput,
    expectedProjectVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ProjectAttributionMutationResult> {
    validateProjectAttributionInput(input);
    validateExpectedProjectVersion(expectedProjectVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/attribution-profiles`,
      idempotencyKey,
      input,
      signal,
      { 'x-a3s-expected-version': String(expectedProjectVersion) }
    );
  }

  listEnvironments(organizationId: string, projectId: string, signal?: AbortSignal): Promise<Environment[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments`,
      signal
    );
  }

  createEnvironment(
    organizationId: string,
    projectId: string,
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<EnvironmentMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/environments`,
      idempotencyKey,
      { name },
      signal
    );
  }

  listOntologies(organizationId: string, projectId: string, signal?: AbortSignal): Promise<Ontology[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/ontologies`,
      signal
    );
  }

  getOntology(organizationId: string, ontologyId: string, signal?: AbortSignal): Promise<Ontology> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/ontologies/${encodeURIComponent(ontologyId)}`,
      signal
    );
  }

  createOntologyFromAcl(
    organizationId: string,
    projectId: string,
    acl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OntologyMutationResult> {
    validateOntologyAcl(acl);
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/ontologies`,
      idempotencyKey,
      acl,
      signal
    );
  }

  listOntologyRevisions(
    organizationId: string,
    ontologyId: string,
    signal?: AbortSignal
  ): Promise<OntologyRevisionSummary[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/ontologies/${encodeURIComponent(ontologyId)}/revisions`,
      signal
    );
  }

  getOntologyRevision(
    organizationId: string,
    ontologyId: string,
    revisionId: string,
    signal?: AbortSignal
  ): Promise<OntologyRevision> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/ontologies/${encodeURIComponent(ontologyId)}` +
        `/revisions/${encodeURIComponent(revisionId)}`,
      signal
    );
  }

  diffOntologyRevisions(
    organizationId: string,
    ontologyId: string,
    fromRevisionId: string,
    toRevisionId: string,
    signal?: AbortSignal
  ): Promise<OntologyDiff> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/ontologies/${encodeURIComponent(ontologyId)}` +
        `/revisions/${encodeURIComponent(fromRevisionId)}` +
        `/diff/${encodeURIComponent(toRevisionId)}`,
      signal
    );
  }

  reviseOntologyFromAcl(
    organizationId: string,
    ontologyId: string,
    acl: string,
    options: ReviseOntologyOptions,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OntologyMutationResult> {
    validateOntologyAcl(acl);
    validateOntologyRevisionControl(options.expectedVersion, options.migrationRuleId);
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/ontologies/${encodeURIComponent(ontologyId)}/revisions`,
      idempotencyKey,
      acl,
      signal,
      {
        'x-a3s-expected-version': String(options.expectedVersion),
        ...(options.migrationRuleId === undefined ? {} : { 'x-a3s-migration-rule': options.migrationRuleId }),
      }
    );
  }

  getWorkflowNodeCatalog(
    organizationId: string,
    projectId: string,
    signal?: AbortSignal
  ): Promise<WorkflowNodeCatalog> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-node-catalog`,
      signal
    );
  }

  listWorkflowDefinitions(
    organizationId: string,
    projectId: string,
    signal?: AbortSignal
  ): Promise<WorkflowDefinition[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-definitions`,
      signal
    );
  }

  getWorkflowDefinition(
    organizationId: string,
    workflowDefinitionId: string,
    signal?: AbortSignal
  ): Promise<WorkflowDefinition> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-definitions/${encodeURIComponent(workflowDefinitionId)}`,
      signal
    );
  }

  createWorkflowDefinitionFromAcl(
    organizationId: string,
    projectId: string,
    input: PublishWorkflowDefinitionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkflowDefinitionMutationResult> {
    validateWorkflowDefinitionPublication(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-definitions`,
      idempotencyKey,
      input,
      signal
    );
  }

  listWorkflowRevisions(
    organizationId: string,
    workflowDefinitionId: string,
    signal?: AbortSignal
  ): Promise<WorkflowRevisionSummary[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-definitions/${encodeURIComponent(workflowDefinitionId)}/revisions`,
      signal
    );
  }

  getWorkflowRevision(
    organizationId: string,
    workflowDefinitionId: string,
    workflowRevisionId: string,
    signal?: AbortSignal
  ): Promise<WorkflowRevision> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-definitions/${encodeURIComponent(workflowDefinitionId)}` +
        `/revisions/${encodeURIComponent(workflowRevisionId)}`,
      signal
    );
  }

  reviseWorkflowDefinitionFromAcl(
    organizationId: string,
    workflowDefinitionId: string,
    input: PublishWorkflowDefinitionInput,
    options: ReviseWorkflowDefinitionOptions,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkflowDefinitionMutationResult> {
    validateWorkflowDefinitionPublication(input);
    validateWorkflowRevisionControl(options.expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-definitions/${encodeURIComponent(workflowDefinitionId)}/revisions`,
      idempotencyKey,
      input,
      signal,
      { 'x-a3s-expected-version': String(options.expectedVersion) }
    );
  }

  listWorkflowGoals(
    organizationId: string,
    projectId: string,
    signal?: AbortSignal
  ): Promise<WorkflowGoal[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-goals`,
      signal
    );
  }

  getWorkflowGoal(
    organizationId: string,
    workflowGoalId: string,
    signal?: AbortSignal
  ): Promise<WorkflowGoal> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-goals/${encodeURIComponent(workflowGoalId)}`,
      signal
    );
  }

  createWorkflowGoalFromAcl(
    organizationId: string,
    projectId: string,
    acl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkflowGoalMutationResult> {
    validateWorkflowGoalAcl(acl);
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-goals`,
      idempotencyKey,
      acl,
      signal
    );
  }

  getWorkflowPlanRevision(
    organizationId: string,
    workflowGoalId: string,
    planRevisionId: string,
    signal?: AbortSignal
  ): Promise<WorkflowPlanRevision> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-goals/${encodeURIComponent(workflowGoalId)}` +
        `/plan-revisions/${encodeURIComponent(planRevisionId)}`,
      signal
    );
  }

  startWorkflowRun(
    organizationId: string,
    projectId: string,
    input: StartWorkflowRunInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkflowRunMutationResult> {
    if (
      input.timeoutSeconds !== undefined &&
      (!Number.isSafeInteger(input.timeoutSeconds) ||
        input.timeoutSeconds < 1 ||
        input.timeoutSeconds > MAX_WORKFLOW_RUN_TIMEOUT_SECONDS)
    ) {
      throw new RangeError(
        `WorkflowRun timeoutSeconds must be between 1 and ${MAX_WORKFLOW_RUN_TIMEOUT_SECONDS}`
      );
    }
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-runs`,
      idempotencyKey,
      input,
      signal
    );
  }

  cancelWorkflowRun(
    organizationId: string,
    workflowRunId: string,
    input: CancelWorkflowRunInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkflowRunMutationResult> {
    if (
      input.reason !== undefined &&
      (input.reason.length < 1 || input.reason.length > 4_096 || /[\0\r\n]/u.test(input.reason))
    ) {
      throw new TypeError('WorkflowRun cancellation reason must contain between 1 and 4096 safe characters');
    }
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}/cancel`,
      idempotencyKey,
      input,
      signal
    );
  }

  listWorkflowRuns(
    organizationId: string,
    projectId: string,
    options: ListWorkflowRunsOptions = {},
    signal?: AbortSignal
  ): Promise<WorkflowRun[]> {
    const parameters = new URLSearchParams();
    setBoundedInteger(
      parameters,
      'limit',
      options.limit,
      1,
      MAX_WORKFLOW_RUN_LIST_LIMIT,
      'WorkflowRun list limit'
    );
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/workflow-runs${encodeQueryParameters(parameters)}`,
      signal
    );
  }

  getWorkflowRun(organizationId: string, workflowRunId: string, signal?: AbortSignal): Promise<WorkflowRun> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}`,
      signal
    );
  }

  waitWorkflowRun(
    organizationId: string,
    workflowRunId: string,
    options: WaitWorkflowRunOptions = {},
    signal?: AbortSignal
  ): Promise<WorkflowRun> {
    const parameters = new URLSearchParams();
    setBoundedInteger(
      parameters,
      'timeoutSeconds',
      options.timeoutSeconds ?? DEFAULT_WORKFLOW_RUN_WAIT_SECONDS,
      0,
      MAX_WORKFLOW_RUN_WAIT_SECONDS,
      'WorkflowRun wait timeoutSeconds'
    );
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}/wait${encodeQueryParameters(parameters)}`,
      signal
    );
  }

  getWorkflowRunOutput(
    organizationId: string,
    workflowRunId: string,
    signal?: AbortSignal
  ): Promise<WorkflowRunOutput> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}/output`,
      signal
    );
  }

  getWorkflowRunVariables(
    organizationId: string,
    workflowRunId: string,
    signal?: AbortSignal
  ): Promise<WorkflowRunVariableInspection> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}/variables`,
      signal
    );
  }

  getWorkflowRunHistory(
    organizationId: string,
    workflowRunId: string,
    options: WorkflowRunHistoryOptions = {},
    signal?: AbortSignal
  ): Promise<WorkflowRunHistoryPage> {
    const parameters = new URLSearchParams();
    setBoundedInteger(
      parameters,
      'afterSequence',
      options.afterSequence,
      0,
      Number.MAX_SAFE_INTEGER,
      'WorkflowRun history afterSequence'
    );
    setBoundedInteger(
      parameters,
      'limit',
      options.limit,
      1,
      MAX_WORKFLOW_RUN_HISTORY_LIMIT,
      'WorkflowRun history limit'
    );
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workflow-runs/${encodeURIComponent(workflowRunId)}/history${encodeQueryParameters(parameters)}`,
      signal
    );
  }

  listHumanTasks(
    organizationId: string,
    projectId: string,
    options: ListHumanTasksOptions = {},
    signal?: AbortSignal
  ): Promise<HumanTaskSummary[]> {
    const parameters = new URLSearchParams();
    if (options.status !== undefined) {
      if (!HUMAN_TASK_STATUSES.has(options.status)) {
        throw new TypeError('HumanTask status is invalid');
      }
      parameters.set('status', options.status);
    }
    setBoundedInteger(
      parameters,
      'limit',
      options.limit,
      1,
      MAX_HUMAN_TASK_LIST_LIMIT,
      'HumanTask list limit'
    );
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/human-tasks${encodeQueryParameters(parameters)}`,
      signal
    );
  }

  getHumanTask(organizationId: string, humanTaskId: string, signal?: AbortSignal): Promise<HumanTask> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/human-tasks/${encodeURIComponent(humanTaskId)}`,
      signal
    );
  }

  claimHumanTask(
    organizationId: string,
    humanTaskId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<HumanTaskMutationResult> {
    return this.changeHumanTaskAssignment(
      organizationId,
      humanTaskId,
      'claim',
      expectedVersion,
      idempotencyKey,
      signal
    );
  }

  releaseHumanTask(
    organizationId: string,
    humanTaskId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<HumanTaskMutationResult> {
    return this.changeHumanTaskAssignment(
      organizationId,
      humanTaskId,
      'release',
      expectedVersion,
      idempotencyKey,
      signal
    );
  }

  submitHumanTask(
    organizationId: string,
    humanTaskId: string,
    submission: HumanTaskInteractionSubmission,
    signal?: AbortSignal
  ): Promise<HumanTaskMutationResult> {
    return this.request(
      'POST',
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/human-tasks/${encodeURIComponent(humanTaskId)}/submission`,
      {
        body: JSON.stringify(submission),
        contentType: 'application/json',
        signal,
      }
    );
  }

  listFormDrafts(organizationId: string, projectId: string, signal?: AbortSignal): Promise<FormDraft[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/forms`,
      signal
    );
  }

  getFormDraft(organizationId: string, formId: string, signal?: AbortSignal): Promise<FormDraft> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/forms/${encodeURIComponent(formId)}`,
      signal
    );
  }

  createFormDraft(
    organizationId: string,
    projectId: string,
    input: FormDraftInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<FormDraftMutationResult> {
    validateFormDraftInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/forms`,
      idempotencyKey,
      {
        name: input.name,
        description: input.description ?? '',
        document: input.document,
      },
      signal
    );
  }

  reviseFormDraft(
    organizationId: string,
    formId: string,
    input: FormDraftInput,
    options: ReviseFormDraftOptions,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<FormDraftMutationResult> {
    validateFormDraftInput(input);
    validateFormVersionControl(options.expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/forms/${encodeURIComponent(formId)}/draft-revisions`,
      idempotencyKey,
      {
        name: input.name,
        description: input.description ?? '',
        document: input.document,
      },
      signal,
      { 'x-a3s-expected-version': String(options.expectedVersion) }
    );
  }

  listFormReleases(organizationId: string, formId: string, signal?: AbortSignal): Promise<FormRelease[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/forms/${encodeURIComponent(formId)}/releases`,
      signal
    );
  }

  getFormRelease(
    organizationId: string,
    formId: string,
    releaseId: string,
    signal?: AbortSignal
  ): Promise<FormRelease> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/forms/${encodeURIComponent(formId)}` +
        `/releases/${encodeURIComponent(releaseId)}`,
      signal
    );
  }

  publishFormRelease(
    organizationId: string,
    formId: string,
    options: PublishFormReleaseOptions,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<FormPublicationMutationResult> {
    validateFormVersionControl(options.expectedVersion);
    return this.request(
      'POST',
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/forms/${encodeURIComponent(formId)}/releases`,
      {
        idempotencyKey,
        signal,
        additionalHeaders: { 'x-a3s-expected-version': String(options.expectedVersion) },
      }
    );
  }

  listAssets(organizationId: string, signal?: AbortSignal): Promise<Asset[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/assets`, signal);
  }

  getAsset(organizationId: string, assetId: string, signal?: AbortSignal): Promise<Asset> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/assets/${encodeURIComponent(assetId)}`,
      signal
    );
  }

  createAsset(
    organizationId: string,
    input: CreateAssetInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AssetMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/assets`,
      idempotencyKey,
      input,
      signal
    );
  }

  archiveAsset(
    organizationId: string,
    assetId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AssetMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}/archive`,
      idempotencyKey,
      signal
    );
  }

  listAssetReleases(organizationId: string, assetId: string, signal?: AbortSignal): Promise<AssetRelease[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}/releases`,
      signal
    );
  }

  getAssetRelease(
    organizationId: string,
    assetId: string,
    assetReleaseId: string,
    signal?: AbortSignal
  ): Promise<AssetRelease> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}/releases/${encodeURIComponent(assetReleaseId)}`,
      signal
    );
  }

  selectAssetRelease(
    organizationId: string,
    assetId: string,
    version?: string,
    signal?: AbortSignal
  ): Promise<AssetRelease> {
    const query = version === undefined ? '' : `?${new URLSearchParams({ version }).toString()}`;
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}/release-selection${query}`,
      signal
    );
  }

  createAssetRelease(
    organizationId: string,
    assetId: string,
    input: CreateAssetReleaseInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AssetReleaseMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}/releases`,
      idempotencyKey,
      input,
      signal
    );
  }

  yankAssetRelease(
    organizationId: string,
    assetId: string,
    assetReleaseId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AssetReleaseMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/yank`,
      idempotencyKey,
      signal
    );
  }

  getMcpServiceProfile(
    organizationId: string,
    assetId: string,
    assetReleaseId: string,
    signal?: AbortSignal
  ): Promise<McpServiceProfile> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/mcp-service-profile`,
      signal
    );
  }

  bindMcpServiceProfileFromAcl(
    organizationId: string,
    assetId: string,
    assetReleaseId: string,
    acl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpServiceProfileMutationResult> {
    return this.postMcpServiceProfileAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/mcp-service-profile`,
      idempotencyKey,
      acl,
      signal
    );
  }

  listNodes(organizationId: string, signal?: AbortSignal): Promise<Node[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/nodes`, signal);
  }

  listNodePools(organizationId: string, signal?: AbortSignal): Promise<NodePool[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/node-pools`, signal);
  }

  getNodePool(organizationId: string, nodePoolId: string, signal?: AbortSignal): Promise<NodePool> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/node-pools/${encodeURIComponent(nodePoolId)}`,
      signal
    );
  }

  createNodePool(
    organizationId: string,
    input: CreateNodePoolInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NodePool> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/node-pools`,
      idempotencyKey,
      input,
      signal
    );
  }

  addNodePoolMembers(
    organizationId: string,
    nodePoolId: string,
    input: AddNodePoolMembersInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NodePool> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/node-pools/${encodeURIComponent(nodePoolId)}/members`,
      idempotencyKey,
      input,
      signal
    );
  }

  requestNodePoolMemberRemoval(
    organizationId: string,
    nodePoolId: string,
    input: RequestNodePoolMemberRemovalInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NodePool> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/node-pools/${encodeURIComponent(nodePoolId)}/members/removal`,
      idempotencyKey,
      input,
      signal
    );
  }

  scheduleNodePoolMaintenance(
    organizationId: string,
    nodePoolId: string,
    input: ScheduleNodePoolMaintenanceInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NodePool> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/node-pools/${encodeURIComponent(nodePoolId)}/maintenance`,
      idempotencyKey,
      input,
      signal
    );
  }

  cancelNodePoolMaintenance(
    organizationId: string,
    nodePoolId: string,
    input: CancelNodePoolMaintenanceInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NodePool> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/node-pools/${encodeURIComponent(nodePoolId)}/maintenance/cancel`,
      idempotencyKey,
      input,
      signal
    );
  }

  searchResources(
    organizationId: string,
    query: string,
    limit = DEFAULT_SEARCH_LIMIT,
    signal?: AbortSignal
  ): Promise<SearchResult[]> {
    const parameters = new URLSearchParams({
      q: validateSearchRequest(query, limit),
      limit: String(limit),
    });
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/search?${parameters.toString()}`,
      signal
    );
  }

  listPluginRegistries(organizationId: string, signal?: AbortSignal): Promise<PluginRegistry[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/plugin-registries`, signal);
  }

  getPluginRegistry(
    organizationId: string,
    registryId: string,
    signal?: AbortSignal
  ): Promise<PluginRegistry> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/plugin-registries/${encodeURIComponent(registryId)}`,
      signal
    );
  }

  searchPluginCatalog(
    organizationId: string,
    registryId: string,
    request: PluginCatalogSearchRequest,
    signal?: AbortSignal
  ): Promise<PluginCatalogPage> {
    return this.postQueryJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/plugin-registries/${encodeURIComponent(registryId)}/catalog/search`,
      request,
      signal
    );
  }

  searchCachedPluginCatalog(
    organizationId: string,
    registryId: string,
    request: PluginCatalogSearchRequest,
    signal?: AbortSignal
  ): Promise<PluginCatalogPage> {
    return this.postQueryJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/plugin-registries/${encodeURIComponent(registryId)}/catalog/cache/search`,
      request,
      signal
    );
  }

  inspectPluginCatalog(
    organizationId: string,
    registryId: string,
    request: PluginCatalogInspectRequest,
    signal?: AbortSignal
  ): Promise<PluginCatalogInspection> {
    return this.postQueryJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/plugin-registries/${encodeURIComponent(registryId)}/catalog/inspect`,
      request,
      signal
    );
  }

  inspectCachedPluginCatalog(
    organizationId: string,
    registryId: string,
    request: PluginCatalogInspectRequest,
    signal?: AbortSignal
  ): Promise<PluginCatalogInspection> {
    return this.postQueryJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/plugin-registries/${encodeURIComponent(registryId)}/catalog/cache/inspect`,
      request,
      signal
    );
  }

  issueEnrollmentToken(
    organizationId: string,
    input: IssueEnrollmentTokenInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<EnrollmentToken> {
    validateEnrollmentTokenInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/enrollment-tokens`,
      idempotencyKey,
      input,
      signal
    );
  }

  markNodeReady(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'ready', expectedVersion, idempotencyKey, signal);
  }

  drainNode(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'drain', expectedVersion, idempotencyKey, signal);
  }

  revokeNode(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'revoke', expectedVersion, idempotencyKey, signal);
  }

  listOperations(organizationId: string, signal?: AbortSignal): Promise<Operation[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/operations?limit=100`, signal);
  }

  listAuditRecords(
    organizationId: string,
    query: AuditRecordQuery = {},
    signal?: AbortSignal
  ): Promise<AuditRecordPage> {
    const parameters = encodeAuditRecordQuery(query);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/audit-records?${parameters.toString()}`,
      signal
    );
  }

  listNotifications(
    organizationId: string,
    query: NotificationQuery = {},
    signal?: AbortSignal
  ): Promise<NotificationPage> {
    const parameters = encodeNotificationQuery(query);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/notifications?${parameters.toString()}`,
      signal
    );
  }

  getNotification(
    organizationId: string,
    notificationId: string,
    signal?: AbortSignal
  ): Promise<Notification> {
    validateNotificationId(notificationId);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/notifications/${encodeURIComponent(notificationId)}`,
      signal
    );
  }

  markNotificationRead(
    organizationId: string,
    notificationId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NotificationMutationResult> {
    validateNotificationId(notificationId);
    validateExpectedNotificationVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/notifications/${encodeURIComponent(notificationId)}/read`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listNotificationAlertPolicies(
    organizationId: string,
    query: NotificationAlertPolicyQuery = {},
    signal?: AbortSignal
  ): Promise<NotificationAlertPolicyPage> {
    const parameters = encodeNotificationAlertPolicyQuery(query);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-alert-policies?${parameters.toString()}`,
      signal
    );
  }

  getNotificationAlertPolicy(
    organizationId: string,
    policyId: string,
    signal?: AbortSignal
  ): Promise<NotificationAlertPolicy> {
    validateNotificationAlertPolicyId(policyId);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-alert-policies/${encodeURIComponent(policyId)}`,
      signal
    );
  }

  createNotificationAlertPolicy(
    organizationId: string,
    definitionAcl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NotificationAlertPolicyMutationResult> {
    validateNotificationAlertPolicyAcl(definitionAcl);
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}/notification-alert-policies`,
      idempotencyKey,
      definitionAcl,
      signal
    );
  }

  revokeNotificationAlertPolicy(
    organizationId: string,
    policyId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<NotificationAlertPolicyMutationResult> {
    validateNotificationAlertPolicyId(policyId);
    validateExpectedNotificationAlertPolicyVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-alert-policies/${encodeURIComponent(policyId)}/revoke`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listOutboundNotificationSubscriptions(
    organizationId: string,
    query: OutboundNotificationSubscriptionQuery = {},
    signal?: AbortSignal
  ): Promise<OutboundNotificationSubscriptionPage> {
    const parameters = encodeOutboundNotificationSubscriptionQuery(query);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-outbound-subscriptions?${parameters.toString()}`,
      signal
    );
  }

  getOutboundNotificationSubscription(
    organizationId: string,
    subscriptionId: string,
    signal?: AbortSignal
  ): Promise<OutboundNotificationSubscription> {
    validateOutboundNotificationSubscriptionId(subscriptionId);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-outbound-subscriptions/${encodeURIComponent(subscriptionId)}`,
      signal
    );
  }

  createOutboundNotificationSubscription(
    organizationId: string,
    definitionAcl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OutboundNotificationSubscriptionMutationResult> {
    validateOutboundNotificationSubscriptionAcl(definitionAcl);
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}/notification-outbound-subscriptions`,
      idempotencyKey,
      definitionAcl,
      signal
    );
  }

  revokeOutboundNotificationSubscription(
    organizationId: string,
    subscriptionId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OutboundNotificationSubscriptionMutationResult> {
    validateOutboundNotificationSubscriptionId(subscriptionId);
    validateExpectedOutboundNotificationSubscriptionVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/notification-outbound-subscriptions/${encodeURIComponent(subscriptionId)}/revoke`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  listBuildRuns(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<BuildRun[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/build-runs?limit=100`,
      signal
    );
  }

  getBuildRun(organizationId: string, buildRunId: string, signal?: AbortSignal): Promise<BuildRun> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}`,
      signal
    );
  }

  getBuildRunLogs(
    organizationId: string,
    buildRunId: string,
    query: CloudLogQuery = {},
    signal?: AbortSignal
  ): Promise<BuildRunLogsPage> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/build-runs/${encodeURIComponent(buildRunId)}/logs${encodeLogQuery(query)}`,
      signal
    );
  }

  getBuildEvidence(organizationId: string, buildRunId: string, signal?: AbortSignal): Promise<BuildEvidence> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}/evidence`,
      signal
    );
  }

  listExecutions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Execution[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/executions?limit=100`,
      signal
    );
  }

  getExecution(organizationId: string, executionId: string, signal?: AbortSignal): Promise<Execution> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/executions/${encodeURIComponent(executionId)}`,
      signal
    );
  }

  listExecutionTemplates(
    organizationId: string,
    projectId: string,
    signal?: AbortSignal
  ): Promise<ExecutionTemplateRevision[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/execution-templates?limit=100`,
      signal
    );
  }

  getExecutionTemplate(
    organizationId: string,
    projectId: string,
    templateId: string,
    revisionId: string,
    signal?: AbortSignal
  ): Promise<ExecutionTemplateRevision> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/execution-templates/${encodeURIComponent(templateId)}` +
        `/revisions/${encodeURIComponent(revisionId)}`,
      signal
    );
  }

  createExecutionTemplate(
    organizationId: string,
    projectId: string,
    input: CreateExecutionTemplateInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ExecutionTemplateMutationResult> {
    validateExecutionTemplateAcl(input.definitionAcl);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/execution-templates`,
      idempotencyKey,
      input,
      signal
    );
  }

  listApplications(
    organizationId: string,
    projectId: string,
    limit = DEFAULT_APPLICATION_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<Application[]> {
    validateApplicationListLimit(limit);
    return this.get(`${this.applicationsPath(organizationId, projectId)}?limit=${limit}`, signal);
  }

  getApplication(
    organizationId: string,
    projectId: string,
    applicationId: string,
    signal?: AbortSignal
  ): Promise<Application> {
    return this.get(this.applicationPath(organizationId, projectId, applicationId), signal);
  }

  listApplicationReleases(
    organizationId: string,
    projectId: string,
    applicationId: string,
    limit = DEFAULT_APPLICATION_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<ApplicationRelease[]> {
    validateApplicationListLimit(limit);
    return this.get(
      `${this.applicationPath(organizationId, projectId, applicationId)}/releases?limit=${limit}`,
      signal
    );
  }

  getApplicationRelease(
    organizationId: string,
    projectId: string,
    applicationId: string,
    releaseId: string,
    signal?: AbortSignal
  ): Promise<ApplicationRelease> {
    return this.get(
      `${this.applicationPath(organizationId, projectId, applicationId)}` +
        `/releases/${encodeURIComponent(releaseId)}`,
      signal
    );
  }

  createApplication(
    organizationId: string,
    projectId: string,
    input: CreateApplicationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationMutationResult> {
    validateApplicationName(input.name);
    validateApplicationDescription(input.description ?? '');
    validateApplicationReleaseAcl(input.releaseAcl);
    return this.postJson(
      this.applicationsPath(organizationId, projectId),
      idempotencyKey,
      { ...input, description: input.description ?? '' },
      signal
    );
  }

  publishApplicationRelease(
    organizationId: string,
    projectId: string,
    applicationId: string,
    input: PublishApplicationReleaseInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationMutationResult> {
    validateApplicationExpectedVersion(input.expectedVersion);
    validateApplicationReleaseAcl(input.releaseAcl);
    return this.postJson(
      `${this.applicationPath(organizationId, projectId, applicationId)}/releases`,
      idempotencyKey,
      input,
      signal
    );
  }

  openApplicationSession(
    organizationId: string,
    projectId: string,
    applicationId: string,
    input: OpenApplicationSessionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationSessionMutationResult> {
    const initialVariables = input.initialVariables ?? {};
    validateApplicationInitialVariables(initialVariables);
    return this.postJson(
      `${this.applicationPath(organizationId, projectId, applicationId)}/sessions`,
      idempotencyKey,
      { ...input, initialVariables },
      signal
    );
  }

  getApplicationSession(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    signal?: AbortSignal
  ): Promise<ApplicationSession> {
    return this.get(this.applicationSessionPath(organizationId, projectId, applicationId, sessionId), signal);
  }

  closeApplicationSession(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    input: ApplicationExpectedVersionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationSessionMutationResult> {
    validateApplicationExpectedVersion(input.expectedVersion);
    return this.postJson(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}/close`,
      idempotencyKey,
      input,
      signal
    );
  }

  requestApplicationInvocation(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    input: RequestApplicationInvocationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationInvocationMutationResult> {
    validateApplicationResponseMode(input.responseMode);
    validateApplicationInvocationInput(input.input);
    if (input.timeoutSeconds !== undefined) {
      validateApplicationInvocationTimeout(input.timeoutSeconds);
    }
    return this.postJson(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}/invocations`,
      idempotencyKey,
      input,
      signal
    );
  }

  getApplicationInvocation(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    invocationId: string,
    signal?: AbortSignal
  ): Promise<ApplicationInvocation> {
    return this.get(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}` +
        `/invocations/${encodeURIComponent(invocationId)}`,
      signal
    );
  }

  cancelApplicationInvocation(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    invocationId: string,
    input: ApplicationExpectedVersionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApplicationInvocationCancellationResult> {
    validateApplicationExpectedVersion(input.expectedVersion);
    return this.postJson(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}` +
        `/invocations/${encodeURIComponent(invocationId)}/cancel`,
      idempotencyKey,
      input,
      signal
    );
  }

  listApplicationMessages(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    afterSequence = 0,
    limit = DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<ApplicationMessage[]> {
    validateApplicationMessageList(afterSequence, limit);
    return this.get(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}` +
        `/messages?afterSequence=${afterSequence}&limit=${limit}`,
      signal
    );
  }

  replayApplicationSession(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string,
    afterSequence = 0,
    limit = DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<ApplicationSessionReplay> {
    validateApplicationMessageList(afterSequence, limit);
    return this.get(
      `${this.applicationSessionPath(organizationId, projectId, applicationId, sessionId)}` +
        `/replay?afterSequence=${afterSequence}&limit=${limit}`,
      signal
    );
  }

  listConnectorProfiles(
    organizationId: string,
    projectId: string,
    environmentId: string,
    limit = DEFAULT_CONNECTOR_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<ConnectorProfile[]> {
    validateConnectorListLimit(limit);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/connector-profiles?limit=${limit}`,
      signal
    );
  }

  getConnectorProfile(
    organizationId: string,
    projectId: string,
    environmentId: string,
    profileId: string,
    signal?: AbortSignal
  ): Promise<ConnectorProfileRecord> {
    return this.get(this.connectorProfilePath(organizationId, projectId, environmentId, profileId), signal);
  }

  listConnectorRevisions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    profileId: string,
    limit = DEFAULT_CONNECTOR_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<ConnectorRevision[]> {
    validateConnectorListLimit(limit);
    return this.get(
      `${this.connectorProfilePath(organizationId, projectId, environmentId, profileId)}` +
        `/revisions?limit=${limit}`,
      signal
    );
  }

  getConnectorRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    profileId: string,
    revisionId: string,
    signal?: AbortSignal
  ): Promise<ConnectorRevision> {
    return this.get(
      `${this.connectorProfilePath(organizationId, projectId, environmentId, profileId)}` +
        `/revisions/${encodeURIComponent(revisionId)}`,
      signal
    );
  }

  createConnectorProfile(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateConnectorProfileInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ConnectorProfileMutationResult> {
    validateConnectorProfileName(input.name);
    validateConnectorDefinitionAcl(input.definitionAcl);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/connector-profiles`,
      idempotencyKey,
      input,
      signal
    );
  }

  reviseConnectorProfile(
    organizationId: string,
    projectId: string,
    environmentId: string,
    profileId: string,
    input: ReviseConnectorProfileInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ConnectorProfileMutationResult> {
    validateConnectorExpectedVersion(input.expectedVersion);
    validateConnectorDefinitionAcl(input.definitionAcl);
    return this.postJson(
      `${this.connectorProfilePath(organizationId, projectId, environmentId, profileId)}/revisions`,
      idempotencyKey,
      input,
      signal
    );
  }

  listDurableCellApplications(
    organizationId: string,
    projectId: string,
    environmentId: string,
    limit = DEFAULT_DURABLE_CELL_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<DurableCellApplication[]> {
    validateDurableCellListLimit(limit);
    return this.get(
      `${this.durableCellApplicationsPath(organizationId, projectId, environmentId)}?limit=${limit}`,
      signal
    );
  }

  getDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationRecord> {
    return this.get(
      this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId),
      signal
    );
  }

  listDurableCellApplicationRevisions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    limit = DEFAULT_DURABLE_CELL_LIST_LIMIT,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationRevision[]> {
    validateDurableCellListLimit(limit);
    return this.get(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}` +
        `/revisions?limit=${limit}`,
      signal
    );
  }

  getDurableCellApplicationRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    revisionId: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationRevision> {
    return this.get(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}` +
        `/revisions/${encodeURIComponent(revisionId)}`,
      signal
    );
  }

  createDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateDurableCellApplicationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationMutationResult> {
    validateDurableCellApplicationName(input.name);
    validateDurableCellApplicationAcl(input.definitionAcl);
    return this.postJson(
      this.durableCellApplicationsPath(organizationId, projectId, environmentId),
      idempotencyKey,
      input,
      signal
    );
  }

  reviseDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    input: ReviseDurableCellApplicationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationMutationResult> {
    validateDurableCellExpectedVersion(input.expectedVersion);
    validateDurableCellApplicationAcl(input.definitionAcl);
    return this.postJson(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}/revisions`,
      idempotencyKey,
      input,
      signal
    );
  }

  startDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationMutationResult> {
    return this.changeDurableCellApplicationState(
      organizationId,
      projectId,
      environmentId,
      applicationId,
      'start',
      expectedVersion,
      idempotencyKey,
      signal
    );
  }

  stopDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationMutationResult> {
    return this.changeDurableCellApplicationState(
      organizationId,
      projectId,
      environmentId,
      applicationId,
      'stop',
      expectedVersion,
      idempotencyKey,
      signal
    );
  }

  deployDurableCellApplication(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    revisionId: string,
    input: DeployDurableCellApplicationInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellDeploymentResult> {
    validateDeployDurableCellApplicationInput(input);
    return this.postJson(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}` +
        `/revisions/${encodeURIComponent(revisionId)}/deployments`,
      idempotencyKey,
      input,
      signal
    );
  }

  publishDurableCellApplicationRoute(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    revisionId: string,
    input: PublishDurableCellApplicationRouteInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellRoutePublicationResult> {
    validatePublishDurableCellApplicationRouteInput(input);
    return this.postJson(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}` +
        `/revisions/${encodeURIComponent(revisionId)}/routes`,
      idempotencyKey,
      input,
      signal
    );
  }

  createExecution(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateExecutionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ExecutionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/executions`,
      idempotencyKey,
      input,
      signal
    );
  }

  listAgentConversations(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<AgentConversation[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/agent-conversations?limit=100`,
      signal
    );
  }

  getAgentConversation(
    organizationId: string,
    conversationId: string,
    signal?: AbortSignal
  ): Promise<AgentConversation> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-conversations/${encodeURIComponent(conversationId)}`,
      signal
    );
  }

  createAgentConversation(
    organizationId: string,
    projectId: string,
    environmentId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AgentConversationMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/agent-conversations`,
      idempotencyKey,
      signal
    );
  }

  listAgentExecutions(
    organizationId: string,
    conversationId: string,
    signal?: AbortSignal
  ): Promise<AgentExecution[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-conversations/${encodeURIComponent(conversationId)}/executions?limit=100`,
      signal
    );
  }

  getAgentExecution(
    organizationId: string,
    executionId: string,
    signal?: AbortSignal
  ): Promise<AgentExecution> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-executions/${encodeURIComponent(executionId)}`,
      signal
    );
  }

  getAgentExecutionChangeSet(
    organizationId: string,
    executionId: string,
    signal?: AbortSignal
  ): Promise<AgentExecutionChangeSet> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-executions/${encodeURIComponent(executionId)}/changes`,
      signal
    );
  }

  startAgentExecution(
    organizationId: string,
    conversationId: string,
    input: StartAgentExecutionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AgentExecutionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-conversations/${encodeURIComponent(conversationId)}/executions`,
      idempotencyKey,
      input,
      signal
    );
  }

  cancelAgentExecution(
    organizationId: string,
    executionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<AgentExecutionMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-executions/${encodeURIComponent(executionId)}/cancel`,
      idempotencyKey,
      signal
    );
  }

  getAgentExecutionEvents(
    organizationId: string,
    conversationId: string,
    query: CloudSequenceQuery = {},
    signal?: AbortSignal
  ): Promise<AgentExecutionEventsPage> {
    const parameters = encodeSequenceQuery(query, 'Agent event', 200);
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/agent-conversations/${encodeURIComponent(conversationId)}/events` +
        encodeQueryParameters(parameters),
      signal
    );
  }

  listWorkloads(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Workload[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments/${encodeURIComponent(environmentId)}/workloads`,
      signal
    );
  }

  getWorkload(organizationId: string, workloadId: string, signal?: AbortSignal): Promise<Workload> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}`,
      signal
    );
  }

  getDeployment(organizationId: string, deploymentId: string, signal?: AbortSignal): Promise<Deployment> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/deployments/${encodeURIComponent(deploymentId)}`,
      signal
    );
  }

  getWorkloadLogs(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    query: CloudLogQuery = {},
    signal?: AbortSignal
  ): Promise<WorkloadLogsPage> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/revisions/${encodeURIComponent(revisionId)}/logs${encodeLogQuery(query)}`,
      signal
    );
  }

  listRoutes(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Route[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments/${encodeURIComponent(environmentId)}/routes`,
      signal
    );
  }

  getRoute(organizationId: string, routeId: string, signal?: AbortSignal): Promise<Route> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/routes/${encodeURIComponent(routeId)}`,
      signal
    );
  }

  listDomainClaims(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<DomainClaim[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/domain-claims`,
      signal
    );
  }

  getDomainClaim(organizationId: string, domainClaimId: string, signal?: AbortSignal): Promise<DomainClaim> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}`,
      signal
    );
  }

  createDomainClaim(
    organizationId: string,
    projectId: string,
    environmentId: string,
    pattern: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/domain-claims`,
      idempotencyKey,
      { pattern },
      signal
    );
  }

  verifyDomainClaim(
    organizationId: string,
    domainClaimId: string,
    proof: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}/verify`,
      idempotencyKey,
      { proof },
      signal
    );
  }

  revokeDomainClaim(
    organizationId: string,
    domainClaimId: string,
    reason: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}/revoke`,
      idempotencyKey,
      { reason },
      signal
    );
  }

  listGatewayScopes(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<GatewayScope[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/gateway-scopes`,
      signal
    );
  }

  createGatewayScope(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateGatewayScopeInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GatewayScopeMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/gateway-scopes`,
      idempotencyKey,
      input,
      signal
    );
  }

  publishRoute(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: PublishRouteInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RoutePublicationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/routes`,
      idempotencyKey,
      input,
      signal
    );
  }

  listGatewayCertificates(organizationId: string, signal?: AbortSignal): Promise<GatewayCertificate[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/gateway-certificates`, signal);
  }

  listMcpCredentials(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<McpCredential[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/mcp-credentials`,
      signal
    );
  }

  getMcpCredential(
    organizationId: string,
    credentialId: string,
    signal?: AbortSignal
  ): Promise<McpCredential> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/mcp-credentials/${encodeURIComponent(credentialId)}`,
      signal
    );
  }

  createMcpCredential(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateMcpCredentialInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpCredentialDeliveryResult> {
    validateMcpCredentialExpiry(input.expiresAt);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/mcp-credentials`,
      idempotencyKey,
      input,
      signal
    );
  }

  rotateMcpCredential(
    organizationId: string,
    credentialId: string,
    input: RotateMcpCredentialInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpCredentialDeliveryResult> {
    validateMcpCredentialExpiry(input.expiresAt);
    validateExpectedMcpCredentialVersion(input.expectedAggregateVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/mcp-credentials/${encodeURIComponent(credentialId)}/rotate`,
      idempotencyKey,
      input,
      signal
    );
  }

  revokeMcpCredential(
    organizationId: string,
    credentialId: string,
    input: RevokeMcpCredentialInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpCredentialMutationResult> {
    validateExpectedMcpCredentialVersion(input.expectedAggregateVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/mcp-credentials/${encodeURIComponent(credentialId)}/revoke`,
      idempotencyKey,
      input,
      signal
    );
  }

  listMcpRoutePolicies(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<McpRoutePolicy[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/mcp-route-policies`,
      signal
    );
  }

  getMcpRoutePolicy(organizationId: string, routeId: string, signal?: AbortSignal): Promise<McpRoutePolicy> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/mcp-route-policies/${encodeURIComponent(routeId)}`,
      signal
    );
  }

  createMcpRoutePolicyFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    acl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpRoutePolicyMutationResult> {
    return this.postMcpRoutePolicyAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/mcp-route-policies`,
      idempotencyKey,
      acl,
      signal
    );
  }

  reviseMcpRoutePolicyFromAcl(
    organizationId: string,
    routeId: string,
    acl: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<McpRoutePolicyMutationResult> {
    return this.postMcpRoutePolicyAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/mcp-route-policies/${encodeURIComponent(routeId)}/revisions`,
      idempotencyKey,
      acl,
      signal
    );
  }

  listSecrets(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Secret[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/secrets`,
      signal
    );
  }

  getSecret(organizationId: string, secretId: string, signal?: AbortSignal): Promise<SecretDetails> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/secrets/${encodeURIComponent(secretId)}`,
      signal
    );
  }

  createSecret(
    organizationId: string,
    projectId: string,
    environmentId: string,
    name: string,
    value: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    validateSecretValue(value);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/secrets`,
      idempotencyKey,
      { name, value },
      signal
    );
  }

  addSecretVersion(
    organizationId: string,
    secretId: string,
    value: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    validateSecretValue(value);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/secrets/${encodeURIComponent(secretId)}/versions`,
      idempotencyKey,
      { value },
      signal
    );
  }

  revokeSecretVersion(
    organizationId: string,
    secretId: string,
    version: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    if (!Number.isSafeInteger(version) || version < 1) {
      throw new RangeError('Secret version must be a positive safe integer');
    }
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/secrets/${encodeURIComponent(secretId)}` +
        `/versions/${version}/revoke`,
      idempotencyKey,
      signal
    );
  }

  listSourceRevisions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<SourceRevision[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-revisions`,
      signal
    );
  }

  resolveSourceRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: ResolveSourceRevisionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SourceRevisionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-revisions`,
      idempotencyKey,
      input,
      signal
    );
  }

  getGithubConnection(organizationId: string, signal?: AbortSignal): Promise<GithubConnection> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/source-connections/github`, signal);
  }

  beginGithubConnection(organizationId: string, signal?: AbortSignal): Promise<GithubConnectionInstall> {
    return this.request(
      'POST',
      `/organizations/${encodeURIComponent(organizationId)}/source-connections/github`,
      { signal }
    );
  }

  listGithubRepositorySubscriptions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscription[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-subscriptions/github`,
      signal
    );
  }

  createGithubRepositorySubscription(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateGithubRepositorySubscriptionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscriptionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-subscriptions/github`,
      idempotencyKey,
      input,
      signal
    );
  }

  deactivateGithubRepositorySubscription(
    organizationId: string,
    projectId: string,
    environmentId: string,
    subscriptionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscriptionMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-subscriptions/github/${encodeURIComponent(subscriptionId)}/deactivate`,
      idempotencyKey,
      signal
    );
  }

  createWorkloadFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postWorkloadAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/workloads`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  updateWorkloadFromAcl(
    organizationId: string,
    workloadId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postWorkloadAcl(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/deployments`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  deploySourceRevisionFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    sourceRevisionId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postWorkloadAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-revisions/${encodeURIComponent(sourceRevisionId)}/workloads`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  deployAgentReleaseFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    assetId: string,
    assetReleaseId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postWorkloadAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/workloads`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  updateAgentReleaseFromAcl(
    organizationId: string,
    workloadId: string,
    assetId: string,
    assetReleaseId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postWorkloadAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/deployments`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  updateWorkload(
    organizationId: string,
    workloadId: string,
    template: ServiceTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/deployments`,
      idempotencyKey,
      { template },
      signal
    );
  }

  deploySourceRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    sourceRevisionId: string,
    name: string,
    template: SourceWorkloadTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-revisions/${encodeURIComponent(sourceRevisionId)}/workloads`,
      idempotencyKey,
      { name, template },
      signal
    );
  }

  deployAgentRelease(
    organizationId: string,
    projectId: string,
    environmentId: string,
    assetId: string,
    assetReleaseId: string,
    name: string,
    template: SourceWorkloadTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/workloads`,
      idempotencyKey,
      { name, template },
      signal
    );
  }

  updateAgentRelease(
    organizationId: string,
    workloadId: string,
    assetId: string,
    assetReleaseId: string,
    template: SourceWorkloadTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/assets/${encodeURIComponent(assetId)}` +
        `/releases/${encodeURIComponent(assetReleaseId)}/deployments`,
      idempotencyKey,
      { template },
      signal
    );
  }

  rollbackWorkload(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/rollback`,
      idempotencyKey,
      { revisionId },
      signal
    );
  }

  bindSkillRelease(
    organizationId: string,
    workloadId: string,
    skillAssetId: string,
    skillAssetReleaseId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/skills/${encodeURIComponent(skillAssetId)}` +
        `/releases/${encodeURIComponent(skillAssetReleaseId)}/bindings`,
      idempotencyKey,
      signal
    );
  }

  unbindSkillRelease(
    organizationId: string,
    workloadId: string,
    skillAssetId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/skills/${encodeURIComponent(skillAssetId)}/bindings`,
      idempotencyKey,
      signal
    );
  }

  cancelDeployment(
    organizationId: string,
    deploymentId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<CancelDeploymentResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/deployments/${encodeURIComponent(deploymentId)}`,
      idempotencyKey,
      signal
    );
  }

  cancelBuildRun(
    organizationId: string,
    buildRunId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<CancelBuildRunResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}`,
      idempotencyKey,
      signal
    );
  }

  cancelExecution(
    organizationId: string,
    executionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ExecutionMutationResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/executions/${encodeURIComponent(executionId)}`,
      idempotencyKey,
      signal
    );
  }

  retryBuildRun(
    organizationId: string,
    buildRunId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RetryBuildRunResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}/retry`,
      idempotencyKey,
      signal
    );
  }

  stopWorkload(
    organizationId: string,
    workloadId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<StopWorkloadResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/stop`,
      idempotencyKey,
      signal
    );
  }

  operationStreamUrl(organizationId: string): string {
    return `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}/operations/stream`;
  }

  eventStreamHeaders(lastEventId?: string): Record<string, string> {
    return {
      Accept: 'text/event-stream',
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
      ...(lastEventId ? { 'Last-Event-ID': lastEventId } : {}),
    };
  }

  workloadLogStreamUrl(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    stream?: WorkloadLogStreamFilter
  ): string {
    const query = new URLSearchParams({ limit: '16' });
    if (stream) {
      query.set('stream', stream);
    }
    return (
      `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}` +
      `/workloads/${encodeURIComponent(workloadId)}` +
      `/revisions/${encodeURIComponent(revisionId)}/logs/stream?${query.toString()}`
    );
  }

  buildRunLogStreamUrl(organizationId: string, buildRunId: string, stream?: WorkloadLogStreamFilter): string {
    const query = new URLSearchParams({ limit: '16' });
    if (stream) {
      query.set('stream', stream);
    }
    return (
      `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}` +
      `/build-runs/${encodeURIComponent(buildRunId)}/logs/stream?${query.toString()}`
    );
  }

  agentExecutionEventStreamUrl(organizationId: string, conversationId: string): string {
    return (
      `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}` +
      `/agent-conversations/${encodeURIComponent(conversationId)}/events/stream?limit=16`
    );
  }

  private get<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request('GET', path, { signal });
  }

  private connectorProfilePath(
    organizationId: string,
    projectId: string,
    environmentId: string,
    profileId: string
  ): string {
    return (
      `/organizations/${encodeURIComponent(organizationId)}` +
      `/projects/${encodeURIComponent(projectId)}` +
      `/environments/${encodeURIComponent(environmentId)}` +
      `/connector-profiles/${encodeURIComponent(profileId)}`
    );
  }

  private applicationsPath(organizationId: string, projectId: string): string {
    return (
      `/organizations/${encodeURIComponent(organizationId)}` +
      `/projects/${encodeURIComponent(projectId)}/applications`
    );
  }

  private applicationPath(organizationId: string, projectId: string, applicationId: string): string {
    return `${this.applicationsPath(organizationId, projectId)}/${encodeURIComponent(applicationId)}`;
  }

  private applicationSessionPath(
    organizationId: string,
    projectId: string,
    applicationId: string,
    sessionId: string
  ): string {
    return (
      `${this.applicationPath(organizationId, projectId, applicationId)}/sessions/` +
      encodeURIComponent(sessionId)
    );
  }

  private durableCellApplicationsPath(
    organizationId: string,
    projectId: string,
    environmentId: string
  ): string {
    return (
      `/organizations/${encodeURIComponent(organizationId)}` +
      `/projects/${encodeURIComponent(projectId)}` +
      `/environments/${encodeURIComponent(environmentId)}/durable-cell-applications`
    );
  }

  private durableCellApplicationPath(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string
  ): string {
    return (
      this.durableCellApplicationsPath(organizationId, projectId, environmentId) +
      `/${encodeURIComponent(applicationId)}`
    );
  }

  private changeDurableCellApplicationState(
    organizationId: string,
    projectId: string,
    environmentId: string,
    applicationId: string,
    action: 'start' | 'stop',
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DurableCellApplicationMutationResult> {
    validateDurableCellExpectedVersion(expectedVersion);
    return this.postJson(
      `${this.durableCellApplicationPath(organizationId, projectId, environmentId, applicationId)}/${action}`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  private getHealth<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request('GET', path, { healthResponse: true, signal });
  }

  private changeNodeState(
    organizationId: string,
    nodeId: string,
    action: 'ready' | 'drain' | 'revoke',
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    validateExpectedNodeVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/nodes/${encodeURIComponent(nodeId)}/actions/${action}`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
  }

  private changeHumanTaskAssignment(
    organizationId: string,
    humanTaskId: string,
    action: 'claim' | 'release',
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<HumanTaskMutationResult> {
    validateExpectedHumanTaskVersion(expectedVersion);
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/human-tasks/${encodeURIComponent(humanTaskId)}/${action}`,
      idempotencyKey,
      signal,
      { 'x-a3s-expected-version': String(expectedVersion) }
    );
  }

  private delete<T>(path: string, idempotencyKey: string, signal?: AbortSignal): Promise<T> {
    return this.request('DELETE', path, { idempotencyKey, signal });
  }

  private post<T>(
    path: string,
    idempotencyKey: string,
    signal?: AbortSignal,
    additionalHeaders?: Readonly<Record<string, string>>
  ): Promise<T> {
    return this.request('POST', path, { idempotencyKey, signal, additionalHeaders });
  }

  private postJson<T>(
    path: string,
    idempotencyKey: string,
    body: unknown,
    signal?: AbortSignal,
    additionalHeaders?: Readonly<Record<string, string>>
  ): Promise<T> {
    return this.request('POST', path, {
      body: JSON.stringify(body),
      contentType: 'application/json',
      idempotencyKey,
      signal,
      additionalHeaders,
    });
  }

  private postQueryJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
    return this.request('POST', path, {
      body: JSON.stringify(body),
      contentType: 'application/json',
      signal,
    });
  }

  private postWorkloadAcl<T>(
    path: string,
    idempotencyKey: string,
    manifest: string,
    signal?: AbortSignal
  ): Promise<T> {
    validateWorkloadAcl(manifest);
    return this.postAcl(path, idempotencyKey, manifest, signal);
  }

  private postMcpServiceProfileAcl<T>(
    path: string,
    idempotencyKey: string,
    acl: string,
    signal?: AbortSignal
  ): Promise<T> {
    validateMcpServiceProfileAcl(acl);
    return this.postAcl(path, idempotencyKey, acl, signal);
  }

  private postMcpRoutePolicyAcl<T>(
    path: string,
    idempotencyKey: string,
    acl: string,
    signal?: AbortSignal
  ): Promise<T> {
    validateMcpRoutePolicyAcl(acl);
    return this.postAcl(path, idempotencyKey, acl, signal);
  }

  private postAcl<T>(
    path: string,
    idempotencyKey: string,
    acl: string,
    signal?: AbortSignal,
    additionalHeaders?: Readonly<Record<string, string>>
  ): Promise<T> {
    return this.request('POST', path, {
      body: acl,
      contentType: A3S_ACL_MEDIA_TYPE,
      idempotencyKey,
      signal,
      additionalHeaders,
    });
  }

  private async request<T>(
    method: 'DELETE' | 'GET' | 'POST',
    path: string,
    options: {
      body?: string;
      contentType?: string;
      healthResponse?: boolean;
      idempotencyKey?: string;
      signal?: AbortSignal;
      additionalHeaders?: Readonly<Record<string, string>>;
      credentials?: RequestCredentials;
    }
  ): Promise<T> {
    if (options.idempotencyKey !== undefined && !isValidIdempotencyKey(options.idempotencyKey)) {
      throw new TypeError('idempotency key is invalid');
    }
    if ((options.body === undefined) !== (options.contentType === undefined)) {
      throw new TypeError('request body and content type must be provided together');
    }
    const controller = new AbortController();
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, this.requestTimeoutMs);
    const abortFromCaller = () => controller.abort();
    options.signal?.addEventListener('abort', abortFromCaller, { once: true });
    if (options.signal?.aborted) {
      controller.abort();
    }

    const headers: Record<string, string> = {
      Accept: 'application/json',
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
    };
    if (options.idempotencyKey !== undefined) {
      headers['Idempotency-Key'] = options.idempotencyKey;
    }
    if (options.body !== undefined) {
      headers['Content-Type'] = options.contentType as string;
    }
    for (const [name, value] of Object.entries(options.additionalHeaders ?? {})) {
      headers[name] = value;
    }

    try {
      const response = await this.fetcher(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: options.body,
        ...(options.credentials ? { credentials: options.credentials } : {}),
        signal: controller.signal,
      });
      return options.healthResponse ? await readHealthResponse<T>(response) : await readResponse<T>(response);
    } catch (error) {
      if (error instanceof CloudApiError) {
        throw error;
      }
      if (options.signal?.aborted) {
        throw new CloudApiError(0, 'Cloud API request was cancelled', 'REQUEST_ABORTED');
      }
      if (timedOut) {
        throw new CloudApiError(0, 'Cloud API request timed out', 'REQUEST_TIMEOUT');
      }
      throw new CloudApiError(0, 'Cloud API request failed', 'NETWORK_ERROR');
    } finally {
      clearTimeout(timeout);
      options.signal?.removeEventListener('abort', abortFromCaller);
    }
  }
}

function validateOidcProviderKey(value: string): void {
  if (!/^[a-z](?:[a-z0-9_-]{0,61}[a-z0-9])?$/.test(value)) {
    throw new TypeError(
      'OIDC provider key must use 1 to 63 lowercase letters, digits, hyphens, or underscores'
    );
  }
}

function setBoundedInteger(
  parameters: URLSearchParams,
  name: string,
  value: number | undefined,
  minimum: number,
  maximum: number,
  label: string
): void {
  if (value === undefined) {
    return;
  }
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new RangeError(`${label} must be between ${minimum} and ${maximum}`);
  }
  parameters.set(name, String(value));
}
