import type {
  ApiToken,
  ApiTokenMutationResult,
  BuildEvidence,
  BuildRun,
  BuildRunLogsPage,
  CancelBuildRunResult,
  CancelDeploymentResult,
  CreateGithubRepositorySubscriptionInput,
  CreateApiTokenInput,
  CreateGatewayScopeInput,
  Deployment,
  DomainClaim,
  DomainClaimMutationResult,
  Environment,
  EnvironmentMutationResult,
  GatewayCertificate,
  GatewayScope,
  GatewayScopeMutationResult,
  GithubConnection,
  GithubConnectionInstall,
  GithubRepositorySubscription,
  GithubRepositorySubscriptionMutationResult,
  Node,
  Operation,
  Organization,
  OrganizationMutationResult,
  Project,
  ProjectMutationResult,
  PublishRouteInput,
  ResolveSourceRevisionInput,
  RetryBuildRunResult,
  Route,
  RoutePublicationResult,
  Secret,
  SecretDetails,
  SecretMutationResult,
  ServiceTemplate,
  SourceWorkloadTemplate,
  StopWorkloadResult,
  SourceRevision,
  SourceRevisionMutationResult,
  Workload,
  WorkloadDeploymentResult,
  WorkloadLogsPage,
  WorkloadLogStreamFilter,
} from './types';
import type { CloudDiagnostics, CloudHealthReport, CloudPlatformInfo } from './diagnostics';
import { CloudApiError } from './error';
import { readHealthResponse, readResponse } from './response';

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
export const MAX_SECRET_VALUE_BYTES = 1024 * 1024;

export function isValidIdempotencyKey(value: string): boolean {
  return /^[A-Za-z0-9._~:/-]{1,255}$/.test(value);
}

function validateApiTokenInput(input: CreateApiTokenInput): void {
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
  if (input.expiresAt !== undefined && input.expiresAt !== null) {
    if (!isRfc3339Timestamp(input.expiresAt)) {
      throw new TypeError('API token expiry must be an RFC 3339 timestamp');
    }
  }
}

function isRfc3339Timestamp(value: string): boolean {
  return (
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(value) &&
    Number.isFinite(Date.parse(value))
  );
}

export class CloudApi {
  readonly baseUrl: string;
  private readonly token: string | undefined;
  private readonly fetcher: CloudFetch;
  private readonly requestTimeoutMs: number;

