import type { ParsedArguments } from './arguments';
import { usageError } from './errors';

export type OutputFormat = 'json' | 'table';
export type ProcessEnvironment = Readonly<Record<string, string | undefined>>;

export interface CloudContext {
  baseUrl: string;
  token?: string;
  organizationId?: string;
  projectId?: string;
  environmentId?: string;
  output: OutputFormat;
  timeoutMs: number;
}

export interface PublicCloudContext {
  url: string;
  organizationId: string | null;
  projectId: string | null;
  environmentId: string | null;
  output: OutputFormat;
  timeoutMs: number;
  tokenConfigured: boolean;
}

const DEFAULT_URL = 'http://127.0.0.1:8080/api/v1';
const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_TIMEOUT_MS = 300_000;
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function resolveContext(arguments_: ParsedArguments, environment: ProcessEnvironment): CloudContext {
  const organizationId = optionalUuid(
    arguments_.organizationId ?? environment.A3S_CLOUD_ORGANIZATION_ID,
    'organization ID'
  );
  const projectId = optionalUuid(arguments_.projectId ?? environment.A3S_CLOUD_PROJECT_ID, 'project ID');
  const environmentId = optionalUuid(
    arguments_.environmentId ?? environment.A3S_CLOUD_ENVIRONMENT_ID,
    'environment ID'
  );
  if (projectId && !organizationId) {
    throw usageError('project context requires an organization ID');
  }
  if (environmentId && !projectId) {
    throw usageError('environment context requires a project ID');
  }
  const baseUrl = normalizeApiUrl(arguments_.url ?? environment.A3S_CLOUD_URL ?? DEFAULT_URL);
  const token = optionalToken(environment.A3S_CLOUD_TOKEN);
  if (token && baseUrl.includes(token)) {
    throw usageError('Cloud API URL cannot contain the configured token');
  }

  return {
    baseUrl,
    token,
    organizationId,
    projectId,
    environmentId,
    output: parseOutput(arguments_.output ?? environment.A3S_CLOUD_OUTPUT ?? 'table'),
    timeoutMs: parseTimeout(arguments_.timeoutMs ?? environment.A3S_CLOUD_TIMEOUT_MS),
  };
}

export function publicContext(context: CloudContext): PublicCloudContext {
  return {
    url: context.baseUrl,
    organizationId: context.organizationId ?? null,
    projectId: context.projectId ?? null,
    environmentId: context.environmentId ?? null,
    output: context.output,
    timeoutMs: context.timeoutMs,
    tokenConfigured: context.token !== undefined,
  };
}

export function requireToken(context: CloudContext): string {
  if (!context.token) {
    throw usageError('A3S_CLOUD_TOKEN is required for API commands');
  }
  return context.token;
}

export function requireOrganization(context: CloudContext): string {
  if (!context.organizationId) {
    throw usageError('an organization ID is required through --organization or A3S_CLOUD_ORGANIZATION_ID');
  }
  return context.organizationId;
}

export function requireProject(context: CloudContext): string {
  if (!context.projectId) {
    throw usageError('a project ID is required through --project or A3S_CLOUD_PROJECT_ID');
  }
  return context.projectId;
}

export function normalizeApiUrl(value: string): string {
  if (!value || value.length > 2_048 || hasUnsafeControl(value)) {
    throw usageError('Cloud API URL is invalid');
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw usageError('Cloud API URL must be absolute');
  }
  if (url.username || url.password || url.search || url.hash) {
    throw usageError('Cloud API URL cannot contain credentials, a query, or a fragment');
  }
  const hostname = url.hostname.startsWith('[') ? url.hostname.slice(1, -1) : url.hostname;
  const loopback = hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '::1';
  const literalHttpLoopback = /^http:\/\/(?:localhost|127\.0\.0\.1|\[::1\])(?::[0-9]+)?(?:\/|$)/i.test(value);
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && loopback && literalHttpLoopback)) {
    throw usageError('Cloud API URL must use HTTPS except for literal localhost or loopback addresses');
  }
  const path = url.pathname.replace(/\/+$/, '');
  if (!path.endsWith('/api/v1')) {
    throw usageError('Cloud API URL path must end with /api/v1');
  }
  url.pathname = path;
  return url.toString();
}

function optionalUuid(value: string | undefined, label: string): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!UUID_PATTERN.test(value)) {
    throw usageError(`${label} must be a UUID`);
  }
  return value.toLowerCase();
}

function optionalToken(value: string | undefined): string | undefined {
  if (value === undefined || value === '') {
    return undefined;
  }
  if (value.length > 8_192 || hasUnsafeControl(value) || !/^[-A-Za-z0-9._~+/]+=*$/.test(value)) {
    throw usageError('A3S_CLOUD_TOKEN is invalid');
  }
  return value;
}

function parseOutput(value: string): OutputFormat {
  if (value !== 'json' && value !== 'table') {
    throw usageError('output must be table or json');
  }
  return value;
}

function parseTimeout(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_TIMEOUT_MS;
  }
  if (!/^[0-9]+$/.test(value)) {
    throw usageError('request timeout must be an integer number of milliseconds');
  }
  const timeout = Number(value);
  if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > MAX_TIMEOUT_MS) {
    throw usageError(`request timeout must be between 1 and ${MAX_TIMEOUT_MS} milliseconds`);
  }
  return timeout;
}

function hasUnsafeControl(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (code <= 0x20 || code === 0x7f) {
      return true;
    }
  }
  return false;
}
