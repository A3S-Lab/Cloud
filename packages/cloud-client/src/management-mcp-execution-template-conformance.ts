import { expect } from 'bun:test';
import {
  arrayValue,
  authenticatedHeaders,
  businessErrorContract,
  callTool,
  type ConformanceEnvironment,
  objectValue,
  requestId,
  restEnvelope,
  uuidValue,
} from './management-mcp-conformance-support';

export const EXECUTION_TEMPLATE_CREATE_IDEMPOTENCY_KEY = 'c0:mcp:rest-execution-template';
export const EXECUTION_TEMPLATE_REJECTED_IDEMPOTENCY_KEY = 'c0:mcp:execution-template-rejected';

export interface ExecutionTemplateConformanceEvidence {
  templateId: string;
  revisionId: string;
  definitionDigest: string;
  requestIds: {
    restCreate: string;
    mcpCreateReplay: string;
    mcpList: string;
    mcpGet: string;
    mcpRejectedAcl: string;
  };
}

export interface ExecutionTemplateNondisclosureEvidence {
  foreignProjectDenial: string;
  missingProjectDenial: string;
}

export async function proveExecutionTemplateConformance(
  environment: ConformanceEnvironment,
  organizationId: string,
  projectId: string,
  credentials: readonly string[]
): Promise<ExecutionTemplateConformanceEvidence> {
  const definitionAcl = await Bun.file('../../contracts/w0.3/execution-template.acl').text();
  const restCreate = await restEnvelope(
    `${environment.baseUrl}/organizations/${organizationId}/projects/${projectId}/execution-templates`,
    'POST',
    authenticatedHeaders(environment.adminToken, EXECUTION_TEMPLATE_CREATE_IDEMPOTENCY_KEY),
    { definitionAcl },
    201,
    credentials,
    'REST ExecutionTemplate publication'
  );
  const restData = objectValue(restCreate.body.data, 'REST ExecutionTemplate mutation data');
  const restTemplate = objectValue(restData.executionTemplate, 'REST published ExecutionTemplate revision');
  const templateId = uuidValue(restTemplate.templateId, 'REST ExecutionTemplate ID');
  const revisionId = uuidValue(restTemplate.revisionId, 'REST ExecutionTemplate revision ID');
  const definitionDigest = stringValue(
    restTemplate.definitionDigest,
    'REST ExecutionTemplate definition digest'
  );
  const canonicalAcl = stringValue(restTemplate.definitionAcl, 'REST ExecutionTemplate canonical ACL');
  expect(restData.replayed).toBe(false);
  expect(restTemplate.organizationId).toBe(organizationId);
  expect(restTemplate.projectId).toBe(projectId);
  expect(restTemplate.capability).toBe('execution.run');
  expect(definitionDigest).toMatch(/^sha256:[0-9a-f]{64}$/);

  const createReplay = await callTool(
    environment,
    environment.adminToken,
    140,
    'a3s_cloud_execution_templates_create',
    {
      projectId,
      definitionAcl,
      idempotencyKey: EXECUTION_TEMPLATE_CREATE_IDEMPOTENCY_KEY,
    },
    credentials,
    'MCP ExecutionTemplate publication replay'
  );
  expect(createReplay.result.isError).toBe(false);
  expect(createReplay.structured.code).toBe(200);
  const replayData = objectValue(createReplay.structured.data, 'MCP ExecutionTemplate replay data');
  const replayedTemplate = objectValue(
    replayData.executionTemplate,
    'MCP replayed ExecutionTemplate revision'
  );
  expect(replayData.replayed).toBe(true);
  expect(replayedTemplate.templateId).toBe(templateId);
  expect(replayedTemplate.revisionId).toBe(revisionId);
  expect(replayedTemplate.definitionDigest).toBe(definitionDigest);
  expect(replayedTemplate.definitionAcl).toBe(canonicalAcl);

  const listed = await callTool(
    environment,
    environment.readOnlyToken,
    141,
    'a3s_cloud_execution_templates_list',
    { projectId },
    credentials,
    'read-only MCP ExecutionTemplate listing'
  );
  expect(listed.result.isError).toBe(false);
  const revisions = arrayValue(listed.structured.data, 'MCP ExecutionTemplate list data');
  expect(revisions).toHaveLength(1);
  const listedTemplate = objectValue(revisions[0], 'listed MCP ExecutionTemplate revision');
  expect(listedTemplate.templateId).toBe(templateId);
  expect(listedTemplate.revisionId).toBe(revisionId);
  expect(listedTemplate.definitionDigest).toBe(definitionDigest);
  expect(listedTemplate.capability).toBe('execution.run');

  const fetched = await callTool(
    environment,
    environment.readOnlyToken,
    142,
    'a3s_cloud_execution_templates_get',
    { projectId, templateId, revisionId },
    credentials,
    'read-only MCP exact ExecutionTemplate lookup'
  );
  expect(fetched.result.isError).toBe(false);
  const fetchedTemplate = objectValue(fetched.structured.data, 'MCP ExecutionTemplate data');
  expect(fetchedTemplate.templateId).toBe(templateId);
  expect(fetchedTemplate.revisionId).toBe(revisionId);
  expect(fetchedTemplate.definitionAcl).toBe(canonicalAcl);
  expect(fetchedTemplate.definitionDigest).toBe(definitionDigest);

  const rejected = await callTool(
    environment,
    environment.adminToken,
    143,
    'a3s_cloud_execution_templates_create',
    {
      projectId,
      definitionAcl: definitionAcl.replace(
        'description = "Runs one bounded Workflow release check"',
        'description = "Runs one bounded Workflow release check"\n  unsupported = true'
      ),
      idempotencyKey: EXECUTION_TEMPLATE_REJECTED_IDEMPOTENCY_KEY,
    },
    credentials,
    'MCP unknown ExecutionTemplate ACL field rejection'
  );
  expect(rejected.result.isError).toBe(true);
  const rejectedContract = businessErrorContract(rejected.structured);
  expect(rejectedContract.code).toBe(422);
  expect(rejectedContract.statusCode).toBe('UNPROCESSABLE_ENTITY');

  return {
    templateId,
    revisionId,
    definitionDigest,
    requestIds: {
      restCreate: requestId(restCreate.body, 'REST ExecutionTemplate request ID'),
      mcpCreateReplay: requestId(createReplay.structured, 'MCP ExecutionTemplate replay request ID'),
      mcpList: requestId(listed.structured, 'MCP ExecutionTemplate list request ID'),
      mcpGet: requestId(fetched.structured, 'MCP ExecutionTemplate get request ID'),
      mcpRejectedAcl: requestId(rejected.structured, 'MCP rejected ExecutionTemplate request ID'),
    },
  };
}

