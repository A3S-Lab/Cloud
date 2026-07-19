import type {
  ApiEnvelope,
  ApiErrorEnvelope,
  Environment,
  Operation,
  Organization,
  Project,
  Deployment,
  CancelDeploymentResult,
  StopWorkloadResult,
  Workload,
  WorkloadLogStreamFilter,
} from '../types/api';

export class CloudApiError extends Error {
  readonly status: number;
  readonly statusCode: string;
  readonly requestId?: string;

  constructor(status: number, message: string, statusCode = 'HTTP_ERROR', requestId?: string) {
    super(message);
    this.name = 'CloudApiError';
    this.status = status;
    this.statusCode = statusCode;
    this.requestId = requestId;
  }
}

export class CloudApi {
  readonly token: string;
  readonly baseUrl: string;

  constructor(token: string, baseUrl = '/api/v1') {
    this.token = token;
    this.baseUrl = baseUrl.replace(/\/$/, '');
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

  listOperations(organizationId: string, signal?: AbortSignal): Promise<Operation[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/operations?limit=100`, signal);
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

  private async get<T>(path: string, signal?: AbortSignal): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: {
        Accept: 'application/json',
        Authorization: `Bearer ${this.token}`,
      },
      signal,
    });
    const payload = (await response.json()) as ApiEnvelope<T> | ApiErrorEnvelope;
    if (!response.ok) {
      const error = payload as ApiErrorEnvelope;
      throw new CloudApiError(response.status, error.message, error.statusCode, error.requestId);
    }
    return (payload as ApiEnvelope<T>).data;
  }

  private async delete<T>(path: string, idempotencyKey: string, signal?: AbortSignal): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'DELETE',
      headers: {
        Accept: 'application/json',
        Authorization: `Bearer ${this.token}`,
        'Idempotency-Key': idempotencyKey,
      },
      signal,
    });
    const payload = (await response.json()) as ApiEnvelope<T> | ApiErrorEnvelope;
    if (!response.ok) {
      const error = payload as ApiErrorEnvelope;
      throw new CloudApiError(response.status, error.message, error.statusCode, error.requestId);
    }
    return (payload as ApiEnvelope<T>).data;
  }

  private async post<T>(path: string, idempotencyKey: string, signal?: AbortSignal): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        Authorization: `Bearer ${this.token}`,
        'Idempotency-Key': idempotencyKey,
      },
      signal,
    });
    const payload = (await response.json()) as ApiEnvelope<T> | ApiErrorEnvelope;
    if (!response.ok) {
      const error = payload as ApiErrorEnvelope;
      throw new CloudApiError(response.status, error.message, error.statusCode, error.requestId);
    }
    return (payload as ApiEnvelope<T>).data;
  }
}
