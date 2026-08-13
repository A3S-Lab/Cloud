import { expect } from 'bun:test';
import { CLOUD_API_CONTRACT_VERSION } from './api';

export const MCP_PROTOCOL_VERSION = '2026-07-28';

export const ADMIN_TOOLS = [
  'a3s_cloud_environments_create',
  'a3s_cloud_environments_list',
  'a3s_cloud_execution_templates_create',
  'a3s_cloud_execution_templates_get',
  'a3s_cloud_execution_templates_list',
  'a3s_cloud_memberships_list',
  'a3s_cloud_memberships_get',
  'a3s_cloud_memberships_create',
  'a3s_cloud_memberships_change_role',
  'a3s_cloud_memberships_revoke',
  'a3s_cloud_membership_invitations_list',
  'a3s_cloud_membership_invitations_get',
  'a3s_cloud_membership_invitations_create',
  'a3s_cloud_membership_invitations_revoke',
  'a3s_cloud_my_membership_invitations_list',
  'a3s_cloud_membership_invitations_accept',
  'a3s_cloud_resource_grants_list',
  'a3s_cloud_resource_grants_get',
  'a3s_cloud_resource_grants_create',
  'a3s_cloud_resource_grants_revoke',
  'a3s_cloud_projects_create',
  'a3s_cloud_projects_list',
  'a3s_cloud_forms_create',
  'a3s_cloud_forms_get',
  'a3s_cloud_forms_list',
  'a3s_cloud_forms_revise',
  'a3s_cloud_form_releases_get',
  'a3s_cloud_form_releases_list',
  'a3s_cloud_form_releases_publish',
  'a3s_cloud_ontologies_create',
  'a3s_cloud_ontologies_get',
  'a3s_cloud_ontologies_list',
  'a3s_cloud_ontologies_revise',
  'a3s_cloud_ontology_revisions_get',
  'a3s_cloud_ontology_revisions_list',
  'a3s_cloud_ontology_revisions_diff',
  'a3s_cloud_workflow_definitions_create',
  'a3s_cloud_workflow_definitions_get',
  'a3s_cloud_workflow_definitions_list',
  'a3s_cloud_workflow_definitions_revise',
  'a3s_cloud_workflow_revisions_get',
  'a3s_cloud_workflow_revisions_list',
  'a3s_cloud_workflow_goals_create',
  'a3s_cloud_workflow_goals_get',
  'a3s_cloud_workflow_goals_list',
  'a3s_cloud_workflow_plan_revisions_get',
  'a3s_cloud_workflow_runs_start',
  'a3s_cloud_workflow_runs_cancel',
  'a3s_cloud_workflow_runs_get',
  'a3s_cloud_workflow_runs_list',
  'a3s_cloud_workflow_runs_wait',
  'a3s_cloud_workflow_run_output_get',
  'a3s_cloud_workflow_run_history_get',
  'a3s_cloud_human_tasks_claim',
  'a3s_cloud_human_tasks_get',
  'a3s_cloud_human_tasks_list',
  'a3s_cloud_human_tasks_release',
  'a3s_cloud_human_tasks_submit',
  'a3s_cloud_search',
  'a3s_cloud_plugin_registries_list',
  'a3s_cloud_plugin_registries_get',
  'a3s_cloud_plugin_catalog_search',
  'a3s_cloud_plugin_catalog_search_cached',
  'a3s_cloud_plugin_catalog_inspect',
  'a3s_cloud_plugin_catalog_inspect_cached',
  'a3s_cloud_nodes_list',
  'a3s_cloud_nodes_get',
  'a3s_cloud_operations_list',
  'a3s_cloud_audit_records_list',
  'a3s_cloud_workloads_list',
  'a3s_cloud_workloads_get',
  'a3s_cloud_workload_logs_get',
  'a3s_cloud_workloads_stop',
  'a3s_cloud_workloads_rollback',
  'a3s_cloud_deployments_get',
  'a3s_cloud_deployments_cancel',
  'a3s_cloud_routes_list',
  'a3s_cloud_routes_get',
  'a3s_cloud_build_runs_list',
  'a3s_cloud_build_runs_get',
  'a3s_cloud_build_run_logs_get',
  'a3s_cloud_build_evidence_get',
  'a3s_cloud_build_runs_cancel',
  'a3s_cloud_build_runs_retry',
] as const;

