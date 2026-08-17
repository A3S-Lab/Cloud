import type { RoutePublicationResult } from './edge';
import type { WorkloadDeploymentResult } from './types';
import { validateWorkloadAcl } from './validation';

export const MAX_DURABLE_CELL_APPLICATION_ACL_BYTES = 256 * 1024;
export const MAX_DURABLE_CELL_SERVICE_PROFILE_ACL_BYTES = 64 * 1024;
export const MAX_OBJECT_NAMESPACE_PROVIDER_PROFILE_ACL_BYTES = 16 * 1024;
export const MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES = 16 * 1024;
export const DEFAULT_DURABLE_CELL_LIST_LIMIT = 50;
export const MAX_DURABLE_CELL_LIST_LIMIT = 200;

export type DurableCellApplicationDesiredState = 'running' | 'stopped';

export interface DurableCellApplication {
  organizationId: string;
  projectId: string;
  environmentId: string;
  applicationId: string;
  name: string;
  desiredState: DurableCellApplicationDesiredState;
  currentRevisionId: string;
  currentRevisionNumber: number;
  currentDefinitionDigest: string;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export interface DurableCellApplicationRevision {
  organizationId: string;
  projectId: string;
  environmentId: string;
  applicationId: string;
  revisionId: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDefinitionDigest: string | null;
  definitionSchema: string;
  definitionAcl: string;
  definitionDigest: string;
  createdBy: string;
  createdAt: string;
}

export interface DurableCellApplicationRecord {
  application: DurableCellApplication;
  revision: DurableCellApplicationRevision;
}

export interface DurableCellApplicationMutationResult {
  record: DurableCellApplicationRecord;
  replayed: boolean;
}

export interface DurableCellDeploymentCorrelation {
  organizationId: string;
  projectId: string;
  environmentId: string;
  applicationId: string;
  applicationRevisionId: string;
  applicationRevisionNumber: number;
  applicationDefinitionDigest: string;
  storageNamespaceId: string;
  workloadId: string;
  workloadRevisionId: string;
  deploymentId: string;
  operationId: string;
  serviceProfileDigest: string;
  serviceTemplateDigest: string;
  providerArtifactDigest: string;
  credentialBindingGeneration: number;
  credentialBindingDigest: string;
  storageProviderProfileDigest: string;
  retentionPolicyDigest: string;
  placementPolicyDigest: string;
  requestedBy: string;
  requestId: string;
  requestedAt: string;
}

export interface DurableCellDeploymentResult {
  correlation: DurableCellDeploymentCorrelation;
  workload: WorkloadDeploymentResult;
  replayed: boolean;
}

export interface DurableCellRoutePublicationResult {
  correlation: DurableCellDeploymentCorrelation;
  publication: RoutePublicationResult;
}

export interface CreateDurableCellApplicationInput {
  name: string;
  definitionAcl: string;
}

export interface ReviseDurableCellApplicationInput {
  expectedVersion: number;
  definitionAcl: string;
}

export interface DeployDurableCellApplicationInput {
  serviceProfileAcl: string;
  storageProviderProfileAcl?: string;
  providerWorkloadAcl: string;
  storageBindingAcl: string;
}

export interface PublishDurableCellApplicationRouteInput {
  serviceProfileAcl: string;
  gatewayScopeId: string;
  domainClaimId: string;
  hostname: string;
  pathPrefix: string;
}

export function validateDurableCellApplicationName(name: string): void {
  const normalized = name.trim();
  if (
    normalized.length === 0 ||
    Array.from(normalized).length > 63 ||
    normalized.includes('\0') ||
    normalized.includes('\r') ||
    normalized.includes('\n')
  ) {
    throw new RangeError('Durable Cell application name must contain 1 to 63 visible characters');
  }
}

export function validateDurableCellApplicationAcl(acl: string): void {
  validateAclBytes(acl, MAX_DURABLE_CELL_APPLICATION_ACL_BYTES, 'Durable Cell application ACL');
}

export function validateDurableCellServiceProfileAcl(acl: string): void {
  validateAclBytes(acl, MAX_DURABLE_CELL_SERVICE_PROFILE_ACL_BYTES, 'Durable Cell Service-profile ACL');
}

export function validateDurableCellStorageBindingAcl(acl: string): void {
  validateAclBytes(acl, MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES, 'Durable Cell storage-binding ACL');
}

export function validateObjectNamespaceProviderProfileAcl(acl: string): void {
  validateAclBytes(
    acl,
    MAX_OBJECT_NAMESPACE_PROVIDER_PROFILE_ACL_BYTES,
    'object namespace provider-profile ACL'
  );
}

export function validateDurableCellExpectedVersion(expectedVersion: number): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected Durable Cell application version must be a positive safe integer');
  }
}

export function validateDurableCellListLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_DURABLE_CELL_LIST_LIMIT) {
    throw new RangeError(`Durable Cell list limit must be between 1 and ${MAX_DURABLE_CELL_LIST_LIMIT}`);
  }
}

export function validateDeployDurableCellApplicationInput(input: DeployDurableCellApplicationInput): void {
  validateDurableCellServiceProfileAcl(input.serviceProfileAcl);
  if (input.storageProviderProfileAcl !== undefined) {
    validateObjectNamespaceProviderProfileAcl(input.storageProviderProfileAcl);
  }
  validateWorkloadAcl(input.providerWorkloadAcl);
  validateDurableCellStorageBindingAcl(input.storageBindingAcl);
}

export function validatePublishDurableCellApplicationRouteInput(
  input: PublishDurableCellApplicationRouteInput
): void {
  validateDurableCellServiceProfileAcl(input.serviceProfileAcl);
}

function validateAclBytes(acl: string, maximum: number, label: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (byteLength < 1 || byteLength > maximum) {
    throw new RangeError(`${label} must contain between 1 and ${maximum} UTF-8 bytes`);
  }
}