  constructor(token: string | undefined, baseUrl = '/api/v1', options: CloudApiClientOptions = {}) {
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

  getPlatform(signal?: AbortSignal): Promise<CloudPlatformInfo> {
    return this.get('/platform', signal);
  }

  getLiveness(signal?: AbortSignal): Promise<CloudHealthReport> {
    return this.getHealth('/health/live', signal);
  }

  getReadiness(signal?: AbortSignal): Promise<CloudHealthReport> {
    return this.getHealth('/health/ready', signal);
  }

  async getDiagnostics(signal?: AbortSignal): Promise<CloudDiagnostics> {
    const [platform, liveness, readiness] = await Promise.all([
      this.getPlatform(signal),
      this.getLiveness(signal),
      this.getReadiness(signal),
    ]);
    return { platform, liveness, readiness };
  }

  listOrganizations(signal?: AbortSignal): Promise<Organization[]> {
    return this.get('/organizations', signal);
  }

  createOrganization(
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<OrganizationMutationResult> {
    return this.postJson('/organizations', idempotencyKey, { name }, signal);
  }

  listApiTokens(organizationId: string, signal?: AbortSignal): Promise<ApiToken[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/api-tokens`, signal);
  }

  getApiToken(organizationId: string, tokenId: string, signal?: AbortSignal): Promise<ApiToken> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens/${encodeURIComponent(tokenId)}`,
      signal
    );
  }

  createApiToken(
    organizationId: string,
    input: CreateApiTokenInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApiTokenMutationResult> {
    validateApiTokenInput(input);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens`,
      idempotencyKey,
      input,
      signal
    );
  }

  revokeApiToken(
    organizationId: string,
    tokenId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ApiTokenMutationResult> {
    return this.delete(
      `/organizations/${encodeURIComponent(organizationId)}/api-tokens/${encodeURIComponent(tokenId)}`,
      idempotencyKey,
      signal
    );
  }

  listProjects(organizationId: string, signal?: AbortSignal): Promise<Project[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/projects`, signal);
  }

  createProject(
    organizationId: string,
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<ProjectMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}/projects`,
      idempotencyKey,
      { name },
      signal
    );
  }

  listEnvironments(organizationId: string, projectId: string, signal?: AbortSignal): Promise<Environment[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/projects/${encodeURIComponent(projectId)}/environments`,
      signal
    );
  }

  createEnvironment(
    organizationId: string,
    projectId: string,
    name: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<EnvironmentMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}/environments`,
      idempotencyKey,
      { name },
      signal
    );
  }

  listNodes(organizationId: string, signal?: AbortSignal): Promise<Node[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/nodes`, signal);
  }

  markNodeReady(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'ready', expectedVersion, idempotencyKey, signal);
  }

  drainNode(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'drain', expectedVersion, idempotencyKey, signal);
  }

  revokeNode(
    organizationId: string,
    nodeId: string,
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    return this.changeNodeState(organizationId, nodeId, 'revoke', expectedVersion, idempotencyKey, signal);
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

  listDomainClaims(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<DomainClaim[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/domain-claims`,
      signal
    );
  }

  getDomainClaim(organizationId: string, domainClaimId: string, signal?: AbortSignal): Promise<DomainClaim> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}`,
      signal
    );
  }

  createDomainClaim(
    organizationId: string,
    projectId: string,
    environmentId: string,
    pattern: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/domain-claims`,
      idempotencyKey,
      { pattern },
      signal
    );
  }

  verifyDomainClaim(
    organizationId: string,
    domainClaimId: string,
    proof: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}/verify`,
      idempotencyKey,
      { proof },
      signal
    );
  }

  revokeDomainClaim(
    organizationId: string,
    domainClaimId: string,
    reason: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<DomainClaimMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/domain-claims/${encodeURIComponent(domainClaimId)}/revoke`,
      idempotencyKey,
      { reason },
      signal
    );
  }

  listGatewayScopes(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<GatewayScope[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/gateway-scopes`,
      signal
    );
  }

  createGatewayScope(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateGatewayScopeInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GatewayScopeMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/gateway-scopes`,
      idempotencyKey,
      input,
      signal
    );
  }

  publishRoute(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: PublishRouteInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<RoutePublicationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/routes`,
      idempotencyKey,
      input,
      signal
    );
  }

  listGatewayCertificates(organizationId: string, signal?: AbortSignal): Promise<GatewayCertificate[]> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/gateway-certificates`, signal);
  }

  listSecrets(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<Secret[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/secrets`,
      signal
    );
  }

  getSecret(organizationId: string, secretId: string, signal?: AbortSignal): Promise<SecretDetails> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}/secrets/${encodeURIComponent(secretId)}`,
      signal
    );
  }

  createSecret(
    organizationId: string,
    projectId: string,
    environmentId: string,
    name: string,
    value: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    validateSecretValue(value);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/secrets`,
      idempotencyKey,
      { name, value },
      signal
    );
  }

  addSecretVersion(
    organizationId: string,
    secretId: string,
    value: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    validateSecretValue(value);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/secrets/${encodeURIComponent(secretId)}/versions`,
      idempotencyKey,
      { value },
      signal
    );
  }

  revokeSecretVersion(
    organizationId: string,
    secretId: string,
    version: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SecretMutationResult> {
    if (!Number.isSafeInteger(version) || version < 1) {
      throw new RangeError('Secret version must be a positive safe integer');
    }
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/secrets/${encodeURIComponent(secretId)}` +
        `/versions/${version}/revoke`,
      idempotencyKey,
      signal
    );
  }

  listSourceRevisions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<SourceRevision[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-revisions`,
      signal
    );
  }

  resolveSourceRevision(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: ResolveSourceRevisionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<SourceRevisionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-revisions`,
      idempotencyKey,
      input,
      signal
    );
  }

  getGithubConnection(organizationId: string, signal?: AbortSignal): Promise<GithubConnection> {
    return this.get(`/organizations/${encodeURIComponent(organizationId)}/source-connections/github`, signal);
  }

  beginGithubConnection(organizationId: string, signal?: AbortSignal): Promise<GithubConnectionInstall> {
    return this.request(
      'POST',
      `/organizations/${encodeURIComponent(organizationId)}/source-connections/github`,
      { signal }
    );
  }

  listGithubRepositorySubscriptions(
    organizationId: string,
    projectId: string,
    environmentId: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscription[]> {
    return this.get(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-subscriptions/github`,
      signal
    );
  }

  createGithubRepositorySubscription(
    organizationId: string,
    projectId: string,
    environmentId: string,
    input: CreateGithubRepositorySubscriptionInput,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscriptionMutationResult> {
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}/source-subscriptions/github`,
      idempotencyKey,
      input,
      signal
    );
  }

  deactivateGithubRepositorySubscription(
    organizationId: string,
    projectId: string,
    environmentId: string,
    subscriptionId: string,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<GithubRepositorySubscriptionMutationResult> {
    return this.post(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/projects/${encodeURIComponent(projectId)}` +
        `/environments/${encodeURIComponent(environmentId)}` +
        `/source-subscriptions/github/${encodeURIComponent(subscriptionId)}/deactivate`,
      idempotencyKey,
      signal
    );
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
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
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

  private getHealth<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request('GET', path, { healthResponse: true, signal });
  }

  private changeNodeState(
    organizationId: string,
    nodeId: string,
    action: 'ready' | 'drain' | 'revoke',
    expectedVersion: number,
    idempotencyKey: string,
    signal?: AbortSignal
  ): Promise<Node> {
    validateExpectedNodeVersion(expectedVersion);
    return this.postJson(
      `/organizations/${encodeURIComponent(organizationId)}` +
        `/nodes/${encodeURIComponent(nodeId)}/actions/${action}`,
      idempotencyKey,
      { expectedVersion },
      signal
    );
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
      healthResponse?: boolean;
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
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
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
      return options.healthResponse ? await readHealthResponse<T>(response) : await readResponse<T>(response);
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

function validateSecretValue(value: string): void {
  const bytes = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (bytes < 1 || bytes > MAX_SECRET_VALUE_BYTES) {
    throw new RangeError('Secret value must contain between 1 byte and 1 MiB');
  }
}

function validateExpectedNodeVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError('expected node version must be a positive safe integer');
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
