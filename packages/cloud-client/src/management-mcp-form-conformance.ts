import { expect } from 'bun:test';
import {
  arrayValue,
  authenticatedHeaders,
  callTool,
  type ConformanceEnvironment,
  objectValue,
  requestId,
  restEnvelope,
  uuidValue,
} from './management-mcp-conformance-support';

export interface FormConformanceEvidence {
  formId: string;
  releaseId: string;
  requestIds: {
    restCreate: string;
    mcpCreateReplay: string;
    mcpRevise: string;
    mcpPublish: string;
    mcpPublishReplay: string;
  };
}

export async function proveFormConformance(
  environment: ConformanceEnvironment,
  organizationId: string,
  projectId: string,
  credentials: readonly string[]
): Promise<FormConformanceEvidence> {
  const createInput = formDraftInput('MCP Approval', 'C0 Management MCP Form', false);
  const createIdempotencyKey = 'c0:mcp:rest-form';
  const restCreate = await restEnvelope(
    `${environment.baseUrl}/organizations/${organizationId}/projects/${projectId}/forms`,
    'POST',
    authenticatedHeaders(environment.adminToken, createIdempotencyKey),
    createInput,
    201,
    credentials,
    'REST Form creation'
  );
  const restCreateData = objectValue(restCreate.body.data, 'REST Form mutation data');
  const restForm = objectValue(restCreateData.form, 'REST Form draft');
  const formId = uuidValue(restForm.id, 'REST Form ID');
  expect(restForm.aggregateVersion).toBe(1);

  const createReplay = await callTool(
    environment,
    environment.adminToken,
    130,
    'a3s_cloud_forms_create',
    { projectId, ...createInput, idempotencyKey: createIdempotencyKey },
    credentials,
    'MCP Form create replay'
  );
  expect(createReplay.result.isError).toBe(false);
  expect(createReplay.structured.code).toBe(200);
  const createReplayData = objectValue(createReplay.structured.data, 'MCP Form create replay data');
  expect(createReplayData.replayed).toBe(true);
  expect(objectValue(createReplayData.form, 'MCP replayed Form').id).toBe(formId);

  const listedDrafts = await callTool(
    environment,
    environment.readOnlyToken,
    131,
    'a3s_cloud_forms_list',
    { projectId },
    credentials,
    'read-only MCP Form listing'
  );
  const drafts = arrayValue(listedDrafts.structured.data, 'MCP Form list data');
  expect(drafts).toHaveLength(1);
  expect(objectValue(drafts[0], 'listed MCP Form').id).toBe(formId);
  const fetchedDraft = await callTool(
    environment,
    environment.readOnlyToken,
    132,
    'a3s_cloud_forms_get',
    { formId },
    credentials,
    'read-only MCP Form lookup'
  );
  expect(objectValue(fetchedDraft.structured.data, 'MCP Form data').name).toBe('MCP Approval');

  const revisedInput = formDraftInput(
    'MCP Approval request',
    'C0 Management MCP Form with a required reason',
    true
  );
  const reviseIdempotencyKey = 'c0:mcp:form-revise';
  const revised = await callTool(
    environment,
    environment.adminToken,
    133,
    'a3s_cloud_forms_revise',
    {
      formId,
      ...revisedInput,
      expectedVersion: 1,
      idempotencyKey: reviseIdempotencyKey,
    },
    credentials,
    'MCP Form revision'
  );
  expect(revised.result.isError).toBe(false);
  expect(revised.structured.code).toBe(201);
  const revisedData = objectValue(revised.structured.data, 'MCP Form revision data');
  expect(objectValue(revisedData.form, 'MCP revised Form').aggregateVersion).toBe(2);

  const publishIdempotencyKey = 'c0:mcp:form-publish';
  const publishArguments = { formId, expectedVersion: 2, idempotencyKey: publishIdempotencyKey };
  const published = await callTool(
    environment,
    environment.adminToken,
    134,
    'a3s_cloud_form_releases_publish',
    publishArguments,
    credentials,
    'MCP Form release publication'
  );
  expect(published.result.isError).toBe(false);
  expect(published.structured.code).toBe(201);
  const publishedData = objectValue(published.structured.data, 'MCP Form publication data');
  const publishedForm = objectValue(publishedData.form, 'published MCP Form');
  const release = objectValue(publishedData.release, 'published MCP Form release');
  const releaseId = uuidValue(release.id, 'published MCP Form release ID');
  expect(publishedForm.aggregateVersion).toBe(3);
  expect(release.sourceDraftVersion).toBe(2);
  expect(release.compilerRevision).toBe('a3s-form-core@0.1.0');

  const publishReplay = await callTool(
    environment,
    environment.adminToken,
    135,
    'a3s_cloud_form_releases_publish',
    publishArguments,
    credentials,
    'MCP Form release publication replay'
  );
  expect(publishReplay.result.isError).toBe(false);
  expect(publishReplay.structured.code).toBe(200);
  const publishReplayData = objectValue(publishReplay.structured.data, 'MCP Form publication replay data');
  expect(publishReplayData.replayed).toBe(true);
  expect(objectValue(publishReplayData.release, 'replayed MCP Form release').id).toBe(releaseId);

  const historicalRevisionReplay = await callTool(
    environment,
    environment.adminToken,
    136,
    'a3s_cloud_forms_revise',
    {
      formId,
      ...revisedInput,
      expectedVersion: 1,
      idempotencyKey: reviseIdempotencyKey,
    },
    credentials,
    'MCP historical Form revision replay'
  );
  const historicalRevisionData = objectValue(
    historicalRevisionReplay.structured.data,
    'historical MCP Form revision data'
  );
  expect(historicalRevisionData.replayed).toBe(true);
  expect(objectValue(historicalRevisionData.form, 'historical revised MCP Form').aggregateVersion).toBe(2);

  const listedReleases = await callTool(
    environment,
    environment.readOnlyToken,
    137,
    'a3s_cloud_form_releases_list',
    { formId },
    credentials,
    'read-only MCP Form release listing'
  );
  const releases = arrayValue(listedReleases.structured.data, 'MCP Form release list data');
  expect(releases).toHaveLength(1);
  expect(objectValue(releases[0], 'listed MCP Form release').id).toBe(releaseId);
  const fetchedRelease = await callTool(
    environment,
    environment.readOnlyToken,
    138,
    'a3s_cloud_form_releases_get',
    { formId, releaseId },
    credentials,
    'read-only MCP Form release lookup'
  );
  expect(objectValue(fetchedRelease.structured.data, 'MCP Form release data').id).toBe(releaseId);

  return {
    formId,
    releaseId,
    requestIds: {
      restCreate: requestId(restCreate.body, 'REST Form create request ID'),
      mcpCreateReplay: requestId(createReplay.structured, 'MCP Form create replay request ID'),
      mcpRevise: requestId(revised.structured, 'MCP Form revise request ID'),
      mcpPublish: requestId(published.structured, 'MCP Form publish request ID'),
      mcpPublishReplay: requestId(publishReplay.structured, 'MCP Form publish replay request ID'),
    },
  };
}

function formDraftInput(title: string, description: string, requireReason: boolean): Record<string, unknown> {
  return {
    name: title,
    description,
    document: {
      kind: 'a3s.form',
      apiVersion: 'a3s.dev/form/v1alpha1',
      revision: 1,
      metadata: { title },
      schema: {
        $schema: 'https://json-schema.org/draft/2020-12/schema',
        type: 'object',
        properties: {
          approved: { type: 'boolean' },
          reason: { type: 'string' },
        },
        required: requireReason ? ['approved', 'reason'] : ['approved'],
        additionalProperties: false,
      },
      ui: {
        root: 'root',
        nodes: [
          { id: 'root', kind: 'root', children: ['approved', 'reason'] },
          {
            id: 'approved',
            kind: 'field',
            schemaPath: '/properties/approved',
            widget: 'switch',
          },
          {
            id: 'reason',
            kind: 'field',
            schemaPath: '/properties/reason',
            widget: 'textarea',
          },
        ],
      },
      rules: [],
      dataSources: [],
      actions: [],
    },
  };
}
