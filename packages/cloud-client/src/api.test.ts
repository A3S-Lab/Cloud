import { describe, expect, it } from 'bun:test';
import {
  A3S_ACL_MEDIA_TYPE,
  CLOUD_API_CONTRACT_VERSION,
  CLOUD_API_MAJOR_VERSION,
  CloudApi,
  CloudApiError,
  type CloudFetch,
  DEFAULT_CLOUD_API_BASE_PATH,
  DEFAULT_WORKFLOW_RUN_WAIT_SECONDS,
  MAX_MCP_ROUTE_POLICY_ACL_BYTES,
  MAX_MCP_SERVICE_PROFILE_ACL_BYTES,
  MAX_ONTOLOGY_ACL_BYTES,
  MAX_WORKFLOW_GOAL_ACL_BYTES,
  MAX_WORKFLOW_PAYLOAD_ACL_BYTES,
  MAX_WORKFLOW_RUN_HISTORY_LIMIT,
  MAX_WORKFLOW_RUN_LIST_LIMIT,
  MAX_WORKFLOW_RUN_TIMEOUT_SECONDS,
  MAX_WORKFLOW_RUN_WAIT_SECONDS,
  MAX_WORKLOAD_ACL_BYTES,
} from './api';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-07-26T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

describe('CloudApi', () => {
  it('pins the shared client to the stable REST contract', () => {
    expect(CLOUD_API_MAJOR_VERSION).toBe(1);
    expect(CLOUD_API_CONTRACT_VERSION).toBe('1.15.0');
    expect(DEFAULT_CLOUD_API_BASE_PATH).toBe('/api/v1');
    expect(new CloudApi(undefined).baseUrl).toBe(DEFAULT_CLOUD_API_BASE_PATH);
  });

  it('uses the shared authenticated transport and encodes tenant paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };
    const api = new CloudApi('a3s_secret', 'https://cloud.example.test/api/v1/', { fetch: fetcher });

    await api.listProjects('organization / one');

    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(
      'https://cloud.example.test/api/v1/organizations/organization%20%2F%20one/projects'
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'GET',
        headers: expect.objectContaining({ Authorization: 'Bearer a3s_secret' }),
      })
    );
    expect(String(calls[0]?.[0])).not.toContain('a3s_secret');
  });

  it('reads public platform and health diagnostics without requiring an authorization header', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      const path = String(args[0]);
      if (path.endsWith('/platform')) {
        return jsonResponse({ name: 'a3s-cloud', version: '0.1.0', role: 'api' });
      }
      if (path.endsWith('/health/live')) {
        return jsonResponse({ status: 'up', checks: {} });
      }
      return jsonResponse(
        {
          status: 'down',
          checks: { repositories: { status: 'down', details: { reason: 'unavailable' } } },
        },
        503
      );
    };

    const diagnostics = await new CloudApi(undefined, '/api/v1', { fetch: fetcher }).getDiagnostics();

    expect(diagnostics).toEqual({
      platform: { name: 'a3s-cloud', version: '0.1.0', role: 'api' },
      liveness: { status: 'up', checks: {} },
      readiness: {
        status: 'down',
        checks: { repositories: { status: 'down', details: { reason: 'unavailable' } } },
      },
    });
    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/platform',
      '/api/v1/health/live',
      '/api/v1/health/ready',
    ]);
    for (const [, init] of calls) {
      expect(init?.headers).not.toHaveProperty('Authorization');
    }
  });

  it('does not reinterpret a real readiness error envelope as a health report', async () => {
    const api = new CloudApi(undefined, '/api/v1', {
      fetch: async () =>
        new Response(
          JSON.stringify({
            code: 503,
            statusCode: 'SERVICE_UNAVAILABLE',
            message: 'Service unavailable',
            details: {},
            requestId: '019c0000-0000-7000-8000-000000000001',
            timestamp: '2026-07-27T00:00:00.000Z',
          }),
          { status: 503, headers: { 'content-type': 'application/json' } }
        ),
    });

    await expect(api.getReadiness()).rejects.toMatchObject({
      status: 503,
      statusCode: 'SERVICE_UNAVAILABLE',
    });
  });

  it('exposes the tenant-scoped node projection', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };

    await new CloudApi('token', '/api/v1', { fetch: fetcher }).listNodes('organization');

    expect(calls[0]?.[0]).toBe('/api/v1/organizations/organization/nodes');
  });

  it('searches only through the bounded tenant-scoped projection endpoint', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse([]);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.searchResources('organization / one', '  Cloud worker  ', 25);

    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/search?q=Cloud+worker&limit=25'
    );
  });

  it('rejects unbounded search inputs before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse([]);
      },
    });

    expect(() => api.searchResources('organization', '')).toThrow(
      'search query must contain 1 to 128 safe characters'
    );
    expect(() => api.searchResources('organization', 'a'.repeat(129))).toThrow(
      'search query must contain 1 to 128 safe characters'
    );
    expect(() => api.searchResources('organization', 'cloud', 0)).toThrow(
      'search result limit must be between 1 and 50'
    );
    expect(() => api.searchResources('organization', 'cloud', 51)).toThrow(
      'search result limit must be between 1 and 50'
    );
    expect(called).toBe(false);
  });

  it('uses non-mutating POST transport for canonical A3S Use catalog queries', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const search = {
      host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
      search: { query: 'a3s', limit: 20 },
    };
    const inspect = {
      host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
      packageId: 'a3s/example',
    };

    await api.listPluginRegistries('organization / one');
    await api.getPluginRegistry('organization / one', 'registry / one');
    await api.searchPluginCatalog('organization / one', 'registry / one', search);
    await api.searchCachedPluginCatalog('organization / one', 'registry / one', search);
    await api.inspectPluginCatalog('organization / one', 'registry / one', inspect);
    await api.inspectCachedPluginCatalog('organization / one', 'registry / one', inspect);

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        body: init?.body,
        contentType: (init?.headers as Partial<Record<string, string>> | undefined)?.['Content-Type'],
        idempotencyKey: (init?.headers as Partial<Record<string, string>> | undefined)?.['Idempotency-Key'],
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization%20%2F%20one/plugin-registries',
        method: 'GET',
        body: undefined,
        contentType: undefined,
        idempotencyKey: undefined,
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/plugin-registries/registry%20%2F%20one',
        method: 'GET',
        body: undefined,
        contentType: undefined,
        idempotencyKey: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/plugin-registries/registry%20%2F%20one/catalog/search',
        method: 'POST',
        body: JSON.stringify(search),
        contentType: 'application/json',
        idempotencyKey: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/plugin-registries/registry%20%2F%20one/catalog/cache/search',
        method: 'POST',
        body: JSON.stringify(search),
        contentType: 'application/json',
        idempotencyKey: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/plugin-registries/registry%20%2F%20one/catalog/inspect',
        method: 'POST',
        body: JSON.stringify(inspect),
        contentType: 'application/json',
        idempotencyKey: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/plugin-registries/registry%20%2F%20one/catalog/cache/inspect',
        method: 'POST',
        body: JSON.stringify(inspect),
        contentType: 'application/json',
        idempotencyKey: undefined,
      },
    ]);
  });

  it('creates core tenant resources through their existing idempotent REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 201);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.createOrganization('Operations', 'cli:organization-1');
    await api.createProject('organization', 'Cloud', 'cli:project-1');
    await api.createEnvironment('organization', 'project', 'Production', 'cli:environment-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        contentType: (init?.headers as Record<string, string>)['Content-Type'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations',
        method: 'POST',
        idempotencyKey: 'cli:organization-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Operations' }),
      },
      {
        input: '/api/v1/organizations/organization/projects',
        method: 'POST',
        idempotencyKey: 'cli:project-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Cloud' }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments',
        method: 'POST',
        idempotencyKey: 'cli:environment-1',
        contentType: 'application/json',
        body: JSON.stringify({ name: 'Production' }),
      },
    ]);
  });

  it('exposes the complete Asset catalog and release lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.listAssets('organization / one');
    await api.getAsset('organization / one', 'asset');
    await api.createAsset('organization / one', { name: 'catalog-agent', kind: 'agent' }, 'asset:create');
    await api.archiveAsset('organization / one', 'asset', 'asset:archive');
    await api.listAssetReleases('organization / one', 'asset');
    await api.getAssetRelease('organization / one', 'asset', 'release');
    await api.selectAssetRelease('organization / one', 'asset', '2.0.0-alpha.1');
    await api.createAssetRelease(
      'organization / one',
      'asset',
      { version: '1.0.0', commitSha: 'a'.repeat(40) },
      'release:create'
    );
    await api.yankAssetRelease('organization / one', 'asset', 'release', 'release:yank');

    expect(calls.map(([request, init]) => [request, init?.method, init?.body])).toEqual([
      ['/api/v1/organizations/organization%20%2F%20one/assets', 'GET', undefined],
      ['/api/v1/organizations/organization%20%2F%20one/assets/asset', 'GET', undefined],
      [
        '/api/v1/organizations/organization%20%2F%20one/assets',
        'POST',
        JSON.stringify({ name: 'catalog-agent', kind: 'agent' }),
      ],
      ['/api/v1/organizations/organization%20%2F%20one/assets/asset/archive', 'POST', undefined],
      ['/api/v1/organizations/organization%20%2F%20one/assets/asset/releases', 'GET', undefined],
      ['/api/v1/organizations/organization%20%2F%20one/assets/asset/releases/release', 'GET', undefined],
      [
        '/api/v1/organizations/organization%20%2F%20one/assets/asset/release-selection?version=2.0.0-alpha.1',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/assets/asset/releases',
        'POST',
        JSON.stringify({ version: '1.0.0', commitSha: 'a'.repeat(40) }),
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/assets/asset/releases/release/yank',
        'POST',
        undefined,
      ],
    ]);
  });

  it('uses one ACL-native versioned Ontology lifecycle with explicit revision headers', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({}, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const acl = 'ontology { schema = "cloud.workflow.ontology.v1" }';

    await api.listOntologies('organization', 'project');
    await api.getOntology('organization', 'ontology');
    await api.createOntologyFromAcl('organization', 'project', acl, 'ontology:create');
    await api.listOntologyRevisions('organization', 'ontology');
    await api.getOntologyRevision('organization', 'ontology', 'revision-one');
    await api.diffOntologyRevisions('organization', 'ontology', 'revision-one', 'revision-two');
    await api.reviseOntologyFromAcl(
      'organization',
      'ontology',
      acl,
      { expectedVersion: 2, migrationRuleId: 'migrate_ticket_v2' },
      'ontology:revise'
    );

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization/projects/project/ontologies',
      '/api/v1/organizations/organization/ontologies/ontology',
      '/api/v1/organizations/organization/projects/project/ontologies',
      '/api/v1/organizations/organization/ontologies/ontology/revisions',
      '/api/v1/organizations/organization/ontologies/ontology/revisions/revision-one',
      '/api/v1/organizations/organization/ontologies/ontology/revisions/revision-one/diff/revision-two',
      '/api/v1/organizations/organization/ontologies/ontology/revisions',
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: acl,
        headers: expect.objectContaining({
          'Content-Type': A3S_ACL_MEDIA_TYPE,
          'Idempotency-Key': 'ontology:create',
        }),
      })
    );
    expect(calls[6]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: acl,
        headers: expect.objectContaining({
          'Content-Type': A3S_ACL_MEDIA_TYPE,
          'Idempotency-Key': 'ontology:revise',
          'x-a3s-expected-version': '2',
          'x-a3s-migration-rule': 'migrate_ticket_v2',
        }),
      })
    );
  });

  it('rejects invalid Ontology ACL and revision controls before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    expect(() => api.createOntologyFromAcl('organization', 'project', '', 'ontology:create')).toThrow();
    expect(() =>
      api.createOntologyFromAcl(
        'organization',
        'project',
        'x'.repeat(MAX_ONTOLOGY_ACL_BYTES + 1),
        'ontology:create'
      )
    ).toThrow();
    expect(() =>
      api.reviseOntologyFromAcl('organization', 'ontology', 'acl', { expectedVersion: 0 }, 'ontology:revise')
    ).toThrow('expected Ontology version must be a positive safe integer');
    expect(() =>
      api.reviseOntologyFromAcl(
        'organization',
        'ontology',
        'acl',
        { expectedVersion: 1, migrationRuleId: 'not/a/rule' },
        'ontology:revise'
      )
    ).toThrow('Ontology migration rule must be a portable rule ID');
    expect(called).toBe(false);
  });

  it('uses one versioned Workflow definition, goal, and deterministic plan lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({}, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const publication = {
      definitionAcl: 'workflow { schema = "cloud.workflow.definition.v1" }',
      payloads: [
        {
          kind: 'configuration' as const,
          acl: 'configuration { schema = "cloud.workflow.configuration.v1" }',
        },
      ],
    };
    const goalAcl = 'goal { schema = "cloud.workflow.goal.v1" }';

    await api.listWorkflowDefinitions('organization', 'project');
    await api.getWorkflowDefinition('organization', 'definition');
    await api.createWorkflowDefinitionFromAcl('organization', 'project', publication, 'workflow:create');
    await api.listWorkflowRevisions('organization', 'definition');
    await api.getWorkflowRevision('organization', 'definition', 'revision');
    await api.reviseWorkflowDefinitionFromAcl(
      'organization',
      'definition',
      publication,
      { expectedVersion: 2 },
      'workflow:revise'
    );
    await api.listWorkflowGoals('organization', 'project');
    await api.getWorkflowGoal('organization', 'goal');
    await api.createWorkflowGoalFromAcl('organization', 'project', goalAcl, 'goal:create');
    await api.getWorkflowPlanRevision('organization', 'goal', 'plan');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization/projects/project/workflow-definitions',
      '/api/v1/organizations/organization/workflow-definitions/definition',
      '/api/v1/organizations/organization/projects/project/workflow-definitions',
      '/api/v1/organizations/organization/workflow-definitions/definition/revisions',
      '/api/v1/organizations/organization/workflow-definitions/definition/revisions/revision',
      '/api/v1/organizations/organization/workflow-definitions/definition/revisions',
      '/api/v1/organizations/organization/projects/project/workflow-goals',
      '/api/v1/organizations/organization/workflow-goals/goal',
      '/api/v1/organizations/organization/projects/project/workflow-goals',
      '/api/v1/organizations/organization/workflow-goals/goal/plan-revisions/plan',
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(publication),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'workflow:create',
        }),
      })
    );
    expect(calls[5]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({
          'x-a3s-expected-version': '2',
        }),
      })
    );
    expect(calls[8]?.[1]).toEqual(
      expect.objectContaining({
        body: goalAcl,
        headers: expect.objectContaining({
          'Content-Type': A3S_ACL_MEDIA_TYPE,
          'Idempotency-Key': 'goal:create',
        }),
      })
    );
  });

  it('rejects invalid Workflow publication, revision, and goal inputs before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    expect(() =>
      api.createWorkflowDefinitionFromAcl(
        'organization',
        'project',
        { definitionAcl: 'workflow {}', payloads: [] },
        'workflow:create'
      )
    ).toThrow('Workflow revision must contain between');
    expect(() =>
      api.createWorkflowDefinitionFromAcl(
        'organization',
        'project',
        {
          definitionAcl: 'workflow {}',
          payloads: [
            {
              kind: 'configuration',
              acl: 'x'.repeat(MAX_WORKFLOW_PAYLOAD_ACL_BYTES + 1),
            },
          ],
        },
        'workflow:create'
      )
    ).toThrow('Workflow payload ACL must contain between');
    expect(() =>
      api.reviseWorkflowDefinitionFromAcl(
        'organization',
        'definition',
        {
          definitionAcl: 'workflow {}',
          payloads: [{ kind: 'configuration', acl: 'configuration {}' }],
        },
        { expectedVersion: 0 },
        'workflow:revise'
      )
    ).toThrow('expected WorkflowDefinition version must be a positive safe integer');
    expect(() =>
      api.createWorkflowGoalFromAcl(
        'organization',
        'project',
        'x'.repeat(MAX_WORKFLOW_GOAL_ACL_BYTES + 1),
        'goal:create'
      )
    ).toThrow('Workflow goal ACL must contain between');
    expect(called).toBe(false);
  });

  it('uses bounded tenant-scoped WorkflowRun mutation, query, wait, output, and history paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({}, args[1]?.method === 'POST' ? 202 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.startWorkflowRun(
      'organization / one',
      'project / one',
      { workflowGoalId: 'goal', planRevisionId: 'plan', timeoutSeconds: 60 },
      'workflow-run:start'
    );
    await api.cancelWorkflowRun(
      'organization / one',
      'run / one',
      { reason: 'operator request' },
      'workflow-run:cancel'
    );
    await api.listWorkflowRuns('organization / one', 'project / one', { limit: 2 });
    await api.getWorkflowRun('organization / one', 'run / one');
    await api.waitWorkflowRun('organization / one', 'run / one');
    await api.getWorkflowRunOutput('organization / one', 'run / one');
    await api.getWorkflowRunHistory('organization / one', 'run / one', {
      afterSequence: 7,
      limit: 10,
    });

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/workflow-runs',
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/cancel',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/workflow-runs?limit=2',
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one',
      `/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/wait?timeoutSeconds=${DEFAULT_WORKFLOW_RUN_WAIT_SECONDS}`,
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/output',
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/history?afterSequence=7&limit=10',
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'workflow-run:start' }),
        body: JSON.stringify({
          workflowGoalId: 'goal',
          planRevisionId: 'plan',
          timeoutSeconds: 60,
        }),
      })
    );
    expect(calls[1]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'workflow-run:cancel' }),
        body: JSON.stringify({ reason: 'operator request' }),
      })
    );
  });

  it('rejects unbounded WorkflowRun options before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    const start = (timeoutSeconds: number) =>
      api.startWorkflowRun(
        'organization',
        'project',
        { workflowGoalId: 'goal', planRevisionId: 'plan', timeoutSeconds },
        'workflow-run:start'
      );

    expect(() => start(0)).toThrow('WorkflowRun timeoutSeconds must be between');
    expect(() => start(MAX_WORKFLOW_RUN_TIMEOUT_SECONDS + 1)).toThrow(
      'WorkflowRun timeoutSeconds must be between'
    );
    expect(() =>
      api.cancelWorkflowRun('organization', 'run', { reason: 'unsafe\nreason' }, 'workflow-run:cancel')
    ).toThrow('WorkflowRun cancellation reason must contain');
    expect(() => api.listWorkflowRuns('organization', 'project', { limit: 0 })).toThrow(
      'WorkflowRun list limit must be between'
    );
    expect(() =>
      api.listWorkflowRuns('organization', 'project', { limit: MAX_WORKFLOW_RUN_LIST_LIMIT + 1 })
    ).toThrow('WorkflowRun list limit must be between');
    expect(() =>
      api.waitWorkflowRun('organization', 'run', { timeoutSeconds: MAX_WORKFLOW_RUN_WAIT_SECONDS + 1 })
    ).toThrow('WorkflowRun wait timeoutSeconds must be between');
    expect(() => api.getWorkflowRunHistory('organization', 'run', { limit: 0 })).toThrow(
      'WorkflowRun history limit must be between'
    );
    expect(() =>
      api.getWorkflowRunHistory('organization', 'run', {
        limit: MAX_WORKFLOW_RUN_HISTORY_LIMIT + 1,
      })
    ).toThrow('WorkflowRun history limit must be between');
    expect(called).toBe(false);
  });

  it('reads and binds an immutable MCP Service Profile as raw A3S ACL', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const acl = 'service { endpoint_path = "/mcp" runtime_port = "mcp" }';

    await api.getMcpServiceProfile('organization / one', 'asset', 'release');
    await api.bindMcpServiceProfileFromAcl('organization / one', 'asset', 'release', acl, 'profile:bind-1');

    expect(calls[0]).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/assets/asset/releases/release/mcp-service-profile',
      expect.objectContaining({ method: 'GET' }),
    ]);
    expect(calls[1]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/assets/asset/releases/release/mcp-service-profile'
    );
    expect(calls[1]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': A3S_ACL_MEDIA_TYPE,
          'Idempotency-Key': 'profile:bind-1',
        }),
        body: acl,
      })
    );
  });

  it('reads, creates, and revises MCP route policy desired state as raw A3S ACL', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const acl = 'mcp_route_policy "route" { policy_revision = 1 }';

    await api.listMcpRoutePolicies('organization / one', 'project', 'environment');
    await api.getMcpRoutePolicy('organization / one', 'route');
    await api.createMcpRoutePolicyFromAcl(
      'organization / one',
      'project',
      'environment',
      acl,
      'mcp-route:create-1'
    );
    await api.reviseMcpRoutePolicyFromAcl('organization / one', 'route', acl, 'mcp-route:revise-1');

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/mcp-route-policies',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/mcp-route-policies/route', 'GET'],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/mcp-route-policies',
        'POST',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/mcp-route-policies/route/revisions', 'POST'],
    ]);
    for (const [call, key] of [
      [calls[2], 'mcp-route:create-1'],
      [calls[3], 'mcp-route:revise-1'],
    ] as const) {
      expect(call?.[1]).toEqual(
        expect.objectContaining({
          method: 'POST',
          headers: expect.objectContaining({
            'Content-Type': A3S_ACL_MEDIA_TYPE,
            'Idempotency-Key': key,
          }),
          body: acl,
        })
      );
    }
  });

  it('uses the shared transport for finite Execution lifecycle operations', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 202);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const digest = `sha256:${'a'.repeat(64)}`;
    const input = {
      artifact: {
        uri: `oci://registry.example/tasks/echo@${digest}`,
        digest,
        mediaType: 'application/vnd.oci.image.manifest.v1+json',
      },
      process: {
        command: ['/app/echo'],
        args: [],
        workingDirectory: null,
        environment: {},
      },
      input: { message: 'hello' },
      resources: {
        cpuMillis: 250,
        memoryBytes: 134_217_728,
        pids: 64,
        ephemeralStorageBytes: null,
        timeoutMs: 5_000,
      },
    };

    await api.listExecutions('organization / one', 'project', 'environment');
    await api.getExecution('organization / one', 'execution');
    await api.createExecution('organization / one', 'project', 'environment', input, 'execution:create');
    await api.cancelExecution('organization / one', 'execution', 'execution:cancel');

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/executions?limit=100',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/executions/execution', 'GET'],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/executions',
        'POST',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/executions/execution', 'DELETE'],
    ]);
    expect((calls[2]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe('execution:create');
    expect(calls[2]?.[1]?.body).toBe(JSON.stringify(input));
    expect((calls[3]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe('execution:cancel');
  });

  it('exposes Agent conversations, executions, and resumable semantic events', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 202 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const input = {
      agentAssetId: 'agent / one',
      agentAssetReleaseId: 'release',
      input: { message: 'hello' },
    };

    await api.listAgentConversations('organization / one', 'project', 'environment');
    await api.getAgentConversation('organization / one', 'conversation');
    await api.createAgentConversation('organization / one', 'project', 'environment', 'conversation:create');
    await api.listAgentExecutions('organization / one', 'conversation');
    await api.getAgentExecution('organization / one', 'execution');
    await api.getAgentExecutionChangeSet('organization / one', 'execution');
    await api.startAgentExecution('organization / one', 'conversation', input, 'agent-execution:start');
    await api.cancelAgentExecution('organization / one', 'execution', 'agent-execution:cancel');
    await api.getAgentExecutionEvents('organization / one', 'conversation', {
      cursor: '7',
      limit: 25,
    });

    expect(calls.map(([request, init]) => [request, init?.method, init?.body])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/agent-conversations?limit=100',
        'GET',
        undefined,
      ],
      ['/api/v1/organizations/organization%20%2F%20one/agent-conversations/conversation', 'GET', undefined],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/agent-conversations',
        'POST',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-conversations/conversation/executions?limit=100',
        'GET',
        undefined,
      ],
      ['/api/v1/organizations/organization%20%2F%20one/agent-executions/execution', 'GET', undefined],
      ['/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/changes', 'GET', undefined],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-conversations/conversation/executions',
        'POST',
        JSON.stringify(input),
      ],
      ['/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/cancel', 'POST', undefined],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-conversations/conversation/events?cursor=7&limit=25',
        'GET',
        undefined,
      ],
    ]);
    expect((calls[2]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe('conversation:create');
    expect((calls[2]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
    expect((calls[6]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-execution:start'
    );
    expect((calls[7]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-execution:cancel'
    );
    expect((calls[7]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
    expect(api.agentExecutionEventStreamUrl('organization / one', 'conversation')).toBe(
      '/api/v1/organizations/organization%20%2F%20one/agent-conversations/conversation/events/stream?limit=16'
    );
  });

  it('rejects invalid Agent event cursors and limits before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() => api.getAgentExecutionEvents('organization', 'conversation', { cursor: '' })).toThrow(
      'Agent event cursor is invalid'
    );
    expect(() => api.getAgentExecutionEvents('organization', 'conversation', { cursor: '1\n2' })).toThrow(
      'Agent event cursor is invalid'
    );
    expect(() => api.getAgentExecutionEvents('organization', 'conversation', { limit: 0 })).toThrow(
      'Agent event limit must be between 1 and 200'
    );
    expect(() => api.getAgentExecutionEvents('organization', 'conversation', { limit: 201 })).toThrow(
      'Agent event limit must be between 1 and 200'
    );
    expect(called).toBe(false);
  });

  it('changes node lifecycle state with explicit optimistic concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false });
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.markNodeReady('organization', 'node', 3, 'cli:node-ready-1');
    await api.drainNode('organization', 'node', 4, 'cli:node-drain-1');
    await api.revokeNode('organization', 'node', 5, 'cli:node-revoke-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/ready',
        method: 'POST',
        idempotencyKey: 'cli:node-ready-1',
        body: JSON.stringify({ expectedVersion: 3 }),
      },
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/drain',
        method: 'POST',
        idempotencyKey: 'cli:node-drain-1',
        body: JSON.stringify({ expectedVersion: 4 }),
      },
      {
        input: '/api/v1/organizations/organization/nodes/node/actions/revoke',
        method: 'POST',
        idempotencyKey: 'cli:node-revoke-1',
        body: JSON.stringify({ expectedVersion: 5 }),
      },
    ]);
  });

  it('rejects an unsafe node aggregate version before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() => api.drainNode('organization', 'node', 0, 'cli:node-drain-1')).toThrow(
      'expected node version must be a positive safe integer'
    );
    expect(() => api.drainNode('organization', 'node', 1.5, 'cli:node-drain-1')).toThrow(
      'expected node version must be a positive safe integer'
    );
    expect(called).toBe(false);
  });

  it('exposes operational resources through the public tenant paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.getWorkload('organization / one', 'workload / one');
    await api.getDeployment('organization / one', 'deployment / one');
    await api.getRoute('organization / one', 'route / one');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/workloads/workload%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/deployments/deployment%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/routes/route%20%2F%20one',
    ]);
  });

  it('exposes complete Edge queries and idempotent mutations through existing REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.listDomainClaims('organization', 'project', 'environment');
    await api.getDomainClaim('organization', 'claim');
    await api.createDomainClaim(
      'organization',
      'project',
      'environment',
      '*.example.test',
      'cli:claim-create-1'
    );
    await api.verifyDomainClaim(
      'organization',
      'claim',
      'a3s-cloud-verification=proof',
      'cli:claim-verify-1'
    );
    await api.revokeDomainClaim('organization', 'claim', 'customer request', 'cli:claim-revoke-1');
    await api.listGatewayScopes('organization', 'project', 'environment');
    await api.createGatewayScope(
      'organization',
      'project',
      'environment',
      { nodeIds: ['node-a', 'node-b'], minReady: 1, maxUnavailable: 1 },
      'cli:scope-create-1'
    );
    await api.publishRoute(
      'organization',
      'project',
      'environment',
      {
        gatewayScopeId: 'scope',
        workloadRevisionId: 'revision',
        domainClaimId: 'claim',
        hostname: 'api.example.test',
        pathPrefix: '/v1',
        portName: 'http',
      },
      'cli:route-publish-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/domain-claims',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/domain-claims',
        method: 'POST',
        idempotencyKey: 'cli:claim-create-1',
        body: JSON.stringify({ pattern: '*.example.test' }),
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim/verify',
        method: 'POST',
        idempotencyKey: 'cli:claim-verify-1',
        body: JSON.stringify({ proof: 'a3s-cloud-verification=proof' }),
      },
      {
        input: '/api/v1/organizations/organization/domain-claims/claim/revoke',
        method: 'POST',
        idempotencyKey: 'cli:claim-revoke-1',
        body: JSON.stringify({ reason: 'customer request' }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/gateway-scopes',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/gateway-scopes',
        method: 'POST',
        idempotencyKey: 'cli:scope-create-1',
        body: JSON.stringify({ nodeIds: ['node-a', 'node-b'], minReady: 1, maxUnavailable: 1 }),
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/routes',
        method: 'POST',
        idempotencyKey: 'cli:route-publish-1',
        body: JSON.stringify({
          gatewayScopeId: 'scope',
          workloadRevisionId: 'revision',
          domainClaimId: 'claim',
          hostname: 'api.example.test',
          pathPrefix: '/v1',
          portName: 'http',
        }),
      },
    ]);
  });

  it('exposes Source queries, connection bootstrap, and idempotent mutations through existing REST paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const recipe = {
      schema: 'a3s.cloud.build-recipe.v1' as const,
      kind: 'dockerfile' as const,
      contextPath: 'services/api',
      dockerfilePath: 'Dockerfile',
      target: 'release',
      platforms: ['linux/amd64' as const, 'linux/arm64' as const],
    };

    await api.listSourceRevisions('organization', 'project', 'environment');
    await api.resolveSourceRevision(
      'organization',
      'project',
      'environment',
      {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        reference: { kind: 'branch', value: 'main' },
        recipe,
      },
      'cli:source-resolve-1'
    );
    await api.getGithubConnection('organization');
    await api.beginGithubConnection('organization');
    await api.listGithubRepositorySubscriptions('organization', 'project', 'environment');
    await api.createGithubRepositorySubscription(
      'organization',
      'project',
      'environment',
      {
        repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
        branch: 'main',
        recipe,
      },
      'cli:source-subscribe-1'
    );
    await api.deactivateGithubRepositorySubscription(
      'organization',
      'project',
      'environment',
      'subscription',
      'cli:source-deactivate-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions',
        method: 'POST',
        idempotencyKey: 'cli:source-resolve-1',
        body: JSON.stringify({
          repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
          reference: { kind: 'branch', value: 'main' },
          recipe,
        }),
      },
      {
        input: '/api/v1/organizations/organization/source-connections/github',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/source-connections/github',
        method: 'POST',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github',
        method: 'POST',
        idempotencyKey: 'cli:source-subscribe-1',
        body: JSON.stringify({
          repository: { provider: 'github', url: 'https://github.com/A3S-Lab/Cloud.git' },
          branch: 'main',
          recipe,
        }),
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-subscriptions/github/subscription/deactivate',
        method: 'POST',
        idempotencyKey: 'cli:source-deactivate-1',
        body: undefined,
      },
    ]);
  });

  it('exposes Secret queries and idempotent mutations without returning plaintext', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const initialValue = 'postgres://cloud:initial@database';
    const rotatedValue = 'postgres://cloud:rotated@database';

    await api.listSecrets('organization', 'project', 'environment');
    await api.getSecret('organization', 'secret');
    await api.createSecret(
      'organization',
      'project',
      'environment',
      'Database URL',
      initialValue,
      'cli:secret-create-1'
    );
    await api.addSecretVersion('organization', 'secret', rotatedValue, 'cli:secret-version-1');
    await api.revokeSecretVersion('organization', 'secret', 1, 'cli:secret-revoke-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/secrets',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/secrets',
        method: 'POST',
        idempotencyKey: 'cli:secret-create-1',
        body: JSON.stringify({ name: 'Database URL', value: initialValue }),
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret/versions',
        method: 'POST',
        idempotencyKey: 'cli:secret-version-1',
        body: JSON.stringify({ value: rotatedValue }),
      },
      {
        input: '/api/v1/organizations/organization/secrets/secret/versions/1/revoke',
        method: 'POST',
        idempotencyKey: 'cli:secret-revoke-1',
        body: undefined,
      },
    ]);
  });

  it('rejects invalid Secret values and versions before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createSecret('organization', 'project', 'environment', 'Empty', '', 'cli:secret-empty')
    ).toThrow('Secret value must contain between 1 byte and 1 MiB');
    expect(() =>
      api.addSecretVersion('organization', 'secret', 'é'.repeat(524_289), 'cli:secret-large')
    ).toThrow('Secret value must contain between 1 byte and 1 MiB');
    expect(() => api.revokeSecretVersion('organization', 'secret', 0, 'cli:secret-version-zero')).toThrow(
      'Secret version must be a positive safe integer'
    );
    expect(called).toBe(false);
  });

  it('reads bounded workload and BuildRun log pages with opaque cursors', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ records: [], nextCursor: null });
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.getWorkloadLogs('organization', 'workload', 'revision', {
      cursor: 'v1:41',
      limit: 25,
      stream: 'stderr',
    });
    await api.getBuildRunLogs('organization', 'build-run', {
      cursor: 'v1:9',
      limit: 10,
      stream: 'stdout',
    });

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization/workloads/workload/revisions/revision/logs?cursor=v1%3A41&limit=25&stream=stderr',
      '/api/v1/organizations/organization/build-runs/build-run/logs?cursor=v1%3A9&limit=10&stream=stdout',
    ]);
    expect(() => api.getBuildRunLogs('organization', 'build-run', { limit: 0 })).toThrow(
      'log limit must be between 1 and 256'
    );
    expect(() => api.getBuildRunLogs('organization', 'build-run', { cursor: 'x'.repeat(1_025) })).toThrow(
      'log cursor is invalid'
    );
  });

  it('sends operational mutations with one explicit idempotency key', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.stopWorkload('organization', 'workload', 'cli:stop-1');
    await api.rollbackWorkload('organization', 'workload', 'revision', 'cli:rollback-1');
    await api.bindSkillRelease('organization', 'workload', 'skill', 'skill-release', 'cli:bind-skill-1');
    await api.unbindSkillRelease('organization', 'workload', 'skill', 'cli:unbind-skill-1');
    await api.cancelDeployment('organization', 'deployment', 'cli:cancel-deployment-1');
    await api.cancelBuildRun('organization', 'build-run', 'cli:cancel-build-1');
    await api.retryBuildRun('organization', 'build-run', 'cli:retry-build-1');

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        headers: init?.headers,
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/workloads/workload/stop',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:stop-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/workloads/workload/rollback',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:rollback-1' }),
        body: JSON.stringify({ revisionId: 'revision' }),
      },
      {
        input:
          '/api/v1/organizations/organization/workloads/workload/skills/skill/releases/skill-release/bindings',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:bind-skill-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/workloads/workload/skills/skill/bindings',
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:unbind-skill-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/deployments/deployment',
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:cancel-deployment-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/build-runs/build-run',
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:cancel-build-1' }),
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization/build-runs/build-run/retry',
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:retry-build-1' }),
        body: undefined,
      },
    ]);
  });

  it('sends ACL desired state unchanged through every workload mutation path', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const manifest = 'version = 1\nworkload "api" {}\n';

    await api.createWorkloadFromAcl('organization', 'project', 'environment', manifest, 'cli:create-1');
    await api.updateWorkloadFromAcl('organization', 'workload', manifest, 'cli:update-1');
    await api.deploySourceRevisionFromAcl(
      'organization',
      'project',
      'environment',
      'source-revision',
      manifest,
      'cli:source-1'
    );
    await api.deployAgentReleaseFromAcl(
      'organization',
      'project',
      'environment',
      'asset',
      'release',
      manifest,
      'cli:agent-deploy-1'
    );
    await api.updateAgentReleaseFromAcl(
      'organization',
      'workload',
      'asset',
      'release-2',
      manifest,
      'cli:agent-update-1'
    );

    expect(
      calls.map(([input, init]) => ({
        input,
        method: init?.method,
        contentType: (init?.headers as Record<string, string>)['Content-Type'],
        idempotencyKey: (init?.headers as Record<string, string> | undefined)?.['Idempotency-Key'],
        body: init?.body,
      }))
    ).toEqual([
      {
        input: '/api/v1/organizations/organization/projects/project/environments/environment/workloads',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:create-1',
        body: manifest,
      },
      {
        input: '/api/v1/organizations/organization/workloads/workload/deployments',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:update-1',
        body: manifest,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/source-revisions/source-revision/workloads',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:source-1',
        body: manifest,
      },
      {
        input:
          '/api/v1/organizations/organization/projects/project/environments/environment/assets/asset/releases/release/workloads',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:agent-deploy-1',
        body: manifest,
      },
      {
        input:
          '/api/v1/organizations/organization/workloads/workload/assets/asset/releases/release-2/deployments',
        method: 'POST',
        contentType: A3S_ACL_MEDIA_TYPE,
        idempotencyKey: 'cli:agent-update-1',
        body: manifest,
      },
    ]);
  });

  it('injects Agent release identity into JSON deployment routes without accepting an artifact', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({}, 202);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const template = {
      process: { command: ['/app/agent'], args: [], workingDirectory: null, environment: {} },
      secrets: [],
      resources: {
        cpuMillis: 250,
        memoryBytes: 134_217_728,
        pids: 64,
        ephemeralStorageBytes: null,
      },
      ports: [{ name: 'http', containerPort: 8080 }],
      health: {
        portName: 'http',
        path: '/health',
        intervalMs: 5_000,
        timeoutMs: 1_000,
        healthyThreshold: 1,
        unhealthyThreshold: 3,
        stabilizationWindowMs: 10_000,
      },
    };

    await api.deployAgentRelease(
      'organization',
      'project',
      'environment',
      'asset',
      'release-1',
      'catalog-agent',
      template,
      'agent:deploy'
    );
    await api.updateAgentRelease('organization', 'workload', 'asset', 'release-2', template, 'agent:update');

    expect(calls.map(([input, init]) => [input, init?.body])).toEqual([
      [
        '/api/v1/organizations/organization/projects/project/environments/environment/assets/asset/releases/release-1/workloads',
        JSON.stringify({ name: 'catalog-agent', template }),
      ],
      [
        '/api/v1/organizations/organization/workloads/workload/assets/asset/releases/release-2/deployments',
        JSON.stringify({ template }),
      ],
    ]);
  });

  it('rejects empty and oversized ACL before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createWorkloadFromAcl('organization', 'project', 'environment', '', 'cli:create-1')
    ).toThrow('workload ACL must contain between');
    expect(() =>
      api.updateWorkloadFromAcl(
        'organization',
        'workload',
        'é'.repeat(MAX_WORKLOAD_ACL_BYTES),
        'cli:update-1'
      )
    ).toThrow('workload ACL must contain between');
    expect(() =>
      api.bindMcpServiceProfileFromAcl('organization', 'asset', 'release', '', 'profile:bind-1')
    ).toThrow('MCP Service profile ACL must contain between');
    expect(() =>
      api.bindMcpServiceProfileFromAcl(
        'organization',
        'asset',
        'release',
        '茅'.repeat(MAX_MCP_SERVICE_PROFILE_ACL_BYTES),
        'profile:bind-2'
      )
    ).toThrow('MCP Service profile ACL must contain between');
    expect(() =>
      api.createMcpRoutePolicyFromAcl('organization', 'project', 'environment', '', 'mcp-route:create-1')
    ).toThrow('MCP route policy ACL must contain between');
    expect(() =>
      api.reviseMcpRoutePolicyFromAcl(
        'organization',
        'route',
        '茅'.repeat(MAX_MCP_ROUTE_POLICY_ACL_BYTES),
        'mcp-route:revise-1'
      )
    ).toThrow('MCP route policy ACL must contain between');
    expect(called).toBe(false);
  });

  it('rejects unsafe idempotency keys before transport', async () => {
    let called = false;
    const fetcher: CloudFetch = async () => {
      called = true;
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    for (const key of ['', 'contains space', 'contains\nnewline', 'é', 'x'.repeat(256)]) {
      await expect(api.stopWorkload('organization', 'workload', key)).rejects.toThrow(
        'idempotency key is invalid'
      );
    }
    expect(called).toBe(false);
  });

  it('creates resumable event-stream headers without putting credentials in URLs', () => {
    const api = new CloudApi('a3s_secret');

    expect(api.eventStreamHeaders('event-7')).toEqual({
      Accept: 'text/event-stream',
      Authorization: 'Bearer a3s_secret',
      'Last-Event-ID': 'event-7',
    });
    expect(api.operationStreamUrl('organization')).not.toContain('a3s_secret');
  });

  it('exposes tenant-scoped API token metadata and idempotent mutations', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const credential = `a3s_${'a'.repeat(64)}`;

    await api.listApiTokens('organization / one');
    await api.getApiToken('organization / one', 'token / one');
    await api.createApiToken(
      'organization / one',
      {
        name: 'automation',
        token: credential,
        scopes: ['project:write', 'build:write'],
        principalId: '019c0000-0000-7000-8000-000000000010',
        expiresAt: '2027-01-02T03:04:05.000Z',
      },
      'client:token-create'
    );
    await api.revokeApiToken('organization / one', 'token / one', 'client:token-revoke');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/api-tokens',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens/token%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens',
      '/api/v1/organizations/organization%20%2F%20one/api-tokens/token%20%2F%20one',
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:token-create' }),
        body: JSON.stringify({
          name: 'automation',
          token: credential,
          scopes: ['project:write', 'build:write'],
          principalId: '019c0000-0000-7000-8000-000000000010',
          expiresAt: '2027-01-02T03:04:05.000Z',
        }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'DELETE',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:token-revoke' }),
      })
    );
    expect(calls.every(([input]) => !String(input).includes(credential))).toBe(true);
  });

  it('rejects invalid API token creation input before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    const valid = {
      name: 'automation',
      token: `a3s_${'a'.repeat(64)}`,
      scopes: ['project:write'],
      expiresAt: null,
    };

    expect(() =>
      api.createApiToken('organization', { ...valid, token: 'not-a-token' }, 'client:token-invalid')
    ).toThrow('API token must use the a3s_ prefix followed by 64 lowercase hex digits');
    expect(() => api.createApiToken('organization', { ...valid, scopes: [] }, 'client:token-scopes')).toThrow(
      'API token must grant at least one scope'
    );
    expect(() =>
      api.createApiToken(
        'organization',
        { ...valid, scopes: ['Project:write'] },
        'client:token-invalid-scope'
      )
    ).toThrow('API token scope must use bounded lowercase domain:action syntax');
    expect(() =>
      api.createApiToken('organization', { ...valid, expiresAt: 'tomorrow' }, 'client:token-invalid-expiry')
    ).toThrow('API token expiry must be an RFC 3339 timestamp');
    expect(() =>
      api.createApiToken(
        'organization',
        { ...valid, expiresAt: '2027-02-30T03:04:05Z' },
        'client:token-invalid-calendar-expiry'
      )
    ).toThrow('API token expiry must be an RFC 3339 timestamp');
    expect(called).toBe(false);
  });

  it('exposes one tenant-scoped membership lifecycle with optimistic concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });

    await api.listMemberships('organization / one');
    await api.getMembership('organization / one', 'membership / one');
    await api.createServiceMembership(
      'organization / one',
      { name: 'release automation', role: 'member' },
      'client:membership-create'
    );
    await api.changeMembershipRole(
      'organization / one',
      'membership / one',
      'restricted',
      2,
      'client:membership-role'
    );
    await api.revokeMembership('organization / one', 'membership / one', 3, 'client:membership-revoke');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/memberships',
      '/api/v1/organizations/organization%20%2F%20one/memberships/membership%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/memberships',
      '/api/v1/organizations/organization%20%2F%20one/memberships/membership%20%2F%20one/role',
      '/api/v1/organizations/organization%20%2F%20one/memberships/membership%20%2F%20one/revocation',
    ]);
    expect(calls.slice(2).map(([, init]) => init)).toEqual([
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:membership-create' }),
        body: JSON.stringify({ name: 'release automation', role: 'member' }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:membership-role' }),
        body: JSON.stringify({ role: 'restricted', expectedVersion: 2 }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:membership-revoke' }),
        body: JSON.stringify({ expectedVersion: 3 }),
      }),
    ]);
  });

  it('rejects invalid membership input before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createServiceMembership('organization', { name: '', role: 'member' }, 'client:membership-name')
    ).toThrow('service principal name must contain 1 to 63 visible characters');
    expect(() =>
      api.changeMembershipRole('organization', 'membership', 'member', 0, 'client:membership-version')
    ).toThrow('expected membership version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('exposes one tenant-scoped Resource Grant lifecycle', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const projectId = '019c0000-0000-7000-8000-000000000031';

    await api.listResourceGrants('organization / one', 'membership / one');
    await api.getResourceGrant('organization / one', 'grant / one');
    await api.createResourceGrant(
      'organization / one',
      'membership / one',
      { scope: { kind: 'project', projectId } },
      'client:resource-grant-create'
    );
    await api.revokeResourceGrant('organization / one', 'grant / one', 1, 'client:resource-grant-revoke');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/memberships/membership%20%2F%20one/resource-grants',
      '/api/v1/organizations/organization%20%2F%20one/resource-grants/grant%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/memberships/membership%20%2F%20one/resource-grants',
      '/api/v1/organizations/organization%20%2F%20one/resource-grants/grant%20%2F%20one/revocation',
    ]);
    expect(calls.slice(2).map(([, init]) => init)).toEqual([
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:resource-grant-create' }),
        body: JSON.stringify({ scope: { kind: 'project', projectId } }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:resource-grant-revoke' }),
        body: JSON.stringify({ expectedVersion: 1 }),
      }),
    ]);
  });

  it('rejects invalid Resource Grant input before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createResourceGrant(
        'organization',
        'membership',
        { scope: { kind: 'node', nodeId: 'not-a-uuid' } },
        'client:resource-grant-node'
      )
    ).toThrow('Resource Grant node ID must be a non-nil UUID');
    expect(() =>
      api.revokeResourceGrant('organization', 'grant', 0, 'client:resource-grant-version')
    ).toThrow('expected Resource Grant version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('preserves only the validated API error contract', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 403,
          statusCode: 'FORBIDDEN',
          message: 'insufficient scope',
          details: { requiredScope: 'node:read' },
          requestId: '019c0000-0000-7000-8000-000000000002',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 403 }
      );

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher }).listNodes('organization')
    ).rejects.toEqual(
      expect.objectContaining({
        status: 403,
        statusCode: 'FORBIDDEN',
        requestId: '019c0000-0000-7000-8000-000000000002',
        details: { requiredScope: 'node:read' },
      })
    );
  });

  it('bounds nested error details before exposing them', async () => {
    const fetcher: CloudFetch = async () =>
      new Response(
        JSON.stringify({
          code: 422,
          statusCode: 'UNPROCESSABLE_ENTITY',
          message: 'invalid request',
          details: {
            long: 'x'.repeat(2_000),
            nested: { one: { two: { three: 'hidden' } } },
          },
          requestId: '019c0000-0000-7000-8000-000000000003',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 422 }
      );

    let failure: unknown;
    try {
      await new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations();
    } catch (error) {
      failure = error;
    }

    expect(failure).toBeInstanceOf(CloudApiError);
    const details = (failure as CloudApiError).details;
    expect(String(details.long)).toHaveLength(1_024);
    expect(details.nested).toEqual({ one: { two: { three: '[truncated]' } } });
  });

  it.each([
    ['non-JSON body', new Response('<html>bad gateway</html>', { status: 502 })],
    ['malformed success envelope', new Response(JSON.stringify({ code: 200, data: [] }), { status: 200 })],
    [
      'mismatched status code',
      new Response(
        JSON.stringify({
          code: 500,
          message: 'Success',
          data: [],
          requestId: 'request-1',
          timestamp: '2026-07-26T00:00:00.000Z',
        }),
        { status: 200 }
      ),
    ],
  ])('maps a %s to one sanitized protocol error', async (_name, response) => {
    const fetcher: CloudFetch = async () => response;

    await expect(new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations()).rejects.toEqual(
      expect.objectContaining({
        status: response.status,
        statusCode: 'INVALID_RESPONSE',
        message: 'Cloud API returned an invalid response',
      })
    );
  });

  it('does not expose transport implementation details or credentials', async () => {
    const fetcher: CloudFetch = async () => {
      throw new Error('connection failed while using a3s_secret');
    };

    let failure: unknown;
    try {
      await new CloudApi('a3s_secret', '/api/v1', { fetch: fetcher }).listOrganizations();
    } catch (error) {
      failure = error;
    }

    expect(failure).toEqual(
      expect.objectContaining({
        status: 0,
        statusCode: 'NETWORK_ERROR',
        message: 'Cloud API request failed',
      })
    );
    expect(String(failure)).not.toContain('a3s_secret');
  });

  it('bounds every request with a stable timeout error', async () => {
    const fetcher: CloudFetch = (_input, init) =>
      new Promise((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), {
          once: true,
        });
      });

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher, requestTimeoutMs: 5 }).listOrganizations()
    ).rejects.toEqual(
      expect.objectContaining({
        status: 0,
        statusCode: 'REQUEST_TIMEOUT',
      })
    );
  });

  it('distinguishes caller cancellation from timeout', async () => {
    const fetcher: CloudFetch = (_input, init) =>
      new Promise((_resolve, reject) => {
        if (init?.signal?.aborted) {
          reject(new DOMException('aborted', 'AbortError'));
          return;
        }
        init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), {
          once: true,
        });
      });
    const controller = new AbortController();
    controller.abort();

    await expect(
      new CloudApi('token', '/api/v1', { fetch: fetcher }).listOrganizations(controller.signal)
    ).rejects.toEqual(
      expect.objectContaining({
        statusCode: 'REQUEST_ABORTED',
      })
    );
  });
});
