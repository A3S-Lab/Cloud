import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_ONTOLOGY_ACL_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ONTOLOGY_ID = '019c0000-0000-7000-8000-000000000003';
const FIRST_REVISION_ID = '019c0000-0000-7000-8000-000000000004';
const SECOND_REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000006';
const DIGEST_A = `sha256:${'a'.repeat(64)}`;
const DIGEST_B = `sha256:${'b'.repeat(64)}`;
const ACL = 'ontology "support" { schema = "cloud.workflow.ontology.v1" }\n';

describe('a3s-cloud Ontology commands', () => {
  it.each([
    [
      ['ontologies', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/ontologies`,
      [ontology()],
    ],
    [
      ['ontologies', 'get', ONTOLOGY_ID],
      `/organizations/${ORGANIZATION_ID}/ontologies/${ONTOLOGY_ID}`,
      ontology(),
    ],
    [
      ['ontologies', 'revisions', ONTOLOGY_ID],
      `/organizations/${ORGANIZATION_ID}/ontologies/${ONTOLOGY_ID}/revisions`,
      [revision()],
    ],
    [
      ['ontologies', 'revision', ONTOLOGY_ID, FIRST_REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/ontologies/${ONTOLOGY_ID}/revisions/${FIRST_REVISION_ID}`,
      revision(),
    ],
    [
      ['ontologies', 'diff', ONTOLOGY_ID, FIRST_REVISION_ID, SECOND_REVISION_ID],
      `/organizations/${ORGANIZATION_ID}/ontologies/${ONTOLOGY_ID}` +
        `/revisions/${FIRST_REVISION_ID}/diff/${SECOND_REVISION_ID}`,
      diff(),
    ],
  ] as const)('queries the authoritative Ontology lifecycle %#', async (argv, path, data) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli([...argv, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(data);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it('creates one bounded ACL-native Ontology through the shared transport', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'ontologies',
        'create',
        '--file=ontology.acl',
        '--idempotency-key=cli:ontology:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('ontology.acl');
          return new TextEncoder().encode(ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope(mutation(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` + `/projects/${PROJECT_ID}/ontologies`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: ACL,
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:ontology:create',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('revises through optimistic concurrency and one target ACL migration rule', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'ontologies',
        'revise',
        ONTOLOGY_ID,
        '--file=ontology.acl',
        '--expected-version=1',
        '--migration-rule=migrate_ticket_v2',
        '--idempotency-key=cli:ontology:revise',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode(ACL),
        fetch: async (...args) => {
          calls.push(args);
          return envelope(mutation(), 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` + `/ontologies/${ONTOLOGY_ID}/revisions`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: ACL,
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:ontology:revise',
          'x-a3s-expected-version': '1',
          'x-a3s-migration-rule': 'migrate_ticket_v2',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects invalid or oversized revision input before transport', async () => {
    let called = false;
    const output = capture();
    const dependencies = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async () => {
        called = true;
        return envelope({});
      },
      readFile: async () => new Uint8Array(MAX_ONTOLOGY_ACL_BYTES + 1),
    };

    const oversized = await runCli(
      [
        'ontologies',
        'revise',
        ONTOLOGY_ID,
        '--file=ontology.acl',
        '--expected-version=1',
        '--idempotency-key=cli:ontology:oversized',
      ],
      dependencies
    );
    const invalidRule = await runCli(
      [
        'ontologies',
        'revise',
        ONTOLOGY_ID,
        '--file=ontology.acl',
        '--expected-version=1',
        '--migration-rule=not/a/rule',
        '--idempotency-key=cli:ontology:invalid-rule',
      ],
      dependencies
    );

    expect(oversized).toBe(ExitCode.Usage);
    expect(invalidRule).toBe(ExitCode.Usage);
    expect(called).toBe(false);
    expect(output.stderr()).toContain('Ontology ACL must contain between');
    expect(output.stderr()).toContain('portable Ontology migration rule ID');
  });
});

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-07T00:00:00.000Z',
    }),
    { status }
  );
}

function capture() {
  let stdout = '';
  let stderr = '';
  return {
    runtime: {
      writeStdout: (value: string) => {
        stdout += value;
      },
      writeStderr: (value: string) => {
        stderr += value;
      },
    },
    stdout: () => stdout,
    stderr: () => stderr,
  };
}

function completeEnvironment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
  };
}

function ontology() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: ONTOLOGY_ID,
    name: 'Support',
    description: 'Support domain',
    currentRevisionId: SECOND_REVISION_ID,
    currentRevisionNumber: 2,
    currentRevisionDigest: DIGEST_B,
    aggregateVersion: 2,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
    updatedAt: '2026-08-07T00:01:00.000Z',
  };
}

function revision() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    ontologyId: ONTOLOGY_ID,
    id: FIRST_REVISION_ID,
    revisionNumber: 1,
    parentRevisionId: null,
    parentDigest: null,
    contractSchema: 'cloud.workflow.ontology.v1',
    compilerSchemaVersion: 1,
    canonicalAcl: ACL,
    contentDigest: DIGEST_A,
    migrationPolicy: { kind: 'initial', ruleId: null, expressionDigest: null },
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-07T00:00:00.000Z',
  };
}

function diff() {
  return {
    ontologyId: ONTOLOGY_ID,
    fromRevisionId: FIRST_REVISION_ID,
    toRevisionId: SECOND_REVISION_ID,
    fromDigest: DIGEST_A,
    toDigest: DIGEST_B,
    breaking: false,
    changes: [
      {
        resourceKind: 'metadata',
        resourceId: 'ontology',
        changeKind: 'changed',
        compatibility: 'compatible',
        changedFields: ['description'],
      },
    ],
  };
}

function mutation() {
  return {
    ontology: ontology(),
    revision: revision(),
    diff: null,
    replayed: false,
  };
}
