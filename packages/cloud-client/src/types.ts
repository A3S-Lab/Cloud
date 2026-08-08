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
  createdAt: string;
}

export interface ProjectMutationResult extends Project {
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
  ApiToken,
  ApiTokenMutationResult,
  CreateApiTokenInput,
  CreateServiceMembershipInput,
  IdentityPrincipalKind,
  Membership,
  MembershipMutationResult,
  MembershipRole,
} from './identity';

export type {
  EnrollmentToken,
  IssueEnrollmentTokenInput,
  Node,
  NodeAvailability,
  NodeState,
} from './node';

export type { SearchResourceKind, SearchResult } from './search';

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
  | 'cancelling'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export interface AgentExecution {
  organizationId: string;
  conversationId: string;
  id: string;
  operationId: string;
  agent: AgentReleaseBinding;
  status: AgentExecutionStatus;
  failure: string | null;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  startedAt: string | null;
  cancellationRequestedAt: string | null;
  finishedAt: string | null;
}

export interface StartAgentExecutionInput {
  agentAssetId: string;
  agentAssetReleaseId: string;
  input?: unknown;
}

export interface AgentExecutionMutationResult {
  conversation: AgentConversation;
  execution: AgentExecution;
  replayed: boolean;
}

export type AgentExecutionEventKind =
  | 'execution_requested'
  | 'model_output'
  | 'execution_failed'
  | 'execution_completed'
  | 'execution_cancelled';

export interface AgentExecutionEvent {
  organizationId: string;
  conversationId: string;
  executionId: string;
  sequence: number;
  kind: AgentExecutionEventKind;
  content: unknown;
  contentDigest: string;
  contentSizeBytes: number;
  occurredAt: string;
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
  topology: 'single_node';
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
  generation: number;
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
  PublishWorkflowDefinitionInput,
  ReviseWorkflowDefinitionOptions,
  WorkflowCapabilityOwner,
  WorkflowCapabilityReference,
  WorkflowCapabilityType,
  WorkflowDefinition,
  WorkflowDefinitionMutationResult,
  WorkflowGoal,
  WorkflowGoalMutationResult,
  WorkflowPayload,
  WorkflowPayloadAclInput,
  WorkflowPayloadKind,
  WorkflowPlan,
  WorkflowPlanEdge,
  WorkflowPlanRevision,
  WorkflowPlanStep,
  WorkflowRevision,
  WorkflowRevisionSummary,
  WorkflowStepKind,
} from './workflow';
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
  GithubRepositorySubscription,
  GithubRepositorySubscriptionMutationResult,
  GitProvider,
  GitReferenceInput,
  GitReferenceKind,
  GitRepository,
  GitRepositoryInput,
  ResolveSourceRevisionInput,
  SourceRevision,
  SourceRevisionMutationResult,
} from './source';

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
