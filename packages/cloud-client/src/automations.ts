export const DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT = 50;
export const MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT = 200;

export type AutomationWebhookEndpointState = 'active' | 'disabled' | 'revoked';
export type AutomationTriggerKind = 'schedule' | 'webhook' | 'plugin_event' | 'source_event';

export interface AutomationWebhookSecretReference {
  secretId: string;
  version: number;
}

export interface AutomationWebhookEndpoint {
  organizationId: string;
  projectId: string;
  environmentId: string;
  endpointId: string;
  endpointKey: string;
  automationId: string;
  revisionId: string;
  revisionDigest: string;
  signatureAlgorithm: string;
  signingSecret: AutomationWebhookSecretReference;
  requestSchemaDigest: string;
  maxBodyBytes: number;
  generation: number;
  state: AutomationWebhookEndpointState;
  createdAt: string;
  stateChangedAt: string | null;
}

export interface AutomationDefinition {
  organizationId: string;
  projectId: string;
  environmentId: string;
  automationId: string;
  name: string;
  triggerKind: AutomationTriggerKind;
  revisionId: string;
  revisionNumber: number;
  revisionDigest: string;
  definitionAcl: string;
  createdAt: string;
  updatedAt: string;
}

export interface AutomationRevision {
  organizationId: string;
  projectId: string;
  environmentId: string;
  automationId: string;
  revisionId: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDigest: string | null;
  revisionAcl: string;
  revisionDigest: string;
}

export interface CreateAutomationWebhookEndpointInput {
  endpointId: string;
  endpointKey: string;
  signingSecret: AutomationWebhookSecretReference;
  maxBodyBytes: number;
  automationId: string;
  revisionId: string;
}

export interface ChangeAutomationWebhookEndpointInput {
  expectedGeneration: number;
}

export interface AutomationDefinitionListOptions {
  limit?: number;
}

export function encodeAutomationDefinitionListOptions(
  options: AutomationDefinitionListOptions = {}
): string {
  const limit = options.limit ?? DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT;
  validateAutomationDefinitionListLimit(limit);
  return `?limit=${limit}`;
}

export function validateAutomationDefinitionListLimit(limit: unknown): asserts limit is number {
  if (
    typeof limit !== 'number' ||
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT
  ) {
    throw new RangeError(
      `Automation definition list limit must be 1 through ${MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT}`
    );
  }
}

export function validateAutomationEndpointKey(value: unknown): asserts value is string {
  if (typeof value !== 'string' || value.length < 1 || value.length > 128) {
    throw new TypeError('Automation webhook endpoint key must be 1 through 128 characters');
  }
}

export function validateAutomationMaxBodyBytes(value: unknown): asserts value is number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1) {
    throw new TypeError('Automation webhook maxBodyBytes must be a positive safe integer');
  }
}

export function validateAutomationExpectedGeneration(value: unknown): asserts value is number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new TypeError('Automation webhook expectedGeneration must be a non-negative safe integer');
  }
}

export function validateAutomationWebhookSecretReference(
  value: unknown
): asserts value is AutomationWebhookSecretReference {
  if (value === null || typeof value !== 'object') {
    throw new TypeError('Automation webhook signingSecret must be an object');
  }
  const secret = value as Partial<AutomationWebhookSecretReference>;
  if (typeof secret.secretId !== 'string' || secret.secretId.length === 0) {
    throw new TypeError('Automation webhook signingSecret.secretId is required');
  }
  if (typeof secret.version !== 'number' || !Number.isSafeInteger(secret.version) || secret.version < 1) {
    throw new TypeError('Automation webhook signingSecret.version must be a positive safe integer');
  }
}
