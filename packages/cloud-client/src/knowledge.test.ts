import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import {
  DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
  DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
  encodeKnowledgeBaseListOptions,
  encodeKnowledgeChunkListOptions,
  encodeKnowledgeDocumentListOptions,
  encodeKnowledgePipelineListOptions,
  DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT,
  DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT,
  MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT,
  MAXIMUM_KNOWLEDGE_DOCUMENT_LIST_LIMIT,
  KNOWLEDGE_CONTRACT_MAX_ACL_BYTES,
  MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
  MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
  validateKnowledgeContentDigest,
  validateKnowledgeContractAcl,
} from './knowledge';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-08-28T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

const REVISION_ACL = 'knowledge_base_revision {\n  schema = "cloud.knowledge-base-revision.v1"\n}\n';
const RELEASE_ACL = 'knowledge_pipeline_release {\n  schema = "cloud.knowledge-pipeline-release.v1"\n}\n';
const DIGEST = `sha256:${'a'.repeat(64)}`;

describe('Knowledge catalog client surface', () => {
  it('uses the exact KnowledgeBase and KnowledgePipeline catalog routes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });

    await api.createKnowledgeBase(
      'organization / one',
      'project / one',
      { revisionAcl: REVISION_ACL },
      'knowledge:create-base'
    );
    await api.listKnowledgeBases('organization / one', 'project / one');
    await api.getKnowledgeBase('organization / one', 'project / one', 'base / one');
    await api.appendKnowledgeBase(
      'organization / one',
      'project / one',
      'base / one',
      { expectedRevisionDigest: DIGEST, revisionAcl: REVISION_ACL },
      'knowledge:append-base'
    );
    await api.createKnowledgePipeline(
      'organization / one',
      'project / one',
      { releaseAcl: RELEASE_ACL },
      'knowledge:create-pipeline'
    );
    await api.listKnowledgePipelines('organization / one', 'project / one');
    await api.getKnowledgePipeline('organization / one', 'project / one', 'pipeline / one');
    await api.publishKnowledgePipeline(
      'organization / one',
      'project / one',
      'pipeline / one',
      { expectedReleaseDigest: DIGEST, releaseAcl: RELEASE_ACL },
      'knowledge:publish-pipeline'
    );

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-bases',
      `/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-bases?limit=${DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT}`,
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-bases/base%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-bases/base%20%2F%20one/revisions',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-pipelines',
      `/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-pipelines?limit=${DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT}`,
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-pipelines/pipeline%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-pipelines/pipeline%20%2F%20one/releases',
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'knowledge:create-base' }),
        body: JSON.stringify({ revisionAcl: REVISION_ACL }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'knowledge:append-base' }),
        body: JSON.stringify({ expectedRevisionDigest: DIGEST, revisionAcl: REVISION_ACL }),
      })
    );
    expect(calls[7]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'knowledge:publish-pipeline' }),
        body: JSON.stringify({ expectedReleaseDigest: DIGEST, releaseAcl: RELEASE_ACL }),
      })
    );
  });

  it('enforces ACL transport bounds, list bounds, and content digest bounds locally', () => {
    expect(encodeKnowledgeBaseListOptions()).toBe(`?limit=${DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT}`);
    expect(encodeKnowledgeBaseListOptions({ limit: MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT })).toBe(
      `?limit=${MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT}`
    );
    expect(encodeKnowledgePipelineListOptions()).toBe(
      `?limit=${DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT}`
    );
    expect(encodeKnowledgePipelineListOptions({ limit: MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT })).toBe(
      `?limit=${MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT}`
    );
    expect(() => encodeKnowledgeBaseListOptions({ limit: 0 })).toThrow(RangeError);
    expect(() => encodeKnowledgePipelineListOptions({ limit: MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT + 1 })).toThrow(
      RangeError
    );
    expect(() => validateKnowledgeContractAcl('', 'KnowledgeBase revision ACL')).toThrow(TypeError);
    expect(() => validateKnowledgeContractAcl('knowledge_base_revision {\rinvalid = true\n}\n', 'ACL')).toThrow(
      TypeError
    );
    expect(() => validateKnowledgeContractAcl('x'.repeat(KNOWLEDGE_CONTRACT_MAX_ACL_BYTES + 1), 'ACL')).toThrow(
      TypeError
    );
    expect(() => validateKnowledgeContentDigest('sha256:deadbeef', 'digest')).toThrow(TypeError);
    validateKnowledgeContentDigest(DIGEST, 'digest');
  });

  it('rejects malformed values before issuing a request', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createKnowledgeBase('organization', 'project', { revisionAcl: '' }, 'knowledge:create')
    ).toThrow(TypeError);
    expect(() =>
      api.appendKnowledgeBase(
        'organization',
        'project',
        'base',
        { expectedRevisionDigest: 'bad', revisionAcl: REVISION_ACL },
        'knowledge:append'
      )
    ).toThrow(TypeError);
    expect(() => api.listKnowledgeBases('organization', 'project', { limit: 201 })).toThrow(RangeError);
    expect(() =>
      api.listKnowledgeDocuments('organization', 'project', { knowledgeBaseId: '' })
    ).toThrow(TypeError);
    expect(() => api.listKnowledgeChunks('organization', 'project', 'document', { limit: 201 })).toThrow(
      RangeError
    );
    expect(called).toBe(false);
  });

  it('enforces document and chunk list bounds locally', () => {
    expect(
      encodeKnowledgeDocumentListOptions({ knowledgeBaseId: '018f0000-0000-7000-8000-000000000301' })
    ).toBe(
      `?knowledgeBaseId=018f0000-0000-7000-8000-000000000301&limit=${DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT}`
    );
    expect(encodeKnowledgeChunkListOptions()).toBe(`?limit=${DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT}`);
    expect(() => encodeKnowledgeDocumentListOptions({ knowledgeBaseId: 'base', limit: 0 })).toThrow(
      RangeError
    );
    expect(() => encodeKnowledgeChunkListOptions({ limit: MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT + 1 })).toThrow(
      RangeError
    );
  });
});

