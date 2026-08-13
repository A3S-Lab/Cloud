import { isRfc3339Timestamp, validateNonNilUuid } from './validation';

export interface AuditRecord {
  id: string;
  organizationId: string;
  actorPrincipalId: string | null;
  action: string;
  aggregateId: string;
  occurredAt: string;
  requestId: string;
}

export interface AuditRecordPage {
  records: AuditRecord[];
  nextCursor: string | null;
}

export interface AuditRecordQuery {
  actorPrincipalId?: string;
  action?: string;
  aggregateId?: string;
  requestId?: string;
  from?: string;
  to?: string;
  cursor?: string;
  limit?: number;
}

export const DEFAULT_AUDIT_RECORD_LIMIT = 50;
export const MAX_AUDIT_RECORD_LIMIT = 200;

const ACTION_PATTERN = /^[a-z-]+(?:\.[a-z-]+){2,}$/;

export function encodeAuditRecordQuery(query: AuditRecordQuery = {}): URLSearchParams {
  const parameters = new URLSearchParams();
  for (const [name, value] of [
    ['actorPrincipalId', query.actorPrincipalId],
    ['aggregateId', query.aggregateId],
    ['requestId', query.requestId],
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
