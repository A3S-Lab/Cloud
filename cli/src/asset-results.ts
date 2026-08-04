import type { Asset, AssetMutationResult, AssetRelease, AssetReleaseMutationResult } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const ASSET_COLUMNS: readonly TableColumn<Asset>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'KIND', value: (row) => row.kind },
  { header: 'STATE', value: (row) => row.state },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

const RELEASE_COLUMNS: readonly TableColumn<AssetRelease>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'VERSION', value: (row) => row.version },
  { header: 'STATE', value: (row) => row.state },
  { header: 'COMMIT', value: (row) => row.commitSha },
  { header: 'ARTIFACT', value: (row) => row.artifact?.digest ?? '' },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function assetsResult(rows: Asset[]): CommandResult {
  return listResult(rows, ASSET_COLUMNS);
}

export function assetResult(row: Asset): CommandResult {
  return singleResult(row, ASSET_COLUMNS);
}

export function assetMutationResult(row: AssetMutationResult): CommandResult {
  return singleResult(row, [...ASSET_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]);
}

export function assetReleasesResult(rows: AssetRelease[]): CommandResult {
  return listResult(rows, RELEASE_COLUMNS);
}

export function assetReleaseResult(row: AssetRelease): CommandResult {
  return singleResult(row, RELEASE_COLUMNS);
}

export function assetReleaseMutationResult(row: AssetReleaseMutationResult): CommandResult {
  return singleResult(row, [...RELEASE_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]);
}

function listResult<T>(rows: T[], columns: readonly TableColumn<T>[]): CommandResult {
  return { json: rows, table: renderTable(rows, columns) };
}

function singleResult<T>(row: T, columns: readonly TableColumn<T>[]): CommandResult {
  return { json: row, table: renderTable([row], columns) };
}
