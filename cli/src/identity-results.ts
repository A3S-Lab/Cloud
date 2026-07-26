import type { ApiToken, ApiTokenMutationResult } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const API_TOKEN_COLUMNS: readonly TableColumn<ApiToken>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'SCOPES', value: (row) => row.scopes.join(',') },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt ?? '' },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function apiTokensResult(rows: ApiToken[]): CommandResult {
  const safeRows = rows.map(safeApiToken);
  return { json: safeRows, table: renderTable(safeRows, API_TOKEN_COLUMNS) };
}

export function apiTokenResult(row: ApiToken): CommandResult {
  const safeRow = safeApiToken(row);
  return { json: safeRow, table: renderTable([safeRow], API_TOKEN_COLUMNS) };
}

export function apiTokenMutationResult(row: ApiTokenMutationResult): CommandResult {
  const safeRow = { ...safeApiToken(row), replayed: row.replayed };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [...API_TOKEN_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function safeApiToken(row: ApiToken): ApiToken {
  return {
    id: row.id,
    organizationId: row.organizationId,
    name: row.name,
    scopes: [...row.scopes],
    aggregateVersion: row.aggregateVersion,
    createdAt: row.createdAt,
    expiresAt: row.expiresAt,
    revokedAt: row.revokedAt,
  };
}
