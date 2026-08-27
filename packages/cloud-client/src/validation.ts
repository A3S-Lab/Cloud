import type {
  CreateApiTokenInput,
  CreateMembershipInput,
  CreateMembershipInvitationInput,
  CreateResourceGrantInput,
  IdentityPrincipalKind,
  MembershipRole,
  ResourceGrantScope,
} from './identity';
import type { IssueEnrollmentTokenInput } from './node';
import type { AgentProviderKind, UpdateProjectAttributionInput } from './types';

export const MAX_SECRET_VALUE_BYTES = 1024 * 1024;
export const MAX_ACL_DOCUMENT_BYTES = 64 * 1024;
export const MAX_MCP_SERVICE_PROFILE_ACL_BYTES = MAX_ACL_DOCUMENT_BYTES;
export const MAX_MCP_ROUTE_POLICY_ACL_BYTES = 512 * 1024;
export const MAX_ONTOLOGY_ACL_BYTES = 1024 * 1024;
export const MAX_WORKFLOW_DEFINITION_ACL_BYTES = 1024 * 1024;
export const MAX_WORKFLOW_PAYLOAD_ACL_BYTES = 256 * 1024;
export const MAX_WORKFLOW_REVISION_PAYLOAD_BYTES = 8 * 1024 * 1024;
export const MAX_WORKFLOW_REVISION_PAYLOADS = 2048;
export const MAX_WORKFLOW_STEP_DESCRIPTOR_BINDINGS_ACL_BYTES = 512 * 1024;
export const MAX_WORKFLOW_STEP_DESCRIPTOR_REGISTRY_ACL_BYTES = 4 * 1024 * 1024;
export const MAX_WORKFLOW_VARIABLE_CONTRACT_ACL_BYTES = 2 * 1024 * 1024;
export const MAX_WORKFLOW_VARIABLE_DEFAULTS_ACL_BYTES = 2 * 1024 * 1024;
export const MAX_WORKFLOW_COMPOSITE_REGIONS_ACL_BYTES = 512 * 1024;
export const MAX_WORKFLOW_GOAL_ACL_BYTES = 256 * 1024;
export const MAX_FORM_DOCUMENT_BYTES = 4 * 1024 * 1024;
export const MAX_EXECUTION_TEMPLATE_ACL_BYTES = 128 * 1024;
export const MAX_WORKLOAD_ACL_BYTES = MAX_ACL_DOCUMENT_BYTES;
export const MAX_PROJECT_ATTRIBUTION_LABELS = 32;

const AGENT_PROVIDER_KINDS: ReadonlySet<AgentProviderKind> = new Set(['a3s.code', 'reference.echo']);

export function validateAgentProviderKind(kind: unknown): asserts kind is AgentProviderKind | undefined {
  if (
    kind !== undefined &&
    (typeof kind !== 'string' || !AGENT_PROVIDER_KINDS.has(kind as AgentProviderKind))
  ) {
    throw new TypeError('Agent provider kind must be a3s.code or reference.echo');
  }
}

export function validateProjectAttributionInput(input: UpdateProjectAttributionInput): void {
  validateVisibleText(input?.businessOwnerReference, 255, 'business owner reference');
  if (input.costAttributionCode !== undefined && input.costAttributionCode !== null) {
    validateVisibleText(input.costAttributionCode, 128, 'cost attribution code');
  }
  const labels = input.labels ?? {};
  if (!labels || typeof labels !== 'object' || Array.isArray(labels)) {
    throw new TypeError('project attribution labels must be an object');
  }
  const entries = Object.entries(labels);
  if (entries.length > MAX_PROJECT_ATTRIBUTION_LABELS) {
    throw new RangeError(
      `project attribution labels cannot contain more than ${MAX_PROJECT_ATTRIBUTION_LABELS} entries`
    );
  }
  for (const [key, value] of entries) {
    if (!/^[a-z][a-z0-9._-]{0,62}$/.test(key)) {
      throw new TypeError(
        'project attribution label keys must start with a lowercase letter and contain at most 63 lowercase ASCII letters, digits, dots, underscores, or hyphens'
      );
    }
    validateVisibleText(value, 255, 'project attribution label value');
  }
}

