import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const CONVERSATION_ID = '019c0000-0000-7000-8000-000000000031';
const EXECUTION_ID = '019c0000-0000-7000-8000-000000000032';
const AGENT_ID = '019c0000-0000-7000-8000-000000000033';
const RELEASE_ID = '019c0000-0000-7000-8000-000000000034';

describe('a3s-cloud Agent commands', () => {
  it.each([
    [
      ['agent-conversations', 'list'],
      `/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/agent-conversations?limit=100`,
      [conversation()],
    ],
    [
      ['agent-conversations', 'get', CONVERSATION_ID],
      `/organizations/${ORGANIZATION_ID}/agent-conversations/${CONVERSATION_ID}`,
      conversation(),
    ],
    [
      ['agent-executions', 'list', CONVERSATION_ID],
      `/organizations/${ORGANIZATION_ID}/agent-conversations/${CONVERSATION_ID}/executions?limit=100`,
      [execution()],
    ],
    [
      ['agent-executions', 'get', EXECUTION_ID],
      `/organizations/${ORGANIZATION_ID}/agent-executions/${EXECUTION_ID}`,
      execution(),
    ],
    [
      ['agent-executions', 'changes', EXECUTION_ID],
      `/organizations/${ORGANIZATION_ID}/agent-executions/${EXECUTION_ID}/changes`,
      executionChangeSet(),
    ],
  ] as const)('queries Agent resources %#', async (command, path, response) => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli([...command, '--output=json'], {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(response);
      },
    });

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(`http://127.0.0.1:8080/api/v1${path}`);
    expect(calls[0]?.[1]?.method).toBe('GET');
    expect(output.stderr()).toBe('');
  });

  it('creates a conversation without fabricating a request body', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli(
      ['agent-conversations', 'create', '--idempotency-key=cli:conversation-1', '--output=json'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ conversation: conversation(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/agent-conversations`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:conversation-1' }),
        body: undefined,
      })
    );
    expect((calls[0]?.[1]?.headers as Record<string, string>)['Content-Type']).toBeUndefined();
  });

  it('starts one execution with an exact published Agent release identity', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli(
      [
        'agent-executions',
        'start',
        CONVERSATION_ID,
        AGENT_ID,
        RELEASE_ID,
        '--provider-kind=reference.echo',
        '--idempotency-key=cli:agent-execution-1',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ conversation: conversation(), execution: execution(), replayed: false }, 202);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/agent-conversations/${CONVERSATION_ID}/executions`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:agent-execution-1' }),
        body: JSON.stringify({
          agentAssetId: AGENT_ID,
          agentAssetReleaseId: RELEASE_ID,
          providerKind: 'reference.echo',
        }),
      })
    );
  });

  it('cancels one execution through the Agent cancellation endpoint', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli(
      [
        'agent-executions',
        'cancel',
        EXECUTION_ID,
        '--idempotency-key=cli:agent-execution-cancel-1',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope(
            {
              conversation: conversation(),
              execution: {
                ...execution(),
                status: 'cancelling',
                cancellationRequestedAt: '2026-08-04T00:02:00.000Z',
              },
              replayed: false,
            },
            202
          );
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/agent-executions/${EXECUTION_ID}/cancel`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'cli:agent-execution-cancel-1',
        }),
        body: undefined,
      })
    );
  });

  it('reads a bounded semantic event page with one opaque cursor', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();

    const exitCode = await runCli(
      ['agent-conversations', 'events', CONVERSATION_ID, '--cursor=8', '--limit=25', '--output=json'],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({
            conversationId: CONVERSATION_ID,
            headSequence: 8,
            records: [],
            nextCursor: null,
          });
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/agent-conversations/${CONVERSATION_ID}/events?cursor=8&limit=25`
    );
  });

  it('rejects invalid event paging and release identities before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    const invalidLimit = await runCli(
      ['agent-conversations', 'events', CONVERSATION_ID, '--limit=201'],
      runtime
    );
    const invalidRelease = await runCli(
      ['agent-executions', 'start', CONVERSATION_ID, AGENT_ID, 'not-a-uuid', '--idempotency-key=cli:invalid'],
      runtime
    );
    const invalidProvider = await runCli(
      [
        'agent-executions',
        'start',
        CONVERSATION_ID,
        AGENT_ID,
        RELEASE_ID,
        '--provider-kind=unknown.provider',
        '--idempotency-key=cli:invalid-provider',
      ],
      runtime
    );

    expect(invalidLimit).toBe(ExitCode.Usage);
    expect(invalidRelease).toBe(ExitCode.Usage);
    expect(invalidProvider).toBe(ExitCode.Usage);
    expect(called).toBe(false);
  });
});

function conversation() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    id: CONVERSATION_ID,
    status: 'active',
    lastEventSequence: 1,
    aggregateVersion: 2,
    createdAt: '2026-08-04T00:00:00.000Z',
    updatedAt: '2026-08-04T00:01:00.000Z',
    closedAt: null,
  };
}

function execution() {
  return {
    organizationId: ORGANIZATION_ID,
    conversationId: CONVERSATION_ID,
    id: EXECUTION_ID,
    operationId: '019c0000-0000-7000-8000-000000000035',
    agent: {
      assetId: AGENT_ID,
      assetReleaseId: RELEASE_ID,
      buildRunId: '019c0000-0000-7000-8000-000000000036',
      artifactUri: `oci://registry.example/agent@sha256:${'a'.repeat(64)}`,
      artifactDigest: `sha256:${'a'.repeat(64)}`,
      artifactMediaType: 'application/vnd.oci.image.manifest.v1+json',
      artifactSizeBytes: 1024,
    },
    provider: {
      kind: 'a3s.code',
      revision: '8.0.1',
      protocol: 'a3s.cloud.agent-provider.v1',
      nativeProtocol: 'a3s.code.agent.v1',
      profileDigest: `sha256:${'b'.repeat(64)}`,
      capabilityDigest: `sha256:${'c'.repeat(64)}`,
    },
    status: 'pending',
    failure: null,
    aggregateVersion: 1,
    requestedAt: '2026-08-04T00:01:00.000Z',
    updatedAt: '2026-08-04T00:01:00.000Z',
    startedAt: null,
    cancellationRequestedAt: null,
    finishedAt: null,
  };
}

function executionChangeSet() {
  return {
    organizationId: ORGANIZATION_ID,
    executionId: EXECUTION_ID,
    batchId: '019c0000-0000-7000-8000-000000000037',
    nodeId: '019c0000-0000-7000-8000-000000000038',
    changeSet: {
      schema: 'a3s.code.agent-change-set.v1',
      identity: {
        schema: 'a3s.code.agent-run-identity.v1',
        protocol: 'a3s.code.agent.v1',
        agent_release_identity: `sha256:${'a'.repeat(64)}`,
        session_id: 'conversation-1',
        run_id: 'run-1',
      },
      state: 'completed',
      format: 'git_unified_diff_v1',
      encoding: 'base64',
      base_tree: `git-tree:${'a'.repeat(40)}`,
      result_tree: `git-tree:${'b'.repeat(40)}`,
      patch_digest: 'sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
      patch_bytes: 0,
      patch_base64: '',
      observed_at_ms: 1_723_000_000_000,
    },
    recordedAt: '2026-08-04T00:02:00.000Z',
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-04T00:00:00.000Z',
    }),
    { status }
  );
}

function completeEnvironment(): Record<string, string> {
  return {
    A3S_CLOUD_TOKEN: `a3s_${'a'.repeat(64)}`,
    A3S_CLOUD_URL: 'http://127.0.0.1:8080/api/v1',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
    A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
  };
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
