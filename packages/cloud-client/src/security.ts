export type SecurityAuditCorrelation = 'verified' | 'missing';

export type GatewayRoutePolicySecurityEventKey =
  | 'edge.mcp-route-policy.created'
  | 'edge.mcp-route-policy.revised';

export interface GatewayRoutePolicyTimelineEntry {
  eventId: string;
  eventKey: GatewayRoutePolicySecurityEventKey;
  schemaVersion: 1;
  organizationId: string;
  projectId: string;
  environmentId: string;
  routeId: string;
  policyRevision: number;
  policyDigest: string;
  occurredAt: string;
  correlationId: string;
  auditCorrelation: SecurityAuditCorrelation;
  auditRecordId: string | null;
  actorPrincipalId: string | null;
}

export interface GatewayRoutePolicyTimelinePage {
  entries: GatewayRoutePolicyTimelineEntry[];
  nextCursor: string | null;
}

export interface SecurityTimelineQuery {
  cursor?: string;
  limit?: number;
}

export const DEFAULT_SECURITY_TIMELINE_LIMIT = 50;
export const MAX_SECURITY_TIMELINE_LIMIT = 100;

export function encodeSecurityTimelineQuery(query: SecurityTimelineQuery = {}): URLSearchParams {
  const parameters = new URLSearchParams();
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 128 || /[\0\r\n]/u.test(query.cursor)) {
      throw new TypeError('security timeline cursor is invalid');
    }
    parameters.set('cursor', query.cursor);
  }
  const limit = query.limit ?? DEFAULT_SECURITY_TIMELINE_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_SECURITY_TIMELINE_LIMIT) {
    throw new RangeError(`security timeline limit must be between 1 and ${MAX_SECURITY_TIMELINE_LIMIT}`);
  }
  parameters.set('limit', String(limit));
  return parameters;
}
