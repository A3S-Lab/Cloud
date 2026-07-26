import type {
  BuildEvidence,
  BuildRun,
  BuildRunLogsPage,
  CancelBuildRunResult,
  CancelDeploymentResult,
  Deployment,
  Environment,
  GatewayCertificate,
  Node,
  Operation,
  Organization,
  Project,
  RetryBuildRunResult,
  Route,
  ServiceTemplate,
  SourceWorkloadTemplate,
  StopWorkloadResult,
  Workload,
  WorkloadDeploymentResult,
  WorkloadLogsPage,
  WorkloadLogStreamFilter,
} from './types';
import { CloudApiError } from './error';
import { readResponse } from './response';

export { CloudApiError } from './error';

export type CloudFetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export interface CloudApiClientOptions {
  fetch?: CloudFetch;
  requestTimeoutMs?: number;
}

export interface CloudLogQuery {
  cursor?: string;
  limit?: number;
  stream?: WorkloadLogStreamFilter;
}

const DEFAULT_REQUEST_TIMEOUT_MS = 30_000;
const MAX_REQUEST_TIMEOUT_MS = 300_000;
export const A3S_ACL_MEDIA_TYPE = 'application/vnd.a3s.acl';
export const MAX_WORKLOAD_ACL_BYTES = 64 * 1024;

export function isValidIdempotencyKey(value: string): boolean {
  return /^[A-Za-z0-9._~:/-]{1,255}$/.test(value);
}

export class CloudApi {
  readonly baseUrl: string;
  private readonly token: string;
  private readonly fetcher: CloudFetch;
  private readonly requestTimeoutMs: number;

  constructor(token: string, baseUrl = '/api/v1', options: CloudApiClientOptions = {}) {
    const normalizedBaseUrl = baseUrl.replace(/\/+$/, '');
    if (!normalizedBaseUrl) {
      throw new TypeError('baseUrl must not be empty');
    }
    const requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    if (
      !Number.isSafeInteger(requestTimeoutMs) ||
      requestTimeoutMs < 1 ||
      requestTimeoutMs > MAX_REQUEST_TIMEOUT_MS
    ) {
      throw new RangeError(`requestTimeoutMs must be between 1 and ${MAX_REQUEST_TIMEOUT_MS}`);
    }
    this.token = token;
    this.baseUrl = normalizedBaseUrl;
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.requestTimeoutMs = requestTimeoutMs;
  }

  listOrganizations(signal?: AbortSignal): Promise<Organization[]> {
    return this.get('/organizations', signal);
  }

