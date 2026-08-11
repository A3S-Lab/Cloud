import type { A3sUseJsonObject, PluginRegistry } from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const PLUGIN_REGISTRY_COLUMNS = [
  { header: 'ID', value: (row: PluginRegistry) => row.id },
  { header: 'NAME', value: (row: PluginRegistry) => row.name },
  { header: 'STATE', value: (row: PluginRegistry) => row.state },
  { header: 'ENDPOINT', value: (row: PluginRegistry) => row.endpoint },
  { header: 'ROOT VERSION', value: (row: PluginRegistry) => row.rootVersion },
  { header: 'ROOT SHA-256', value: (row: PluginRegistry) => row.rootSha256 },
] as const;

export function pluginRegistriesResult(rows: PluginRegistry[]): CommandResult {
  return { json: rows, table: renderTable(rows, PLUGIN_REGISTRY_COLUMNS) };
}

export function pluginRegistryResult(row: PluginRegistry): CommandResult {
  return { json: row, table: renderTable([row], PLUGIN_REGISTRY_COLUMNS) };
}

export function pluginCatalogResult(value: A3sUseJsonObject): CommandResult {
  return { json: value, table: `${JSON.stringify(value, null, 2)}\n` };
}
