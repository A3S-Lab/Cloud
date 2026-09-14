import type {
  AutomationDefinition,
  AutomationRevision,
  AutomationWebhookEndpoint,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const ENDPOINT_COLUMNS = [
  { header: 'KEY', value: (row: AutomationWebhookEndpoint) => row.endpointKey },
  { header: 'ENDPOINT', value: (row: AutomationWebhookEndpoint) => row.endpointId },
  { header: 'STATE', value: (row: AutomationWebhookEndpoint) => row.state },
  { header: 'GENERATION', value: (row: AutomationWebhookEndpoint) => row.generation },
  { header: 'AUTOMATION', value: (row: AutomationWebhookEndpoint) => row.automationId },
  { header: 'REVISION', value: (row: AutomationWebhookEndpoint) => row.revisionId },
] as const;

const DEFINITION_COLUMNS = [
  { header: 'NAME', value: (row: AutomationDefinition) => row.name },
  { header: 'AUTOMATION', value: (row: AutomationDefinition) => row.automationId },
  { header: 'TRIGGER', value: (row: AutomationDefinition) => row.triggerKind },
  { header: 'REVISION', value: (row: AutomationDefinition) => row.revisionNumber },
  { header: 'DIGEST', value: (row: AutomationDefinition) => row.revisionDigest },
  { header: 'UPDATED AT', value: (row: AutomationDefinition) => row.updatedAt },
] as const;

const REVISION_COLUMNS = [
  { header: 'AUTOMATION', value: (row: AutomationRevision) => row.automationId },
  { header: 'NUMBER', value: (row: AutomationRevision) => row.revisionNumber },
  { header: 'REVISION', value: (row: AutomationRevision) => row.revisionId },
  { header: 'DIGEST', value: (row: AutomationRevision) => row.revisionDigest },
  { header: 'PARENT', value: (row: AutomationRevision) => row.parentRevisionId ?? '' },
] as const;

export function automationWebhookEndpointResult(row: AutomationWebhookEndpoint): CommandResult {
  return { json: row, table: renderTable([row], ENDPOINT_COLUMNS) };
}

export function automationDefinitionsResult(rows: AutomationDefinition[]): CommandResult {
  return { json: rows, table: renderTable(rows, DEFINITION_COLUMNS) };
}

export function automationDefinitionResult(row: AutomationDefinition): CommandResult {
  return { json: row, table: renderTable([row], DEFINITION_COLUMNS) };
}

export function automationRevisionResult(row: AutomationRevision): CommandResult {
  return { json: row, table: renderTable([row], REVISION_COLUMNS) };
}
