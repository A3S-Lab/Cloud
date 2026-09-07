import type {
  A3sUseJsonObject,
  PluginAssignment,
  PluginAssignmentMutationResult,
  PluginPlanProjection,
  PluginRegistry,
  PluginRegistryMutationResult,
} from '@a3s/cloud-client';
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

const PLUGIN_ASSIGNMENT_COLUMNS = [
  { header: 'ID', value: (row: PluginAssignment) => row.id },
  { header: 'PACKAGE', value: (row: PluginAssignment) => row.packageId },
  { header: 'VERSION', value: (row: PluginAssignment) => row.version },
  { header: 'DESIRED', value: (row: PluginAssignment) => row.desiredState },
  { header: 'HOST', value: (row: PluginAssignment) => row.targetHostId },
  { header: 'GENERATION', value: (row: PluginAssignment) => row.assignmentGeneration },
  { header: 'AGGREGATE', value: (row: PluginAssignment) => row.aggregateVersion },
] as const;

const PLUGIN_PLAN_PROJECTION_COLUMNS = [
  { header: 'ID', value: (row: PluginPlanProjection) => row.id },
  { header: 'PACKAGE', value: (row: PluginPlanProjection) => row.rootPackageId },
  { header: 'ACTION', value: (row: PluginPlanProjection) => row.action },
  { header: 'AUTHORITY', value: (row: PluginPlanProjection) => row.authorityDecision },
  {
    header: 'AWAITS CONFIRMATION',
    value: (row: PluginPlanProjection) => (row.awaitsConfirmation ? 'yes' : 'no'),
  },
  { header: 'PLAN DIGEST', value: (row: PluginPlanProjection) => row.planDigest },
] as const;

export function pluginRegistriesResult(rows: PluginRegistry[]): CommandResult {
  return { json: rows, table: renderTable(rows, PLUGIN_REGISTRY_COLUMNS) };
}

export function pluginRegistryResult(row: PluginRegistry): CommandResult {
  return { json: row, table: renderTable([row], PLUGIN_REGISTRY_COLUMNS) };
}

export function pluginRegistryMutationResult(result: PluginRegistryMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable([result.registry], PLUGIN_REGISTRY_COLUMNS),
  };
}

export function pluginAssignmentsResult(rows: PluginAssignment[]): CommandResult {
  return { json: rows, table: renderTable(rows, PLUGIN_ASSIGNMENT_COLUMNS) };
}

export function pluginAssignmentResult(row: PluginAssignment): CommandResult {
  return { json: row, table: renderTable([row], PLUGIN_ASSIGNMENT_COLUMNS) };
}

export function pluginAssignmentMutationResult(result: PluginAssignmentMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable([result.assignment], PLUGIN_ASSIGNMENT_COLUMNS),
  };
}

export function pluginPlanProjectionResult(row: PluginPlanProjection): CommandResult {
  return { json: row, table: renderTable([row], PLUGIN_PLAN_PROJECTION_COLUMNS) };
}

export function pluginCatalogResult(value: A3sUseJsonObject): CommandResult {
  return { json: value, table: `${JSON.stringify(value, null, 2)}\n` };
}
