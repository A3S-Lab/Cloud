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
import { proveFormConformance } from './management-mcp-form-conformance';
import {
  proveExecutionTemplateConformance,
  proveExecutionTemplateNondisclosure,
} from './management-mcp-execution-template-conformance';
import { proveOntologyConformance } from './management-mcp-ontology-conformance';

const conformanceIt = process.env.A3S_CLOUD_C0_MCP_CONFORMANCE === '1' ? it : it.skip;

it('pins the current Files, Developer Workflows, source discovery, signed-audit, and retention management MCP catalogs', () => {
  expect(ADMIN_TOOLS).toHaveLength(157);
  expect(READ_ONLY_TOOLS).toHaveLength(90);
  expect(ADMIN_TOOLS.filter((tool) => tool === 'a3s_cloud_workload_profiles_accept')).toEqual([
    'a3s_cloud_workload_profiles_accept',
  ]);
  for (const tool of [
    'a3s_cloud_workload_profiles_get',
    'a3s_cloud_workload_profile_revisions_list',
    'a3s_cloud_workload_profile_revisions_get',
    'a3s_cloud_pull_request_preview_policies_get',
    'a3s_cloud_pull_request_preview_policy_revisions_list',
    'a3s_cloud_pull_request_preview_policy_revisions_get',
    'a3s_cloud_pull_request_previews_get',
  ] as const) {
    expect(ADMIN_TOOLS.filter((candidate) => candidate === tool)).toEqual([tool]);
    expect(READ_ONLY_TOOLS.filter((candidate) => candidate === tool)).toEqual([tool]);
  }
  expect(ADMIN_TOOLS.filter((tool) => tool === 'a3s_cloud_pull_request_preview_policies_accept')).toEqual([
    'a3s_cloud_pull_request_preview_policies_accept',
  ]);
  for (const tool of [
    'a3s_cloud_github_installation_repositories_list',
    'a3s_cloud_github_repository_references_list',
  ] as const) {
    expect(ADMIN_TOOLS.filter((candidate) => candidate === tool)).toEqual([tool]);
    expect(READ_ONLY_TOOLS).not.toContain(tool);
  }
  for (const tool of [
    'a3s_cloud_user_files_reserve',
    'a3s_cloud_user_files_list',
    'a3s_cloud_user_files_get',
    'a3s_cloud_user_files_tombstone',
    'a3s_cloud_user_file_quota_get',
  ] as const) {
    expect(ADMIN_TOOLS.filter((candidate) => candidate === tool)).toEqual([tool]);
  }
  for (const tool of [
    'a3s_cloud_user_files_list',
    'a3s_cloud_user_files_get',
    'a3s_cloud_user_file_quota_get',
  ] as const) {
    expect(READ_ONLY_TOOLS.filter((candidate) => candidate === tool)).toEqual([tool]);
  }
  expect(READ_ONLY_TOOLS).not.toContain('a3s_cloud_user_files_reserve');
  expect(READ_ONLY_TOOLS).not.toContain('a3s_cloud_user_files_tombstone');
  expect(ADMIN_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_records_export')).toEqual([
    'a3s_cloud_audit_records_export',
  ]);
  expect(READ_ONLY_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_records_export')).toEqual([
    'a3s_cloud_audit_records_export',
  ]);
  expect(ADMIN_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_records_export_manifest')).toEqual([
    'a3s_cloud_audit_records_export_manifest',
  ]);
  expect(READ_ONLY_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_records_export_manifest')).toEqual([
    'a3s_cloud_audit_records_export_manifest',
  ]);
  expect(ADMIN_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_retention_get')).toEqual([
    'a3s_cloud_audit_retention_get',
  ]);
  expect(READ_ONLY_TOOLS.filter((tool) => tool === 'a3s_cloud_audit_retention_get')).toEqual([
    'a3s_cloud_audit_retention_get',
  ]);
});

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

    const discovered = await mcpRequest(
      environment,
      environment.adminToken,
      {
        jsonrpc: '2.0',
        id: 1,
        method: 'server/discover',
      },
      200,
      credentials,
      'MCP server discovery'
    );
    expect(discovered.body.jsonrpc).toBe('2.0');
    expect(discovered.body.id).toBe(1);
    const discoverResult = objectValue(discovered.body.result, 'MCP discovery result');
    expect(discoverResult.resultType).toBe('complete');
    expect(discoverResult.supportedVersions).toEqual([MCP_PROTOCOL_VERSION]);
    expect(objectValue(discoverResult.capabilities, 'MCP discovery capabilities')).toEqual({
      tools: {},
    });
    const discoverMetadata = objectValue(discoverResult._meta, 'MCP discovery metadata');
    const serverInfo = objectValue(
      discoverMetadata['io.modelcontextprotocol/serverInfo'],
      'MCP server information'
    );
    expect(serverInfo.name).toBe('a3s-cloud');
    expect(discoverResult.ttlMs).toBe(0);
    expect(discoverResult.cacheScope).toBe('private');
    expect(discovered.response.headers.get('mcp-session-id')).toBeNull();

    const unsupportedVersion = await mcpRequest(
      environment,
      environment.adminToken,
      { jsonrpc: '2.0', id: 101, method: 'tools/list' },
      400,
      credentials,
      'unsupported modern MCP version',
      { bodyProtocolVersion: '1900-01-01', headerProtocolVersion: '1900-01-01' }
    );
    const unsupportedError = objectValue(unsupportedVersion.body.error, 'unsupported-version error');
    expect(unsupportedError.code).toBe(-32022);
    expect(unsupportedError.data).toEqual({
      supported: [MCP_PROTOCOL_VERSION],
      requested: '1900-01-01',
    });

    const headerMismatch = await mcpRequest(
      environment,
      environment.adminToken,
      { jsonrpc: '2.0', id: 102, method: 'tools/list' },
      400,
      credentials,
      'MCP protocol header mismatch',
      { headerProtocolVersion: '2025-06-18' }
    );
    expect(objectValue(headerMismatch.body.error, 'header-mismatch error').code).toBe(-32020);

    const methodHeaderMismatch = await mcpRequest(
      environment,
      environment.adminToken,
      { jsonrpc: '2.0', id: 104, method: 'tools/list' },
      400,
      credentials,
      'MCP method header mismatch',
      { methodHeader: 'tools/call' }
    );
    expect(objectValue(methodHeaderMismatch.body.error, 'method-mismatch error').code).toBe(-32020);

    const nameHeaderMismatch = await mcpRequest(
      environment,
      environment.adminToken,
      toolCall(105, 'a3s_cloud_projects_list', {}),
      400,
      credentials,
      'MCP name header mismatch',
      { nameHeader: 'a3s_cloud_projects_create' }
    );
    expect(objectValue(nameHeaderMismatch.body.error, 'name-mismatch error').code).toBe(-32020);

    const missingClientCapabilities = await mcpRequest(
      environment,
      environment.adminToken,
      { jsonrpc: '2.0', id: 106, method: 'tools/list' },
      400,
      credentials,
      'missing MCP client capabilities',
      { includeClientCapabilities: false }
    );
    expect(objectValue(missingClientCapabilities.body.error, 'missing-capabilities error').code).toBe(-32602);

    const ignoredLegacySession = await mcpRequest(
      environment,
      environment.adminToken,
      { jsonrpc: '2.0', id: 107, method: 'tools/list' },
      200,
      credentials,
      'ignored legacy MCP session identifier',
      { sessionIdHeader: 'legacy-session' }
    );
    expect(ignoredLegacySession.response.headers.get('mcp-session-id')).toBeNull();

    const legacyInitialize = await mcpRequest(
      environment,
      environment.adminToken,
      {
        jsonrpc: '2.0',
        id: 103,
        method: 'initialize',
        params: {
          protocolVersion: '2025-06-18',
          capabilities: {},
          clientInfo: { name: 'legacy-client', version: '1.0.0' },
        },
      },
      404,
      credentials,
      'removed legacy MCP initialize'
    );
    expect(objectValue(legacyInitialize.body.error, 'legacy initialize error').code).toBe(-32601);

    const adminCatalog = await listTools(
      environment,
      environment.adminToken,
      2,
      credentials,
      'administrator catalog'
    );
    const adminToolNames = toolNames(adminCatalog);
    expect(adminToolNames).toEqual([...ADMIN_TOOLS]);
    const readOnlyToolSet = new Set<string>([
      ...READ_ONLY_TOOLS,
      'a3s_cloud_memberships_list',
      'a3s_cloud_memberships_get',
      'a3s_cloud_membership_invitations_list',
      'a3s_cloud_membership_invitations_get',
      'a3s_cloud_resource_grants_list',
      'a3s_cloud_resource_grants_get',
      'a3s_cloud_application_sessions_get',
      'a3s_cloud_application_sessions_replay',
      'a3s_cloud_application_invocations_get',
      'a3s_cloud_application_messages_list',
      'a3s_cloud_github_installation_repositories_list',
      'a3s_cloud_github_repository_references_list',
    ]);
    const destructiveToolSet = new Set<string>([
      'a3s_cloud_memberships_revoke',
      'a3s_cloud_membership_invitations_revoke',
      'a3s_cloud_resource_grants_revoke',
      'a3s_cloud_recipient_contacts_revoke',
      'a3s_cloud_workloads_stop',
      'a3s_cloud_deployments_cancel',
      'a3s_cloud_build_runs_cancel',
      'a3s_cloud_workflow_runs_cancel',
      'a3s_cloud_application_sessions_close',
      'a3s_cloud_application_invocations_cancel',
      'a3s_cloud_user_files_tombstone',
    ]);
    for (const tool of toolDefinitions(adminCatalog)) {
      expect(tool.annotations.readOnlyHint).toBe(readOnlyToolSet.has(tool.name));
      expect(tool.annotations.destructiveHint).toBe(destructiveToolSet.has(tool.name));
      expect(tool.annotations.idempotentHint).toBe(true);
      expect(tool.annotations.openWorldHint).toBe(false);
    }

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

    const ontologyEvidence = await proveOntologyConformance(
      environment,
      organizationId,
      projectId,
      credentials
    );
    const {
      ontologyId,
      firstRevisionId: firstOntologyRevisionId,
      secondRevisionId: secondOntologyRevisionId,
    } = ontologyEvidence;

    // The audit query intentionally adds no writer. Exercise it only after a
    // lifecycle that atomically commits a known foundation audit record.
    const auditPage = await callTool(
      environment,
      environment.readOnlyToken,
      108,
      'a3s_cloud_audit_records_list',
      {
        action: 'workflow.ontology.created',
        aggregateId: ontologyId,
        limit: 25,
      },
      credentials,
      'MCP tenant audit listing'
    );
    expect(auditPage.result.isError).toBe(false);
    const auditData = objectValue(auditPage.structured.data, 'MCP tenant audit page');
    const auditRecords = arrayValue(auditData.records, 'MCP tenant audit records');
    expect(auditRecords).toHaveLength(1);
    for (const value of auditRecords) {
      const record = objectValue(value, 'MCP tenant audit record');
      expect(Object.keys(record).sort()).toEqual([
        'action',
        'actorPrincipalId',
        'aggregateId',
        'attributionProfileId',
        'attributionStatus',
        'environmentId',
        'id',
        'occurredAt',
        'organizationId',
        'projectId',
        'requestId',
      ]);
      expect(record.organizationId).toBe(organizationId);
      expect(record.action).toBe('workflow.ontology.created');
      expect(record.aggregateId).toBe(ontologyId);
      expect(record.projectId).toBe(projectId);
      expect(record.environmentId).toBeNull();
      expect(record.attributionProfileId).toBeNull();
      expect(record.attributionStatus).toBe('profile_missing');
      expect(record).not.toHaveProperty('details');
    }

    const manifestFrom = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
    const manifestTo = new Date(Date.now() + 5 * 60 * 1000).toISOString();
    const manifestQuery = new URLSearchParams({
      action: 'workflow.ontology.created',
      aggregateId: ontologyId,
      from: manifestFrom,
      to: manifestTo,
      pageSize: '1',
    });
    const restManifest = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/audit-records/export/manifest?${manifestQuery.toString()}`,
      'GET',
      authenticatedHeaders(environment.readOnlyToken, 'c0:mcp:audit-manifest-rest'),
      undefined,
      200,
      credentials,
      'REST complete audit manifest'
    );
    const mcpManifest = await callTool(
      environment,
      environment.readOnlyToken,
      110,
      'a3s_cloud_audit_records_export_manifest',
      {
        action: 'workflow.ontology.created',
        aggregateId: ontologyId,
        from: manifestFrom,
        to: manifestTo,
        pageSize: 1,
      },
      credentials,
      'MCP complete audit manifest'
    );
    expect(mcpManifest.result.isError).toBe(false);
    for (const [label, value] of [
      ['REST complete audit manifest', restManifest.body.data],
      ['MCP complete audit manifest', mcpManifest.structured.data],
    ] as const) {
      const bundle = objectValue(value, label);
      const manifest = objectValue(bundle.manifest, `${label} envelope`);
      const envelope = objectValue(manifest.envelope, `${label} DSSE envelope`);
      expect(envelope.payloadType).toBe('application/vnd.a3s.cloud.audit-export-manifest.v1+json');
      expect(arrayValue(bundle.pages, `${label} pages`)).toHaveLength(1);
      expect(JSON.stringify(bundle)).not.toContain('details');
    }

    const restRetention = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/audit-records/retention`,
      'GET',
      authenticatedHeaders(environment.readOnlyToken, 'c0:mcp:audit-retention-read'),
      undefined,
      200,
      credentials,
      'REST audit retention status'
    );
    const retentionStatus = await callTool(
      environment,
      environment.readOnlyToken,
      109,
      'a3s_cloud_audit_retention_get',
      {},
      credentials,
      'MCP audit retention status'
    );
    expect(retentionStatus.result.isError).toBe(false);
    expect(retentionStatus.structured.data).toEqual(restRetention.body.data);
    const retentionData = objectValue(retentionStatus.structured.data, 'audit retention data');
    expect(retentionData.organizationId).toBe(organizationId);
    expect(typeof retentionData.retentionMs).toBe('number');
    expect(String(retentionData.policyDigest)).toMatch(/^sha256:[0-9a-f]{64}$/u);
    expect(typeof retentionData.currentPolicyApplied).toBe('boolean');
    expect(typeof retentionData.totalDeletedRecords).toBe('number');
    expect(typeof retentionData.version).toBe('number');

    const formEvidence = await proveFormConformance(environment, organizationId, projectId, credentials);
    const { formId, releaseId: formReleaseId } = formEvidence;
    const executionTemplateEvidence = await proveExecutionTemplateConformance(
      environment,
      organizationId,
      projectId,
      credentials
    );
    const {
      templateId: executionTemplateId,
      revisionId: executionTemplateRevisionId,
      definitionDigest: executionTemplateDefinitionDigest,
    } = executionTemplateEvidence;

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
      {
        id: 60,
        name: 'a3s_cloud_plugin_registries_list',
        arguments: {},
        label: 'MCP Plugin Registry listing',
      },
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
        id: 61,
        name: 'a3s_cloud_plugin_registries_get',
        arguments: { registryId: missingOperationalId },
        label: 'MCP missing Plugin Registry lookup',
      },
      {
        id: 62,
        name: 'a3s_cloud_plugin_catalog_search',
        arguments: {
          registryId: missingOperationalId,
          host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
          search: { query: 'a3s', limit: 20 },
        },
        label: 'MCP missing online Plugin catalog search',
      },
      {
        id: 63,
        name: 'a3s_cloud_plugin_catalog_search_cached',
        arguments: {
          registryId: missingOperationalId,
          host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
          search: { query: 'a3s', limit: 20 },
        },
        label: 'MCP missing cached Plugin catalog search',
      },
      {
        id: 64,
        name: 'a3s_cloud_plugin_catalog_inspect',
        arguments: {
          registryId: missingOperationalId,
          host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
          packageId: 'a3s/example',
        },
        label: 'MCP missing online Plugin catalog inspection',
      },
      {
        id: 65,
        name: 'a3s_cloud_plugin_catalog_inspect_cached',
        arguments: {
          registryId: missingOperationalId,
          host: { target: 'x86_64-unknown-linux-gnu', useVersion: '0.3.0' },
          packageId: 'a3s/example',
        },
        label: 'MCP missing cached Plugin catalog inspection',
      },
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
      {
        id: 34,
        name: 'a3s_cloud_workload_logs_get',
        arguments: { workloadId: missingOperationalId, revisionId: missingOperationalId },
        label: 'MCP missing workload log lookup',
      },
      {
        id: 35,
        name: 'a3s_cloud_build_run_logs_get',
        arguments: { buildRunId: missingOperationalId },
        label: 'MCP missing BuildRun log lookup',
      },
      {
        id: 36,
        name: 'a3s_cloud_build_evidence_get',
        arguments: { buildRunId: missingOperationalId },
        label: 'MCP missing build evidence lookup',
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

    const missingMutationRequestIds: Record<string, string> = {};
    for (const entry of [
      {
        id: 41,
        name: 'a3s_cloud_workloads_stop',
        arguments: {
          workloadId: missingOperationalId,
          idempotencyKey: 'c0:mcp:missing-workload-stop',
        },
        label: 'MCP missing Workload stop',
      },
      {
        id: 42,
        name: 'a3s_cloud_workloads_rollback',
        arguments: {
          workloadId: missingOperationalId,
          sourceRevisionId: missingOperationalId,
          idempotencyKey: 'c0:mcp:missing-workload-rollback',
        },
        label: 'MCP missing Workload rollback',
      },
      {
        id: 43,
        name: 'a3s_cloud_deployments_cancel',
        arguments: {
          deploymentId: missingOperationalId,
          idempotencyKey: 'c0:mcp:missing-deployment-cancel',
        },
        label: 'MCP missing Deployment cancellation',
      },
      {
        id: 44,
        name: 'a3s_cloud_build_runs_cancel',
        arguments: {
          buildRunId: missingOperationalId,
          idempotencyKey: 'c0:mcp:missing-build-run-cancel',
        },
        label: 'MCP missing BuildRun cancellation',
      },
      {
        id: 45,
        name: 'a3s_cloud_build_runs_retry',
        arguments: {
          buildRunId: missingOperationalId,
          idempotencyKey: 'c0:mcp:missing-build-run-retry',
        },
        label: 'MCP missing BuildRun retry',
      },
    ]) {
      const missing = await callTool(
        environment,
        environment.adminToken,
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
      missingMutationRequestIds[entry.name] = requestId(missing.structured, `${entry.label} request ID`);
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
      {
        id: 37,
        name: 'a3s_cloud_workload_logs_get',
        arguments: { workloadId: missingOperationalId, revisionId: missingOperationalId, limit: 0 },
      },
      {
        id: 38,
        name: 'a3s_cloud_build_run_logs_get',
        arguments: { buildRunId: missingOperationalId, limit: 257 },
      },
      {
        id: 39,
        name: 'a3s_cloud_build_run_logs_get',
        arguments: { buildRunId: missingOperationalId, cursor: '1' },
      },
      {
        id: 40,
        name: 'a3s_cloud_workload_logs_get',
        arguments: {
          workloadId: missingOperationalId,
          revisionId: missingOperationalId,
          stream: 'combined',
        },
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

    for (const entry of [
      {
        id: 46,
        name: 'a3s_cloud_workloads_stop',
        arguments: { workloadId: missingOperationalId },
        label: 'Workload stop without idempotency',
      },
      {
        id: 47,
        name: 'a3s_cloud_workloads_rollback',
        arguments: {
          workloadId: missingOperationalId,
          idempotencyKey: 'c0:mcp:rollback-without-source',
        },
        label: 'Workload rollback without source revision',
      },
      {
        id: 48,
        name: 'a3s_cloud_deployments_cancel',
        arguments: {
          deploymentId: missingOperationalId,
          idempotencyKey: 'c0:mcp:forged-deployment-tenant',
          organizationId: crypto.randomUUID(),
        },
        label: 'Deployment cancellation with forged organization',
      },
      {
        id: 49,
        name: 'a3s_cloud_build_runs_cancel',
        arguments: { buildRunId: missingOperationalId, idempotencyKey: '' },
        label: 'BuildRun cancellation with empty idempotency',
      },
      {
        id: 50,
        name: 'a3s_cloud_build_runs_retry',
        arguments: { idempotencyKey: 'c0:mcp:retry-without-build-run' },
        label: 'BuildRun retry without BuildRun',
      },
    ]) {
      const rejected = await mcpRequest(
        environment,
        environment.adminToken,
        toolCall(entry.id, entry.name, entry.arguments),
        200,
        credentials,
        entry.label
      );
      const error = objectValue(rejected.body.error, `${entry.label} error`);
      expect(error.code).toBe(-32602);
      expect(error.message).toBe('Invalid tool arguments');
    }

    const workloadAcl = `version = 1

workload "mcp-stop" {
  artifact {
    uri = "oci://registry.example.test/a3s/mcp-stop:conformance"
  }
  resources {
    cpu_millis = 100
    memory_bytes = 33554432
    pids = 32
  }
  port "http" {
    container_port = 8080
  }
  health {
    port_name = "http"
    path = "/health"
    interval_ms = 1000
    timeout_ms = 500
    healthy_threshold = 1
    unhealthy_threshold = 3
    stabilization_window_ms = 1000
  }
}
`;
    const workloadCreate = await restEnvelope(
      `${environment.baseUrl}/organizations/${organizationId}/projects/${projectId}/environments/${environmentId}/workloads`,
      'POST',
      {
        ...authenticatedHeaders(environment.adminToken, 'c0:mcp:workload-create'),
        'content-type': 'application/vnd.a3s.acl',
      },
      workloadAcl,
      202,
      credentials,
      'REST ACL Workload creation'
    );
    const workloadCreateData = objectValue(workloadCreate.body.data, 'REST Workload creation data');
    const workloadId = uuidValue(workloadCreateData.workloadId, 'REST Workload ID');

    const workloadStop = await callTool(
      environment,
      environment.adminToken,
      51,
      'a3s_cloud_workloads_stop',
      { workloadId, idempotencyKey: 'c0:mcp:workload-stop' },
      credentials,
      'MCP Workload stop'
    );
    expect(workloadStop.result.isError).toBe(false);
    expect(workloadStop.structured.code).toBe(202);
    const workloadStopData = objectValue(workloadStop.structured.data, 'MCP Workload stop data');
    expect(workloadStopData.workloadId).toBe(workloadId);
    expect(workloadStopData.replayed).toBe(false);

    const workloadStopReplay = await callTool(
      environment,
      environment.adminToken,
      59,
      'a3s_cloud_workloads_stop',
      { workloadId, idempotencyKey: 'c0:mcp:workload-stop' },
      credentials,
      'MCP Workload stop replay'
    );
    expect(workloadStopReplay.result.isError).toBe(false);
    expect(workloadStopReplay.structured.code).toBe(200);
    const workloadStopReplayData = objectValue(
      workloadStopReplay.structured.data,
      'MCP Workload stop replay data'
    );
    expect(workloadStopReplayData.workloadId).toBe(workloadId);
    expect(workloadStopReplayData.replayed).toBe(true);

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
    const executionTemplateNondisclosure = await proveExecutionTemplateNondisclosure(
      environment,
      foreignProjectId,
      credentials
    );

    const foreignOntologyAcl = await Bun.file('../../contracts/w0.1/ontology.acl').text();
    const foreignOntology = await restEnvelope(
      `${environment.baseUrl}/organizations/${foreignOrganizationId}/projects/${foreignProjectId}/ontologies`,
      'POST',
      {
        ...authenticatedHeaders(environment.adminToken, 'c0:mcp:foreign-ontology'),
        'content-type': 'application/vnd.a3s.acl',
      },
      foreignOntologyAcl,
      201,
      credentials,
      'REST foreign Ontology creation'
    );
    const foreignOntologyData = objectValue(foreignOntology.body.data, 'REST foreign Ontology data');
    const foreignOntologyId = uuidValue(
      objectValue(foreignOntologyData.ontology, 'REST foreign Ontology aggregate').id,
      'REST foreign Ontology ID'
    );

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

    const foreignOntologyResult = await callTool(
      environment,
      environment.adminToken,
      121,
      'a3s_cloud_ontologies_get',
      { ontologyId: foreignOntologyId },
      credentials,
      'MCP foreign-Ontology lookup'
    );
    const missingOntologyResult = await callTool(
      environment,
      environment.adminToken,
      122,
      'a3s_cloud_ontologies_get',
      { ontologyId: crypto.randomUUID() },
      credentials,
      'MCP missing-Ontology lookup'
    );
    expect(foreignOntologyResult.result.isError).toBe(true);
    expect(missingOntologyResult.result.isError).toBe(true);
    const foreignOntologyErrorContract = businessErrorContract(foreignOntologyResult.structured);
    expect(foreignOntologyErrorContract).toEqual(businessErrorContract(missingOntologyResult.structured));
    expect(foreignOntologyErrorContract.code).toBe(404);
    expect(foreignOntologyErrorContract.statusCode).toBe('NOT_FOUND');
    expect(JSON.stringify(foreignOntologyResult.structured)).not.toContain(foreignOntologyId);
    expect(JSON.stringify(foreignOntologyResult.structured)).not.toContain('Support');

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
      schema: 'a3s.cloud.c0-management-mcp.evidence.v8',
      cloudRevision: environment.cloudRevision,
      apiContractVersion: CLOUD_API_CONTRACT_VERSION,
      mcpProtocolVersion: MCP_PROTOCOL_VERSION,
      persistence: 'postgresql-17-through-a3s-orm',
      surfaces: ['rest', 'management-mcp'],
      resources: {
        organizationId,
        projectId,
        ontologyId,
        firstOntologyRevisionId,
        secondOntologyRevisionId,
        formId,
        formReleaseId,
        executionTemplateId,
        executionTemplateRevisionId,
        executionTemplateDefinitionDigest,
        environmentId,
        workloadId,
        foreignOrganizationId,
        foreignProjectId,
        foreignOntologyId,
        readOnlyTokenId,
      },
      catalogs: { administrator: adminToolNames, readOnly: readOnlyToolNames },
      requestIds: {
        bootstrap: requestId(bootstrap.body, 'bootstrap request ID'),
        readOnlyTokenCreate: requestId(readOnlyToken.body, 'token-create request ID'),
        restProjectCreate: requestId(restProject.body, 'REST project request ID'),
        mcpProjectReplay: requestId(projectReplay.structured, 'MCP replay request ID'),
        restOntologyCreate: ontologyEvidence.requestIds.restCreate,
        mcpOntologyCreateReplay: ontologyEvidence.requestIds.mcpCreateReplay,
        mcpOntologyCompatibleRevision: ontologyEvidence.requestIds.mcpCompatibleRevision,
        mcpOntologyExplicitMigration: ontologyEvidence.requestIds.mcpExplicitMigration,
        restFormCreate: formEvidence.requestIds.restCreate,
        mcpFormCreateReplay: formEvidence.requestIds.mcpCreateReplay,
        mcpFormRevise: formEvidence.requestIds.mcpRevise,
        mcpFormPublish: formEvidence.requestIds.mcpPublish,
        mcpFormPublishReplay: formEvidence.requestIds.mcpPublishReplay,
        restExecutionTemplateCreate: executionTemplateEvidence.requestIds.restCreate,
        mcpExecutionTemplateCreateReplay: executionTemplateEvidence.requestIds.mcpCreateReplay,
        mcpExecutionTemplateList: executionTemplateEvidence.requestIds.mcpList,
        mcpExecutionTemplateGet: executionTemplateEvidence.requestIds.mcpGet,
        mcpExecutionTemplateRejectedAcl: executionTemplateEvidence.requestIds.mcpRejectedAcl,
        restEnvironmentCreate: requestId(restEnvironment.body, 'REST environment request ID'),
        restWorkloadCreate: requestId(workloadCreate.body, 'REST Workload request ID'),
        mcpWorkloadStop: requestId(workloadStop.structured, 'MCP Workload stop request ID'),
        mcpWorkloadStopReplay: requestId(
          workloadStopReplay.structured,
          'MCP Workload stop replay request ID'
        ),
        operationalLists: operationalListRequestIds,
        missingOperationalResources: missingOperationalRequestIds,
        missingOperationalMutations: missingMutationRequestIds,
        foreignProjectDenial: requestId(foreignProjectResult.structured, 'foreign-project denial request ID'),
        missingProjectDenial: requestId(missingProjectResult.structured, 'missing-project denial request ID'),
        foreignOntologyDenial: requestId(
          foreignOntologyResult.structured,
          'foreign-Ontology denial request ID'
        ),
        missingOntologyDenial: requestId(
          missingOntologyResult.structured,
          'missing-Ontology denial request ID'
        ),
        foreignExecutionTemplateProjectDenial: executionTemplateNondisclosure.foreignProjectDenial,
        missingExecutionTemplateProjectDenial: executionTemplateNondisclosure.missingProjectDenial,
        tokenRevocation: requestId(revoked.body, 'token-revocation request ID'),
        revokedToken: requestId(revokedRequest.body, 'revoked-token request ID'),
      },
      checks: [
        'modern-per-request-metadata-and-header-validation',
        'server-discovery-and-version-negotiation',
        'legacy-initialize-removal',
        'legacy-session-identifier-ignored-without-state',
        'scope-derived-tool-catalogs',
        'hidden-mutation-denial-without-side-effect',
        'rest-to-mcp-idempotency-replay',
        'acl-native-versioned-ontology-lifecycle',
        'ontology-migration-and-historical-replay',
        'native-form-draft-release-lifecycle',
        'form-rest-to-mcp-replay-and-historical-replay',
        'immutable-execution-template-rest-to-mcp-replay',
        'execution-template-exact-read-and-acl-rejection',
        'execution-template-cross-tenant-nondisclosure',
        'operational-read-query-catalog',
        'bounded-operational-query-arguments',
        'paged-log-and-evidence-query-boundaries',
        'replay-safe-operational-mutation-catalog',
        'strict-operational-mutation-arguments',
        'mcp-operational-mutation-idempotency-replay',
        'principal-derived-tenant-context',
        'foreign-and-missing-resource-error-equivalence',
        'ontology-cross-tenant-nondisclosure',
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