export const READ_ONLY_TOOLS = [
  'a3s_cloud_environments_list',
  'a3s_cloud_execution_templates_get',
  'a3s_cloud_execution_templates_list',
  'a3s_cloud_my_membership_invitations_list',
  'a3s_cloud_projects_list',
  'a3s_cloud_forms_get',
  'a3s_cloud_forms_list',
  'a3s_cloud_form_releases_get',
  'a3s_cloud_form_releases_list',
  'a3s_cloud_ontologies_get',
  'a3s_cloud_ontologies_list',
  'a3s_cloud_ontology_revisions_get',
  'a3s_cloud_ontology_revisions_list',
  'a3s_cloud_ontology_revisions_diff',
  'a3s_cloud_workflow_definitions_get',
  'a3s_cloud_workflow_definitions_list',
  'a3s_cloud_workflow_revisions_get',
  'a3s_cloud_workflow_revisions_list',
  'a3s_cloud_workflow_goals_get',
  'a3s_cloud_workflow_goals_list',
  'a3s_cloud_workflow_plan_revisions_get',
  'a3s_cloud_workflow_runs_get',
  'a3s_cloud_workflow_runs_list',
  'a3s_cloud_workflow_runs_wait',
  'a3s_cloud_workflow_run_output_get',
  'a3s_cloud_workflow_run_history_get',
  'a3s_cloud_human_tasks_get',
  'a3s_cloud_human_tasks_list',
  'a3s_cloud_search',
  'a3s_cloud_plugin_registries_list',
  'a3s_cloud_plugin_registries_get',
  'a3s_cloud_plugin_catalog_search',
  'a3s_cloud_plugin_catalog_search_cached',
  'a3s_cloud_plugin_catalog_inspect',
  'a3s_cloud_plugin_catalog_inspect_cached',
  'a3s_cloud_nodes_list',
  'a3s_cloud_nodes_get',
  'a3s_cloud_operations_list',
  'a3s_cloud_audit_records_list',
  'a3s_cloud_workloads_list',
  'a3s_cloud_workloads_get',
  'a3s_cloud_workload_logs_get',
  'a3s_cloud_deployments_get',
  'a3s_cloud_routes_list',
  'a3s_cloud_routes_get',
  'a3s_cloud_build_runs_list',
  'a3s_cloud_build_runs_get',
  'a3s_cloud_build_run_logs_get',
  'a3s_cloud_build_evidence_get',
] as const;

export interface ConformanceEnvironment {
  baseUrl: string;
  bootstrapToken: string;
  adminToken: string;
  readOnlyToken: string;
  cloudRevision: string;
  evidenceFile: string;
}

interface JsonResponse {
  response: Response;
  body: JsonObject;
}

interface ToolResponse {
  result: JsonObject;
  structured: JsonObject;
}

type JsonObject = Record<string, unknown>;

export interface McpRequestOptions {
  bodyProtocolVersion?: string;
  headerProtocolVersion?: string;
  methodHeader?: string;
  nameHeader?: string | null;
  sessionIdHeader?: string;
  includeClientInfo?: boolean;
  includeClientCapabilities?: boolean;
}

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function conformanceEnvironment(): ConformanceEnvironment {
  const environment = {
    baseUrl: requiredEnvironment('A3S_CLOUD_C0_MCP_BASE_URL'),
    bootstrapToken: requiredEnvironment('A3S_CLOUD_C0_MCP_BOOTSTRAP_TOKEN'),
    adminToken: requiredEnvironment('A3S_CLOUD_C0_MCP_ADMIN_TOKEN'),
    readOnlyToken: requiredEnvironment('A3S_CLOUD_C0_MCP_READ_ONLY_TOKEN'),
    cloudRevision: requiredEnvironment('A3S_CLOUD_C0_MCP_CLOUD_REVISION'),
    evidenceFile: requiredEnvironment('A3S_CLOUD_C0_MCP_EVIDENCE_FILE'),
  };
  if (!/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}\/api\/v1$/.test(environment.baseUrl)) {
    throw new Error('A3S_CLOUD_C0_MCP_BASE_URL must be a loopback REST v1 URL');
  }
  for (const [name, token] of [
    ['A3S_CLOUD_C0_MCP_ADMIN_TOKEN', environment.adminToken],
    ['A3S_CLOUD_C0_MCP_READ_ONLY_TOKEN', environment.readOnlyToken],
  ] as const) {
    if (!/^a3s_[0-9a-f]{64}$/.test(token)) {
      throw new Error(`${name} must be a valid generated API token`);
    }
  }
  if (environment.adminToken === environment.readOnlyToken) {
    throw new Error('C0 MCP conformance credentials must be distinct');
  }
  if (!/^[0-9a-f]{40}$/.test(environment.cloudRevision)) {
    throw new Error('A3S_CLOUD_C0_MCP_CLOUD_REVISION must be an exact Git revision');
  }
  if (!environment.evidenceFile.startsWith('/')) {
    throw new Error('A3S_CLOUD_C0_MCP_EVIDENCE_FILE must be absolute');
  }
  return environment;
}