function validateVisibleText(value: unknown, maxChars: number, label: string): void {
  if (
    typeof value !== 'string' ||
    value.trim() !== value ||
    [...value].length < 1 ||
    [...value].length > maxChars ||
    /\p{Cc}/u.test(value)
  ) {
    throw new TypeError(`${label} must contain 1 to ${maxChars} visible characters`);
  }
}

export function validateApiTokenInput(input: CreateApiTokenInput): void {
  if (!/^a3s_[0-9a-f]{64}$/.test(input.token)) {
    throw new TypeError('API token must use the a3s_ prefix followed by 64 lowercase hex digits');
  }
  if (!Array.isArray(input.scopes) || input.scopes.length === 0) {
    throw new TypeError('API token must grant at least one scope');
  }
  const uniqueScopes = new Set<string>();
  for (const scope of input.scopes) {
    if (typeof scope !== 'string' || scope.length > 63 || !/^[a-z-]+:[a-z-]+$/.test(scope)) {
      throw new TypeError('API token scope must use bounded lowercase domain:action syntax');
    }
    if (uniqueScopes.has(scope)) {
      throw new TypeError('API token scopes must be unique');
    }
    uniqueScopes.add(scope);
  }
  if (input.expiresAt !== undefined && input.expiresAt !== null && !isRfc3339Timestamp(input.expiresAt)) {
    throw new TypeError('API token expiry must be an RFC 3339 timestamp');
  }
}

export function validateMembershipInput(input: CreateMembershipInput): void {
  validateIdentityPrincipalKind(input.principalKind);
  validateResourceName(input.name, 'identity principal name');
  validateMembershipRole(input.role);
}

export function validateIdentityPrincipalKind(kind: IdentityPrincipalKind): void {
  if (!['human', 'service'].includes(kind)) {
    throw new TypeError('identity principal kind must be human or service');
  }
}

export function validateMembershipRole(role: MembershipRole): void {
  if (!['owner', 'admin', 'member', 'restricted'].includes(role)) {
    throw new TypeError('membership role must be owner, admin, member, or restricted');
  }
}

export function validateExpectedMembershipVersion(value: number): void {
  validateExpectedVersion(value, 'membership');
}

export function validateMembershipInvitationInput(input: CreateMembershipInvitationInput): void {
  validateNonNilUuid(input.principalId, 'membership invitation principal ID');
  validateMembershipRole(input.role);
  if (!isRfc3339Timestamp(input.expiresAt)) {
    throw new TypeError('membership invitation expiry must be an RFC 3339 timestamp');
  }
}

export function validateExpectedMembershipInvitationVersion(value: number): void {
  validateExpectedVersion(value, 'membership invitation');
}

export function validateResourceGrantInput(input: CreateResourceGrantInput): void {
  const scope = input?.scope as ResourceGrantScope | undefined;
  if (!scope || typeof scope !== 'object') {
    throw new TypeError('Resource Grant scope is required');
  }
  switch (scope.kind) {
    case 'project':
      validateNonNilUuid(scope.projectId, 'Resource Grant project ID');
      return;
    case 'environment':
      validateNonNilUuid(scope.projectId, 'Resource Grant project ID');
      validateNonNilUuid(scope.environmentId, 'Resource Grant environment ID');
      return;
    case 'node':
      validateNonNilUuid(scope.nodeId, 'Resource Grant node ID');
      return;
    default:
      throw new TypeError('Resource Grant scope kind must be project, environment, or node');
  }
}

export function validateExpectedResourceGrantVersion(value: number): void {
  validateExpectedVersion(value, 'Resource Grant');
}

export function validateExpectedProjectVersion(value: number): void {
  validateExpectedVersion(value, 'project');
}

export function validateNonNilUuid(value: string, label: string): void {
  if (
    typeof value !== 'string' ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value) ||
    value === '00000000-0000-0000-0000-000000000000'
  ) {
    throw new TypeError(`${label} must be a non-nil UUID`);
  }
}

function validateResourceName(value: string, label: string): void {
  if (
    typeof value !== 'string' ||
    value.trim() !== value ||
    [...value].length < 1 ||
    [...value].length > 63 ||
    /[\0\r\n]/.test(value)
  ) {
    throw new TypeError(`${label} must contain 1 to 63 visible characters`);
  }
}

