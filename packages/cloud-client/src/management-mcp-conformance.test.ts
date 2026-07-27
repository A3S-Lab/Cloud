import { expect, it } from 'bun:test';
import { CLOUD_API_CONTRACT_VERSION } from './api';
import {
  ADMIN_TOOLS,
  arrayValue,
  assertCredentialFree,
  authenticatedHeaders,
  businessErrorContract,
  callTool,
  conformanceEnvironment,
  listTools,
  MCP_PROTOCOL_VERSION,
  mcpRequest,
  objectValue,
  READ_ONLY_TOOLS,
  requestId,
  restEnvelope,
  toolCall,
  toolDefinitions,
  toolNames,
  uuidValue,
} from './management-mcp-conformance-support';

const conformanceIt = process.env.A3S_CLOUD_C0_MCP_CONFORMANCE === '1' ? it : it.skip;

conformanceIt(
  'proves REST and management MCP against one real A3S ORM PostgreSQL control plane',
  async () => {
    const environment = conformanceEnvironment();
    const credentials = [environment.bootstrapToken, environment.adminToken, environment.readOnlyToken];

    const bootstrap = await restEnvelope(
      `${environment.baseUrl}/bootstrap`,
      'POST',
      {
        'content-type': 'application/json',
        'idempotency-key': 'c0:mcp:bootstrap',
        'x-a3s-bootstrap-token': environment.bootstrapToken,
      },
      {
        organizationName: 'C0 MCP Primary Tenant',
        tokenName: 'c0-mcp-admin',
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

    const readOnlyToken = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/api-tokens`,
      'POST',
      authenticatedHeaders(environment.adminToken, 'c0:mcp:read-only-token'),
      {
        name: 'C0 MCP read only',
        token: environment.readOnlyToken,
        scopes: ['cloud:read'],
        expiresAt: null,
      },
      201,
      credentials,
      'REST read-only token creation'
    );
    const readOnlyTokenData = objectValue(readOnlyToken.body.data, 'REST read-only token data');
    const readOnlyTokenId = uuidValue(readOnlyTokenData.id, 'REST read-only token ID');
    expect(readOnlyTokenData.scopes).toEqual(['cloud:read']);

    const initialized = await mcpRequest(
      environment,
      environment.adminToken,
      {
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: MCP_PROTOCOL_VERSION,
          capabilities: {},
          clientInfo: { name: 'a3s-cloud-c0-gate', version: '1.0.0' },
        },
      },
      200,
      credentials,
      'MCP initialize'
    );
    expect(initialized.body.jsonrpc).toBe('2.0');
    expect(initialized.body.id).toBe(1);
    const initializeResult = objectValue(initialized.body.result, 'MCP initialize result');
    expect(initializeResult.protocolVersion).toBe(MCP_PROTOCOL_VERSION);
    expect(initialized.response.headers.get('mcp-session-id')).toBeNull();

    const adminCatalog = await listTools(
      environment,
      environment.adminToken,
      2,
      credentials,
      'administrator catalog'
    );
    const adminToolNames = toolNames(adminCatalog);
    expect(adminToolNames).toEqual([...ADMIN_TOOLS]);

    const readOnlyCatalog = await listTools(
      environment,
      environment.readOnlyToken,
      3,
      credentials,
      'read-only catalog'
    );
    const readOnlyToolNames = toolNames(readOnlyCatalog);
    expect(readOnlyToolNames).toEqual([...READ_ONLY_TOOLS]);
    expect(toolDefinitions(readOnlyCatalog).every((tool) => tool.annotations.readOnlyHint === true)).toBe(
      true
    );

    const hiddenMutation = await mcpRequest(
      environment,
      environment.readOnlyToken,
      toolCall(4, 'a3s_cloud_projects_create', {
        name: 'Hidden Mutation Must Not Exist',
        idempotencyKey: 'c0:mcp:hidden-mutation',
      }),
      200,
      credentials,
      'hidden mutation invocation'
    );
    const hiddenError = objectValue(hiddenMutation.body.error, 'hidden mutation JSON-RPC error');
    expect(hiddenError.code).toBe(-32602);
    expect(hiddenError.message).toBe('Unknown or unavailable tool');

    const emptyProjects = await callTool(
      environment,
      environment.readOnlyToken,
      5,
      'a3s_cloud_projects_list',
      {},
      credentials,
      'read-only project listing after hidden mutation'
    );
    expect(emptyProjects.result.isError).toBe(false);
    expect(arrayValue(emptyProjects.structured.data, 'empty project list')).toEqual([]);

    const projectIdempotencyKey = 'c0:mcp:rest-project';
    const restProject = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/projects`,
      'POST',
      authenticatedHeaders(environment.adminToken, projectIdempotencyKey),
      { name: 'MCP Conformance Project' },
      201,
      credentials,
      'REST project creation'
    );
    const restProjectData = objectValue(restProject.body.data, 'REST project data');
    const projectId = uuidValue(restProjectData.id, 'REST project ID');
    expect(restProjectData.replayed).toBe(false);

    const projectReplay = await callTool(
      environment,
      environment.adminToken,
      6,
      'a3s_cloud_projects_create',
      { name: 'MCP Conformance Project', idempotencyKey: projectIdempotencyKey },
      credentials,
      'MCP project replay'
    );
    expect(projectReplay.result.isError).toBe(false);
    expect(projectReplay.structured.code).toBe(200);
    const replayData = objectValue(projectReplay.structured.data, 'MCP project replay data');
    expect(replayData.id).toBe(projectId);
    expect(replayData.replayed).toBe(true);

    const restEnvironment = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/projects/${projectId}/environments`,
      'POST',
      authenticatedHeaders(environment.adminToken, 'c0:mcp:operational-environment'),
      { name: 'MCP Operational Environment' },
      201,
      credentials,
      'REST operational environment creation'
    );
    const restEnvironmentData = objectValue(restEnvironment.body.data, 'REST environment data');
    const environmentId = uuidValue(restEnvironmentData.id, 'REST environment ID');

    const operationalListRequestIds: Record<string, string> = {};
    for (const entry of [
      { id: 20, name: 'a3s_cloud_nodes_list', arguments: {}, label: 'MCP node listing' },
      { id: 21, name: 'a3s_cloud_operations_list', arguments: {}, label: 'MCP operation listing' },
      {
        id: 22,
        name: 'a3s_cloud_workloads_list',
        arguments: { projectId, environmentId },
        label: 'MCP workload listing',
      },
      {
        id: 23,
        name: 'a3s_cloud_routes_list',
        arguments: { projectId, environmentId },
        label: 'MCP route listing',
      },
      {
        id: 24,
        name: 'a3s_cloud_build_runs_list',
        arguments: { projectId, environmentId },
        label: 'MCP BuildRun listing',
      },
    ]) {
      const listed = await callTool(
        environment,
        environment.readOnlyToken,
        entry.id,
        entry.name,
        entry.arguments,
        credentials,
        entry.label
      );
      expect(listed.result.isError).toBe(false);
      expect(arrayValue(listed.structured.data, `${entry.label} data`)).toEqual([]);
      operationalListRequestIds[entry.name] = requestId(listed.structured, `${entry.label} request ID`);
    }

    const missingOperationalId = crypto.randomUUID();
    const missingOperationalRequestIds: Record<string, string> = {};
    for (const entry of [
      {
        id: 25,
        name: 'a3s_cloud_nodes_get',
        arguments: { nodeId: missingOperationalId },
        label: 'MCP missing node lookup',
      },
      {
        id: 26,
        name: 'a3s_cloud_workloads_get',
        arguments: { workloadId: missingOperationalId },
        label: 'MCP missing workload lookup',
      },
      {
        id: 27,
        name: 'a3s_cloud_deployments_get',
        arguments: { deploymentId: missingOperationalId },
        label: 'MCP missing deployment lookup',
      },
      {
        id: 28,
        name: 'a3s_cloud_routes_get',
        arguments: { routeId: missingOperationalId },
        label: 'MCP missing route lookup',
      },
      {
        id: 29,
        name: 'a3s_cloud_build_runs_get',
        arguments: { buildRunId: missingOperationalId },
        label: 'MCP missing BuildRun lookup',
      },
    ]) {
      const missing = await callTool(
        environment,
        environment.readOnlyToken,
        entry.id,
        entry.name,
        entry.arguments,
        credentials,
        entry.label
      );
      expect(missing.result.isError).toBe(true);
      const contract = businessErrorContract(missing.structured);
      expect(contract.code).toBe(404);
      expect(contract.statusCode).toBe('NOT_FOUND');
      missingOperationalRequestIds[entry.name] = requestId(missing.structured, `${entry.label} request ID`);
    }

    for (const entry of [
      { id: 30, name: 'a3s_cloud_operations_list', arguments: { limit: 0 } },
      { id: 31, name: 'a3s_cloud_operations_list', arguments: { limit: 201 } },
      {
        id: 32,
        name: 'a3s_cloud_build_runs_list',
        arguments: { projectId, environmentId, limit: 0 },
      },
      {
        id: 33,
        name: 'a3s_cloud_build_runs_list',
        arguments: { projectId, environmentId, limit: 201 },
      },
    ]) {
      const rejected = await mcpRequest(
        environment,
        environment.readOnlyToken,
        toolCall(entry.id, entry.name, entry.arguments),
        200,
        credentials,
        `${entry.name} invalid limit ${entry.arguments.limit}`
      );
      const error = objectValue(rejected.body.error, `${entry.name} invalid-limit error`);
      expect(error.code).toBe(-32602);
    }

    const foreignOrganization = await restEnvelope(
      `${environment.baseUrl}/organizations`,
      'POST',
      authenticatedHeaders(environment.adminToken, 'c0:mcp:foreign-organization'),
      { name: 'C0 MCP Foreign Tenant' },
      201,
      credentials,
      'REST foreign organization creation'
    );
    const foreignOrganizationData = objectValue(
      foreignOrganization.body.data,
      'REST foreign organization data'
    );
    const foreignOrganizationId = uuidValue(foreignOrganizationData.id, 'REST foreign organization ID');
    const foreignProject = await restEnvelope(
      `${environment.baseUrl}/organizations/${foreignOrganizationId}/projects`,
      'POST',
      authenticatedHeaders(environment.adminToken, 'c0:mcp:foreign-project'),
      { name: 'MCP Foreign Project' },
      201,
      credentials,
      'REST foreign project creation'
    );
    const foreignProjectData = objectValue(foreignProject.body.data, 'REST foreign project data');
    const foreignProjectId = uuidValue(foreignProjectData.id, 'REST foreign project ID');

    const foreignProjectResult = await callTool(
      environment,
      environment.adminToken,
      7,
      'a3s_cloud_environments_list',
      { projectId: foreignProjectId },
      credentials,
      'MCP foreign-project lookup'
    );
    const missingProjectResult = await callTool(
      environment,
      environment.adminToken,
      8,
      'a3s_cloud_environments_list',
      { projectId: crypto.randomUUID() },
      credentials,
      'MCP missing-project lookup'
    );
    expect(foreignProjectResult.result.isError).toBe(true);
    expect(missingProjectResult.result.isError).toBe(true);
    const foreignErrorContract = businessErrorContract(foreignProjectResult.structured);
    expect(foreignErrorContract).toEqual(businessErrorContract(missingProjectResult.structured));
    expect(foreignErrorContract.code).toBe(404);
    expect(foreignErrorContract.statusCode).toBe('NOT_FOUND');
    expect(JSON.stringify(foreignProjectResult.structured)).not.toContain(foreignProjectId);
    expect(JSON.stringify(foreignProjectResult.structured)).not.toContain('MCP Foreign Project');

    const forgedTenant = await mcpRequest(
      environment,
      environment.adminToken,
      toolCall(9, 'a3s_cloud_projects_list', { organizationId: foreignOrganizationId }),
      200,
      credentials,
      'forged organization argument'
    );
    const forgedTenantError = objectValue(forgedTenant.body.error, 'forged tenant JSON-RPC error');
    expect(forgedTenantError.code).toBe(-32602);

    const revoked = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/api-tokens/${readOnlyTokenId}`,
      'DELETE',
      authenticatedHeaders(environment.adminToken, 'c0:mcp:read-only-token-revoke'),
      undefined,
      200,
      credentials,
      'REST read-only token revocation'
    );
    expect(objectValue(revoked.body.data, 'REST revoked token data').revokedAt).not.toBeNull();

    const revokedRequest = await mcpRequest(
      environment,
      environment.readOnlyToken,
      { jsonrpc: '2.0', id: 10, method: 'tools/list' },
      401,
      credentials,
      'revoked-token next MCP request'
    );
    expect(revokedRequest.body.code).toBe(401);
    expect(revokedRequest.body.statusCode).toBe('UNAUTHORIZED');

    const evidence = {
      schema: 'a3s.cloud.c0-management-mcp.evidence.v2',
      cloudRevision: environment.cloudRevision,
      apiContractVersion: CLOUD_API_CONTRACT_VERSION,
      mcpProtocolVersion: MCP_PROTOCOL_VERSION,
      persistence: 'postgresql-17-through-a3s-orm',
      surfaces: ['rest', 'management-mcp'],
      resources: {
        organizationId,
        projectId,
        environmentId,
        foreignOrganizationId,
        foreignProjectId,
        readOnlyTokenId,
      },
      catalogs: { administrator: adminToolNames, readOnly: readOnlyToolNames },
      requestIds: {
        bootstrap: requestId(bootstrap.body, 'bootstrap request ID'),
        readOnlyTokenCreate: requestId(readOnlyToken.body, 'token-create request ID'),
        restProjectCreate: requestId(restProject.body, 'REST project request ID'),
        mcpProjectReplay: requestId(projectReplay.structured, 'MCP replay request ID'),
        restEnvironmentCreate: requestId(restEnvironment.body, 'REST environment request ID'),
        operationalLists: operationalListRequestIds,
        missingOperationalResources: missingOperationalRequestIds,
        foreignProjectDenial: requestId(foreignProjectResult.structured, 'foreign-project denial request ID'),
        missingProjectDenial: requestId(missingProjectResult.structured, 'missing-project denial request ID'),
        tokenRevocation: requestId(revoked.body, 'token-revocation request ID'),
        revokedToken: requestId(revokedRequest.body, 'revoked-token request ID'),
      },
      checks: [
        'stateless-protocol-initialization',
        'scope-derived-tool-catalogs',
        'hidden-mutation-denial-without-side-effect',
        'rest-to-mcp-idempotency-replay',
        'operational-read-query-catalog',
        'bounded-operational-query-arguments',
        'principal-derived-tenant-context',
        'foreign-and-missing-resource-error-equivalence',
        'immediate-token-revocation',
        'credential-free-evidence',
      ],
    };
    const renderedEvidence = `${JSON.stringify(evidence, null, 2)}\n`;
    assertCredentialFree(renderedEvidence, credentials, 'management MCP evidence');
    await Bun.write(environment.evidenceFile, renderedEvidence);
  },
  60_000
);
