import { expect, it } from 'bun:test';
import { CLOUD_API_CONTRACT_VERSION, CloudApi, CloudApiError } from './api';

const conformanceIt = process.env.A3S_CLOUD_C0_CONFORMANCE === '1' ? it : it.skip;
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

interface ConformanceEnvironment {
  baseUrl: string;
  bootstrapToken: string;
  adminToken: string;
  restrictedToken: string;
  cloudRevision: string;
  cliBinary: string;
  evidenceFile: string;
}

interface CliContext {
  token: string;
  organizationId: string;
  projectId?: string;
}

interface CliResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

type JsonObject = Record<string, unknown>;

conformanceIt(
  'proves REST, the maintained client, and the compiled CLI against one real Cloud API',
  async () => {
    const environment = conformanceEnvironment();
    const credentials = [environment.bootstrapToken, environment.adminToken, environment.restrictedToken];
    const publicApi = new CloudApi(undefined, environment.baseUrl);
    const diagnostics = await publicApi.getDiagnostics();
    expect(diagnostics.liveness.status).toBe('up');
    expect(diagnostics.readiness.status).toBe('up');

    const bootstrap = await postEnvelope(
      `${environment.baseUrl}/bootstrap`,
      {
        'content-type': 'application/json',
        'idempotency-key': 'c0:bootstrap:primary',
        'x-a3s-bootstrap-token': environment.bootstrapToken,
      },
      {
        organizationName: 'C0 Primary Tenant',
        tokenName: 'c0-admin',
        token: environment.adminToken,
        expiresAt: null,
      },
      201,
      credentials,
      'REST bootstrap'
    );
    const bootstrapData = objectValue(bootstrap.body.data, 'REST bootstrap data');
    const organization = objectValue(bootstrapData.organization, 'REST bootstrap organization');
    const organizationId = uuidValue(organization.id, 'REST bootstrap organization ID');
    expect(bootstrap.response.headers.get('x-a3s-api-contract-version')).toBe(CLOUD_API_CONTRACT_VERSION);

    const client = new CloudApi(environment.adminToken, environment.baseUrl);
    const organizations = await client.listOrganizations();
    expect(organizations.map((candidate) => candidate.id)).toContain(organizationId);

    const projectKey = 'c0:client-cli:project';
    const project = await client.createProject(organizationId, 'Cross Surface Project', projectKey);
    expect(project.replayed).toBe(false);
    expect(project.organizationId).toBe(organizationId);

    const projectReplay = await runCli(
      environment,
      credentials,
      ['projects', 'create', 'Cross Surface Project', `--idempotency-key=${projectKey}`],
      { token: environment.adminToken, organizationId }
    );
    expect(projectReplay.exitCode).toBe(0);
    expect(projectReplay.stderr).toBe('');
    const replayedProject = objectValue(parseJson(projectReplay.stdout, 'CLI project replay'), 'CLI project');
    expect(replayedProject.id).toBe(project.id);
    expect(replayedProject.replayed).toBe(true);

    const conflict = await runCli(
      environment,
      credentials,
      ['projects', 'create', 'Changed Project', `--idempotency-key=${projectKey}`],
      { token: environment.adminToken, organizationId }
    );
    expect(conflict.exitCode).toBe(5);
    expect(conflict.stdout).toBe('');
    const conflictError = cliError(conflict.stderr, 'CLI idempotency conflict');
    expect(conflictError.status).toBe(409);
    expect(conflictError.statusCode).toBe('CONFLICT');

    const environmentKey = 'c0:rest-cli:environment';
    const createdEnvironment = await postEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/projects/${project.id}/environments`,
      {
        authorization: `Bearer ${environment.adminToken}`,
        'content-type': 'application/json',
        'idempotency-key': environmentKey,
      },
      { name: 'Cross Surface Environment' },
      201,
      credentials,
      'REST environment creation'
    );
    const environmentData = objectValue(createdEnvironment.body.data, 'REST environment data');
    const environmentId = uuidValue(environmentData.id, 'REST environment ID');
    expect(environmentData.replayed).toBe(false);

    const environmentReplay = await runCli(
      environment,
      credentials,
      ['environments', 'create', 'Cross Surface Environment', `--idempotency-key=${environmentKey}`],
      { token: environment.adminToken, organizationId, projectId: project.id }
    );
    expect(environmentReplay.exitCode).toBe(0);
    expect(environmentReplay.stderr).toBe('');
    const replayedEnvironment = objectValue(
      parseJson(environmentReplay.stdout, 'CLI environment replay'),
      'CLI environment'
    );
    expect(replayedEnvironment.id).toBe(environmentId);
    expect(replayedEnvironment.replayed).toBe(true);

    const clientEnvironments = await client.listEnvironments(organizationId, project.id);
    expect(clientEnvironments.map((candidate) => candidate.id)).toEqual([environmentId]);
    const clientSearch = await client.searchResources(organizationId, 'Cross Surface', 20);
    const clientSearchIds = new Set(clientSearch.map((result) => result.id));
    expect(clientSearchIds.has(project.id)).toBe(true);
    expect(clientSearchIds.has(environmentId)).toBe(true);

    const cliSearch = await runCli(
      environment,
      credentials,
      ['search', 'resources', 'Cross Surface', '--limit=20'],
      { token: environment.adminToken, organizationId }
    );
    expect(cliSearch.exitCode).toBe(0);
    const cliSearchRows = arrayValue(parseJson(cliSearch.stdout, 'CLI search'), 'CLI search results');
    const cliSearchIds = new Set(
      cliSearchRows.map((row, index) =>
        stringValue(objectValue(row, `CLI search row ${index}`).id, 'CLI search ID')
      )
    );
    expect(cliSearchIds.has(project.id)).toBe(true);
    expect(cliSearchIds.has(environmentId)).toBe(true);

    const isolatedOrganization = await client.createOrganization(
      'C0 Isolated Tenant',
      'c0:isolation:organization'
    );
    const isolationSentinel = await client.createProject(
      isolatedOrganization.id,
      'Isolation Sentinel',
      'c0:isolation:sentinel'
    );
    const restricted = await client.createApiToken(
      organizationId,
      {
        name: 'c0-restricted',
        token: environment.restrictedToken,
        scopes: ['project:write'],
        expiresAt: null,
      },
      'c0:isolation:token'
    );
    assertCredentialFree(JSON.stringify(restricted), credentials, 'client API-token projection');

    const restrictedClient = new CloudApi(environment.restrictedToken, environment.baseUrl);
    expect((await restrictedClient.listProjects(organizationId)).map((candidate) => candidate.id)).toContain(
      project.id
    );
    const clientDenial = await capturedError(() => restrictedClient.listProjects(isolatedOrganization.id));
    expect(clientDenial).toBeInstanceOf(CloudApiError);
    expect((clientDenial as CloudApiError).status).toBe(403);
    expect((clientDenial as CloudApiError).statusCode).toBe('FORBIDDEN');

    const cliDenial = await runCli(environment, credentials, ['projects', 'list'], {
      token: environment.restrictedToken,
      organizationId: isolatedOrganization.id,
    });
    expect(cliDenial.exitCode).toBe(3);
    expect(cliDenial.stdout).toBe('');
    const deniedError = cliError(cliDenial.stderr, 'CLI tenant denial');
    expect(deniedError.status).toBe(403);
    expect(deniedError.statusCode).toBe('FORBIDDEN');
    expect(cliDenial.stderr).not.toContain(isolationSentinel.name);

    const revoked = await client.revokeApiToken(organizationId, restricted.id, 'c0:isolation:token-revoke');
    expect(revoked.replayed).toBe(false);
    const revokedCli = await runCli(environment, credentials, ['projects', 'list'], {
      token: environment.restrictedToken,
      organizationId,
    });
    expect(revokedCli.exitCode).toBe(3);
    const revokedError = cliError(revokedCli.stderr, 'CLI revoked token');
    expect(revokedError.status).toBe(401);
    expect(revokedError.statusCode).toBe('UNAUTHORIZED');

    const evidence = {
      schema: 'a3s.cloud.c0-cross-surface.evidence.v1',
      cloudRevision: environment.cloudRevision,
      apiContractVersion: CLOUD_API_CONTRACT_VERSION,
      persistence: 'postgresql-17-through-a3s-orm',
      surfaces: ['rest', 'typescript-client', 'compiled-cli'],
      resources: {
        organizationId,
        projectId: project.id,
        environmentId,
        isolatedOrganizationId: isolatedOrganization.id,
        restrictedTokenId: restricted.id,
      },
      requestIds: {
        bootstrap: bootstrap.body.requestId,
        environmentCreate: createdEnvironment.body.requestId,
        idempotencyConflict: stringValue(conflictError.requestId, 'conflict request ID'),
        tenantDenial: stringValue(deniedError.requestId, 'denial request ID'),
        revokedToken: stringValue(revokedError.requestId, 'revoked-token request ID'),
      },
      checks: [
        'public-diagnostics',
        'rest-bootstrap',
        'client-to-cli-idempotency-replay',
        'rest-to-cli-idempotency-replay',
        'stable-conflict-error',
        'shared-authorized-search',
        'cross-tenant-denial',
        'immediate-token-revocation',
        'credential-free-projections',
      ],
    };
    const renderedEvidence = `${JSON.stringify(evidence, null, 2)}\n`;
    assertCredentialFree(renderedEvidence, credentials, 'cross-surface evidence');
    await Bun.write(environment.evidenceFile, renderedEvidence);
  },
  60_000
);

function conformanceEnvironment(): ConformanceEnvironment {
  const environment = {
    baseUrl: requiredEnvironment('A3S_CLOUD_C0_BASE_URL'),
    bootstrapToken: requiredEnvironment('A3S_CLOUD_C0_BOOTSTRAP_TOKEN'),
    adminToken: requiredEnvironment('A3S_CLOUD_C0_ADMIN_TOKEN'),
    restrictedToken: requiredEnvironment('A3S_CLOUD_C0_RESTRICTED_TOKEN'),
    cloudRevision: requiredEnvironment('A3S_CLOUD_C0_CLOUD_REVISION'),
    cliBinary: requiredEnvironment('A3S_CLOUD_C0_CLI_BIN'),
    evidenceFile: requiredEnvironment('A3S_CLOUD_C0_EVIDENCE_FILE'),
  };
  if (!/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}\/api\/v1$/.test(environment.baseUrl)) {
    throw new Error('A3S_CLOUD_C0_BASE_URL must be a loopback REST v1 URL');
  }
  for (const [name, token] of [
    ['A3S_CLOUD_C0_ADMIN_TOKEN', environment.adminToken],
    ['A3S_CLOUD_C0_RESTRICTED_TOKEN', environment.restrictedToken],
  ] as const) {
    if (!/^a3s_[0-9a-f]{64}$/.test(token)) {
      throw new Error(`${name} must be a valid generated API token`);
    }
  }
  if (environment.adminToken === environment.restrictedToken) {
    throw new Error('C0 conformance credentials must be distinct');
  }
  if (!/^[0-9a-f]{40}$/.test(environment.cloudRevision)) {
    throw new Error('A3S_CLOUD_C0_CLOUD_REVISION must be an exact Git revision');
  }
  if (!environment.cliBinary.startsWith('/') || !environment.evidenceFile.startsWith('/')) {
    throw new Error('C0 conformance CLI and evidence paths must be absolute');
  }
  return environment;
}

function requiredEnvironment(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required for C0 cross-surface conformance`);
  }
  return value;
}