  listProjects(organizationId: string, signal?: AbortSignal): Promise<Project[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/projects`, signal);
  }

  listEnvironments(organizationId: string, projectId: string, signal?: AbortSignal): Promise<Environment[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments`,
      signal
    );
  }

  listNodes(organizationId: string, signal?: AbortSignal): Promise<Node[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/nodes`, signal);
  }

  listOperations(organizationId: string, signal?: AbortSignal): Promise<Operation[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/operations?limit=100`, signal);
  }

  listBuildRuns(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<BuildRun[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/build-runs?limit=100`,
      signal
    );
  }

  getBuildRun(organizationId: string, buildRunId: string, signal?: AbortSignal): Promise<BuildRun> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}`,
      signal
    );
  }

  getBuildRunLogs(
    organizationId: string,
    buildRunId: string,
    query: CloudLogQuery = {},
    signal?: AbortSignal
  ): Promise<BuildRunLogsPage> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/build-runs/${encodeURIComponent(buildRunId)}/logs${encodeLogQuery(query)}`,
      signal
    );
  }

  getBuildEvidence(organizationId: string, buildRunId: string, signal?: AbortSignal): Promise<BuildEvidence> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}/evidence`,
      signal
    );
  }

  listWorkloads(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Workload[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments/${encodeURIComponent(environmentId)}/workloads`,
      signal
    );
  }

  getWorkload(organizationId: string, workloadId: string, signal?: AbortSignal): Promise<Workload> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}`,
      signal
    );
  }

  getDeployment(organizationId: string, deploymentId: string, signal?: AbortSignal): Promise<Deployment> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/deployments/${encodeURIComponent(deploymentId)}`,
      signal
    );
  }

  getWorkloadLogs(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    query: CloudLogQuery = {},
    signal?: AbortSignal
  ): Promise<WorkloadLogsPage> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/workloads/${encodeURIComponent(workloadId)}` +
        `/revisions/${encodeURIComponent(revisionId)}/logs${encodeLogQuery(query)}`,
      signal
    );
  }

  listRoutes(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Route[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments/${encodeURIComponent(environmentId)}/routes`,
      signal
    );
  }

  getRoute(organizationId: string, routeId: string, signal?: AbortSignal): Promise<Route> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/routes/${encodeURIComponent(routeId)}`,
      signal
    );
  }

  listGatewayCertificates(organizationId: string, signal?: AbortSignal): Promise<GatewayCertificate[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/gateway-certificates`, signal);
  }

  createWorkloadFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/workloads`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  updateWorkloadFromAcl(
    organizationId: string,
    workloadId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/deployments`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  deploySourceRevisionFromAcl(
    organizationId: string,
    projectId: string,
    environmentId: string,
    sourceRevisionId: string,
    manifest: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postAcl(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-revisions/${encodeURIComponent(sourceRevisionId)}/workloads`,
      idempotencyKey,
      manifest,
      signal
    );
  }

  updateWorkload(
    organizationId: string,
    workloadId: string,
    template: ServiceTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/deployments`,
      idempotencyKey,
      { template },
      signal
    );
  }

  deploySourceRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    sourceRevisionId: string,
    name: string,
    template: SourceWorkloadTemplate,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-revisions/${encodeURIComponent(sourceRevisionId)}/workloads`,
      idempotencyKey,
      { name, template },
      signal
    );
  }

  rollbackWorkload(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<WorkloadDeploymentResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/rollback`,
      idempotencyKey,
      { revisionId },
      signal
    );
  }

  cancelDeployment(
    organizationId: string,
    deploymentId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<CancelDeploymentResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/deployments/${encodeURIComponent(deploymentId)}`,
      idempotencyKey,
      signal
    );
  }

  cancelBuildRun(
    organizationId: string,
    buildRunId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<CancelBuildRunResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}`,
      idempotencyKey,
      signal
    );
  }

  retryBuildRun(
    organizationId: string,
    buildRunId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RetryBuildRunResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}/build-runs/${encodeURIComponent(buildRunId)}/retry`,
      idempotencyKey,
      signal
    );
  }

  stopWorkload(
    organizationId: string,
    workloadId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<StopWorkloadResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}/workloads/${encodeURIComponent(workloadId)}/stop`,
      idempotencyKey,
      signal
    );
  }

  operationStreamUrl(organizationId: string): string {
    return `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}/operations/stream`;
  }

  eventStreamHeaders(lastEventId?: string): Record<string, string> {
    return {
      Accept: 'text/event-stream',
      Authorization: `Bearer ${this.token}`,
      ...(lastEventId ? { 'Last-Event-ID': lastEventId } : {}),
    };
  }

  workloadLogStreamUrl(
    organizationId: string,
    workloadId: string,
    revisionId: string,
    stream?: WorkloadLogStreamFilter
  ): string {
    const query = new URLSearchParams({ limit: '16' });
    if (stream) {
      query.set('stream', stream);
    }
    return (
      `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}` +
      `/workloads/${encodeURIComponent(workloadId)}` +
      `/revisions/${encodeURIComponent(revisionId)}/logs/stream?${query.toString()}`
    );
  }

  buildRunLogStreamUrl(organizationId: string, buildRunId: string, stream?: WorkloadLogStreamFilter): string {
    const query = new URLSearchParams({ limit: '16' });
    if (stream) {
      query.set('stream', stream);
    }
    return (
      `${this.baseUrl}/organizations/${encodeURIComponent(organizationId)}` +
      `/build-runs/${encodeURIComponent(buildRunId)}/logs/stream?${query.toString()}`
    );
  }

  private get<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request('GET', path, { signal });
  }

  private delete<T>(path: string, idempotencyKey: string, signal?: AbortSignal): Promise<T> {
    return this.request('DELETE', path, { idempotencyKey, signal });
  }

  private post<T>(path: string, idempotencyKey: string, signal?: AbortSignal): Promise<T> {
    return this.request('POST', path, { idempotencyKey, signal });
  }

  private postJson<T>(path: string, idempotencyKey: string, body: unknown, signal?: AbortSignal): Promise<T> {
    return this.request('POST', path, {
      body: JSON.stringify(body),
      contentType: 'application/json',
      idempotencyKey,
      signal,
    });
  }

  private postAcl<T>(
    path: string,
    idempotencyKey: string,
    manifest: string,
    signal?: AbortSignal
  ): Promise<T> {
    validateWorkloadAcl(manifest);
    return this.request('POST', path, {
      body: manifest,
      contentType: A3S_ACL_MEDIA_TYPE,
      idempotencyKey,
      signal,
    });
  }

  private async request<T>(
    method: 'DELETE' | 'GET' | 'POST',
    path: string,
    options: {
      body?: string;
      contentType?: string;
      idempotencyKey?: string;
      signal?: AbortSignal;
    }
  ): Promise<T> {
    if (options.idempotencyKey !== undefined && !isValidIdempotencyKey(options.idempotencyKey)) {
      throw new TypeError('idempotency key is invalid');
    }
    if ((options.body === undefined) !== (options.contentType === undefined)) {
      throw new TypeError('request body and content type must be provided together');
    }
    const controller = new AbortController();
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, this.requestTimeoutMs);
    const abortFromCaller = () => controller.abort();
    options.signal?.addEventListener('abort', abortFromCaller, { once: true });
    if (options.signal?.aborted) {
      controller.abort();
    }

    const headers: Record<string, string> = {
      Accept: 'application/json',
      Authorization: `Bearer ${this.token}`,
    };
    if (options.idempotencyKey !== undefined) {
      headers['Idempotency-Key'] = options.idempotencyKey;
    }
    if (options.body !== undefined) {
      headers['Content-Type'] = options.contentType as string;
    }

    try {
      const response = await this.fetcher(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: options.body,
        signal: controller.signal,
      });
      return await readResponse<T>(response);
    } catch (error) {
      if (error instanceof CloudApiError) {
        throw error;
      }
      if (options.signal?.aborted) {
        throw new CloudApiError(0, 'Cloud API request was cancelled', 'REQUEST_ABORTED');
      }
      if (timedOut) {
        throw new CloudApiError(0, 'Cloud API request timed out', 'REQUEST_TIMEOUT');
      }
      throw new CloudApiError(0, 'Cloud API request failed', 'NETWORK_ERROR');
    } finally {
      clearTimeout(timeout);
      options.signal?.removeEventListener('abort', abortFromCaller);
    }
  }
}

function validateWorkloadAcl(manifest: string): void {
  const bytes = new TextEncoder().encode(manifest).byteLength;
  if (bytes < 1 || bytes > MAX_WORKLOAD_ACL_BYTES) {
    throw new RangeError(`workload ACL must contain between 1 and ${MAX_WORKLOAD_ACL_BYTES} UTF-8 bytes`);
  }
}

function encodeLogQuery(query: CloudLogQuery): string {
  const parameters = new URLSearchParams();
  if (query.cursor !== undefined) {
    if (query.cursor.length === 0 || query.cursor.length > 1_024 || hasUnsafeControl(query.cursor)) {
      throw new TypeError('log cursor is invalid');
    }
    parameters.set('cursor', query.cursor);
  }
  if (query.limit !== undefined) {
    if (!Number.isSafeInteger(query.limit) || query.limit < 1 || query.limit > 256) {
      throw new RangeError('log limit must be between 1 and 256');
    }
    parameters.set('limit', String(query.limit));
  }
  if (query.stream !== undefined) {
    if (query.stream !== 'stdout' && query.stream !== 'stderr') {
      throw new TypeError('log stream must be stdout or stderr');
    }
    parameters.set('stream', query.stream);
  }
  const encoded = parameters.toString();
  return encoded ? `?${encoded}` : '';
}

function hasUnsafeControl(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (code <= 0x20 || (code >= 0x7f && code <= 0x9f)) {
      return true;
    }
  }
  return false;
}
