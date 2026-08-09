import { describe, expect, it } from 'bun:test';
import { type CloudFetch, MAX_FORM_DOCUMENT_BYTES } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const FORM_ID = '019c0000-0000-7000-8000-000000000003';
const RELEASE_ID = '019c0000-0000-7000-8000-000000000004';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000005';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const FORM_INPUT = {
  name: 'Approval',
  description: 'Manager approval',
  document: {
    kind: 'a3s.form',
    apiVersion: 'a3s.dev/form/v1alpha1',
    revision: 1,
    metadata: { title: 'Approval' },
    schema: { type: 'object' },
    ui: { root: 'root', nodes: [{ id: 'root', kind: 'root', children: [] }] },
    rules: [],
    dataSources: [],
    actions: [],
  },
};

describe('a3s-cloud Form commands', () => {
  it.each([
    [['forms', 'list'], `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/forms`, [draft()]],
    [['forms', 'get', FORM_ID], `/organizations/${ORGANIZATION_ID}/forms/${FORM_ID}`, draft()],
    [
      ['form-releases', 'list', FORM_ID],
      `/organizations/${ORGANIZATION_ID}/forms/${FORM_ID}/releases`,
      [release()],
    ],
    [
      ['form-releases', 'get', FORM_ID, RELEASE_ID],
      `/organizations/${ORGANIZATION_ID}/forms/${FORM_ID}/releases/${RELEASE_ID}`,
      release(),
    ],
  ] as const)('queries the authoritative Form lifecycle %#', async (argv, path, data) => {
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
    expect(JSON.parse(output.stdout())).toEqual(data);
    expect(output.stderr()).toBe('');
  });

  it('creates and revises a bounded native Form document from JSON transport', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async (path: string) => {
        expect(path).toBe('form-draft.json');
        return new TextEncoder().encode(JSON.stringify(FORM_INPUT));
      },
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope({ form: draft(), replayed: false }, 201);
      },
    };
    const created = await runCli(
      ['forms', 'create', '--file=form-draft.json', '--idempotency-key=cli:form:create', '--output=json'],
      runtime
    );
    const revised = await runCli(
      [
        'forms',
        'revise',
        FORM_ID,
        '--file=form-draft.json',
        '--expected-version=1',
        '--idempotency-key=cli:form:revise',
        '--output=json',
      ],
      runtime
    );

    expect(created).toBe(ExitCode.Success);
    expect(revised).toBe(ExitCode.Success);
    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/forms`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/forms/${FORM_ID}/draft-revisions`,
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(FORM_INPUT),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:form:create',
        }),
      })
    );
    expect(calls[1]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(FORM_INPUT),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:form:revise',
          'x-a3s-expected-version': '1',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('publishes an immutable Form release without a synthetic request body', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'form-releases',
        'publish',
        FORM_ID,
        '--expected-version=2',
        '--idempotency-key=cli:form:publish',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ form: draft(), release: release(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/forms/${FORM_ID}/releases`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: undefined,
        headers: expect.objectContaining({
          'Idempotency-Key': 'cli:form:publish',
          'x-a3s-expected-version': '2',
        }),
      })
    );
    expect((calls[0]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
    expect(output.stderr()).toBe('');
  });

  it('rejects malformed, oversized, and unversioned Form mutations before transport', async () => {
    let called = false;
    const malformed = capture();
    const malformedExit = await runCli(
      ['forms', 'create', '--file=form.json', '--idempotency-key=cli:form:malformed'],
      {
        ...malformed.runtime,
        environment: completeEnvironment(),
        readFile: async () => new TextEncoder().encode(JSON.stringify({ ...FORM_INPUT, extra: true })),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    const oversized = capture();
    const oversizedExit = await runCli(
      ['forms', 'create', '--file=form.json', '--idempotency-key=cli:form:oversized'],
      {
        ...oversized.runtime,
        environment: completeEnvironment(),
        readFile: async () => new Uint8Array(MAX_FORM_DOCUMENT_BYTES + 8 * 1024 + 1),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );
    const unversioned = capture();
    const unversionedExit = await runCli(
      ['form-releases', 'publish', FORM_ID, '--idempotency-key=cli:form:unversioned'],
      {
        ...unversioned.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );

    expect(malformedExit).toBe(ExitCode.Usage);
    expect(oversizedExit).toBe(ExitCode.Usage);
    expect(unversionedExit).toBe(ExitCode.Usage);
    expect(malformed.stderr()).toContain('only name, optional description, and document');
    expect(oversized.stderr()).toContain('Form draft input must contain between');
    expect(unversioned.stderr()).toContain('--expected-version must be a positive safe integer');
    expect(called).toBe(false);
  });
});

function draft() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: FORM_ID,
    name: FORM_INPUT.name,
    description: FORM_INPUT.description,
    document: FORM_INPUT.document,
    draftDigest: DIGEST,
    aggregateVersion: 2,
    latestRelease: null,
    createdBy: PRINCIPAL_ID,
    updatedBy: PRINCIPAL_ID,
    createdAt: '2026-08-09T00:00:00.000Z',
    updatedAt: '2026-08-09T00:00:00.000Z',
  };
}

function release() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    formId: FORM_ID,
    id: RELEASE_ID,
    revision: 1,
    sourceDraftVersion: 2,
    name: FORM_INPUT.name,
    description: FORM_INPUT.description,
    normalizedDocument: FORM_INPUT.document,
    formPlan: { schema: 'a3s.dev/form-plan/v1' },
    compilerRevision: 'a3s-form-core@0.1.0',
    schemaProfile: 'a3s.dev/form-schema-profile/1',
    contentDigest: DIGEST,
    releaseRef: {
      apiVersion: 'a3s.dev/form-release-ref/v1',
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      formId: FORM_ID,
      releaseId: RELEASE_ID,
      uri: `a3s://forms/${FORM_ID}/releases/${RELEASE_ID}`,
      revision: 1,
      digest: DIGEST,
      compilerRevision: 'a3s-form-core@0.1.0',
      schemaProfile: 'a3s.dev/form-schema-profile/1',
      mode: 'interaction',
    },
    publishedBy: PRINCIPAL_ID,
    publishedAt: '2026-08-09T00:00:00.000Z',
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-09T00:00:00.000Z',
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
