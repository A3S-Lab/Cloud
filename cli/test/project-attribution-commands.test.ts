import { describe, expect, it } from 'bun:test';
import type { CloudFetch } from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const PROFILE_ID = '019c0000-0000-7000-8000-000000000003';

describe('a3s-cloud project attribution commands', () => {
  it('reads current and exact immutable profiles', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        return envelope(profile());
      },
    };

    expect(await runCli(['project-attribution', 'get', '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(['project-attribution', 'get', PROFILE_ID, '--output=json'], runtime)
    ).toBe(ExitCode.Success);
    expect(calls.map(([input]) => input)).toEqual([
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/attribution-profile`,
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/attribution-profiles/${PROFILE_ID}`,
    ]);
  });

  it('creates a new profile with exact labels and project version', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'project-attribution',
        'update',
        'engineering/platform',
        '--cost-attribution-code=CC-1042',
        '--label=service.tier=critical',
        '--label=region=global',
        '--expected-version=2',
        '--idempotency-key=cli:project-attribution:2',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope(
            {
              project: {
                organizationId: ORGANIZATION_ID,
                id: PROJECT_ID,
                name: 'Platform',
                aggregateVersion: 3,
                currentAttributionProfileId: PROFILE_ID,
                createdAt: '2026-08-14T00:00:00.000Z',
              },
              attributionProfile: profile(),
              replayed: false,
            },
            201
          );
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/attribution-profiles`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Idempotency-Key': 'cli:project-attribution:2',
          'x-a3s-expected-version': '2',
        }),
        body: JSON.stringify({
          businessOwnerReference: 'engineering/platform',
          costAttributionCode: 'CC-1042',
          labels: { 'service.tier': 'critical', region: 'global' },
        }),
      })
    );
  });

  it('rejects duplicate labels before transport', async () => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(
      [
        'project-attribution',
        'update',
        'engineering/platform',
        '--label=team=platform',
        '--label=team=other',
        '--expected-version=1',
        '--idempotency-key=cli:duplicate',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        fetch: async () => {
          called = true;
          return envelope({});
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('is duplicated');
    expect(called).toBe(false);
  });
});

function profile() {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    id: PROFILE_ID,
    previousProfileId: null,
    businessOwnerReference: 'engineering/platform',
    costAttributionCode: 'CC-1042',
    labels: { region: 'global', 'service.tier': 'critical' },
    createdBy: '019c0000-0000-7000-8000-000000000004',
    createdAt: '2026-08-14T00:01:00.000Z',
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-14T00:00:00.000Z',
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
