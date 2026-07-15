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
}

export interface WorkloadRevision {
  id: string;
  generation: number;
  artifactSourceUri: string;
  expectedArtifactDigest: string | null;
  requestDigest: string;
  artifactUri: string | null;
  artifactDigest: string | null;
  artifactMediaType: string | null;
  templateDigest: string | null;
  createdAt: string;
  resolvedAt: string | null;
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