export function validateEnrollmentTokenInput(input: IssueEnrollmentTokenInput): void {
  if (!/^a3sn_[0-9a-f]{64}$/.test(input.token)) {
    throw new TypeError(
      'node enrollment token must use the a3sn_ prefix followed by 64 lowercase hex digits'
    );
  }
  if (
    typeof input.name !== 'string' ||
    input.name.trim() !== input.name ||
    [...input.name].length < 1 ||
    [...input.name].length > 63 ||
    /[\0\r\n]/.test(input.name)
  ) {
    throw new TypeError('node enrollment token name must contain 1 to 63 visible characters');
  }
  if (!isRfc3339Timestamp(input.expiresAt)) {
    throw new TypeError('enrollment credential expiry must be an RFC 3339 timestamp');
  }
}

export function validateExpectedNodeVersion(value: number): void {
  validateExpectedVersion(value, 'node');
}

export function validateMcpCredentialExpiry(value: string): void {
  if (!isRfc3339Timestamp(value)) {
    throw new TypeError('MCP credential expiry must be an RFC 3339 timestamp');
  }
}

export function validateExpectedMcpCredentialVersion(value: number): void {
  validateExpectedVersion(value, 'MCP credential');
}

export function validateExpectedHumanTaskVersion(value: number): void {
  validateExpectedVersion(value, 'HumanTask');
}

function validateExpectedVersion(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError(`expected ${label} version must be a positive safe integer`);
  }
}

export function validateSecretValue(value: string): void {
  const bytes = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (bytes < 1 || bytes > MAX_SECRET_VALUE_BYTES) {
    throw new RangeError('Secret value must contain between 1 byte and 1 MiB');
  }
}

export function validateWorkloadAcl(manifest: string): void {
  validateAclBytes(manifest, MAX_WORKLOAD_ACL_BYTES, 'workload ACL');
}

export function validateMcpServiceProfileAcl(acl: string): void {
  validateAclBytes(acl, MAX_MCP_SERVICE_PROFILE_ACL_BYTES, 'MCP Service profile ACL');
}

export function validateMcpRoutePolicyAcl(acl: string): void {
  validateAclBytes(acl, MAX_MCP_ROUTE_POLICY_ACL_BYTES, 'MCP route policy ACL');
}

export function validateOntologyAcl(acl: string): void {
  validateAclBytes(acl, MAX_ONTOLOGY_ACL_BYTES, 'Ontology ACL');
}

export function validateOntologyRevisionControl(expectedVersion: number, migrationRuleId?: string): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected Ontology version must be a positive safe integer');
  }
  if (migrationRuleId !== undefined && !/^[A-Za-z0-9_-]{1,96}$/.test(migrationRuleId)) {
    throw new TypeError('Ontology migration rule must be a portable rule ID');
  }
}

export function validateWorkflowDefinitionPublication(input: {
  definitionAcl: string;
  payloads: ReadonlyArray<{ kind: string; acl: string }>;
  semanticContracts?: {
    descriptorBindingsAcl: string;
    descriptorRegistryAcl: string;
    variableContractAcl: string;
    variableDefaultsAcl?: string;
    compositeRegionsAcl?: string;
  };
}): void {
  validateAclBytes(input.definitionAcl, MAX_WORKFLOW_DEFINITION_ACL_BYTES, 'Workflow definition ACL');
  if (
    !Array.isArray(input.payloads) ||
    input.payloads.length < 1 ||
    input.payloads.length > MAX_WORKFLOW_REVISION_PAYLOADS
  ) {
    throw new RangeError(
      `Workflow revision must contain between 1 and ${MAX_WORKFLOW_REVISION_PAYLOADS} payloads`
    );
  }
  let totalBytes = 0;
  for (const payload of input.payloads) {
    if (!['configuration', 'data_schema', 'policy'].includes(payload.kind)) {
      throw new TypeError('Workflow payload kind must be configuration, data_schema, or policy');
    }
    validateAclBytes(payload.acl, MAX_WORKFLOW_PAYLOAD_ACL_BYTES, 'Workflow payload ACL');
    totalBytes += new TextEncoder().encode(payload.acl).byteLength;
  }
  if (totalBytes > MAX_WORKFLOW_REVISION_PAYLOAD_BYTES) {
    throw new RangeError(
      `Workflow revision payloads must contain at most ${MAX_WORKFLOW_REVISION_PAYLOAD_BYTES} UTF-8 bytes`
    );
  }
  if (input.semanticContracts) {
    validateAclBytes(
      input.semanticContracts.descriptorBindingsAcl,
      MAX_WORKFLOW_STEP_DESCRIPTOR_BINDINGS_ACL_BYTES,
      'Workflow descriptor bindings ACL'
    );
    validateAclBytes(
      input.semanticContracts.descriptorRegistryAcl,
      MAX_WORKFLOW_STEP_DESCRIPTOR_REGISTRY_ACL_BYTES,
      'Workflow descriptor registry ACL'
    );
    validateAclBytes(
      input.semanticContracts.variableContractAcl,
      MAX_WORKFLOW_VARIABLE_CONTRACT_ACL_BYTES,
      'Workflow variable contract ACL'
    );
    if (input.semanticContracts.variableDefaultsAcl !== undefined) {
      validateAclBytes(
        input.semanticContracts.variableDefaultsAcl,
        MAX_WORKFLOW_VARIABLE_DEFAULTS_ACL_BYTES,
        'Workflow variable defaults ACL'
      );
    }
    if (input.semanticContracts.compositeRegionsAcl !== undefined) {
      validateAclBytes(
        input.semanticContracts.compositeRegionsAcl,
        MAX_WORKFLOW_COMPOSITE_REGIONS_ACL_BYTES,
        'Workflow composite regions ACL'
      );
    }
  }
}

