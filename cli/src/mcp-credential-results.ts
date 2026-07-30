import type {
  McpCredential,
  McpCredentialDeliveryResult,
  McpCredentialMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const MCP_CREDENTIAL_COLUMNS: readonly TableColumn<McpCredential>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PREFIX', value: (row) => row.prefix },
  { header: 'GENERATION', value: (row) => row.generation },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt },
  { header: 'REVOKED AT', value: (row) => row.revokedAt },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function mcpCredentialsResult(rows: McpCredential[]): CommandResult {
  const safeRows = rows.map(safeMcpCredential);
  return { json: safeRows, table: renderTable(safeRows, MCP_CREDENTIAL_COLUMNS) };
}

export function mcpCredentialResult(row: McpCredential): CommandResult {
  const safeRow = safeMcpCredential(row);
  return { json: safeRow, table: renderTable([safeRow], MCP_CREDENTIAL_COLUMNS) };
}

export function mcpCredentialDeliveryResult(row: McpCredentialDeliveryResult): CommandResult {
  const safeRow = {
    ...safeMcpCredential(row),
    secret: row.secret,
    replayed: row.replayed,
  };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [
        ...MCP_CREDENTIAL_COLUMNS,
        { header: 'ONE-TIME SECRET', value: (value) => value.secret },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function mcpCredentialMutationResult(row: McpCredentialMutationResult): CommandResult {
  const safeRow = {
    ...safeMcpCredential(row),
    replayed: row.replayed,
  };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [...MCP_CREDENTIAL_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function safeMcpCredential(row: McpCredential): McpCredential {
  return {
    id: row.id,
    organizationId: row.organizationId,
    projectId: row.projectId,
    environmentId: row.environmentId,
    prefix: row.prefix,
    generation: row.generation,
    aggregateVersion: row.aggregateVersion,
    expiresAt: row.expiresAt,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    revokedAt: row.revokedAt,
  };
}
