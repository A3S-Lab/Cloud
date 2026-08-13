import type { AuditRecord, AuditRecordPage } from '@a3s/cloud-client';
import { renderTable, sanitizeCell, type TableColumn } from './output';
import type { CommandResult } from './results';

const AUDIT_RECORD_COLUMNS: readonly TableColumn<AuditRecord>[] = [
  { header: 'OCCURRED AT', value: (row) => row.occurredAt },
  { header: 'ACTION', value: (row) => row.action },
  { header: 'ACTOR', value: (row) => row.actorPrincipalId ?? '' },
  { header: 'AGGREGATE', value: (row) => row.aggregateId },
  { header: 'REQUEST', value: (row) => row.requestId },
  { header: 'ID', value: (row) => row.id },
];

export function auditRecordsResult(page: AuditRecordPage): CommandResult {
  const table = renderTable(page.records, AUDIT_RECORD_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}