export function validateWorkflowRevisionControl(expectedVersion: number): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected WorkflowDefinition version must be a positive safe integer');
  }
}

export function validateWorkflowGoalAcl(acl: string): void {
  validateAclBytes(acl, MAX_WORKFLOW_GOAL_ACL_BYTES, 'Workflow goal ACL');
}

export function validateExecutionTemplateAcl(acl: string): void {
  validateAclBytes(acl, MAX_EXECUTION_TEMPLATE_ACL_BYTES, 'ExecutionTemplate ACL');
}

export function validateFormDraftInput(input: {
  name: string;
  description?: string;
  document: unknown;
}): void {
  validateFormText(input.name, 'Form name', 1, 120);
  validateFormText(input.description ?? '', 'Form description', 0, 4_096);
  if (typeof input.document !== 'object' || input.document === null || Array.isArray(input.document)) {
    throw new TypeError('Form document must be a JSON object');
  }
  let encoded: string | undefined;
  try {
    encoded = JSON.stringify(input.document);
  } catch {
    throw new TypeError('Form document must be JSON serializable');
  }
  if (encoded === undefined) {
    throw new TypeError('Form document must serialize to a JSON object');
  }
  const transported = JSON.parse(encoded) as unknown;
  if (typeof transported !== 'object' || transported === null || Array.isArray(transported)) {
    throw new TypeError('Form document must serialize to a JSON object');
  }
  if (new TextEncoder().encode(encoded).byteLength > MAX_FORM_DOCUMENT_BYTES) {
    throw new RangeError(`Form document must contain at most ${MAX_FORM_DOCUMENT_BYTES} UTF-8 bytes`);
  }
}

export function validateFormVersionControl(expectedVersion: number): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected Form draft version must be a positive safe integer');
  }
}

function validateFormText(
  value: string,
  label: string,
  minimumTrimmedCharacters: number,
  maximumCharacters: number
): void {
  if (
    typeof value !== 'string' ||
    [...value.trim()].length < minimumTrimmedCharacters ||
    [...value].length > maximumCharacters ||
    value.includes('\0')
  ) {
    throw new TypeError(
      `${label} must contain between ${minimumTrimmedCharacters} and ${maximumCharacters} characters`
    );
  }
}

function validateAclBytes(value: string, maximumBytes: number, label: string): void {
  const bytes = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (bytes < 1 || bytes > maximumBytes) {
    throw new RangeError(`${label} must contain between 1 and ${maximumBytes} UTF-8 bytes`);
  }
}

export function isRfc3339Timestamp(value: string): boolean {
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|[+-](\d{2}):(\d{2}))$/.exec(value);
  if (!match || !Number.isFinite(Date.parse(value))) {
    return false;
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHour = match[7] === undefined ? 0 : Number(match[7]);
  const offsetMinute = match[8] === undefined ? 0 : Number(match[8]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return (
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= (days[month - 1] ?? 0) &&
    hour <= 23 &&
    minute <= 59 &&
    second <= 59 &&
    offsetHour <= 23 &&
    offsetMinute <= 59
  );
}
