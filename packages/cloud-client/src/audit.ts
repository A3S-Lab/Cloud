import { isRfc3339Timestamp, validateNonNilUuid } from './validation';

export type AuditAttributionStatus =
  | 'legacy_unknown'
  | 'not_applicable'
  | 'profile_missing'
  | 'profile_bound';

export interface AuditRecord {
  id: string;
  organizationId: string;
  actorPrincipalId: string | null;
  action: string;
  aggregateId: string;
  occurredAt: string;
  requestId: string;
  projectId: string | null;
  environmentId: string | null;
  attributionProfileId: string | null;
  attributionStatus: AuditAttributionStatus;
}

export interface AuditRecordPage {
  records: AuditRecord[];
  nextCursor: string | null;
}

export interface AuditRetentionStatus {
  organizationId: string;
  retentionMs: number;
  policyDigest: string;
  appliedPolicyDigest: string | null;
  currentPolicyApplied: boolean;
  recordsAvailableFrom: string | null;
  recordsDeletedBefore: string | null;
  totalDeletedRecords: number;
  lastSweptAt: string | null;
  lastCompletedAt: string | null;
  nextScanAt: string;
  version: number;
}

export interface AuditExportDsseSignature {
  keyId: string;
  signature: string;
}

export interface AuditExportDsseEnvelope {
  payloadType: 'application/vnd.a3s.cloud.audit-export.v1+json';
  payload: string;
  signatures: AuditExportDsseSignature[];
}

export interface AuditExportSigningKey {
  algorithm: 'ed25519';
  keyId: string;
  publicKey: string;
  keyVersion?: number;
}

export interface AuditExport {
  envelope: AuditExportDsseEnvelope;
  signingKey: AuditExportSigningKey;
}

export interface AuditExportManifestDsseEnvelope {
  payloadType: 'application/vnd.a3s.cloud.audit-export-manifest.v1+json';
  payload: string;
  signatures: AuditExportDsseSignature[];
}

export interface AuditExportManifest {
  envelope: AuditExportManifestDsseEnvelope;
  signingKey: AuditExportSigningKey;
}

export interface AuditExportFilter {
  actorPrincipalId: string | null;
  action: string | null;
  aggregateId: string | null;
  requestId: string | null;
  projectId: string | null;
  environmentId: string | null;
  attributionProfileId: string | null;
  attributionStatus: AuditAttributionStatus | null;
  from: string;
  to: string;
  limit: number;
}

export interface AuditExportDocument {
  schema: 'a3s.cloud.audit-export.v1';
  organizationId: string;
  filter: AuditExportFilter;
  cursor: string | null;
  generatedAt: string;
  records: AuditRecord[];
  nextCursor: string | null;
}

export interface AuditExportManifestFilter {
  actorPrincipalId: string | null;
  action: string | null;
  aggregateId: string | null;
  requestId: string | null;
  projectId: string | null;
  environmentId: string | null;
  attributionProfileId: string | null;
  attributionStatus: AuditAttributionStatus | null;
  from: string;
  to: string;
  pageSize: number;
}

export interface AuditExportManifestRetention {
  retentionMs: number;
  policyDigest: string;
  appliedPolicyDigest: string | null;
  currentPolicyApplied: boolean;
  recordsAvailableFrom: string | null;
  recordsDeletedBefore: string | null;
  version: number;
}

export interface AuditExportManifestPage {
  index: number;
  cursor: string | null;
  nextCursor: string | null;
  recordCount: number;
  signingKeyId: string;
  payloadSha256: string;
}

export interface AuditExportManifestDocument {
  schema: 'a3s.cloud.audit-export-manifest.v1';
  organizationId: string;
  filter: AuditExportManifestFilter;
  generatedAt: string;
  retention: AuditExportManifestRetention;
  recordCount: number;
  pageCount: number;
  pages: AuditExportManifestPage[];
}

export interface AuditExportManifestBundle {
  manifest: AuditExportManifest;
  pages: AuditExport[];
}

export interface AuditRecordQuery {
  actorPrincipalId?: string;
  action?: string;
  aggregateId?: string;
  requestId?: string;
  projectId?: string;
  environmentId?: string;
  attributionProfileId?: string;
  attributionStatus?: AuditAttributionStatus;
  from?: string;
  to?: string;
  cursor?: string;
  limit?: number;
}

export type AuditExportQuery = Omit<AuditRecordQuery, 'from' | 'to'> & {
  from: string;
  to: string;
};