const DOCUMENT_ACL = `knowledge_document {
  document_id = "018f0000-0000-7000-8000-000000000303"
  knowledge_base_id = "018f0000-0000-7000-8000-000000000301"
  knowledge_base_revision_id = "018f0000-0000-7000-8000-000000000302"
  organization_id = "018f0000-0000-7000-8000-000000000201"
  project_id = "018f0000-0000-7000-8000-000000000202"
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
  chunk_id = "018f0000-0000-7000-8000-000000000304"
  content_digest = "sha256:0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"
  document_id = "018f0000-0000-7000-8000-000000000303"
  object_ref = "organizations/org/projects/proj/knowledge/chunks/0"
  ordinal = 0
  organization_id = "018f0000-0000-7000-8000-000000000201"
  parent_chunk_id = ""
  project_id = "018f0000-0000-7000-8000-000000000202"
  provenance_digest = "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
  schema = "cloud.knowledge-chunk.v1"
  structure = "general"
  tags = ["section-1"]
}
`;

describe('KnowledgeDocument/Chunk lifecycle client surface', () => {
  it('uses the exact KnowledgeDocument and KnowledgeChunk lifecycle routes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });

    await api.createKnowledgeDocument(
      'organization / one',
      'project / one',
      { documentAcl: DOCUMENT_ACL },
      'knowledge:create-document'
    );
    await api.listKnowledgeDocuments('organization / one', 'project / one', {
      knowledgeBaseId: 'base / one',
    });
    await api.getKnowledgeDocument('organization / one', 'project / one', 'document / one');
    await api.createKnowledgeChunk(
      'organization / one',
      'project / one',
      'document / one',
      { chunkAcl: CHUNK_ACL },
      'knowledge:create-chunk'
    );
    await api.listKnowledgeChunks('organization / one', 'project / one', 'document / one');
    await api.getKnowledgeChunk('organization / one', 'project / one', 'chunk / one');

    expect(calls.map(([input]) => input)).toEqual([
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-documents',
      `/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-documents?knowledgeBaseId=base%20%2F%20one&limit=${DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT}`,
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-documents/document%20%2F%20one',
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-documents/document%20%2F%20one/chunks',
      `/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-documents/document%20%2F%20one/chunks?limit=${DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT}`,
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/knowledge-chunks/chunk%20%2F%20one',
    ]);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'knowledge:create-document' }),
        body: JSON.stringify({ documentAcl: DOCUMENT_ACL }),
      })
    );
    expect(calls[3]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'knowledge:create-chunk' }),
        body: JSON.stringify({ chunkAcl: CHUNK_ACL }),
      })
    );
  });

  it('rejects malformed document and chunk ACL before issuing a request', () => {
    let called = false;
    const api = new CloudApi('caller-token', '/api/v1', {
      fetch: async () => {
        called = true;
        return jsonResponse({});
      },
    });

    expect(() =>
      api.createKnowledgeDocument('organization', 'project', { documentAcl: '' }, 'knowledge:create-document')
    ).toThrow(TypeError);
    expect(() =>
      api.createKnowledgeChunk(
        'organization',
        'project',
        'document',
        { chunkAcl: '' },
        'knowledge:create-chunk'
      )
    ).toThrow(TypeError);
    expect(() =>
      api.listKnowledgeDocuments('organization', 'project', { knowledgeBaseId: '' })
    ).toThrow(TypeError);
    expect(() => api.listKnowledgeChunks('organization', 'project', 'document', { limit: 201 })).toThrow(
      RangeError
    );
    expect(called).toBe(false);
  });

  it('enforces document and chunk list bounds locally', () => {
    expect(
      encodeKnowledgeDocumentListOptions({ knowledgeBaseId: '018f0000-0000-7000-8000-000000000301' })
    ).toBe(
      `?knowledgeBaseId=018f0000-0000-7000-8000-000000000301&limit=${DEFAULT_KNOWLEDGE_DOCUMENT_LIST_LIMIT}`
    );
    expect(encodeKnowledgeChunkListOptions()).toBe(`?limit=${DEFAULT_KNOWLEDGE_CHUNK_LIST_LIMIT}`);
    expect(() => encodeKnowledgeDocumentListOptions({ knowledgeBaseId: 'base', limit: 0 })).toThrow(
      RangeError
    );
    expect(() => encodeKnowledgeChunkListOptions({ limit: MAXIMUM_KNOWLEDGE_CHUNK_LIST_LIMIT + 1 })).toThrow(
      RangeError
    );
  });
});
