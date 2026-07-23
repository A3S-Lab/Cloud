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

export interface Project {
  organizationId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export interface Environment {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

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
  revision: WorkloadRevision;
  operationId: string;
  nodeId: string | null;
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
}

export interface Workload {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  name: string;
  desiredState: 'running' | 'stopped';
  desiredRevision: WorkloadRevision | null;
  activeRevision: WorkloadRevision | null;
  deployments: Deployment[];
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export type RouteState = 'pending' | 'publishing' | 'active' | 'rejected';

export interface Route {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  gatewayNodeId: string;
  hostname: string;
  pathPrefix: string;
  domainClaimId: string | null;
  domainPattern: string | null;
  gatewayCertificateId: string | null;
  workloadId: string;
  workloadRevisionId: string;
  portName: string;
  state: RouteState;
  gatewayRevision: number | null;
  gatewayCommandId: string | null;
  snapshotDigest: string | null;
  failure: string | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  activatedAt: string | null;
}

export type GatewayCertificateState = 'provisioning' | 'issued' | 'ready' | 'failed' | 'revoked';

export interface GatewayCertificate {
  id: string;
  organizationId: string;
  nodeId: string;
  domainClaimIds: string[];
  dnsNames: string[];
  gatewayRevision: number;
  gatewayCommandId: string;
  snapshotDigest: string;
  state: GatewayCertificateState;
  serialNumber: string | null;
  fingerprint: string | null;
  issuedAt: string | null;
  expiresAt: string | null;
  failure: string | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  readyAt: string | null;
  revokedAt: string | null;
}

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
