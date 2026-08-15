export const MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES = 64 * 1024;
export const DEFAULT_CONNECTOR_LIST_LIMIT = 50;
export const MAX_CONNECTOR_LIST_LIMIT = 200;

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

export interface CreateConnectorProfileInput {
  name: string;
  definitionAcl: string;
}

export interface ReviseConnectorProfileInput {
  expectedVersion: number;
  definitionAcl: string;
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

export function validateConnectorListLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_CONNECTOR_LIST_LIMIT) {
    throw new RangeError(`Connector list limit must be between 1 and ${MAX_CONNECTOR_LIST_LIMIT}`);
  }
}
