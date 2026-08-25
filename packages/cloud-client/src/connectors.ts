export const MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES = 64 * 1024;
export const MAX_CONNECTOR_REVISION_REVOCATION_REASON_BYTES = 1024;
export const MAX_CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_BYTES = 1024;
export const DEFAULT_CONNECTOR_LIST_LIMIT = 50;
export const MAX_CONNECTOR_LIST_LIMIT = 200;
export const DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT = 50;
export const MAX_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT = 100;

export interface ConnectorProfile {
  organizationId: string;
  projectId: string;
  environmentId: string;
  profileId: string;
  name: string;
  currentRevisionId: string;
  currentRevisionNumber: number;
  currentRevisionDigest: string;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export interface ConnectorRevision {
  organizationId: string;
  projectId: string;
  environmentId: string;
  profileId: string;
  revisionId: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDigest: string | null;
  definitionKind: string;
  definitionSchema: string;
  definitionAcl: string;
  definitionDigest: string;
  createdBy: string;
  createdAt: string;
}

export interface ConnectorProfileRecord {
  profile: ConnectorProfile;
  revision: ConnectorRevision;
}

export interface ConnectorProfileMutationResult {
  record: ConnectorProfileRecord;
  replayed: boolean;
}

export interface ConnectorRevisionRevocation {
  organizationId: string;
  projectId: string;
  environmentId: string;
  profileId: string;
  revisionId: string;
  revisionNumber: number;
  definitionDigest: string;
  reason: string;
  revokedBy: string;
  revokedAt: string;
}

export interface ConnectorRevisionRevocationMutationResult {
  revocation: ConnectorRevisionRevocation;
  replayed: boolean;
}

export type ConnectorExecutionAttemptState = 'reserved' | 'dispatching' | 'terminal';
export type ConnectorExecutionRecoveryState =
  | 'reserved'
  | 'reservation_expired'
  | 'in_flight'
  | 'indeterminate'
  | 'completed';
export type ConnectorExecutionOutcome = 'accepted' | 'retryable' | 'rejected' | 'indeterminate';

export interface ConnectorExecutionAttempt {
  organizationId: string;
  projectId: string;
  environmentId: string;
  profileId: string;
  revisionId: string;
  attemptId: string;
  requestDigest: string;
  requestBodyBytes: number;
  state: ConnectorExecutionAttemptState;
  recoveryState: ConnectorExecutionRecoveryState;
  reservedAt: string;
  leaseExpiresAt: string;
  dispatchStartedAt: string | null;
  outcomeDeadlineAt: string | null;
  terminalAt: string | null;
  createdAt: string;
  observedAt: string;
  evidenceOutcome: ConnectorExecutionOutcome | null;
  responseStatus: number | null;
  responseDigest: string | null;
  responseBodyBytes: number | null;
  retryAfterSeconds: number | null;
  evidenceStartedAt: string | null;
  evidenceCompletedAt: string | null;
}

export interface ConnectorExecutionAttemptPage {
  attempts: ConnectorExecutionAttempt[];
  nextCursor: string | null;
}

export interface ConnectorExecutionAttemptResolution {
  organizationId: string;
  projectId: string;
  environmentId: string;
  profileId: string;
  revisionId: string;
  attemptId: string;
  requestDigest: string;
  requestBodyBytes: number;
  dispatchStartedAt: string;
  outcomeDeadlineAt: string;
  resolution: 'indeterminate';
  reason: string;
  resolvedBy: string;
  resolvedAt: string;
}

export interface ConnectorExecutionAttemptResolutionMutationResult {
  resolution: ConnectorExecutionAttemptResolution;
  replayed: boolean;
}

export interface ConnectorExecutionAttemptQuery {
  cursor?: string;
  limit?: number;
}

export interface CreateConnectorProfileInput {
  name: string;
  definitionAcl: string;
}

export interface ReviseConnectorProfileInput {
  expectedVersion: number;
  definitionAcl: string;
}

export interface RevokeConnectorRevisionInput {
  reason: string;
}

export interface ResolveConnectorExecutionAttemptInput {
  reason: string;
}

export function validateConnectorProfileName(name: string): void {
  const normalized = name.trim();
  if (
    normalized.length === 0 ||
    Array.from(normalized).length > 63 ||
    normalized.includes('\0') ||
    normalized.includes('\r') ||
    normalized.includes('\n')
  ) {
    throw new RangeError('Connector profile name must contain 1 to 63 visible characters');
  }
}

export function validateConnectorDefinitionAcl(acl: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (byteLength < 1 || byteLength > MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES) {
    throw new RangeError(
      `Connector definition ACL must contain between 1 and ${MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES} UTF-8 bytes`
    );
  }
}

export function validateConnectorExpectedVersion(expectedVersion: number): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected Connector profile version must be a positive safe integer');
  }
}

export function validateConnectorRevisionRevocationReason(reason: string): void {
  const normalized = reason.trim();
  const byteLength = new TextEncoder().encode(normalized).byteLength;
  if (
    byteLength < 1 ||
    byteLength > MAX_CONNECTOR_REVISION_REVOCATION_REASON_BYTES ||
    /\p{Cc}/u.test(normalized)
  ) {
    throw new RangeError(
      `Connector revision revocation reason must contain between 1 and ${MAX_CONNECTOR_REVISION_REVOCATION_REASON_BYTES} control-free UTF-8 bytes`
    );
  }
}

export function validateConnectorExecutionAttemptResolutionReason(reason: string): void {
  const normalized = reason.trim();
  const byteLength = new TextEncoder().encode(normalized).byteLength;
  if (
    byteLength < 1 ||
    byteLength > MAX_CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_BYTES ||
    /\p{Cc}/u.test(normalized)
  ) {
    throw new RangeError(
      `Connector execution attempt resolution reason must contain between 1 and ${MAX_CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_BYTES} control-free UTF-8 bytes`
    );
  }
}

export function encodeConnectorExecutionAttemptQuery(query: ConnectorExecutionAttemptQuery = {}): string {
  const limit = query.limit ?? DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT) {
    throw new RangeError(
      `Connector execution attempt list limit must be between 1 and ${MAX_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT}`
    );
  }
  if (
    query.cursor !== undefined &&
    (query.cursor.length < 1 ||
      query.cursor.length > 128 ||
      query.cursor.includes('\0') ||
      query.cursor.includes('\r') ||
      query.cursor.includes('\n'))
  ) {
    throw new RangeError('Connector execution attempt cursor is invalid');
  }
  const parameters = new URLSearchParams({ limit: String(limit) });
  if (query.cursor !== undefined) {
    parameters.set('cursor', query.cursor);
  }
  return parameters.toString();
}

export function validateConnectorListLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_CONNECTOR_LIST_LIMIT) {
    throw new RangeError(`Connector list limit must be between 1 and ${MAX_CONNECTOR_LIST_LIMIT}`);
  }
}