export async function proveExecutionTemplateNondisclosure(
  environment: ConformanceEnvironment,
  foreignProjectId: string,
  credentials: readonly string[]
): Promise<ExecutionTemplateNondisclosureEvidence> {
  const missingProjectId = crypto.randomUUID();
  const foreign = await callTool(
    environment,
    environment.adminToken,
    144,
    'a3s_cloud_execution_templates_list',
    { projectId: foreignProjectId },
    credentials,
    'MCP foreign-project ExecutionTemplate listing'
  );
  const missing = await callTool(
    environment,
    environment.adminToken,
    150,
    'a3s_cloud_execution_templates_list',
    { projectId: missingProjectId },
    credentials,
    'MCP missing-project ExecutionTemplate listing'
  );
  expect(foreign.result.isError).toBe(true);
  expect(missing.result.isError).toBe(true);
  const foreignContract = businessErrorContract(foreign.structured);
  expect(foreignContract).toEqual(businessErrorContract(missing.structured));
  expect(foreignContract.code).toBe(404);
  expect(foreignContract.statusCode).toBe('NOT_FOUND');
  expect(JSON.stringify(foreign.structured)).not.toContain(foreignProjectId);
  expect(JSON.stringify(missing.structured)).not.toContain(missingProjectId);
  return {
    foreignProjectDenial: requestId(
      foreign.structured,
      'foreign-project ExecutionTemplate denial request ID'
    ),
    missingProjectDenial: requestId(
      missing.structured,
      'missing-project ExecutionTemplate denial request ID'
    ),
  };
}

function stringValue(value: unknown, label: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}
