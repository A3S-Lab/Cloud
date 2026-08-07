import { expect } from 'bun:test';
import {
  arrayValue,
  authenticatedHeaders,
  type ConformanceEnvironment,
  callTool,
  objectValue,
  requestId,
  restEnvelope,
  uuidValue,
} from './management-mcp-conformance-support';

export interface OntologyConformanceEvidence {
  ontologyId: string;
  firstRevisionId: string;
  secondRevisionId: string;
  requestIds: {
    restCreate: string;
    mcpCreateReplay: string;
    mcpCompatibleRevision: string;
    mcpExplicitMigration: string;
  };
}

export async function proveOntologyConformance(
  environment: ConformanceEnvironment,
  organizationId: string,
  projectId: string,
  credentials: readonly string[]
): Promise<OntologyConformanceEvidence> {
  const ontologyAcl = await Bun.file('../../contracts/w0.1/ontology.acl').text();
  const createIdempotencyKey = 'c0:mcp:rest-ontology';
  const restCreate = await restEnvelope(
    `${environment.baseUrl}/organizations/${organizationId}/projects/${projectId}/ontologies`,
    'POST',
    {
      ...authenticatedHeaders(environment.adminToken, createIdempotencyKey),
      'content-type': 'application/vnd.a3s.acl',
    },
    ontologyAcl,
    201,
    credentials,
    'REST Ontology creation'
  );
  const restCreateData = objectValue(restCreate.body.data, 'REST Ontology data');
  const aggregate = objectValue(restCreateData.ontology, 'REST Ontology aggregate');
  const initialRevision = objectValue(restCreateData.revision, 'REST Ontology revision');
  const ontologyId = uuidValue(aggregate.id, 'REST Ontology ID');
  const firstRevisionId = uuidValue(initialRevision.id, 'REST initial Ontology revision ID');

  const createReplay = await callTool(
    environment,
    environment.adminToken,
    110,
    'a3s_cloud_ontologies_create',
    { projectId, acl: ontologyAcl, idempotencyKey: createIdempotencyKey },
    credentials,
    'MCP Ontology create replay'
  );
  expect(createReplay.result.isError).toBe(false);
  expect(createReplay.structured.code).toBe(200);
  const createReplayData = objectValue(createReplay.structured.data, 'MCP Ontology create replay data');
  expect(createReplayData.replayed).toBe(true);

  const list = await callTool(
    environment,
    environment.readOnlyToken,
    111,
    'a3s_cloud_ontologies_list',
    { projectId },
    credentials,
    'read-only MCP Ontology listing'
  );
  expect(arrayValue(list.structured.data, 'MCP Ontology list data')).toHaveLength(1);
  const get = await callTool(
    environment,
    environment.readOnlyToken,
    112,
    'a3s_cloud_ontologies_get',
    { ontologyId },
    credentials,
    'read-only MCP Ontology lookup'
  );
  expect(objectValue(get.structured.data, 'MCP Ontology data').id).toBe(ontologyId);

  const compatibleAcl = ontologyAcl.replace(
    'Deterministic W0.1 Ontology contract fixture',
    'C0 Management MCP compatible revision'
  );
  const compatibleIdempotencyKey = 'c0:mcp:ontology-compatible';
  const compatibleRevision = await callTool(
    environment,
    environment.adminToken,
    113,
    'a3s_cloud_ontologies_revise',
    {
      ontologyId,
      acl: compatibleAcl,
      expectedVersion: 1,
      idempotencyKey: compatibleIdempotencyKey,
    },
    credentials,
    'MCP compatible Ontology revision'
  );
  expect(compatibleRevision.result.isError).toBe(false);
  expect(compatibleRevision.structured.code).toBe(201);
  const compatibleData = objectValue(compatibleRevision.structured.data, 'compatible Ontology mutation data');
  const compatibleRevisionData = objectValue(compatibleData.revision, 'compatible Ontology revision data');
  const secondRevisionId = uuidValue(compatibleRevisionData.id, 'compatible Ontology revision ID');
  expect(objectValue(compatibleRevisionData.migrationPolicy, 'compatible migration policy').kind).toBe(
    'compatible'
  );

  const revisions = await callTool(
    environment,
    environment.readOnlyToken,
    114,
    'a3s_cloud_ontology_revisions_list',
    { ontologyId },
    credentials,
    'read-only MCP Ontology revision listing'
  );
  expect(arrayValue(revisions.structured.data, 'MCP Ontology revision list data')).toHaveLength(2);
  const revision = await callTool(
    environment,
    environment.readOnlyToken,
    115,
    'a3s_cloud_ontology_revisions_get',
    { ontologyId, revisionId: secondRevisionId },
    credentials,
    'read-only MCP Ontology revision lookup'
  );
  expect(objectValue(revision.structured.data, 'MCP Ontology revision data').canonicalAcl).toBe(
    compatibleAcl
  );
  const diff = await callTool(
    environment,
    environment.readOnlyToken,
    116,
    'a3s_cloud_ontology_revisions_diff',
    {
      ontologyId,
      fromRevisionId: firstRevisionId,
      toRevisionId: secondRevisionId,
    },
    credentials,
    'read-only MCP Ontology revision diff'
  );
  expect(objectValue(diff.structured.data, 'MCP Ontology diff data').breaking).toBe(false);

  const breakingAcl = breakingOntologyMigrationAcl(compatibleAcl);
  const rejectedMigration = await callTool(
    environment,
    environment.adminToken,
    117,
    'a3s_cloud_ontologies_revise',
    {
      ontologyId,
      acl: breakingAcl,
      expectedVersion: 2,
      idempotencyKey: 'c0:mcp:ontology-breaking-rejected',
    },
    credentials,
    'MCP breaking Ontology revision without migration rule'
  );
  expect(rejectedMigration.result.isError).toBe(true);
  expect(rejectedMigration.structured.code).toBe(422);

  const explicitMigration = await callTool(
    environment,
    environment.adminToken,
    118,
    'a3s_cloud_ontologies_revise',
    {
      ontologyId,
      acl: breakingAcl,
      expectedVersion: 2,
      migrationRuleId: 'migrate_ticket_v2',
      idempotencyKey: 'c0:mcp:ontology-breaking',
    },
    credentials,
    'MCP explicit Ontology migration'
  );
  expect(explicitMigration.result.isError).toBe(false);
  expect(explicitMigration.structured.code).toBe(201);

  const historicalCreateReplay = await callTool(
    environment,
    environment.adminToken,
    119,
    'a3s_cloud_ontologies_create',
    { projectId, acl: ontologyAcl, idempotencyKey: createIdempotencyKey },
    credentials,
    'MCP historical Ontology create replay'
  );
  const historicalCreateData = objectValue(
    historicalCreateReplay.structured.data,
    'historical Ontology create replay data'
  );
  expect(
    objectValue(historicalCreateData.ontology, 'historical Ontology aggregate').currentRevisionNumber
  ).toBe(1);
  const historicalRevisionReplay = await callTool(
    environment,
    environment.adminToken,
    120,
    'a3s_cloud_ontologies_revise',
    {
      ontologyId,
      acl: compatibleAcl,
      expectedVersion: 1,
      idempotencyKey: compatibleIdempotencyKey,
    },
    credentials,
    'MCP historical Ontology revision replay'
  );
  const historicalRevisionData = objectValue(
    historicalRevisionReplay.structured.data,
    'historical Ontology revision replay data'
  );
  expect(
    objectValue(historicalRevisionData.ontology, 'historical revised Ontology aggregate')
      .currentRevisionNumber
  ).toBe(2);

  return {
    ontologyId,
    firstRevisionId,
    secondRevisionId,
    requestIds: {
      restCreate: requestId(restCreate.body, 'REST Ontology request ID'),
      mcpCreateReplay: requestId(createReplay.structured, 'MCP Ontology create replay request ID'),
      mcpCompatibleRevision: requestId(
        compatibleRevision.structured,
        'MCP compatible Ontology revision request ID'
      ),
      mcpExplicitMigration: requestId(
        explicitMigration.structured,
        'MCP explicit Ontology migration request ID'
      ),
    },
  };
}

function breakingOntologyMigrationAcl(compatibleAcl: string): string {
  const changed = compatibleAcl.replace(
    'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    'sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'
  );
  if (!changed.endsWith('}\n')) {
    throw new Error('public Ontology fixture must end with its root block');
  }
  return `${changed.slice(0, -2)}
  rule "migrate_ticket_v2" {
    label = "Migrate ticket v2"
    kind = "migration"
    expression_digest = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
  }
}
`;
}
