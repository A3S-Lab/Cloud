export interface ApiEnvelope<T> {
  code: number;
  message: string;
  data: T;
  requestId: string;
  timestamp: string;
}

export interface ApiErrorEnvelope {
  code: number;
  statusCode: string;
  message: string;
  details: Record<string, unknown>;
  requestId: string;
  timestamp: string;
}

export interface Organization {
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export interface OrganizationMutationResult extends Organization {
  replayed: boolean;
}

export interface Project {
  organizationId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  currentAttributionProfileId: string | null;
  createdAt: string;
}

export interface ProjectMutationResult extends Project {
  replayed: boolean;
}

export interface ProjectAttributionProfile {
  organizationId: string;
  projectId: string;
  id: string;
  previousProfileId: string | null;
  businessOwnerReference: string;
  costAttributionCode: string | null;
  labels: Record<string, string>;
  createdBy: string;
  createdAt: string;
}

export interface UpdateProjectAttributionInput {
  businessOwnerReference: string;
  costAttributionCode?: string | null;
  labels?: Record<string, string>;
}

export interface ProjectAttributionMutationResult {
  project: Project;
  attributionProfile: ProjectAttributionProfile;
  replayed: boolean;
}

export interface Environment {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export interface EnvironmentMutationResult extends Environment {
  replayed: boolean;
}

export type PluginRegistryState = 'active' | 'disabled';

export interface PluginRegistry {
  organizationId: string;
  id: string;
  name: string;
  endpoint: string;
  rootObjectRef: string;
  rootSha256: string;
  rootVersion: number;
  state: PluginRegistryState;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

/**
 * Canonical JSON owned and versioned by A3S Use. Cloud transports these values
 * without maintaining a second catalog field model.
 */
export type A3sUseJsonObject = Readonly<Record<string, unknown>>;
export type PluginCatalogSearchRequest = A3sUseJsonObject;
export type PluginCatalogInspectRequest = A3sUseJsonObject;
export type PluginCatalogPage = A3sUseJsonObject;
export type PluginCatalogInspection = A3sUseJsonObject;

export type AssetKind = 'agent' | 'mcp' | 'skill';
export type AssetState = 'active' | 'archived';

export interface CreateAssetInput {
  name: string;
  kind: AssetKind;
}

export interface Asset {
  organizationId: string;
  id: string;
  name: string;
  kind: AssetKind;
  state: AssetState;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  archivedAt: string | null;
}

export interface AssetMutationResult extends Asset {
  replayed: boolean;
}

export type AssetReleaseState = 'draft' | 'published' | 'yanked';
export type AssetReleaseArtifactKind = 'oci_service' | 'skill_bundle';

export interface CreateAssetReleaseInput {
  version: string;
  commitSha: string;
}

export interface AssetReleaseArtifact {
  kind: AssetReleaseArtifactKind;
  digest: string;
  mediaType: string;
  sizeBytes: number;
}

export interface AssetReleaseProvenance {
  buildRunId: string;
  provenanceDigest: string;
}

export interface AssetRelease {
  organizationId: string;
  assetId: string;
  id: string;
  version: string;
  state: AssetReleaseState;
  commitSha: string;
  manifestDigest: string;
  artifact: AssetReleaseArtifact | null;
  provenance: AssetReleaseProvenance | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  publishedAt: string | null;
  yankedAt: string | null;
}

export interface AssetReleaseMutationResult extends AssetRelease {
  replayed: boolean;
}

export interface McpServiceProfileSpec {
  protocolVersions: string[];
  endpointPath: string;
  runtimePort: string;
  healthPath: string;
  requestSse: boolean;
  subscriptions: boolean;
  serverDiscover: boolean;
  expectedCapabilities: string[];
  maxRequestBytes: number;
  maxResponseBytes: number;
  maxStreamSeconds: number;
}

export interface McpServiceProfile {
  organizationId: string;
  assetId: string;
  assetReleaseId: string;
  profileDigest: string;
  acl: string;
  spec: McpServiceProfileSpec;
  createdAt: string;
}

export interface McpServiceProfileMutationResult extends McpServiceProfile {
  replayed: boolean;
}

export type {
  FormCanonicalValue,
  FormDraft,
  FormDraftInput,
  FormDraftMutationResult,
  FormPublicationMutationResult,
  FormRelease,
  FormReleaseRef,
  FormReleaseSummary,
  PublishFormReleaseOptions,
  ReviseFormDraftOptions,
} from './form';
export type {
  ApiToken,
  ApiTokenMutationResult,
  CreateApiTokenInput,
  CreateMembershipInput,
  CreateMembershipInvitationInput,
  CreateResourceGrantInput,
  CompleteRecipientContactVerificationInput,
  IdentityPrincipalKind,
  Membership,
  MembershipInvitation,
  MembershipInvitationAcceptanceResult,
  MembershipInvitationMutationResult,
  MembershipInvitationStatus,
  MembershipMutationResult,
  MembershipRole,
  OidcAuthorizationStart,
  OidcCallbackResult,
  OidcLinkResult,
  OidcLoginResult,
  RecipientContact,
  RecipientContactMutationResult,
  RecipientContactStatus,
  RequestRecipientContactVerificationInput,
  ResourceGrant,
  ResourceGrantMutationResult,
  ResourceGrantScope,
} from './identity';
export type {
  AcceptPlatformRolePolicyInput,
  ApproveTenantSupportGrantInput,
  ChangePlatformRoleBindingInput,
  CreatePlatformRoleBindingInput,
  DecisionEvidence,
  PlatformPermission,
  PlatformRole,
  PlatformRoleBinding,
  PlatformRoleBindingMutationResult,
  PlatformRolePermission,
  PlatformRolePolicy,
  PlatformRolePolicyMutationResult,
  ProposeTenantSupportGrantInput,
  TenantSupportGrant,
  TenantSupportGrantApproval,
  TenantSupportGrantApprovalMutationResult,
  TenantSupportGrantApprovalOutcome,
  TenantSupportGrantLifecycle,
  TenantSupportGrantMutationResult,
  TenantSupportGrantProposal,
  TenantSupportGrantProposalMutationResult,
  TenantSupportPermission,
  TenantSupportScope,
  TrustDomainRevision,
  TrustDomainRevisionMutationResult,
  WorkloadIdentityFormat,
  WorkloadIdentityPolicyRevision,
  WorkloadIdentityPolicyRevisionMutationResult,
  WorkloadIdentityProviderInspection,
} from './privileged-management';
export type {
  AddNodePoolMembersInput,
  CancelNodePoolMaintenanceInput,
  CreateNodePoolInput,
  EnrollmentToken,
  IssueEnrollmentTokenInput,
  Node,
  NodeAvailability,
  NodePool,
  NodePoolMaintenance,
  NodePoolMaintenanceStatus,
  NodePoolMemberRemoval,
  NodeState,
  RequestNodePoolMemberRemovalInput,
  ScheduleNodePoolMaintenanceInput,
} from './node';
export type { SearchResourceKind, SearchResult } from './search';
export type {
  ReserveUserFileInput,
  UserFile,
  UserFileListOptions,
  UserFileMutationResult,
  UserFileQuota,
  UserFileState,
} from './files';

export type OperationStatus = 'queued' | 'running' | 'suspended' | 'succeeded' | 'failed' | 'cancelled';

export interface Operation {
  id: string;
  organizationId: string;
  subjectKind: string;
  subjectId: string;
  workflowName: string;
  workflowVersion: string;
  status: OperationStatus;
  lastSequence: number;
  requestedAt: string;
  updatedAt: string;
  error: string | null;
  rollbackSourceRevisionId?: string;
  externalSourceRevisionId?: string;
  buildRunId?: string;
}

export type BuildRunStatus =
  | 'queued'
  | 'preparing'
  | 'prepared'
  | 'scheduled'
  | 'running'
  | 'validating'
  | 'publishing'
  | 'attesting'
  | 'cancelling'
  | 'cleanup_pending'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export interface OciDescriptor {
  mediaType: string;
  digest: string;
  size: number;
}

export interface ValidatedOciBuildOutput {
  descriptor: OciDescriptor;
  platforms: string[];
  contentBytes: number;
  blobCount: number;
}

export interface OciPublicationTarget {
  registry: string;
  repository: string;
  descriptor: OciDescriptor;
}

export interface PublishedOciArtifact {
  uri: string;
  digest: string;
  mediaType: string;
  sizeBytes: number;
}

export interface BuildEvidenceSigningKey {
  algorithm: 'ed25519';
  keyId: string;
  publicKey: string;
  keyVersion?: number;
}

export interface BuildEvidenceSummary {
  schema: string;
  verificationState: 'verified';
  sbomDigest: string;
  provenanceDigest: string;
  signingKeyAlgorithm: 'ed25519';
  signingKeyId: string;
  signingKeyVersion: number | null;
  attestedAt: string;
}

export interface BuildEvidence {
  schema: string;
  buildRunId: string;
  operationId: string;
  sourceRevisionId: string;
  attempt: number;
  repository: string;
  commitSha: string;
  sourceContentDigest: string;
  recipe: Record<string, unknown>;
  recipeDigest: string;
  buildRequestDigest: string;
  builder: {
    uri: string;
    digest: string;
  };
  platforms: string[];
  artifact: PublishedOciArtifact;
  sbom: Record<string, unknown>;
  sbomDigest: string;
  provenance: Record<string, unknown>;
  provenanceDigest: string;
  envelope: {
    payloadType: string;
    payload: string;
    signatures: Array<{
      keyId: string;
      signature: string;
    }>;
  };
  signingKey: BuildEvidenceSigningKey;
  verificationState: 'verified';
  attestedAt: string;
}

export interface BuildRun {
  organizationId: string;
  projectId: string;
  environmentId: string;
  id: string;
  sourceRevisionId: string;
  attempt: number;
  retryOfBuildRunId: string | null;
  operationId: string;
  status: BuildRunStatus;
  sourceContentDigest: string | null;
  output: ValidatedOciBuildOutput | null;
  publicationTarget: OciPublicationTarget | null;
  publishedArtifact: PublishedOciArtifact | null;
  evidenceSummary: BuildEvidenceSummary | null;
  failure: string | null;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  startedAt: string | null;
  cancellationRequestedAt: string | null;
  finishedAt: string | null;
}

export interface CancelBuildRunResult {
  buildRunId: string;
  operationId: string;
  status: BuildRunStatus;
  cancellationRequestedAt: string | null;
  replayed: boolean;
}

export interface RetryBuildRunResult {
  buildRunId: string;
  operationId: string;
  sourceRevisionId: string;
  attempt: number;
  retryOfBuildRunId: string;
  status: BuildRunStatus;
  replayed: boolean;
}

export interface BuildRunLogsPage {
  buildRunId: string;
  operationId: string;
  generation: number;
  records: WorkloadLogRecord[];
  nextCursor: string | null;
}

export type ExecutionStatus =
  | 'queued'
  | 'scheduled'
  | 'running'
  | 'cancelling'
  | 'cleanup_pending'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export interface ExecutionArtifact {
  uri: string;
  digest: string;
  mediaType: string;
}

export interface ExecutionProcess {
  command: string[];
  args: string[];
  workingDirectory: string | null;
  environment: Record<string, string>;
}

export interface ExecutionResources {
  cpuMillis: number;
  memoryBytes: number;
  pids: number;
  ephemeralStorageBytes: number | null;
  timeoutMs: number;
}

export interface CreateExecutionInput {
  artifact: ExecutionArtifact;
  process: ExecutionProcess;
  input: unknown;
  resources: ExecutionResources;
}

export interface CreateExecutionTemplateInput {
  definitionAcl: string;
}

export interface ExecutionTemplateRevision {
  organizationId: string;
  projectId: string;
  templateId: string;
  revisionId: string;
  definitionAcl: string;
  definitionDigest: string;
  capability: 'execution.run';
  createdBy: string;
  createdAt: string;
}

export interface ExecutionTemplateMutationResult {
  executionTemplate: ExecutionTemplateRevision;
  replayed: boolean;
}

export interface WorkflowExecutionBinding {
  workflowRunId: string;
  planRevisionId: string;
  planDigest: string;
  stepId: string;
  stepAttempt: number;
  executionTemplateId: string;
  executionTemplateRevisionId: string;
  executionTemplateDigest: string;
}

export type ExecutionOutcome =
  | { kind: 'succeeded'; exitCode: number }
  | { kind: 'failed'; exitCode: number | null; reason: string }
  | { kind: 'cancelled' };

export interface Execution {
  organizationId: string;
  projectId: string;
  environmentId: string;
  id: string;
  operationId: string;
  workflow: WorkflowExecutionBinding | null;
  template: CreateExecutionInput;
  templateDigest: string;
  status: ExecutionStatus;
  outcome: ExecutionOutcome | null;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  startedAt: string | null;
  cancellationRequestedAt: string | null;
  finishedAt: string | null;
}

export interface ExecutionMutationResult {
  execution: Execution;
  replayed: boolean;
}

export type AgentConversationStatus = 'active' | 'closed';

export interface AgentConversation {
  organizationId: string;
  projectId: string;
  environmentId: string;
  id: string;
  status: AgentConversationStatus;
  lastEventSequence: number;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  closedAt: string | null;
}

export interface AgentConversationMutationResult {
  conversation: AgentConversation;
  replayed: boolean;
}

export interface AgentReleaseBinding {
  assetId: string;
  assetReleaseId: string;
  buildRunId: string;
  artifactUri: string;
  artifactDigest: string;
  artifactMediaType: string;
  artifactSizeBytes: number;
}

export type AgentExecutionStatus =
  | 'pending'
  | 'running'
  | 'awaiting_approval'
  | 'cancelling'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export type AgentProviderKind = 'a3s.code' | 'reference.echo';

export interface AgentProviderProfile {
  kind: AgentProviderKind;
  revision: string;
  protocol: 'a3s.cloud.agent-provider.v1';
  nativeProtocol: string;
  profileDigest: string;
  capabilityDigest: string;
}

export type AgentProviderCapability =
  | 'cancellation'
  | 'change_set'
  | 'checkpoints'
  | 'cleanup'
  | 'event_pages'
  | 'pause_resume'
  | 'recovery'
  | 'streaming_output'
  | 'tool_calls';

export interface HarnessAgentReleaseBinding {
  organizationId: string;
  assetId: string;
  assetReleaseId: string;
  buildRunId: string;
  artifactDigest: string;
}

export interface HarnessProviderBinding {
  kind: AgentProviderKind;
  revision: string;
  profileDigest: string;
  capabilityDigest: string;
}

export interface HarnessWorkspaceBinding {
  workloadId: string;
  workloadRevisionId: string;
  runtimeUnitId: string;
  runtimeGeneration: number;
  runtimeSpecDigest: string;
  workingDirectory: string | null;
}

export interface HarnessSkillBinding {
  assetId: string;
  assetReleaseId: string;
  artifactDigest: string;
}

export interface HarnessMcpBinding {
  assetId: string;
  assetReleaseId: string;
  profileDigest: string;
}

export interface HarnessModelBinding {
  modelId: string;
  modelRevisionId: string;
  profileDigest: string;
}

export type HarnessSecretTarget =
  | { kind: 'environment'; variable: string }
  | { kind: 'file'; path: string; mode: number }
  | { kind: 'registry_credential' };

export interface HarnessSecretReference {
  name: string;
  secretId: string;
  version: number;
  target: HarnessSecretTarget;
}

export interface HarnessToolBinding {
  name: string;
  revision: string;
  contractDigest: string;
  approvalRequired: boolean;
}

export interface HarnessInvocationProfile {
  schema: 'a3s.cloud.harness-invocation-profile.v1';
  agent: HarnessAgentReleaseBinding;
  provider: HarnessProviderBinding;
  instructionsDigest: string;
  environmentPolicyDigest: string;
  securityPolicyDigest: string;
  workspace: HarnessWorkspaceBinding;
  skills: HarnessSkillBinding[];
  mcpServers: HarnessMcpBinding[];
  models: HarnessModelBinding[];
  secrets: HarnessSecretReference[];
  tools: HarnessToolBinding[];
  requiredCapabilities: AgentProviderCapability[];
}

export interface AgentExecution {
  organizationId: string;
  conversationId: string;
  id: string;
  operationId: string;
  agent: AgentReleaseBinding;
  provider: AgentProviderProfile;
  invocationProfile: HarnessInvocationProfile | null;
  lineage: AgentExecutionLineage | null;
  status: AgentExecutionStatus;
  failure: string | null;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  startedAt: string | null;
  cancellationRequestedAt: string | null;
  finishedAt: string | null;
}

export interface AgentExecutionLineage {
  parentExecutionId: string;
  parentCheckpointId: string;
  parentCheckpointDigest: string;
  depth: number;
}

export interface StartAgentExecutionInput {
  agentAssetId: string;
  agentAssetReleaseId: string;
  providerKind?: AgentProviderKind;
  input?: unknown;
}

export interface AgentExecutionMutationResult {
  conversation: AgentConversation;
  execution: AgentExecution;
  replayed: boolean;
}

export interface AgentExecutionCheckpointObject {
  schema: 'a3s.cloud.agent-execution-checkpoint-object.v1';
  namespace: 'agent-checkpoints';
  objectRef: string;
  digest: string;
  sizeBytes: number;
  mediaType: 'application/vnd.a3s.agent-execution-checkpoint+json;version=1';
}

export interface AgentExecutionTelemetryCorrelation {
  operationId: string;
  providerRunIdentityDigest: string;
  nodeId: string;
  workloadId: string;
  workloadRevisionId: string;
  deploymentId: string;
  replicaId: string;
  runtimeUnitId: string;
  runtimeGeneration: number;
}

export interface AgentExecutionCheckpoint {
  organizationId: string;
  projectId: string;
  environmentId: string;
  conversationId: string;
  executionId: string;
  id: string;
  throughEventSequence: number;
  eventCount: number;
  agentArtifactDigest: string;
  providerProfileDigest: string;
  invocationProfileDigest: string;
  object: AgentExecutionCheckpointObject;
  telemetryCorrelation: AgentExecutionTelemetryCorrelation;
  aggregateVersion: number;
  capturedAt: string;
}

export interface CaptureAgentExecutionCheckpointInput {
  throughEventSequence?: number | null;
}

export interface AgentExecutionCheckpointMutationResult {
  checkpoint: AgentExecutionCheckpoint;
  replayed: boolean;
}

export interface ForkAgentExecutionInput {
  input?: unknown;
}

export interface ListAgentExecutionCheckpointsOptions {
  limit?: number;
}

export type AgentApprovalCheckpointStatus =
  | 'pending'
  | 'approved'
  | 'denied'
  | 'expired'
  | 'resumed'
  | 'cancelled';

export type AgentApprovalOutcome = 'approved' | 'denied' | 'expired';

export interface AgentApprovalCheckpoint {
  organizationId: string;
  projectId: string;
  environmentId: string;
  conversationId: string;
  executionId: string;
  id: string;
  providerRunIdentityDigest: string;
  invocationProfileDigest: string;
  sourceEventSequence: number;
  callId: string;
  tool: HarnessToolBinding;
  request: AgentToolPayloadIdentity;
  status: AgentApprovalCheckpointStatus;
  decisionId: string | null;
  outcome: AgentApprovalOutcome | null;
  decidedBy: string | null;
  authorizationDecisionId: string | null;
  authorizationDecisionDigest: string | null;
  reason: string | null;
  decisionDigest: string | null;
  resumeCommandId: string | null;
  resumeCommandDigest: string | null;
  aggregateVersion: number;
  requestedAt: string;
  expiresAt: string;
  updatedAt: string;
  decidedAt: string | null;
  resumedAt: string | null;
  cancelledAt: string | null;
}

export interface ListAgentApprovalCheckpointsOptions {
  status?: AgentApprovalCheckpointStatus;
  limit?: number;
}

export interface DecideAgentApprovalCheckpointInput {
  outcome: Exclude<AgentApprovalOutcome, 'expired'>;
  reason?: string | null;
}

export interface AgentApprovalCheckpointMutationResult {
  checkpoint: AgentApprovalCheckpoint;
  replayed: boolean;
}

export type AgentProtocolTerminalRunState = 'completed' | 'failed' | 'cancelled';

export interface AgentProtocolRunIdentityV1 {
  schema: string;
  protocol: string;
  agent_release_identity: string;
  session_id: string;
  run_id: string;
}

export interface AgentProtocolChangeSetV1 {
  schema: 'a3s.code.agent-change-set.v1';
  identity: AgentProtocolRunIdentityV1;
  state: AgentProtocolTerminalRunState;
  format: 'git_unified_diff_v1';
  encoding: 'base64';
  base_tree: string;
  result_tree: string;
  patch_digest: string;
  patch_bytes: number;
  patch_base64: string;
  observed_at_ms: number;
}

export interface AgentExecutionChangeSet {
  organizationId: string;
  executionId: string;
  batchId: string;
  nodeId: string;
  changeSet: AgentProtocolChangeSetV1;
  recordedAt: string;
}

export type AgentExecutionEventKind =
  | 'execution_requested'
  | 'model_output'
  | 'tool_request'
  | 'tool_result'
  | 'approval_resolved'
  | 'execution_failed'
  | 'execution_completed'
  | 'execution_cancelled';

export interface AgentToolPayloadIdentity {
  digest: string;
  sizeBytes: number;
  mediaType: string;
}

export interface AgentToolRequestEventContent {
  callId: string;
  tool: HarnessToolBinding;
  request: AgentToolPayloadIdentity;
}

export interface AgentToolResultEventContent {
  callId: string;
  tool: HarnessToolBinding;
  requestDigest: string;
  outcome: 'succeeded' | 'failed';
  result: AgentToolPayloadIdentity;
}

interface AgentApprovalResolutionEventContentBase {
  checkpointId: string;
  decisionId: string;
  decisionDigest: string;
}

export type AgentApprovalResolutionEventContent = AgentApprovalResolutionEventContentBase &
  (
    | {
        outcome: 'approved' | 'denied';
        decidedBy: string;
        authorizationDecision: { id: string; digest: string };
        reason: string | null;
      }
    | {
        outcome: 'expired';
        decidedBy: null;
        authorizationDecision: null;
        reason: null;
      }
  );

interface AgentExecutionEventBase {
  organizationId: string;
  conversationId: string;
  executionId: string;
  sequence: number;
  contentDigest: string;
  contentSizeBytes: number;
  occurredAt: string;
}

export type AgentExecutionEvent = AgentExecutionEventBase &
  (
    | { kind: 'execution_requested'; content: unknown }
    | { kind: 'model_output'; content: { text: string } }
    | { kind: 'tool_request'; content: AgentToolRequestEventContent }
    | { kind: 'tool_result'; content: AgentToolResultEventContent }
    | { kind: 'approval_resolved'; content: AgentApprovalResolutionEventContent }
    | { kind: 'execution_failed'; content: { reason: string } }
    | { kind: 'execution_completed' | 'execution_cancelled'; content: Record<string, never> }
  );

interface AgentExecutionCheckpointEventBase {
  sequence: number;
  contentDigest: string;
  contentSizeBytes: number;
  occurredAt: string;
}

export type AgentExecutionCheckpointEvent = AgentExecutionCheckpointEventBase &
  (
    | { kind: 'execution_requested'; content: unknown }
    | { kind: 'model_output'; content: { text: string } }
    | { kind: 'tool_request'; content: AgentToolRequestEventContent }
    | { kind: 'tool_result'; content: AgentToolResultEventContent }
    | { kind: 'approval_resolved'; content: AgentApprovalResolutionEventContent }
    | { kind: 'execution_failed'; content: { reason: string } }
    | { kind: 'execution_completed' | 'execution_cancelled'; content: Record<string, never> }
  );

export interface AgentExecutionCheckpointSnapshot {
  schema: 'a3s.cloud.agent-execution-checkpoint.v1';
  organizationId: string;
  conversationId: string;
  executionId: string;
  operationId: string;
  agentArtifactDigest: string;
  providerProfileDigest: string;
  invocationProfileDigest: string;
  throughEventSequence: number;
  eventCount: number;
  telemetryCorrelation: AgentExecutionTelemetryCorrelation;
  events: AgentExecutionCheckpointEvent[];
  capturedAt: string;
}

export interface AgentExecutionTrajectoryOptions {
  cursor?: string;
  throughSequence?: number;
  limit?: number;
}

export interface AgentExecutionTrajectoryPage {
  executionId: string;
  records: AgentExecutionEvent[];
  nextCursor: string | null;
}

export interface AgentExecutionEventsPage {
  conversationId: string;
  headSequence: number;
  records: AgentExecutionEvent[];
  nextCursor: string | null;
}

export interface ServiceTemplate {
  artifact: OciArtifactReference;
  process: ServiceProcess;
  secrets: SecretBinding[];
  resources: ServiceResources;
  ports: ServicePort[];
  health: HttpHealthCheck;
}

export type SourceWorkloadTemplate = Omit<ServiceTemplate, 'artifact'>;

export interface OciArtifactReference {
  uri: string;
  expectedDigest: string | null;
}

export interface ServiceProcess {
  command: string[];
  args: string[];
  workingDirectory: string | null;
  environment: Record<string, string>;
}

export interface SecretBinding {
  name: string;
  secretId: string;
  version: number;
  target: SecretBindingTarget;
}

export type SecretBindingTarget =
  | { kind: 'environment'; variable: string }
  | { kind: 'file'; path: string; mode: number }
  | { kind: 'registry_credential' };

export interface ServiceResources {
  cpuMillis: number;
  memoryBytes: number;
  pids: number;
  ephemeralStorageBytes: number | null;
}

export interface ServicePort {
  name: string;
  containerPort: number;
}

export interface HttpHealthCheck {
  portName: string;
  path: string;
  intervalMs: number;
  timeoutMs: number;
  healthyThreshold: number;
  unhealthyThreshold: number;
  stabilizationWindowMs: number;
}

export interface WorkloadRevision {
  id: string;
  generation: number;
  requestedTemplate: ServiceTemplate;
  artifactSourceUri: string;
  expectedArtifactDigest: string | null;
  requestDigest: string;
  artifactUri: string | null;
  artifactDigest: string | null;
  artifactMediaType: string | null;
  templateDigest: string | null;
  createdAt: string;
  resolvedAt: string | null;
  externalSourceRevisionId?: string;
  buildRunId?: string;
  agentBinding?: AgentWorkloadRevisionBinding;
  mcpBinding?: McpWorkloadRevisionBinding;
  skillBindings: SkillWorkloadRevisionBinding[];
}

export interface AgentWorkloadRevisionBinding {
  organizationId: string;
  assetId: string;
  assetReleaseId: string;
  buildRunId: string;
}

export interface McpWorkloadRevisionBinding {
  organizationId: string;
  assetId: string;
  assetReleaseId: string;
  profileDigest: string;
}

export interface SkillWorkloadRevisionBinding {
  organizationId: string;
  assetId: string;
  assetReleaseId: string;
  artifactDigest: string;
  artifactMediaType: string;
  artifactSizeBytes: number;
  mountName: string;
  mountTarget: string;
}

export interface DeploymentOperation {
  status: OperationStatus;
  lastSequence: number;
  error: string | null;
  updatedAt: string;
}

export type DeploymentStatus =
  | 'queued'
  | 'resolving'
  | 'scheduled'
  | 'applying'
  | 'verifying'
  | 'retiring'
  | 'cancelling'
  | 'cleanup_pending'
  | 'active'
  | 'failed'
  | 'orphaned'
  | 'cancelled';

export type RuntimeUnitState =
  | 'accepted'
  | 'preparing'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'stopped'
  | 'succeeded'
  | 'failed'
  | 'unknown';

export type RuntimeHealthState = 'unknown' | 'starting' | 'healthy' | 'unhealthy';

export interface ObservedRuntime {
  reportId: string;
  nodeId: string;
  commandId: string | null;
  unitId: string;
  generation: number;
  specDigest: string;
  state: RuntimeUnitState;
  healthState: RuntimeHealthState | null;
  healthMessage: string | null;
  providerResourceId: string | null;
  providerBuild: string | null;
  failureCode: string | null;
  failureMessage: string | null;
  observedAt: string;
  receivedAt: string;
}

export interface Deployment {
  id: string;
  workloadId: string;
  replicaId: string;
  replicaGeneration: number;
  memberId: string;
  placementGeneration: number;
  revision: WorkloadRevision;
  operationId: string;
  nodeId: string | null;
  runtimeUnitId: string;
  runtimeGeneration: number;
  commandId: string | null;
  cleanupCommandId: string | null;
  retirementCommandId: string | null;
  status: DeploymentStatus;
  failure: string | null;
  operation: DeploymentOperation | null;
  observedRuntime: ObservedRuntime | null;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  activatedAt: string | null;
  cancellationRequestedAt: string | null;
  cancelledAt: string | null;
}

export interface ManagedWorkloadOwner {
  kind: string;
  ownerId: string;
  ownerGeneration: number;
  ownerSpecDigest: string;
}

export interface EffectivePlacementPolicy {
  schema: string;
  generation: number;
  desiredReplicas: number;
  membersPerReplica: number;
  topology: 'single_node' | 'multi_node';
  replicaAntiAffinity: 'required';
  nodePoolId: string | null;
  digest: string;
}

export interface WorkloadControl {
  managedOwner: ManagedWorkloadOwner | null;
  placementPolicy: EffectivePlacementPolicy;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export interface WorkloadReplicaMember {
  id: string;
  ordinal: number;
  nodeId: string | null;
  placementGeneration: number;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export interface WorkloadReplica {
  id: string;
  ordinal: number;
  revisionId: string;
  revisionGeneration: number;
  generation: number;
  lifecycle: 'desired' | 'retiring' | 'retired';
  evacuationNodeId: string | null;
  retirementCommandId: string | null;
  runtimeFencedAt: string | null;
  members: WorkloadReplicaMember[];
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export interface CancelDeploymentResult {
  deploymentId: string;
  operationId: string;
  status: DeploymentStatus;
  replayed: boolean;
}

export interface StopWorkloadResult {
  organizationId: string;
  workloadId: string;
  operationId: string;
  desiredState: 'stopped';
  requestedAt: string;
  replayed: boolean;
}

export interface WorkloadDeploymentResult {
  organizationId: string;
  projectId: string;
  environmentId: string;
  workloadId: string;
  revisionId: string;
  deploymentId: string;
  operationId: string;
  generation: number;
  status: DeploymentStatus;
  artifactSourceUri: string;
  expectedArtifactDigest: string | null;
  requestDigest: string;
  artifactDigest: string | null;
  templateDigest: string | null;
  requestedAt: string;
  replayed: boolean;
  rollbackSourceRevisionId?: string;
  externalSourceRevisionId?: string;
  buildRunId?: string;
  skillBindings: SkillWorkloadRevisionBinding[];
}

export interface Workload {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  name: string;
  desiredState: 'running' | 'stopped';
  control: WorkloadControl;
  replicas: WorkloadReplica[];
  desiredRevision: WorkloadRevision | null;
  activeRevision: WorkloadRevision | null;
  deployments: Deployment[];
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export type {
  CreateGatewayScopeInput,
  CreateMcpCredentialInput,
  DomainClaim,
  DomainClaimMutationResult,
  DomainClaimState,
  GatewayCertificate,
  GatewayCertificateState,
  GatewayScope,
  GatewayScopeMutationResult,
  McpCredential,
  McpCredentialDeliveryResult,
  McpCredentialMutationResult,
  McpCredentialState,
  McpRoutePolicy,
  McpRoutePolicyGrant,
  McpRoutePolicyLimit,
  McpRoutePolicyMutationResult,
  PublishRouteInput,
  RevokeMcpCredentialInput,
  RotateMcpCredentialInput,
  Route,
  RoutePublicationResult,
  RouteState,
} from './edge';
export type {
  Ontology,
  OntologyChange,
  OntologyChangeCompatibility,
  OntologyChangeKind,
  OntologyDiff,
  OntologyMigrationPolicy,
  OntologyMigrationPolicyKind,
  OntologyMutationResult,
  OntologyResourceKind,
  OntologyRevision,
  OntologyRevisionSummary,
  ReviseOntologyOptions,
} from './ontology';
export type {
  Secret,
  SecretDetails,
  SecretMutationResult,
  SecretState,
  SecretVersion,
  SecretVersionState,
} from './secret';
export type {
  BuildPlatform,
  CreateGithubRepositorySubscriptionInput,
  DockerfileBuildRecipe,
  GithubConnection,
  GithubConnectionInstall,
  GithubConnectionStatus,
  GithubDiscoveredBranch,
  GithubDiscoveredReference,
  GithubDiscoveredReferenceKind,
  GithubDiscoveredRepository,
  GithubDiscoveredTag,
  GithubRepositoryDiscoveryPage,
  GithubRepositoryReferenceDiscoveryPage,
  GithubRepositorySubscription,
  GithubRepositorySubscriptionMutationResult,
  GithubSourceDiscoveryPageOptions,
  GitProvider,
  GitReferenceInput,
  GitReferenceKind,
  GitRepository,
  GitRepositoryInput,
  ResolveSourceRevisionInput,
  SourceRevision,
  SourceRevisionMutationResult,
} from './source';
export type {
  CancelWorkflowRunInput,
  HumanTask,
  HumanTaskAssignmentPolicy,
  HumanTaskInteractionAssignment,
  HumanTaskInteractionBinding,
  HumanTaskInteractionIdentity,
  HumanTaskInteractionOutcome,
  HumanTaskInteractionOutputMapping,
  HumanTaskInteractionRequest,
  HumanTaskInteractionSubmission,
  HumanTaskInteractionSubmissionAssignment,
  HumanTaskMutationResult,
  HumanTaskStatus,
  HumanTaskSummary,
  ListHumanTasksOptions,
  ListWorkflowRunsOptions,
  PublishWorkflowDefinitionInput,
  ReviseWorkflowDefinitionOptions,
  StartWorkflowRunInput,
  WaitWorkflowRunOptions,
  WorkflowCapabilityOwner,
  WorkflowCapabilityReference,
  WorkflowCapabilityType,
  WorkflowDefinition,
  WorkflowDefinitionMutationResult,
  WorkflowDataType,
  WorkflowGoal,
  WorkflowGoalMutationResult,
  WorkflowNodeCatalog,
  WorkflowNodeCatalogAvailability,
  WorkflowNodeCatalogEntry,
  WorkflowNodeExecutionClass,
  WorkflowNodeGateState,
  WorkflowNodeOwner,
  WorkflowPayload,
  WorkflowPayloadAclInput,
  WorkflowPayloadKind,
  WorkflowSemanticContract,
  WorkflowSemanticContractAclsInput,
  WorkflowSemanticContractKind,
  WorkflowPlan,
  WorkflowPlanEdge,
  WorkflowPlanRevision,
  WorkflowPlanStep,
  WorkflowStepDefaultOutputContract,
  WorkflowStepDefaultOutputEvidence,
  WorkflowStepFailureContract,
  WorkflowStepFailureClassification,
  WorkflowStepFailureOutput,
  WorkflowStepFallbackMode,
  WorkflowExecutionFailureDetails,
  WorkflowExecutionOutcome,
  WorkflowExecutionStepOutput,
  WorkflowStepPort,
  WorkflowStepPortCardinality,
  WorkflowStepRetryClassification,
  WorkflowStepDescriptorBinding,
  WorkflowRevision,
  WorkflowRevisionSummary,
  WorkflowRun,
  WorkflowRunDiagnostic,
  WorkflowRunDiagnosticCode,
  WorkflowRunDiagnostics,
  WorkflowRunDiagnosticSeverity,
  WorkflowRunDiagnosticStatus,
  WorkflowRunEvidenceCorrelation,
  WorkflowRunFlowStatistics,
  WorkflowRunHistoryEvent,
  WorkflowRunHistoryOptions,
  WorkflowRunHistoryPage,
  WorkflowRunMutationResult,
  WorkflowRunObservedFlowStatus,
  WorkflowRunOutput,
  WorkflowRunStepStatistics,
  WorkflowRunVariable,
  WorkflowRunVariableInspection,
  WorkflowRunVariableState,
  WorkflowRunStatus,
  WorkflowStepKind,
  WorkflowStepEvidenceReference,
  WorkflowStepProjection,
  WorkflowStepProjectionStatus,
  WorkflowVariableMutationMode,
  WorkflowVariableScope,
  WorkflowVariableStorageClass,
} from './workflow';

export type WorkloadLogStreamFilter = 'stdout' | 'stderr';
export type WorkloadLogRecordKind = 'data' | 'gap';
export type WorkloadLogGapReason =
  | 'missing'
  | 'corrupt'
  | 'retained'
  | 'compacted'
  | 'provider_cursor_lost'
  | 'provider_disconnected';

export interface WorkloadLogRecord {
  kind: WorkloadLogRecordKind;
  sourceCursor: string | null;
  sequence: number;
  observedAtMs: number | null;
  stream: WorkloadLogStreamFilter | null;
  data: string | null;
  gapReason: WorkloadLogGapReason | null;
  fromSequence: number | null;
  throughSequence: number | null;
  compactedChunks: number | null;
}

export interface WorkloadLogsPage {
  workloadId: string;
  revisionId: string;
  nodeId: string | null;
  unitId: string;
  generation: number;
  records: WorkloadLogRecord[];
  nextCursor: string | null;
}