export type AuditExportManifestQuery = Omit<AuditRecordQuery, 'from' | 'to' | 'cursor' | 'limit'> & {
  from: string;
  to: string;
  pageSize?: number;
};

export const DEFAULT_AUDIT_RECORD_LIMIT = 50;
export const MAX_AUDIT_RECORD_LIMIT = 200;
export const MAX_AUDIT_EXPORT_WINDOW_DAYS = 31;
export const DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE = 200;
export const MAX_AUDIT_EXPORT_MANIFEST_PAGES = 8;

const ACTION_PATTERN = /^[a-z-]+(?:\.[a-z-]+){2,}$/;

export function encodeAuditRecordQuery(query: AuditRecordQuery = {}): URLSearchParams {
  const parameters = new URLSearchParams();
  for (const [name, value] of [
    ['actorPrincipalId', query.actorPrincipalId],
    ['aggregateId', query.aggregateId],
    ['requestId', query.requestId],
    ['projectId', query.projectId],
    ['environmentId', query.environmentId],
    ['attributionProfileId', query.attributionProfileId],
  ] as const) {
    if (value !== undefined) {
      validateNonNilUuid(value, `audit ${name}`);
      parameters.set(name, value);
    }
  }
  if (query.action !== undefined) {
    if (query.action.length > 255 || /[\0\r\n]/u.test(query.action) || !ACTION_PATTERN.test(query.action)) {
      throw new TypeError('audit action must use bounded lowercase dot-separated segments');
    }
    parameters.set('action', query.action);
  }
  if (query.attributionStatus !== undefined) {
    if (
      !['legacy_unknown', 'not_applicable', 'profile_missing', 'profile_bound'].includes(
        query.attributionStatus
      )
    ) {
      throw new TypeError('audit attribution status is invalid');
    }
    parameters.set('attributionStatus', query.attributionStatus);
  }
  for (const [name, value] of [
    ['from', query.from],
    ['to', query.to],
  ] as const) {
    if (value !== undefined) {
      if (!isRfc3339Timestamp(value)) {
        throw new TypeError(`audit ${name} must be an RFC 3339 timestamp`);
      }
      parameters.set(name, value);
    }
  }
  if (query.from !== undefined && query.to !== undefined && Date.parse(query.from) > Date.parse(query.to)) {
    throw new RangeError('audit from timestamp must not exceed to timestamp');
  }
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 128 || /[\0\r\n]/.test(query.cursor)) {
      throw new TypeError('audit record cursor is invalid');
    }
    parameters.set('cursor', query.cursor);
  }
  const limit = query.limit ?? DEFAULT_AUDIT_RECORD_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_AUDIT_RECORD_LIMIT) {
    throw new RangeError(`audit record limit must be between 1 and ${MAX_AUDIT_RECORD_LIMIT}`);
  }
  parameters.set('limit', String(limit));
  return parameters;
}

export function encodeAuditExportQuery(query: AuditExportQuery): URLSearchParams {
  if (query.from === undefined || query.to === undefined) {
    throw new TypeError('audit export requires both from and to timestamps');
  }
  const parameters = encodeAuditRecordQuery(query);
  const windowMilliseconds = Date.parse(query.to) - Date.parse(query.from);
  if (windowMilliseconds > MAX_AUDIT_EXPORT_WINDOW_DAYS * 24 * 60 * 60 * 1000) {
    throw new RangeError(`audit export window must not exceed ${MAX_AUDIT_EXPORT_WINDOW_DAYS} days`);
  }
  return parameters;
}

export function encodeAuditExportManifestQuery(query: AuditExportManifestQuery): URLSearchParams {
  const untypedQuery = query as AuditExportManifestQuery & Pick<AuditRecordQuery, 'cursor' | 'limit'>;
  if (untypedQuery.cursor !== undefined || untypedQuery.limit !== undefined) {
    throw new TypeError('audit export manifest does not accept cursor or limit; use pageSize');
  }
  const { pageSize = DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE, ...filter } = query;
  if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > MAX_AUDIT_RECORD_LIMIT) {
    throw new RangeError(`audit export manifest page size must be between 1 and ${MAX_AUDIT_RECORD_LIMIT}`);
  }
  const parameters = encodeAuditExportQuery({
    ...filter,
    limit: pageSize,
  });
  parameters.delete('limit');
  parameters.set('pageSize', String(pageSize));
  return parameters;
}
