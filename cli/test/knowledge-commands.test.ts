import { describe, expect, it } from 'bun:test';
import { type CloudFetch, type KnowledgeBase, type KnowledgePipeline } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const KNOWLEDGE_BASE_ID = '019c0000-0000-7000-8000-000000000003';
const PIPELINE_ID = '019c0000-0000-7000-8000-000000000004';
const REVISION_ID = '019c0000-0000-7000-8000-000000000005';
const RELEASE_ID = '019c0000-0000-7000-8000-000000000006';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const NEXT_DIGEST = `sha256:${'b'.repeat(64)}`;
const REVISION_ACL = 'knowledge_base_revision {\n  schema = "cloud.knowledge-base-revision.v1"\n}\n';
const RELEASE_ACL = 'knowledge_pipeline_release {\n  schema = "cloud.knowledge-pipeline-release.v1"\n}\n';

describe('a3s-cloud Knowledge catalog commands', () => {
  it('creates KnowledgeBases from a bounded .acl file with caller-owned idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'knowledge-bases',
        'create',
        '--file=knowledge-base.acl',
        '--idempotency-key=cli:knowledge:create-base',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('knowledge-base.acl');
          return new TextEncoder().encode(REVISION_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ knowledgeBase: knowledgeBase(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(knowledgeBaseBase());
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ revisionAcl: REVISION_ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:knowledge:create-base',
        }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('lists, gets, appends, creates pipelines, and publishes through exact catalog routes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async (path: string) => {
        if (path === 'knowledge-base.acl') {
          return new TextEncoder().encode(REVISION_ACL);
        }
        expect(path).toBe('knowledge-pipeline.acl');
        return new TextEncoder().encode(RELEASE_ACL);
      },
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        const path = String(args[0]);
        const method = String(args[1]?.method ?? 'GET');
        if (path.endsWith('/releases')) {
          return envelope({ knowledgePipeline: knowledgePipeline(NEXT_DIGEST), replayed: false });
        }
        if (path.endsWith('/revisions')) {
          return envelope({ knowledgeBase: knowledgeBase(NEXT_DIGEST, 2), replayed: false });
        }
        if (path === knowledgePipelineBase() && method === 'POST') {
          return envelope({ knowledgePipeline: knowledgePipeline(), replayed: false }, 201);
        }
        if (path.includes(`/knowledge-pipelines/${PIPELINE_ID}`)) {
          return envelope(knowledgePipeline());
        }
        if (path.startsWith(`${knowledgePipelineBase()}?`) || path === knowledgePipelineBase()) {
          return envelope([knowledgePipeline()]);
        }
        if (path.includes(`/knowledge-bases/${KNOWLEDGE_BASE_ID}`)) {
          return envelope(knowledgeBase());
        }
        return envelope([knowledgeBase()]);
      },
    };

    expect(await runCli(['knowledge-bases', 'list', '--output=json'], runtime)).toBe(ExitCode.Success);
    expect(await runCli(['knowledge-bases', 'get', KNOWLEDGE_BASE_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(
        [
          'knowledge-bases',
          'append',
          KNOWLEDGE_BASE_ID,
          '--file=knowledge-base.acl',
          `--expected-digest=${DIGEST}`,
          '--idempotency-key=cli:knowledge:append',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'knowledge-pipelines',
          'create',
          '--file=knowledge-pipeline.acl',
          '--idempotency-key=cli:knowledge:create-pipeline',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(await runCli(['knowledge-pipelines', 'list', '--output=json'], runtime)).toBe(ExitCode.Success);
    expect(await runCli(['knowledge-pipelines', 'get', PIPELINE_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(
        [
          'knowledge-pipelines',
          'publish',
          PIPELINE_ID,
          '--file=knowledge-pipeline.acl',
          `--expected-digest=${DIGEST}`,
          '--idempotency-key=cli:knowledge:publish',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);

    expect(calls.map(([input]) => String(input))).toEqual([
      `${knowledgeBaseBase()}?limit=50`,
      `${knowledgeBaseBase()}/${KNOWLEDGE_BASE_ID}`,
      `${knowledgeBaseBase()}/${KNOWLEDGE_BASE_ID}/revisions`,
      knowledgePipelineBase(),
      `${knowledgePipelineBase()}?limit=50`,
      `${knowledgePipelineBase()}/${PIPELINE_ID}`,
      `${knowledgePipelineBase()}/${PIPELINE_ID}/releases`,
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ expectedRevisionDigest: DIGEST, revisionAcl: REVISION_ACL }),
      })
    );
    expect(calls[6]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ expectedReleaseDigest: DIGEST, releaseAcl: RELEASE_ACL }),
      })
    );
    expect(output.stderr()).toBe('');
  });
});

function knowledgeBase(revisionDigest = DIGEST, generation = 1): KnowledgeBase {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    knowledgeBaseId: KNOWLEDGE_BASE_ID,
    revisionId: REVISION_ID,
    generation,
    name: 'Product FAQ',
    contractSchema: 'cloud.knowledge-base-revision.v1',
    revisionAcl: REVISION_ACL,
    revisionDigest,
    createdAt: '2026-08-28T00:00:00.000Z',
    updatedAt: '2026-08-28T00:00:00.000Z',
  };
}

