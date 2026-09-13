import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import {
  DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT,
  DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
  encodeKnowledgeBaseListOptions,
  encodeKnowledgePipelineListOptions,
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
    expect(called).toBe(false);
  });
});
