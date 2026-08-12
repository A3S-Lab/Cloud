import type {
  ApiToken,
  ApiTokenMutationResult,
  Membership,
  MembershipMutationResult,
  ResourceGrant,
  ResourceGrantMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const API_TOKEN_COLUMNS: readonly TableColumn<ApiToken>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
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
    principalId: row.principalId,
    name: row.name,
    scopes: [...row.scopes],
    aggregateVersion: row.aggregateVersion,
    createdAt: row.createdAt,
    expiresAt: row.expiresAt,
    revokedAt: row.revokedAt,
  };
}

const MEMBERSHIP_COLUMNS: readonly TableColumn<Membership>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'NAME', value: (row) => row.principalName },
  { header: 'KIND', value: (row) => row.principalKind },
  { header: 'ROLE', value: (row) => row.role },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function membershipsResult(rows: Membership[]): CommandResult {
  return { json: rows, table: renderTable(rows, MEMBERSHIP_COLUMNS) };
}

export function membershipResult(row: Membership): CommandResult {
  return { json: row, table: renderTable([row], MEMBERSHIP_COLUMNS) };
}

export function membershipMutationResult(row: MembershipMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...MEMBERSHIP_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const RESOURCE_GRANT_COLUMNS: readonly TableColumn<ResourceGrant>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'MEMBERSHIP', value: (row) => row.membershipId },
  { header: 'KIND', value: (row) => row.scope.kind },
  { header: 'RESOURCE', value: resourceGrantScopeIdentity },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function resourceGrantsResult(rows: ResourceGrant[]): CommandResult {
  return { json: rows, table: renderTable(rows, RESOURCE_GRANT_COLUMNS) };
}

export function resourceGrantResult(row: ResourceGrant): CommandResult {
  return { json: row, table: renderTable([row], RESOURCE_GRANT_COLUMNS) };
}

export function resourceGrantMutationResult(row: ResourceGrantMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...RESOURCE_GRANT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function resourceGrantScopeIdentity(row: ResourceGrant): string {
  switch (row.scope.kind) {
    case 'project':
      return row.scope.projectId;
    case 'environment':
      return `${row.scope.projectId}/${row.scope.environmentId}`;
    case 'node':
      return row.scope.nodeId;
  }
}
