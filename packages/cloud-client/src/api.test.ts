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
  MAX_HUMAN_TASK_LIST_LIMIT,
  MAX_MCP_ROUTE_POLICY_ACL_BYTES,
  MAX_MCP_SERVICE_PROFILE_ACL_BYTES,
  MAX_ONTOLOGY_ACL_BYTES,
  MAX_WORKFLOW_COMPOSITE_REGIONS_ACL_BYTES,
  MAX_WORKFLOW_GOAL_ACL_BYTES,
  MAX_WORKFLOW_PAYLOAD_ACL_BYTES,
  MAX_WORKFLOW_RUN_HISTORY_LIMIT,
  MAX_WORKFLOW_RUN_LIST_LIMIT,
  MAX_WORKFLOW_RUN_TIMEOUT_SECONDS,
  MAX_WORKFLOW_RUN_WAIT_SECONDS,
  MAX_WORKFLOW_STEP_DESCRIPTOR_BINDINGS_ACL_BYTES,
  MAX_WORKFLOW_VARIABLE_DEFAULTS_ACL_BYTES,
  MAX_WORKLOAD_ACL_BYTES,
} from './api';
import {
  MAX_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT,
  MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES,
} from './connectors';
import {
  MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES,
  validateDeployDurableCellApplicationInput,
} from './durable-cells';

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
    expect(CLOUD_API_CONTRACT_VERSION).toBe('1.79.0');
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

  it('reads exact project attribution revisions and writes with project concurrency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });

    await api.getProjectAttribution('organization / one', 'project / one');
    await api.getProjectAttributionRevision('organization / one', 'project / one', 'profile / one');
    await api.updateProjectAttribution(
      'organization / one',
      'project / one',
      {
        businessOwnerReference: 'engineering/platform',
        costAttributionCode: 'CC-1042',
        labels: { region: 'global', 'service.tier': 'critical' },
      },
      2,
      'client:project-attribution:2'
    );

    expect(calls.map(([input, init]) => ({ input, method: init?.method }))).toEqual([
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/attribution-profile',
        method: 'GET',
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/attribution-profiles/profile%20%2F%20one',
        method: 'GET',
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/attribution-profiles',
        method: 'POST',
      },
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({
          'Idempotency-Key': 'client:project-attribution:2',
          'x-a3s-expected-version': '2',
        }),
        body: JSON.stringify({
          businessOwnerReference: 'engineering/platform',
          costAttributionCode: 'CC-1042',
          labels: { region: 'global', 'service.tier': 'critical' },
        }),
      })
    );
  });

  it('rejects invalid project attribution before transport', async () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.updateProjectAttribution(
        'organization',
        'project',
        { businessOwnerReference: 'owner', labels: { 'Invalid.Key': 'value' } },
        1,
        'invalid-label'
      )
    ).toThrow('project attribution label keys');
    expect(() =>
      api.updateProjectAttribution(
        'organization',
        'project',
        { businessOwnerReference: ' owner ' },
        1,
        'invalid-owner'
      )
    ).toThrow('business owner reference');
    expect(() =>
      api.updateProjectAttribution(
        'organization',
        'project',
        { businessOwnerReference: 'owner' },
        0,
        'invalid-version'
      )
    ).toThrow('project version');
    expect(called).toBe(false);
  });

  it('starts browser-safe OIDC login and link flows without exposing credentials in URLs', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ authorizationUrl: 'https://identity.example.test/authorize?state=opaque' });
    };
    const api = new CloudApi('caller-secret', '/api/v1', { fetch: fetcher });

    expect(api.oidcLoginUrl('organization / one', 'workforce_oidc')).toBe(
      '/api/v1/identity/oidc/workforce_oidc/login?organization_id=organization+%2F+one'
    );
    const started = await api.beginOidcLink('organization / one', 'workforce_oidc');

    expect(started.authorizationUrl).toContain('identity.example.test');
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/identity/oidc/workforce_oidc/link'
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        credentials: 'include',
        headers: expect.objectContaining({ Authorization: 'Bearer caller-secret' }),
      })
    );
    expect(String(calls[0]?.[0])).not.toContain('caller-secret');
    expect(() => api.oidcLoginUrl('organization', 'Workforce')).toThrow('OIDC provider key');
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

  it('manages node pools and maintenance through tenant-scoped endpoints', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false });
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const startsAt = '2026-08-13T00:00:00.000Z';
    const endsAt = '2026-08-13T01:00:00.000Z';

    await api.listNodePools('organization / one');
    await api.getNodePool('organization / one', 'pool / one');
    await api.createNodePool(
      'organization / one',
      { name: 'Primary Workers', memberNodeIds: ['node-a'] },
      'cli:pool-create'
    );
    await api.addNodePoolMembers(
      'organization / one',
      'pool / one',
      { expectedVersion: 1, memberNodeIds: ['node-b'] },
      'cli:pool-members'
    );
    await api.requestNodePoolMemberRemoval(
      'organization / one',
      'pool / one',
      { expectedVersion: 2, memberNodeIds: ['node-b'] },
      'cli:pool-member-removal'
    );
    await api.scheduleNodePoolMaintenance(
      'organization / one',
      'pool / one',
      {
        expectedVersion: 3,
        targetNodeIds: ['node-a'],
        startsAt,
        endsAt,
        reason: 'kernel upgrade',
      },
      'cli:pool-maintenance'
    );
    await api.cancelNodePoolMaintenance(
      'organization / one',
      'pool / one',
      { expectedVersion: 4, maintenanceGeneration: 1 },
      'cli:pool-maintenance-cancel'
    );

    expect(calls.map(([input, init]) => ({ input, method: init?.method, body: init?.body }))).toEqual([
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools',
        method: 'GET',
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools/pool%20%2F%20one',
        method: 'GET',
        body: undefined,
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools',
        method: 'POST',
        body: JSON.stringify({ name: 'Primary Workers', memberNodeIds: ['node-a'] }),
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools/pool%20%2F%20one/members',
        method: 'POST',
        body: JSON.stringify({ expectedVersion: 1, memberNodeIds: ['node-b'] }),
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools/pool%20%2F%20one/members/removal',
        method: 'POST',
        body: JSON.stringify({ expectedVersion: 2, memberNodeIds: ['node-b'] }),
      },
      {
        input: '/api/v1/organizations/organization%20%2F%20one/node-pools/pool%20%2F%20one/maintenance',
        method: 'POST',
        body: JSON.stringify({
          expectedVersion: 3,
          targetNodeIds: ['node-a'],
          startsAt,
          endsAt,
          reason: 'kernel upgrade',
        }),
      },
      {
        input:
          '/api/v1/organizations/organization%20%2F%20one/node-pools/pool%20%2F%20one/maintenance/cancel',
        method: 'POST',
        body: JSON.stringify({ expectedVersion: 4, maintenanceGeneration: 1 }),
      },
    ]);
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
      semanticContracts: {
        descriptorBindingsAcl:
          'descriptor_bindings "support.workflow" { schema = "cloud.workflow.step-descriptor-bindings.v1" }',
        descriptorRegistryAcl:
          'descriptor_registry "support.workflow" { schema = "cloud.workflow.step-descriptor-registry.v1" }',
        variableContractAcl:
          'variable_contract "support.workflow" { schema = "cloud.workflow.variable-contract.v1" }',
        variableDefaultsAcl:
          'variable_defaults "support.workflow" { schema = "cloud.workflow.variable-defaults.v1" }',
        compositeRegionsAcl:
          'composite_regions "support.workflow" { schema = "cloud.workflow.composite-regions.v1" }',
      },
    };
    const goalAcl = 'goal { schema = "cloud.workflow.goal.v1" }';

    await api.getWorkflowNodeCatalog('organization', 'project');
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
      '/api/v1/organizations/organization/projects/project/workflow-node-catalog',
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
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(publication),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'workflow:create',
        }),
      })
    );
    expect(calls[6]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({
          'x-a3s-expected-version': '2',
        }),
      })
    );
    expect(calls[9]?.[1]).toEqual(
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
      api.createWorkflowDefinitionFromAcl(
        'organization',
        'project',
        {
          definitionAcl: 'workflow {}',
          payloads: [{ kind: 'configuration', acl: 'configuration {}' }],
          semanticContracts: {
            descriptorBindingsAcl: 'x'.repeat(MAX_WORKFLOW_STEP_DESCRIPTOR_BINDINGS_ACL_BYTES + 1),
            descriptorRegistryAcl: 'descriptor_registry {}',
            variableContractAcl: 'variable_contract {}',
          },
        },
        'workflow:create'
      )
    ).toThrow('Workflow descriptor bindings ACL must contain between');
    expect(() =>
      api.createWorkflowDefinitionFromAcl(
        'organization',
        'project',
        {
          definitionAcl: 'workflow {}',
          payloads: [{ kind: 'configuration', acl: 'configuration {}' }],
          semanticContracts: {
            descriptorBindingsAcl: 'descriptor_bindings {}',
            descriptorRegistryAcl: 'descriptor_registry {}',
            variableContractAcl: 'variable_contract {}',
            variableDefaultsAcl: 'x'.repeat(MAX_WORKFLOW_VARIABLE_DEFAULTS_ACL_BYTES + 1),
          },
        },
        'workflow:create'
      )
    ).toThrow('Workflow variable defaults ACL must contain between');
    expect(() =>
      api.createWorkflowDefinitionFromAcl(
        'organization',
        'project',
        {
          definitionAcl: 'workflow {}',
          payloads: [{ kind: 'configuration', acl: 'configuration {}' }],
          semanticContracts: {
            descriptorBindingsAcl: 'descriptor_bindings {}',
            descriptorRegistryAcl: 'descriptor_registry {}',
            variableContractAcl: 'variable_contract {}',
            compositeRegionsAcl: 'x'.repeat(MAX_WORKFLOW_COMPOSITE_REGIONS_ACL_BYTES + 1),
          },
        },
        'workflow:create'
      )
    ).toThrow('Workflow composite regions ACL must contain between');
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

  it('uses bounded tenant-scoped WorkflowRun mutation, query, wait, output, variables, diagnostics, and history paths', async () => {
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
    await api.getWorkflowRunVariables('organization / one', 'run / one');
    await api.getWorkflowRunDiagnostics('organization / one', 'run / one');
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
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/variables',
      '/api/v1/organizations/organization%20%2F%20one/workflow-runs/run%20%2F%20one/diagnostics',
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

  it('uses bounded reads and native HumanTask mutations', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const api = new CloudApi('token', '/api/v1', {
      fetch: async (...args) => {
        calls.push(args);
        return jsonResponse([]);
      },
    });

    await api.listHumanTasks('organization / one', 'project / one', {
      status: 'claimed',
      limit: 25,
    });
    await api.getHumanTask('organization / one', 'task / one');
    await api.claimHumanTask('organization / one', 'task / one', 2, 'human-task:claim');
    await api.releaseHumanTask('organization / one', 'task / one', 3, 'human-task:release');
    const submission = {
      apiVersion: 'a3s.dev/form-interaction-submission/v1' as const,
      submissionId: '019c0000-0000-7000-8000-000000000020',
      requestId: 'request-1',
      requestDigest: `sha256:${'a'.repeat(64)}`,
      identity: {
        workflowRunId: '019c0000-0000-7000-8000-000000000021',
        flowRunId: 'flow-1',
        stepId: 'review',
        stepAttempt: 1,
        humanTaskId: '019c0000-0000-7000-8000-000000000022',
        flowHookId: 'review-1',
      },
      form: {
        apiVersion: 'a3s.dev/form-release-ref/v1' as const,
        organizationId: '019c0000-0000-7000-8000-000000000001',
        projectId: '019c0000-0000-7000-8000-000000000002',
        formId: '019c0000-0000-7000-8000-000000000003',
        releaseId: '019c0000-0000-7000-8000-000000000004',
        uri: 'a3s://forms/019c0000-0000-7000-8000-000000000003/releases/1',
        revision: 1,
        digest: `sha256:${'b'.repeat(64)}`,
        compilerRevision: 'a3s-form-core@0.1.0',
        schemaProfile: 'a3s.dev/form-schema-profile/1',
        mode: 'interaction' as const,
      },
      assignment: {
        policyId: 'approval-policy',
        policyRevision: 1,
        policyDigest: `sha256:${'c'.repeat(64)}`,
      },
      taskVersion: 3,
      principalId: '019c0000-0000-7000-8000-000000000005',
      outcome: 'approve' as const,
      idempotencyKey: 'human-task:submit',
      submittedAt: '2026-08-12T00:00:00.000Z',
      value: { approved: true },
      valueDigest: `sha256:${'d'.repeat(64)}`,
    };
    await api.submitHumanTask('organization / one', 'task / one', submission);

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/human-tasks?status=claimed&limit=25',
      '/api/v1/organizations/organization%20%2F%20one/human-tasks/task%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/human-tasks/task%20%2F%20one/claim',
      '/api/v1/organizations/organization%20%2F%20one/human-tasks/task%20%2F%20one/release',
      '/api/v1/organizations/organization%20%2F%20one/human-tasks/task%20%2F%20one/submission',
    ]);
    expect(calls.slice(2, 4).map(([, init]) => init)).toEqual([
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'human-task:claim',
          'x-a3s-expected-version': '2',
        }),
        body: undefined,
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'human-task:release',
          'x-a3s-expected-version': '3',
        }),
        body: undefined,
      }),
    ]);
    expect(calls[4]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.not.objectContaining({
          'Idempotency-Key': expect.anything(),
          'x-a3s-expected-version': expect.anything(),
        }),
        body: JSON.stringify(submission),
      })
    );
  });

  it('rejects invalid HumanTask filters before transport', () => {
    let called = false;
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse([]);
      },
    });

    expect(() => api.listHumanTasks('organization', 'project', { limit: 0 })).toThrow(
      'HumanTask list limit must be between'
    );
    expect(() =>
      api.listHumanTasks('organization', 'project', { limit: MAX_HUMAN_TASK_LIST_LIMIT + 1 })
    ).toThrow('HumanTask list limit must be between');
    expect(() => api.listHumanTasks('organization', 'project', { status: 'unknown' as never })).toThrow(
      'HumanTask status is invalid'
    );
    expect(() => api.claimHumanTask('organization', 'task', 0, 'human-task:claim')).toThrow(
      'expected HumanTask version must be a positive safe integer'
    );
    expect(() =>
      api.releaseHumanTask('organization', 'task', Number.MAX_SAFE_INTEGER + 1, 'human-task:release')
    ).toThrow('expected HumanTask version must be a positive safe integer');
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
    await api.listExecutionTemplates('organization / one', 'project');
    await api.getExecutionTemplate('organization / one', 'project', 'template / one', 'revision / one');
    await api.createExecutionTemplate(
      'organization / one',
      'project',
      { definitionAcl: 'execution_template "echo" { schema = "cloud.execution-template.v1" }' },
      'execution-template:create'
    );
    await api.createExecution('organization / one', 'project', 'environment', input, 'execution:create');
    await api.cancelExecution('organization / one', 'execution', 'execution:cancel');

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/executions?limit=100',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/executions/execution', 'GET'],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/execution-templates?limit=100',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/execution-templates/template%20%2F%20one/revisions/revision%20%2F%20one',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/projects/project/execution-templates', 'POST'],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/executions',
        'POST',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/executions/execution', 'DELETE'],
    ]);
    expect((calls[4]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'execution-template:create'
    );
    expect((calls[5]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe('execution:create');
    expect(calls[5]?.[1]?.body).toBe(JSON.stringify(input));
    expect((calls[6]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe('execution:cancel');
  });

  it('exposes bounded project-scoped Application release management', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const releaseAcl = 'application_release { schema = "cloud.application.release.v1" }\n';

    await api.listApplications('organization / one', 'project / one', 25);
    await api.getApplication('organization / one', 'project / one', 'application / one');
    await api.listApplicationReleases('organization / one', 'project / one', 'application / one', 30);
    await api.getApplicationRelease(
      'organization / one',
      'project / one',
      'application / one',
      'release / one'
    );
    await api.createApplication(
      'organization / one',
      'project / one',
      { name: 'Support assistant', releaseAcl },
      'application:create'
    );
    await api.publishApplicationRelease(
      'organization / one',
      'project / one',
      'application / one',
      { expectedVersion: 2, releaseAcl },
      'application:publish'
    );
    await api.openApplicationSession(
      'organization / one',
      'project / one',
      'application / one',
      { releaseId: 'release / one' },
      'application:session-open'
    );
    await api.getApplicationSession(
      'organization / one',
      'project / one',
      'application / one',
      'session / one'
    );
    await api.closeApplicationSession(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      { expectedVersion: 2 },
      'application:session-close'
    );
    await api.requestApplicationInvocation(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      {
        ontologyId: 'ontology / one',
        ontologyRevisionId: 'ontology revision / one',
        responseMode: 'blocking',
        input: { query: 'hello' },
        timeoutSeconds: 300,
      },
      'application:invoke'
    );
    await api.getApplicationInvocation(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      'invocation / one'
    );
    await api.cancelApplicationInvocation(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      'invocation / one',
      { expectedVersion: 2 },
      'application:invocation-cancel'
    );
    await api.listApplicationMessages(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      7,
      25
    );
    await api.replayApplicationSession(
      'organization / one',
      'project / one',
      'application / one',
      'session / one',
      7,
      25
    );

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications?limit=25',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/releases?limit=30',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/releases/release%20%2F%20one',
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications', 'POST'],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/releases',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/close',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/invocations',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/invocations/invocation%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/invocations/invocation%20%2F%20one/cancel',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/messages?afterSequence=7&limit=25',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/applications/application%20%2F%20one/sessions/session%20%2F%20one/replay?afterSequence=7&limit=25',
        'GET',
      ],
    ]);
    expect(calls[4]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:create' }),
        body: JSON.stringify({
          name: 'Support assistant',
          releaseAcl,
          description: '',
        }),
      })
    );
    expect(calls[5]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:publish' }),
        body: JSON.stringify({ expectedVersion: 2, releaseAcl }),
      })
    );
    expect(calls[6]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:session-open' }),
        body: JSON.stringify({ releaseId: 'release / one', initialVariables: {} }),
      })
    );
    expect(calls[8]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:session-close' }),
        body: JSON.stringify({ expectedVersion: 2 }),
      })
    );
    expect(calls[9]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:invoke' }),
        body: JSON.stringify({
          ontologyId: 'ontology / one',
          ontologyRevisionId: 'ontology revision / one',
          responseMode: 'blocking',
          input: { query: 'hello' },
          timeoutSeconds: 300,
        }),
      })
    );
    expect(calls[11]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'application:invocation-cancel' }),
        body: JSON.stringify({ expectedVersion: 2 }),
      })
    );

    expect(() =>
      api.createApplication(
        'organization',
        'project',
        { name: '   ', releaseAcl },
        'application:invalid-name'
      )
    ).toThrow('Application name must contain');
    expect(() =>
      api.createApplication(
        'organization',
        'project',
        { name: 'Assistant', releaseAcl: '' },
        'application:invalid-acl'
      )
    ).toThrow('Application release ACL must contain between');
    expect(() =>
      api.publishApplicationRelease(
        'organization',
        'project',
        'application',
        { expectedVersion: 0, releaseAcl },
        'application:invalid-version'
      )
    ).toThrow('expected Application version');
    expect(() => api.listApplications('organization', 'project', 201)).toThrow(
      'Application list limit must be between'
    );
    expect(() =>
      api.openApplicationSession(
        'organization',
        'project',
        'application',
        { releaseId: 'release', initialVariables: [] as unknown as Record<string, unknown> },
        'application:invalid-session'
      )
    ).toThrow('Application initial variables must be a JSON object');
    expect(() =>
      api.requestApplicationInvocation(
        'organization',
        'project',
        'application',
        'session',
        {
          ontologyId: 'ontology',
          ontologyRevisionId: 'revision',
          responseMode: 'blocking',
          input: [] as unknown as Record<string, unknown>,
        },
        'application:invalid-invocation'
      )
    ).toThrow('Application invocation input must be a JSON object');
    expect(() =>
      api.listApplicationMessages('organization', 'project', 'application', 'session', -1)
    ).toThrow('afterSequence must be a non-negative');
    expect(() =>
      api.closeApplicationSession(
        'organization',
        'project',
        'application',
        'session',
        { expectedVersion: 0 },
        'application:invalid-session-close'
      )
    ).toThrow('expected Application version');
    expect(() =>
      api.cancelApplicationInvocation(
        'organization',
        'project',
        'application',
        'session',
        'invocation',
        { expectedVersion: 0 },
        'application:invalid-invocation-cancel'
      )
    ).toThrow('expected Application version');
    expect(() =>
      api.replayApplicationSession('organization', 'project', 'application', 'session', 0, 501)
    ).toThrow('Application message list limit must be between');
  });

  it('exposes bounded ACL-native Connector profile and revision management', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const acl = 'connector_http { schema = "cloud.connector.http.v1" }\n';

    await api.listConnectorProfiles('organization / one', 'project', 'environment', 25);
    await api.getConnectorProfile('organization / one', 'project', 'environment', 'profile / one');
    await api.listConnectorRevisions('organization / one', 'project', 'environment', 'profile / one', 30);
    await api.getConnectorRevision(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one'
    );
    await api.getConnectorRevisionRevocation(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one'
    );
    await api.listUnresolvedConnectorExecutionAttempts(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one',
      { cursor: 'v1:123:attempt / cursor', limit: 25 }
    );
    await api.getConnectorExecutionAttempt(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one',
      'attempt / one'
    );
    await api.getConnectorExecutionAttemptResolution(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one',
      'attempt / one'
    );
    await api.createConnectorProfile(
      'organization / one',
      'project',
      'environment',
      { name: 'Incident webhook', definitionAcl: acl },
      'connector:create'
    );
    await api.reviseConnectorProfile(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      { expectedVersion: 2, definitionAcl: acl },
      'connector:revise'
    );
    await api.revokeConnectorRevision(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one',
      { reason: '  destination credential was compromised  ' },
      'connector:revoke'
    );
    await api.resolveConnectorExecutionAttempt(
      'organization / one',
      'project',
      'environment',
      'profile / one',
      'revision / one',
      'attempt / one',
      { reason: '  provider outcome could not be established  ' },
      'connector:resolve-attempt'
    );

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles?limit=25',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions?limit=30',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/revocation',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/execution-attempts?limit=25&cursor=v1%3A123%3Aattempt+%2F+cursor',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/execution-attempts/attempt%20%2F%20one',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/execution-attempts/attempt%20%2F%20one/resolution',
        'GET',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/revocation',
        'POST',
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment/connector-profiles/profile%20%2F%20one/revisions/revision%20%2F%20one/execution-attempts/attempt%20%2F%20one/resolution',
        'POST',
      ],
    ]);
    expect(calls[8]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'connector:create' }),
        body: JSON.stringify({ name: 'Incident webhook', definitionAcl: acl }),
      })
    );
    expect(calls[9]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'connector:revise' }),
        body: JSON.stringify({ expectedVersion: 2, definitionAcl: acl }),
      })
    );
    expect(calls[10]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'connector:revoke' }),
        body: JSON.stringify({ reason: 'destination credential was compromised' }),
      })
    );
    expect(calls[11]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'connector:resolve-attempt' }),
        body: JSON.stringify({ reason: 'provider outcome could not be established' }),
      })
    );

    expect(() =>
      api.createConnectorProfile(
        'organization',
        'project',
        'environment',
        { name: '   ', definitionAcl: acl },
        'connector:invalid-name'
      )
    ).toThrow('Connector profile name must contain');
    expect(() =>
      api.createConnectorProfile(
        'organization',
        'project',
        'environment',
        { name: 'Webhook', definitionAcl: '' },
        'connector:invalid-acl'
      )
    ).toThrow('Connector definition ACL must contain between');
    expect(() =>
      api.reviseConnectorProfile(
        'organization',
        'project',
        'environment',
        'profile',
        {
          expectedVersion: 1,
          definitionAcl: '界'.repeat(MAX_CONNECTOR_HTTP_DEFINITION_ACL_BYTES),
        },
        'connector:oversized-acl'
      )
    ).toThrow('Connector definition ACL must contain between');
    expect(() =>
      api.reviseConnectorProfile(
        'organization',
        'project',
        'environment',
        'profile',
        { expectedVersion: 0, definitionAcl: acl },
        'connector:invalid-version'
      )
    ).toThrow('expected Connector profile version must be a positive safe integer');
    expect(() => api.listConnectorProfiles('organization', 'project', 'environment', 201)).toThrow(
      'Connector list limit must be between 1 and 200'
    );
    expect(() =>
      api.listUnresolvedConnectorExecutionAttempts(
        'organization',
        'project',
        'environment',
        'profile',
        'revision',
        { limit: MAX_CONNECTOR_EXECUTION_ATTEMPT_LIST_LIMIT + 1 }
      )
    ).toThrow('Connector execution attempt list limit must be between 1 and 100');
    expect(() =>
      api.resolveConnectorExecutionAttempt(
        'organization',
        'project',
        'environment',
        'profile',
        'revision',
        'attempt',
        { reason: 'line\nbreak' },
        'connector:invalid-attempt-resolution'
      )
    ).toThrow('Connector execution attempt resolution reason must contain between');
    expect(() =>
      api.revokeConnectorRevision(
        'organization',
        'project',
        'environment',
        'profile',
        'revision',
        { reason: 'line\nbreak' },
        'connector:invalid-revocation'
      )
    ).toThrow('Connector revision revocation reason must contain between');
    expect(() =>
      api.revokeConnectorRevision(
        'organization',
        'project',
        'environment',
        'profile',
        'revision',
        { reason: '界'.repeat(342) },
        'connector:oversized-revocation'
      )
    ).toThrow('Connector revision revocation reason must contain between');
    expect(calls).toHaveLength(12);
  });

  it('reuses the Durable Cells REST authority with bounded ACL-native inputs', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ replayed: false }, args[1]?.method === 'POST' ? 201 : 200);
    };
    const api = new CloudApi('token', '/api/v1', { fetch: fetcher });
    const applicationAcl = 'durable_cell_application { schema = "cloud.durable-cell.application.v1" }\n';
    const serviceProfileAcl = 'durable_cell_service { schema = "cloud.durable-cell.service.v1" }\n';
    const storageProviderProfileAcl =
      'object_namespace_provider "s3_compatible" { schema = "cloud.object-namespace.provider-profile.v1" }\n';
    const providerWorkloadAcl = 'version = 1\nworkload "durable-cell-provider" {}\n';
    const storageBindingAcl = 'durable_cell_deployment { schema = "cloud.durable-cell.deployment.v1" }\n';
    const base =
      '/api/v1/organizations/organization%20%2F%20one/projects/project/environments/environment' +
      '/durable-cell-applications';
    const application = `${base}/application%20%2F%20one`;
    const revision = `${application}/revisions/revision%20%2F%20one`;

    await api.listDurableCellApplications('organization / one', 'project', 'environment', 25);
    await api.getDurableCellApplication('organization / one', 'project', 'environment', 'application / one');
    await api.listDurableCellApplicationRevisions(
      'organization / one',
      'project',
      'environment',
      'application / one',
      30
    );
    await api.getDurableCellApplicationRevision(
      'organization / one',
      'project',
      'environment',
      'application / one',
      'revision / one'
    );
    await api.createDurableCellApplication(
      'organization / one',
      'project',
      'environment',
      { name: 'Counter cells', definitionAcl: applicationAcl },
      'durable-cell:create'
    );
    await api.reviseDurableCellApplication(
      'organization / one',
      'project',
      'environment',
      'application / one',
      { expectedVersion: 2, definitionAcl: applicationAcl },
      'durable-cell:revise'
    );
    await api.startDurableCellApplication(
      'organization / one',
      'project',
      'environment',
      'application / one',
      3,
      'durable-cell:start'
    );
    await api.stopDurableCellApplication(
      'organization / one',
      'project',
      'environment',
      'application / one',
      4,
      'durable-cell:stop'
    );
    await api.deployDurableCellApplication(
      'organization / one',
      'project',
      'environment',
      'application / one',
      'revision / one',
      { serviceProfileAcl, storageProviderProfileAcl, providerWorkloadAcl, storageBindingAcl },
      'durable-cell:deploy'
    );
    await api.publishDurableCellApplicationRoute(
      'organization / one',
      'project',
      'environment',
      'application / one',
      'revision / one',
      {
        serviceProfileAcl,
        gatewayScopeId: '019c0000-0000-7000-8000-000000000071',
        domainClaimId: '019c0000-0000-7000-8000-000000000072',
        hostname: 'cells.example.test',
        pathPrefix: '/',
      },
      'durable-cell:route'
    );

    expect(calls.map(([request, init]) => [request, init?.method])).toEqual([
      [`${base}?limit=25`, 'GET'],
      [application, 'GET'],
      [`${application}/revisions?limit=30`, 'GET'],
      [revision, 'GET'],
      [base, 'POST'],
      [`${application}/revisions`, 'POST'],
      [`${application}/start`, 'POST'],
      [`${application}/stop`, 'POST'],
      [`${revision}/deployments`, 'POST'],
      [`${revision}/routes`, 'POST'],
    ]);
    expect(calls[4]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'durable-cell:create' }),
        body: JSON.stringify({ name: 'Counter cells', definitionAcl: applicationAcl }),
      })
    );
    expect(calls[8]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'durable-cell:deploy' }),
        body: JSON.stringify({
          serviceProfileAcl,
          storageProviderProfileAcl,
          providerWorkloadAcl,
          storageBindingAcl,
        }),
      })
    );
    expect(calls[9]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'durable-cell:route' }),
      })
    );

    expect(() => api.listDurableCellApplications('organization', 'project', 'environment', 201)).toThrow(
      'Durable Cell list limit must be between 1 and 200'
    );
    expect(() =>
      api.createDurableCellApplication(
        'organization',
        'project',
        'environment',
        { name: '   ', definitionAcl: applicationAcl },
        'durable-cell:invalid-name'
      )
    ).toThrow('Durable Cell application name must contain');
    expect(() =>
      api.reviseDurableCellApplication(
        'organization',
        'project',
        'environment',
        'application',
        { expectedVersion: 0, definitionAcl: applicationAcl },
        'durable-cell:invalid-version'
      )
    ).toThrow('expected Durable Cell application version must be a positive safe integer');
    expect(() =>
      api.deployDurableCellApplication(
        'organization',
        'project',
        'environment',
        'application',
        'revision',
        { serviceProfileAcl, storageProviderProfileAcl, providerWorkloadAcl: '', storageBindingAcl },
        'durable-cell:invalid-provider'
      )
    ).toThrow('workload ACL must contain between');
    expect(() =>
      api.deployDurableCellApplication(
        'organization',
        'project',
        'environment',
        'application',
        'revision',
        {
          serviceProfileAcl,
          storageProviderProfileAcl,
          providerWorkloadAcl,
          storageBindingAcl: '界'.repeat(MAX_DURABLE_CELL_STORAGE_BINDING_ACL_BYTES),
        },
        'durable-cell:invalid-storage'
      )
    ).toThrow('Durable Cell storage-binding ACL must contain between');
    expect(() =>
      validateDeployDurableCellApplicationInput({
        serviceProfileAcl,
        providerWorkloadAcl,
        storageBindingAcl,
      })
    ).not.toThrow();
    expect(calls).toHaveLength(10);
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
      providerKind: 'reference.echo' as const,
      input: { message: 'hello' },
    };

    await api.listAgentConversations('organization / one', 'project', 'environment');
    await api.getAgentConversation('organization / one', 'conversation');
    await api.createAgentConversation('organization / one', 'project', 'environment', 'conversation:create');
    await api.listAgentExecutions('organization / one', 'conversation');
    await api.getAgentExecution('organization / one', 'execution');
    await api.getAgentExecutionChangeSet('organization / one', 'execution');
    await api.listAgentExecutionCheckpoints('organization / one', 'execution', { limit: 20 });
    await api.getAgentExecutionCheckpoint('organization / one', 'execution', 'checkpoint / one');
    await api.getAgentExecutionCheckpointSnapshot('organization / one', 'execution', 'checkpoint / one');
    await api.captureAgentExecutionCheckpoint(
      'organization / one',
      'execution',
      { throughEventSequence: 42 },
      'agent-checkpoint:capture'
    );
    await api.forkAgentExecution(
      'organization / one',
      'execution',
      'checkpoint / one',
      { input: { message: 'continue differently' } },
      'agent-execution:fork'
    );
    await api.getAgentExecutionTrajectory('organization / one', 'execution', {
      cursor: '7',
      throughSequence: 42,
      limit: 25,
    });
    await api.listAgentApprovalCheckpoints('organization / one', 'execution', {
      status: 'pending',
      limit: 25,
    });
    await api.getAgentApprovalCheckpoint('organization / one', 'execution', 'checkpoint / one');
    await api.decideAgentApprovalCheckpoint(
      'organization / one',
      'execution',
      'checkpoint / one',
      { outcome: 'approved', reason: 'Reviewed' },
      3,
      'agent-approval:decide'
    );
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
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/checkpoints?limit=20',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/checkpoints/checkpoint%20%2F%20one',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/checkpoints/checkpoint%20%2F%20one/snapshot',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/checkpoints',
        'POST',
        JSON.stringify({ throughEventSequence: 42 }),
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/checkpoints/checkpoint%20%2F%20one/fork',
        'POST',
        JSON.stringify({ input: { message: 'continue differently' } }),
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/trajectory?cursor=7&limit=25&throughSequence=42',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/approval-checkpoints?status=pending&limit=25',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/approval-checkpoints/checkpoint%20%2F%20one',
        'GET',
        undefined,
      ],
      [
        '/api/v1/organizations/organization%20%2F%20one/agent-executions/execution/approval-checkpoints/checkpoint%20%2F%20one/decision',
        'POST',
        JSON.stringify({ outcome: 'approved', reason: 'Reviewed' }),
      ],
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
    expect((calls[9]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-checkpoint:capture'
    );
    expect((calls[10]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-execution:fork'
    );
    expect((calls[14]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-approval:decide'
    );
    expect((calls[14]?.[1]?.headers as Record<string, string>)['x-a3s-expected-version']).toBe('3');
    expect((calls[15]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-execution:start'
    );
    expect((calls[16]?.[1]?.headers as Record<string, string>)['Idempotency-Key']).toBe(
      'agent-execution:cancel'
    );
    expect((calls[16]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
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
    expect(() =>
      api.startAgentExecution(
        'organization',
        'conversation',
        {
          agentAssetId: 'agent',
          agentAssetReleaseId: 'release',
          providerKind: 'unknown.provider' as never,
        },
        'agent:start'
      )
    ).toThrow('Agent provider kind must be a3s.code or reference.echo');
    expect(() =>
      api.listAgentApprovalCheckpoints('organization', 'execution', { status: 'unknown' as never })
    ).toThrow('Agent approval checkpoint status is invalid');
    expect(() => api.listAgentApprovalCheckpoints('organization', 'execution', { limit: 1_001 })).toThrow(
      'Agent approval checkpoint limit must be between 1 and 1000'
    );
    expect(() => api.listAgentExecutionCheckpoints('organization', 'execution', { limit: 1_001 })).toThrow(
      'Agent execution checkpoint limit must be between 1 and 1000'
    );
    expect(() =>
      api.captureAgentExecutionCheckpoint(
        'organization',
        'execution',
        { throughEventSequence: 0 },
        'agent-checkpoint:invalid'
      )
    ).toThrow('Agent checkpoint event sequence must be a positive safe integer');
    expect(() =>
      api.forkAgentExecution(
        'organization',
        'execution',
        'checkpoint',
        { input: { value: BigInt(1) } },
        'agent-fork:invalid'
      )
    ).toThrow('Agent fork input must be JSON serializable');
    expect(() =>
      api.getAgentExecutionTrajectory('organization', 'execution', { throughSequence: 0 })
    ).toThrow('Agent trajectory through sequence must be a positive safe integer');
    expect(() => api.getAgentExecutionTrajectory('organization', 'execution', { limit: 201 })).toThrow(
      'Agent execution trajectory limit must be between 1 and 200'
    );
    expect(() =>
      api.decideAgentApprovalCheckpoint(
        'organization',
        'execution',
        'checkpoint',
        { outcome: 'expired' as never },
        1,
        'agent-approval:invalid'
      )
    ).toThrow('Agent approval decision outcome must be approved or denied');
    expect(() =>
      api.decideAgentApprovalCheckpoint(
        'organization',
        'execution',
        'checkpoint',
        { outcome: 'denied' },
        0,
        'agent-approval:invalid-version'
      )
    ).toThrow('expected Agent approval checkpoint version must be a positive safe integer');
    expect(() =>
      api.decideAgentApprovalCheckpoint(
        'organization',
        'execution',
        'checkpoint',
        { outcome: 'denied', reason: '\u754c'.repeat(342) },
        1,
        'agent-approval:invalid-reason'
      )
    ).toThrow('Agent approval decision reason is invalid');
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
    await api.listGithubInstallationRepositories('organization', {
      cursor: 'repository_cursor',
      limit: 25,
    });
    await api.listGithubRepositoryReferences('organization', 'https://github.com/a3s-lab/cloud', 'branch', {
      limit: 10,
    });
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
          '/api/v1/organizations/organization/source-connections/github/repositories' +
          '?cursor=repository_cursor&limit=25',
        method: 'GET',
        idempotencyKey: undefined,
        body: undefined,
      },
      {
        input:
          '/api/v1/organizations/organization/source-connections/github/repository-references' +
          '?repositoryUrl=https%3A%2F%2Fgithub.com%2Fa3s-lab%2Fcloud&kind=branch&limit=10',
        method: 'GET',
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

  it('rejects unbounded or non-canonical GitHub source discovery queries before transport', () => {
    const api = new CloudApi('token', '/api/v1', {
      fetch: async () => jsonResponse({}),
    });

    expect(() => api.listGithubInstallationRepositories('organization', { limit: 0 })).toThrow(
      'GitHub source discovery limit must be between 1 and 100'
    );
    expect(() => api.listGithubInstallationRepositories('organization', { cursor: 'invalid=' })).toThrow(
      'GitHub source discovery cursor is invalid'
    );
    expect(() =>
      api.listGithubRepositoryReferences('organization', 'https://github.com/A3S-Lab/Cloud.git', 'branch')
    ).toThrow('GitHub source discovery repository URL must be canonical');
    expect(() =>
      api.listGithubRepositoryReferences('organization', 'https://github.com/a3s-lab/cloud.git', 'branch')
    ).toThrow('GitHub source discovery repository URL must be canonical');
    expect(() =>
      api.listGithubRepositoryReferences(
        'organization',
        'https://github.com/a3s-lab/cloud',
        'commit' as never
      )
    ).toThrow('GitHub source discovery reference kind must be branch or tag');
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
    await api.createMembership(
      'organization / one',
      { principalKind: 'human', name: 'release operator', role: 'member' },
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
        body: JSON.stringify({ principalKind: 'human', name: 'release operator', role: 'member' }),
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
      api.createMembership(
        'organization',
        { principalKind: 'human', name: '', role: 'member' },
        'client:membership-name'
      )
    ).toThrow('identity principal name must contain 1 to 63 visible characters');
    expect(() =>
      api.createMembership(
        'organization',
        { principalKind: 'robot' as never, name: 'automation', role: 'member' },
        'client:membership-kind'
      )
    ).toThrow('identity principal kind must be human or service');
    expect(() =>
      api.changeMembershipRole('organization', 'membership', 'member', 0, 'client:membership-version')
    ).toThrow('expected membership version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('exposes principal-bound membership invitations across administrator and self paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const principalId = '019c0000-0000-7000-8000-000000000032';
    const input = {
      principalId,
      role: 'restricted' as const,
      expiresAt: '2026-08-20T03:04:05.000Z',
    };

    await api.listMembershipInvitations('organization / one');
    await api.getMembershipInvitation('organization / one', 'invitation / one');
    await api.listMyMembershipInvitations();
    await api.createMembershipInvitation('organization / one', input, 'client:membership-invitation-create');
    await api.acceptMembershipInvitation('invitation / one', 1, 'client:membership-invitation-accept');
    await api.revokeMembershipInvitation(
      'organization / one',
      'invitation / two',
      2,
      'client:membership-invitation-revoke'
    );

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/membership-invitations',
      '/api/v1/organizations/organization%20%2F%20one/membership-invitations/invitation%20%2F%20one',
      '/api/v1/membership-invitations',
      '/api/v1/organizations/organization%20%2F%20one/membership-invitations',
      '/api/v1/membership-invitations/invitation%20%2F%20one/acceptance',
      '/api/v1/organizations/organization%20%2F%20one/membership-invitations/invitation%20%2F%20two/revocation',
    ]);
    expect(calls.slice(3).map(([, init]) => init)).toEqual([
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'client:membership-invitation-create',
        }),
        body: JSON.stringify(input),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'client:membership-invitation-accept',
        }),
        body: JSON.stringify({ expectedVersion: 1 }),
      }),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'client:membership-invitation-revoke',
        }),
        body: JSON.stringify({ expectedVersion: 2 }),
      }),
    ]);
  });

  it('queries bounded tenant audit records with canonical filters and cursor pagination', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ records: [], nextCursor: null });
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    await api.listAuditRecords('organization / one', {
      actorPrincipalId: '019c0000-0000-7000-8000-000000000032',
      action: 'identity.membership.created',
      aggregateId: '019c0000-0000-7000-8000-000000000033',
      requestId: '019c0000-0000-7000-8000-000000000034',
      projectId: '019c0000-0000-7000-8000-000000000036',
      environmentId: '019c0000-0000-7000-8000-000000000037',
      attributionProfileId: '019c0000-0000-7000-8000-000000000038',
      attributionStatus: 'profile_bound',
      from: '2026-08-12T00:00:00Z',
      to: '2026-08-13T00:00:00Z',
      cursor: 'v1:1786579200000000:019c0000-0000-7000-8000-000000000035',
      limit: 25,
    });
    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/audit-records?' +
        'actorPrincipalId=019c0000-0000-7000-8000-000000000032&' +
        'aggregateId=019c0000-0000-7000-8000-000000000033&' +
        'requestId=019c0000-0000-7000-8000-000000000034&' +
        'projectId=019c0000-0000-7000-8000-000000000036&' +
        'environmentId=019c0000-0000-7000-8000-000000000037&' +
        'attributionProfileId=019c0000-0000-7000-8000-000000000038&' +
        'action=identity.membership.created&attributionStatus=profile_bound&' +
        'from=2026-08-12T00%3A00%3A00Z&' +
        'to=2026-08-13T00%3A00%3A00Z&' +
        'cursor=v1%3A1786579200000000%3A019c0000-0000-7000-8000-000000000035&limit=25'
    );
  });

  it('exports one bounded signed audit page with a required explicit window', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({
        envelope: {
          payloadType: 'application/vnd.a3s.cloud.audit-export.v1+json',
          payload: 'e30=',
          signatures: [{ keyId: 'a'.repeat(64), signature: 'signature' }],
        },
        signingKey: { algorithm: 'ed25519', keyId: 'a'.repeat(64), publicKey: 'public-key' },
      });
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    await api.exportAuditRecords('organization / one', {
      from: '2026-08-01T00:00:00Z',
      to: '2026-08-13T00:00:00Z',
      action: 'identity.membership.created',
      limit: 25,
    });
    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/audit-records/export?' +
        'action=identity.membership.created&from=2026-08-01T00%3A00%3A00Z&' +
        'to=2026-08-13T00%3A00%3A00Z&limit=25'
    );
  });

  it('exports one complete bounded audit manifest without a client cursor', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({
        manifest: {
          envelope: {
            payloadType: 'application/vnd.a3s.cloud.audit-export-manifest.v1+json',
            payload: 'e30=',
            signatures: [{ keyId: 'a'.repeat(64), signature: 'signature' }],
          },
          signingKey: { algorithm: 'ed25519', keyId: 'a'.repeat(64), publicKey: 'public-key' },
        },
        pages: [],
      });
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    await api.exportAuditRecordManifest('organization / one', {
      from: '2026-08-01T00:00:00Z',
      to: '2026-08-13T00:00:00Z',
      action: 'identity.membership.created',
      pageSize: 125,
    });
    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/audit-records/export/manifest?' +
        'action=identity.membership.created&from=2026-08-01T00%3A00%3A00Z&' +
        'to=2026-08-13T00%3A00%3A00Z&pageSize=125'
    );
  });

  it('gets the organization audit retention authority without query ambiguity', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({
        organizationId: '019c0000-0000-7000-8000-000000000031',
        retentionMs: 7_776_000_000,
        policyDigest: `sha256:${'a'.repeat(64)}`,
        appliedPolicyDigest: null,
        currentPolicyApplied: false,
        recordsAvailableFrom: null,
        recordsDeletedBefore: null,
        totalDeletedRecords: 0,
        lastSweptAt: null,
        lastCompletedAt: null,
        nextScanAt: '1970-01-01T00:00:00Z',
        version: 0,
      });
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const status = await api.getAuditRetentionStatus('organization / one');
    expect(calls[0]?.[0]).toBe('/api/v1/organizations/organization%20%2F%20one/audit-records/retention');
    expect(calls[0]?.[1]).toEqual(expect.objectContaining({ method: 'GET' }));
    expect(status.retentionMs).toBe(7_776_000_000);
    expect(status.currentPolicyApplied).toBe(false);
  });

  it('rejects invalid audit query values before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({ records: [], nextCursor: null });
      },
    });
    expect(() => api.listAuditRecords('organization', { actorPrincipalId: 'not-a-uuid' })).toThrow(
      'audit actorPrincipalId must be a non-nil UUID'
    );
    expect(() => api.listAuditRecords('organization', { action: 'Invalid action' })).toThrow(
      'audit action must use bounded lowercase dot-separated segments'
    );
    expect(() => api.listAuditRecords('organization', { action: 'identity.membership.created\n' })).toThrow(
      'audit action must use bounded lowercase dot-separated segments'
    );
    expect(() => api.listAuditRecords('organization', { cursor: '' })).toThrow(
      'audit record cursor is invalid'
    );
    expect(() =>
      api.listAuditRecords('organization', {
        attributionStatus: 'invalid' as 'profile_bound',
      })
    ).toThrow('audit attribution status is invalid');
    expect(() => api.listAuditRecords('organization', { limit: 201 })).toThrow(
      'audit record limit must be between 1 and 200'
    );
    expect(() =>
      api.listAuditRecords('organization', {
        from: '2026-08-14T00:00:00Z',
        to: '2026-08-13T00:00:00Z',
      })
    ).toThrow('audit from timestamp must not exceed to timestamp');
    expect(() =>
      api.exportAuditRecords('organization', {
        from: '2026-07-01T00:00:00Z',
        to: '2026-08-02T00:00:00Z',
      })
    ).toThrow('audit export window must not exceed 31 days');
    expect(() =>
      api.exportAuditRecords('organization', {
        from: '2026-08-01T00:00:00Z',
      } as never)
    ).toThrow('audit export requires both from and to timestamps');
    expect(() =>
      api.exportAuditRecordManifest('organization', {
        from: '2026-08-01T00:00:00Z',
        to: '2026-08-02T00:00:00Z',
        pageSize: 201,
      })
    ).toThrow('audit export manifest page size must be between 1 and 200');
    expect(() =>
      api.exportAuditRecordManifest('organization', {
        from: '2026-08-01T00:00:00Z',
        to: '2026-08-02T00:00:00Z',
        cursor: 'v1:1786582923000000:019c0000-0000-7000-8000-000000000001',
      } as never)
    ).toThrow('audit export manifest does not accept cursor or limit; use pageSize');
    expect(called).toBe(false);
  });

  it('queries a bounded Gateway Route policy security timeline', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({ entries: [], nextCursor: null });
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    await api.listGatewayRoutePolicySecurityTimeline(
      'organization / one',
      '019c0000-0000-7000-8000-000000000036',
      {
        cursor: 'v1:1786579200000000:019c0000-0000-7000-8000-000000000037',
        limit: 25,
      }
    );
    expect(calls[0]?.[0]).toBe(
      '/api/v1/organizations/organization%20%2F%20one/security-investigations/gateway-routes/' +
        '019c0000-0000-7000-8000-000000000036/timeline?' +
        'cursor=v1%3A1786579200000000%3A019c0000-0000-7000-8000-000000000037&limit=25'
    );
  });

  it('rejects invalid security timeline scope and pagination before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({ entries: [], nextCursor: null });
      },
    });
    expect(() => api.listGatewayRoutePolicySecurityTimeline('organization', 'not-a-uuid')).toThrow(
      'Gateway Route policy security timeline route ID must be a non-nil UUID'
    );
    expect(() =>
      api.listGatewayRoutePolicySecurityTimeline('organization', '019c0000-0000-7000-8000-000000000036', {
        cursor: '',
      })
    ).toThrow('security timeline cursor is invalid');
    expect(() =>
      api.listGatewayRoutePolicySecurityTimeline('organization', '019c0000-0000-7000-8000-000000000036', {
        limit: 101,
      })
    ).toThrow('security timeline limit must be between 1 and 100');
    expect(called).toBe(false);
  });

  it('lists, gets, and idempotently reads personal notifications', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const notificationId = '019c0000-0000-7000-8000-000000000036';

    await api.listNotifications('organization / one', {
      unreadOnly: true,
      cursor: `v1:1786579200000000:${notificationId}`,
      limit: 25,
    });
    await api.getNotification('organization / one', notificationId);
    await api.markNotificationRead('organization / one', notificationId, 1, 'client:notification:read');

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/notifications?' +
          `unreadOnly=true&cursor=v1%3A1786579200000000%3A${notificationId}&limit=25`,
        'GET',
      ],
      [`/api/v1/organizations/organization%20%2F%20one/notifications/${notificationId}`, 'GET'],
      [`/api/v1/organizations/organization%20%2F%20one/notifications/${notificationId}/read`, 'POST'],
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({ 'Idempotency-Key': 'client:notification:read' }),
        body: JSON.stringify({ expectedVersion: 1 }),
      })
    );
  });

  it('rejects invalid notification identifiers, cursors, limits, and versions before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    expect(() => api.listNotifications('organization', { cursor: '' })).toThrow(
      'notification cursor is invalid'
    );
    expect(() => api.listNotifications('organization', { limit: 201 })).toThrow(
      'notification limit must be between 1 and 200'
    );
    expect(() => api.getNotification('organization', 'not-a-uuid')).toThrow(
      'notification ID must be a non-nil UUID'
    );
    expect(() =>
      api.markNotificationRead(
        'organization',
        '019c0000-0000-7000-8000-000000000036',
        0,
        'client:notification:read'
      )
    ).toThrow('expected notification version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('manages ACL-native notification alert policies through recipient-bound paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const policyId = '019c0000-0000-7000-8000-000000000038';
    const acl = 'schema = "cloud.notification.alert-policy.v1"\n';

    await api.listNotificationAlertPolicies('organization / one', {
      cursor: `v1:1786579200000000:${policyId}`,
      limit: 25,
    });
    await api.getNotificationAlertPolicy('organization / one', policyId);
    await api.createNotificationAlertPolicy(
      'organization / one',
      acl,
      'client:notification-alert-policy:create'
    );
    await api.revokeNotificationAlertPolicy(
      'organization / one',
      policyId,
      1,
      'client:notification-alert-policy:revoke'
    );

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/notification-alert-policies?' +
          `cursor=v1%3A1786579200000000%3A${policyId}&limit=25`,
        'GET',
      ],
      [`/api/v1/organizations/organization%20%2F%20one/notification-alert-policies/${policyId}`, 'GET'],
      ['/api/v1/organizations/organization%20%2F%20one/notification-alert-policies', 'POST'],
      [
        `/api/v1/organizations/organization%20%2F%20one/notification-alert-policies/${policyId}/revoke`,
        'POST',
      ],
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'client:notification-alert-policy:create',
        }),
        body: acl,
      })
    );
    expect(calls[3]?.[1]).toEqual(expect.objectContaining({ body: JSON.stringify({ expectedVersion: 1 }) }));
  });

  it('rejects invalid notification alert policy inputs before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    expect(() => api.listNotificationAlertPolicies('organization', { cursor: '' })).toThrow(
      'notification alert policy cursor is invalid'
    );
    expect(() => api.listNotificationAlertPolicies('organization', { limit: 201 })).toThrow(
      'notification alert policy limit must be between 1 and 200'
    );
    expect(() => api.getNotificationAlertPolicy('organization', 'not-a-uuid')).toThrow(
      'notification alert policy ID must be a non-nil UUID'
    );
    expect(() => api.createNotificationAlertPolicy('organization', '', 'client:alert-policy:create')).toThrow(
      'notification alert policy ACL must contain between 1 and 16384 UTF-8 bytes'
    );
    expect(() =>
      api.revokeNotificationAlertPolicy(
        'organization',
        '019c0000-0000-7000-8000-000000000038',
        0,
        'client:alert-policy:revoke'
      )
    ).toThrow('expected notification alert policy version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('manages ACL-native outbound notification subscriptions through recipient-bound paths', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const subscriptionId = '019c0000-0000-7000-8000-000000000037';
    const acl = 'schema = "cloud.notification.outbound-subscription.v1"\n';

    await api.listOutboundNotificationSubscriptions('organization / one', {
      cursor: `v1:1786579200000000:${subscriptionId}`,
      limit: 25,
    });
    await api.getOutboundNotificationSubscription('organization / one', subscriptionId);
    await api.createOutboundNotificationSubscription(
      'organization / one',
      acl,
      'client:notification-subscription:create'
    );
    await api.revokeOutboundNotificationSubscription(
      'organization / one',
      subscriptionId,
      1,
      'client:notification-subscription:revoke'
    );

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [
        '/api/v1/organizations/organization%20%2F%20one/notification-outbound-subscriptions?' +
          `cursor=v1%3A1786579200000000%3A${subscriptionId}&limit=25`,
        'GET',
      ],
      [
        `/api/v1/organizations/organization%20%2F%20one/notification-outbound-subscriptions/${subscriptionId}`,
        'GET',
      ],
      ['/api/v1/organizations/organization%20%2F%20one/notification-outbound-subscriptions', 'POST'],
      [
        `/api/v1/organizations/organization%20%2F%20one/notification-outbound-subscriptions/${subscriptionId}/revoke`,
        'POST',
      ],
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'client:notification-subscription:create',
        }),
        body: acl,
      })
    );
    expect(calls[3]?.[1]).toEqual(expect.objectContaining({ body: JSON.stringify({ expectedVersion: 1 }) }));
  });

  it('rejects invalid outbound notification subscription inputs before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });
    expect(() => api.listOutboundNotificationSubscriptions('organization', { cursor: '' })).toThrow(
      'outbound notification subscription cursor is invalid'
    );
    expect(() => api.listOutboundNotificationSubscriptions('organization', { limit: 201 })).toThrow(
      'outbound notification subscription limit must be between 1 and 200'
    );
    expect(() => api.getOutboundNotificationSubscription('organization', 'not-a-uuid')).toThrow(
      'outbound notification subscription ID must be a non-nil UUID'
    );
    expect(() =>
      api.createOutboundNotificationSubscription('organization', '', 'client:subscription:create')
    ).toThrow('outbound notification subscription ACL must contain between 1 and 16384 UTF-8 bytes');
    expect(() =>
      api.revokeOutboundNotificationSubscription(
        'organization',
        '019c0000-0000-7000-8000-000000000037',
        0,
        'client:subscription:revoke'
      )
    ).toThrow('expected outbound notification subscription version must be a positive safe integer');
    expect(called).toBe(false);
  });

  it('rejects invalid membership invitation input before transport', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createMembershipInvitation(
        'organization',
        { principalId: 'not-a-uuid', role: 'member', expiresAt: '2026-08-20T03:04:05Z' },
        'client:membership-invitation-principal'
      )
    ).toThrow('membership invitation principal ID must be a non-nil UUID');
    expect(() =>
      api.createMembershipInvitation(
        'organization',
        {
          principalId: '019c0000-0000-7000-8000-000000000032',
          role: 'member',
          expiresAt: 'tomorrow',
        },
        'client:membership-invitation-expiry'
      )
    ).toThrow('membership invitation expiry must be an RFC 3339 timestamp');
    expect(() =>
      api.acceptMembershipInvitation('invitation', 0, 'client:membership-invitation-version')
    ).toThrow('expected membership invitation version must be a positive safe integer');
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