function knowledgePipeline(releaseDigest = DIGEST): KnowledgePipeline {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    pipelineId: PIPELINE_ID,
    releaseId: RELEASE_ID,
    name: 'FAQ ingest',
    contractSchema: 'cloud.knowledge-pipeline-release.v1',
    releaseAcl: RELEASE_ACL,
    releaseDigest,
    createdAt: '2026-08-28T00:00:00.000Z',
    updatedAt: '2026-08-28T00:00:00.000Z',
  };
}

function knowledgeBaseBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/knowledge-bases`
  );
}

function knowledgePipelineBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/knowledge-pipelines`
  );
}

function completeEnvironment(): Record<string, string> {
  return {
    A3S_CLOUD_TOKEN: 'a3s_secret',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-28T00:00:00.000Z',
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

const DOCUMENT_ID = '018f0000-0000-7000-8000-000000000303';
const CHUNK_ID = '018f0000-0000-7000-8000-000000000304';
const DOCUMENT_ACL = `knowledge_document {
  document_id = "${DOCUMENT_ID}"
  knowledge_base_id = "018f0000-0000-7000-8000-000000000301"
  knowledge_base_revision_id = "018f0000-0000-7000-8000-000000000302"
  organization_id = "${ORGANIZATION_ID}"
  project_id = "${PROJECT_ID}"
  provenance_digest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
  retention_until = "2027-08-21T00:00:00.000000Z"
  schema = "cloud.knowledge-document.v1"
  tags = ["returns"]
  title = "Return policy"
  source {
    content_digest = "sha256:0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
    kind = "admitted_user_file"
    user_file_id = "018f0000-0000-7000-8000-000000000203"
  }
}
`;
const CHUNK_ACL = `knowledge_chunk {
  chunk_id = "${CHUNK_ID}"
  content_digest = "sha256:0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"
  document_id = "${DOCUMENT_ID}"
  object_ref = "organizations/org/projects/proj/knowledge/chunks/0"
  ordinal = 0
  organization_id = "${ORGANIZATION_ID}"
  project_id = "${PROJECT_ID}"
  parent_chunk_id = ""
  provenance_digest = "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
  schema = "cloud.knowledge-chunk.v1"
  structure = "general"
  tags = ["section-1"]
}
`;

describe('a3s-cloud KnowledgeDocument/Chunk lifecycle commands', () => {
  it('creates and gets documents and chunks through exact lifecycle routes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async (path: string) => {
        if (path === 'knowledge-document.acl') {
          return new TextEncoder().encode(DOCUMENT_ACL);
        }
        expect(path).toBe('knowledge-chunk.acl');
        return new TextEncoder().encode(CHUNK_ACL);
      },
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        const path = String(args[0]);
        const method = String(args[1]?.method ?? 'GET');
        if (path.endsWith('/chunks') && method === 'POST') {
          return envelope({ knowledgeChunk: knowledgeChunk(), replayed: false }, 201);
        }
        if (path === knowledgeDocumentBase() && method === 'POST') {
          return envelope({ knowledgeDocument: knowledgeDocument(), replayed: false }, 201);
        }
        if (path.includes(`/knowledge-documents/${DOCUMENT_ID}`)) {
          return envelope(knowledgeDocument());
        }
        if (path.includes(`/knowledge-chunks/${CHUNK_ID}`)) {
          return envelope(knowledgeChunk());
        }
        throw new Error(`unexpected fetch ${method} ${path}`);
      },
    };

    expect(
      await runCli(
        [
          'knowledge-documents',
          'create',
          '--file=knowledge-document.acl',
          '--idempotency-key=cli:knowledge:create-document',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(
      await runCli(['knowledge-documents', 'get', DOCUMENT_ID, '--output=json'], runtime)
    ).toBe(ExitCode.Success);
    expect(
      await runCli(
        [
          'knowledge-chunks',
          'create',
          DOCUMENT_ID,
          '--file=knowledge-chunk.acl',
          '--idempotency-key=cli:knowledge:create-chunk',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(await runCli(['knowledge-chunks', 'get', CHUNK_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );

    expect(calls.map(([input]) => String(input))).toEqual([
      knowledgeDocumentBase(),
      `${knowledgeDocumentBase()}/${DOCUMENT_ID}`,
      `${knowledgeDocumentBase()}/${DOCUMENT_ID}/chunks`,
      `${knowledgeChunkBase()}/${CHUNK_ID}`,
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ documentAcl: DOCUMENT_ACL }),
      })
    );
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ chunkAcl: CHUNK_ACL }),
      })
    );
    expect(output.stderr()).toBe('');
  });
});

function knowledgeDocument() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    documentId: DOCUMENT_ID,
    knowledgeBaseId: KNOWLEDGE_BASE_ID,
    knowledgeBaseRevisionId: REVISION_ID,
    title: 'Return policy',
    contractSchema: 'cloud.knowledge-document.v1',
    documentAcl: DOCUMENT_ACL,
    documentDigest: DIGEST,
    createdAt: '2026-08-28T00:00:00.000Z',
  };
}

function knowledgeChunk() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    documentId: DOCUMENT_ID,
    chunkId: CHUNK_ID,
    ordinal: 0,
    contractSchema: 'cloud.knowledge-chunk.v1',
    chunkAcl: CHUNK_ACL,
    chunkDigest: DIGEST,
    createdAt: '2026-08-28T00:00:00.000Z',
  };
}

function knowledgeDocumentBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/knowledge-documents`
  );
}

function knowledgeChunkBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` +
    `/projects/${PROJECT_ID}/knowledge-chunks`
  );
}
