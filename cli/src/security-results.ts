import type { GatewayRoutePolicyTimelineEntry, GatewayRoutePolicyTimelinePage } from '@a3s/cloud-client';
import { renderTable, sanitizeCell, type TableColumn } from './output';
import type { CommandResult } from './results';

const SECURITY_TIMELINE_COLUMNS: readonly TableColumn<GatewayRoutePolicyTimelineEntry>[] = [
  { header: 'OCCURRED AT', value: (row) => row.occurredAt },
  { header: 'EVENT', value: (row) => row.eventKey },
  { header: 'POLICY REV', value: (row) => row.policyRevision },
  { header: 'AUDIT', value: (row) => row.auditCorrelation },
  { header: 'ACTOR', value: (row) => row.actorPrincipalId ?? '' },
  { header: 'CORRELATION', value: (row) => row.correlationId },
  { header: 'EVENT ID', value: (row) => row.eventId },
];

export function gatewayRoutePolicyTimelineResult(page: GatewayRoutePolicyTimelinePage): CommandResult {
  const table = renderTable(page.entries, SECURITY_TIMELINE_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}
