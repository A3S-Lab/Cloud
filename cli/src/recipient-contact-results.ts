import type { RecipientContact, RecipientContactMutationResult } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const RECIPIENT_CONTACT_COLUMNS: readonly TableColumn<RecipientContact>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'ADDRESS', value: (row) => row.addressHint },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'VERIFIED AT', value: (row) => row.verifiedAt ?? '' },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function recipientContactsResult(rows: RecipientContact[]): CommandResult {
  const safeRows = rows.map(safeRecipientContact);
  return { json: safeRows, table: renderTable(safeRows, RECIPIENT_CONTACT_COLUMNS) };
}

export function recipientContactResult(row: RecipientContact): CommandResult {
  const safeRow = safeRecipientContact(row);
  return { json: safeRow, table: renderTable([safeRow], RECIPIENT_CONTACT_COLUMNS) };
}

export function recipientContactMutationResult(row: RecipientContactMutationResult): CommandResult {
  const safeRow = { ...safeRecipientContact(row), replayed: row.replayed };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [...RECIPIENT_CONTACT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function safeRecipientContact(row: RecipientContact): RecipientContact {
  return {
    id: row.id,
    principalId: row.principalId,
    addressDigest: row.addressDigest,
    addressHint: row.addressHint,
    aggregateVersion: row.aggregateVersion,
    status: row.status,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    verifiedAt: row.verifiedAt,
    revokedAt: row.revokedAt,
  };
}