async function postEnvelope(
  url: string,
  headers: Record<string, string>,
  body: JsonObject,
  expectedStatus: number,
  credentials: readonly string[],
  label: string
): Promise<{ response: Response; body: JsonObject }> {
  const response = await fetch(url, { method: 'POST', headers, body: JSON.stringify(body) });
  const text = await response.text();
  assertCredentialFree(text, credentials, label);
  if (response.status !== expectedStatus) {
    throw new Error(`${label} returned HTTP ${response.status}, expected ${expectedStatus}`);
  }
  const envelope = objectValue(parseJson(text, label), `${label} envelope`);
  expect(envelope.code).toBe(expectedStatus);
  const requestId = uuidValue(envelope.requestId, `${label} request ID`);
  expect(response.headers.get('x-request-id')).toBe(requestId);
  expect(response.headers.get('x-a3s-api-contract-version')).toBe(CLOUD_API_CONTRACT_VERSION);
  return { response, body: envelope };
}

async function runCli(
  environment: ConformanceEnvironment,
  credentials: readonly string[],
  arguments_: readonly string[],
  context: CliContext
): Promise<CliResult> {
  const childEnvironment: Record<string, string> = {
    PATH: process.env.PATH ?? '/usr/bin:/bin',
    A3S_CLOUD_TOKEN: context.token,
    A3S_CLOUD_URL: environment.baseUrl,
    A3S_CLOUD_ORGANIZATION_ID: context.organizationId,
    A3S_CLOUD_OUTPUT: 'json',
    A3S_CLOUD_TIMEOUT_MS: '10000',
  };
  if (context.projectId) {
    childEnvironment.A3S_CLOUD_PROJECT_ID = context.projectId;
  }
  const child = Bun.spawn([environment.cliBinary, ...arguments_], {
    env: childEnvironment,
    stdout: 'pipe',
    stderr: 'pipe',
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  assertCredentialFree(stdout, credentials, 'CLI standard output');
  assertCredentialFree(stderr, credentials, 'CLI standard error');
  return { exitCode, stdout, stderr };
}

function cliError(text: string, label: string): JsonObject {
  const output = objectValue(parseJson(text, label), `${label} output`);
  return objectValue(output.error, `${label} error`);
}

async function capturedError(operation: () => Promise<unknown>): Promise<unknown> {
  try {
    await operation();
  } catch (error) {
    return error;
  }
  throw new Error('expected operation to fail');
}

function parseJson(text: string, label: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new Error(`${label} did not return valid JSON`);
  }
}

function objectValue(value: unknown, label: string): JsonObject {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as JsonObject;
}

function arrayValue(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) {
    throw new Error(`${label} must be an array`);
  }
  return value;
}

function stringValue(value: unknown, label: string): string {
  if (typeof value !== 'string' || !value) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function uuidValue(value: unknown, label: string): string {
  const candidate = stringValue(value, label);
  if (!UUID_PATTERN.test(candidate)) {
    throw new Error(`${label} must be a UUID`);
  }
  return candidate;
}

function assertCredentialFree(value: string, credentials: readonly string[], label: string): void {
  if (credentials.some((credential) => value.includes(credential))) {
    throw new Error(`${label} exposed a credential`);
  }
}