export function authenticatedHeaders(token: string, idempotencyKey: string): Record<string, string> {
  return {
    authorization: `Bearer ${token}`,
    'content-type': 'application/json',
    'idempotency-key': idempotencyKey,
  };
}

export async function restEnvelope(
  url: string,
  method: 'POST' | 'DELETE',
  headers: Record<string, string>,
  body: JsonObject | string | undefined,
  expectedStatus: number,
  credentials: readonly string[],
  label: string
): Promise<JsonResponse> {
  const response = await fetch(url, {
    method,
    headers,
    body: typeof body === 'string' ? body : body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  assertCredentialFree(text, credentials, label);
  if (response.status !== expectedStatus) {
    throw new Error(`${label} returned HTTP ${response.status}, expected ${expectedStatus}`);
  }
  const envelope = objectValue(parseJson(text, label), `${label} envelope`);
  expect(envelope.code).toBe(expectedStatus);
  const envelopeRequestId = requestId(envelope, `${label} request ID`);
  expect(response.headers.get('x-request-id')).toBe(envelopeRequestId);
  expect(response.headers.get('x-a3s-api-contract-version')).toBe(CLOUD_API_CONTRACT_VERSION);
  return { response, body: envelope };
}

export async function mcpRequest(
  environment: ConformanceEnvironment,
  token: string,
  body: JsonObject,
  expectedStatus: number,
  credentials: readonly string[],
  label: string,
  options: McpRequestOptions = {}
): Promise<JsonResponse> {
  const method = stringValue(body.method, `${label} method`);
  const params = body.params === undefined ? {} : objectValue(body.params, `${label} params`);
  const metadata: JsonObject = {
    'io.modelcontextprotocol/protocolVersion': options.bodyProtocolVersion ?? MCP_PROTOCOL_VERSION,
  };
  if (options.includeClientInfo !== false) {
    metadata['io.modelcontextprotocol/clientInfo'] = {
      name: 'a3s-cloud-c0-gate',
      version: '1.0.0',
    };
  }
  if (options.includeClientCapabilities !== false) {
    metadata['io.modelcontextprotocol/clientCapabilities'] = {};
  }
  const requestBody = {
    ...body,
    params: { ...params, _meta: metadata },
  };
  const headers: Record<string, string> = {
    accept: 'application/json, text/event-stream',
    authorization: `Bearer ${token}`,
    'content-type': 'application/json',
    'mcp-protocol-version': options.headerProtocolVersion ?? MCP_PROTOCOL_VERSION,
    'mcp-method': options.methodHeader ?? method,
  };
  const derivedName =
    method === 'tools/call' || method === 'prompts/get'
      ? params.name
      : method === 'resources/read'
        ? params.uri
        : undefined;
  const name = options.nameHeader === undefined ? derivedName : options.nameHeader;
  if (name !== null && name !== undefined) {
    headers['mcp-name'] = stringValue(name, `${label} name`);
  }
  if (options.sessionIdHeader !== undefined) {
    headers['mcp-session-id'] = options.sessionIdHeader;
  }
  const response = await fetch(`${environment.baseUrl}/mcp`, {
    method: 'POST',
    headers,
    body: JSON.stringify(requestBody),
  });
  const text = await response.text();
  assertCredentialFree(text, credentials, label);
  if (response.status !== expectedStatus) {
    throw new Error(`${label} returned HTTP ${response.status}, expected ${expectedStatus}`);
  }
  const parsed = objectValue(parseJson(text, label), `${label} response`);
  if (expectedStatus === 200) {
    expect(response.headers.get('mcp-protocol-version')).toBe(MCP_PROTOCOL_VERSION);
    expect(response.headers.get('mcp-session-id')).toBeNull();
    expect(response.headers.get('cache-control')).toBe('no-store');
  }
  return { response, body: parsed };
}

export async function listTools(
  environment: ConformanceEnvironment,
  token: string,
  id: number,
  credentials: readonly string[],
  label: string
): Promise<JsonObject> {
  const response = await mcpRequest(
    environment,
    token,
    { jsonrpc: '2.0', id, method: 'tools/list' },
    200,
    credentials,
    label
  );
  expect(response.body.jsonrpc).toBe('2.0');
  expect(response.body.id).toBe(id);
  const result = objectValue(response.body.result, `${label} result`);
  expect(result.resultType).toBe('complete');
  return result;
}

export async function callTool(
  environment: ConformanceEnvironment,
  token: string,
  id: number,
  name: string,
  arguments_: JsonObject,
  credentials: readonly string[],
  label: string
): Promise<ToolResponse> {
  const response = await mcpRequest(
    environment,
    token,
    toolCall(id, name, arguments_),
    200,
    credentials,
    label
  );
  expect(response.body.jsonrpc).toBe('2.0');
  expect(response.body.id).toBe(id);
  const result = objectValue(response.body.result, `${label} result`);
  expect(result.resultType).toBe('complete');
  const structured = objectValue(result.structuredContent, `${label} structured content`);
  const content = arrayValue(result.content, `${label} content`);
  expect(content).toHaveLength(1);
  const textContent = objectValue(content[0], `${label} text content`);
  expect(textContent.type).toBe('text');
  expect(parseJson(stringValue(textContent.text, `${label} text`), `${label} text`)).toEqual(structured);
  return { result, structured };
}

export function toolCall(id: number, name: string, arguments_: JsonObject): JsonObject {
  return {
    jsonrpc: '2.0',
    id,
    method: 'tools/call',
    params: { name, arguments: arguments_ },
  };
}

export function toolDefinitions(catalog: JsonObject): Array<{ name: string; annotations: JsonObject }> {
  return arrayValue(catalog.tools, 'tool catalog').map((value, index) => {
    const tool = objectValue(value, `tool ${index}`);
    return {
      name: stringValue(tool.name, `tool ${index} name`),
      annotations: objectValue(tool.annotations, `tool ${index} annotations`),
    };
  });
}

export function toolNames(catalog: JsonObject): string[] {
  return toolDefinitions(catalog).map((tool) => tool.name);
}

export function businessErrorContract(envelope: JsonObject): JsonObject {
  return {
    code: envelope.code,
    statusCode: envelope.statusCode,
    message: envelope.message,
    details: envelope.details,
  };
}

export function requestId(envelope: JsonObject, label: string): string {
  return uuidValue(envelope.requestId, label);
}

export function objectValue(value: unknown, label: string): JsonObject {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as JsonObject;
}

export function arrayValue(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new Error(`${label} must be an array`);
  }
  return value;
}

export function uuidValue(value: unknown, label: string): string {
  const candidate = stringValue(value, label);
  if (!UUID_PATTERN.test(candidate)) {
    throw new Error(`${label} must be a UUID`);
  }
  return candidate;
}

export function assertCredentialFree(value: string, credentials: readonly string[], label: string): void {
  if (credentials.some((credential) => value.includes(credential))) {
    throw new Error(`${label} exposed a credential`);
  }
}

function requiredEnvironment(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required for C0 management MCP conformance`);
  }
  return value;
}

function parseJson(text: string, label: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new Error(`${label} did not return valid JSON`);
  }
}

function stringValue(value: unknown, label: string): string {
  if (typeof value !== 'string' || !value) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}
