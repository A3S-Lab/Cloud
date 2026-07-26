import type { Secret, SecretDetails, SecretMutationResult, SecretVersion } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const SECRET_COLUMNS: readonly TableColumn<Secret>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'STATE', value: (row) => row.state },
  { header: 'CURRENT VERSION', value: (row) => row.currentVersion },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function secretsResult(rows: Secret[]): CommandResult {
  const safeRows = rows.map(safeSecret);
  return { json: safeRows, table: renderTable(safeRows, SECRET_COLUMNS) };
}

export function secretDetailsResult(row: SecretDetails): CommandResult {
  const safeRow = {
    ...safeSecret(row),
    versions: row.versions.map(safeSecretVersion),
  };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [
        ...SECRET_COLUMNS,
        {
          header: 'VERSIONS',
          value: (value) => value.versions.map((version) => `${version.version}:${version.state}`).join(','),
        },
      ]
    ),
  };
}

export function secretMutationResult(row: SecretMutationResult): CommandResult {
  const safeRow = {
    ...safeSecret(row),
    version: safeSecretVersion(row.version),
    replayed: row.replayed,
  };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [
        ...SECRET_COLUMNS,
        { header: 'CHANGED VERSION', value: (value) => value.version.version },
        { header: 'VERSION STATE', value: (value) => value.version.state },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

function safeSecret(row: Secret): Secret {
  return {
    id: row.id,
    organizationId: row.organizationId,
    projectId: row.projectId,
    environmentId: row.environmentId,
    name: row.name,
    state: row.state,
    currentVersion: row.currentVersion,
    aggregateVersion: row.aggregateVersion,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    revokedAt: row.revokedAt,
  };
}

function safeSecretVersion(row: SecretVersion): SecretVersion {
  return {
    version: row.version,
    state: row.state,
    aggregateVersion: row.aggregateVersion,
    createdAt: row.createdAt,
    revokedAt: row.revokedAt,
  };
}
