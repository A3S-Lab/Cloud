import { describe, expect, it } from 'bun:test';
import { parseArguments } from '../src/arguments';
import { CliError } from '../src/errors';

describe('parseArguments', () => {
  it('accepts global options before or after the command', () => {
    expect(
      parseArguments([
        'nodes',
        '--organization=019c0000-0000-7000-8000-000000000001',
        'list',
        '--output',
        'json',
      ])
    ).toEqual(
      expect.objectContaining({
        positionals: ['nodes', 'list'],
        organizationId: '019c0000-0000-7000-8000-000000000001',
        output: 'json',
      })
    );
  });

  it('parses bounded log query options without interpreting the cursor', () => {
    expect(
      parseArguments([
        'build-runs',
        'logs',
        '019c0000-0000-7000-8000-000000000005',
        '--cursor=v1:41',
        '--limit',
        '25',
        '--stream=stderr',
      ])
    ).toEqual(
      expect.objectContaining({
        positionals: ['build-runs', 'logs', '019c0000-0000-7000-8000-000000000005'],
        cursor: 'v1:41',
        limit: '25',
        stream: 'stderr',
      })
    );
  });

  it('parses an explicit mutation idempotency key', () => {
    expect(
      parseArguments([
        'workloads',
        'stop',
        '019c0000-0000-7000-8000-000000000004',
        '--idempotency-key=release:stop-42',
      ])
    ).toEqual(
      expect.objectContaining({
        idempotencyKey: 'release:stop-42',
      })
    );
  });

  it('parses one ACL desired-state file', () => {
    expect(
      parseArguments([
        'workloads',
        'create',
        '--file=deploy/workload.acl',
        '--idempotency-key=release:create-42',
      ])
    ).toEqual(
      expect.objectContaining({
        file: 'deploy/workload.acl',
        idempotencyKey: 'release:create-42',
      })
    );
  });

  it('parses one optimistic-concurrency version', () => {
    expect(
      parseArguments([
        'nodes',
        'drain',
        '019c0000-0000-7000-8000-000000000004',
        '--expected-version=7',
        '--idempotency-key=release:drain-42',
      ])
    ).toEqual(
      expect.objectContaining({
        expectedVersion: '7',
        idempotencyKey: 'release:drain-42',
      })
    );
  });

  it('parses explicit node bootstrap credential and release options', () => {
    expect(
      parseArguments([
        'nodes',
        'bootstrap',
        'worker-1',
        '--enrollment-token-stdin',
        '--expires-at=2026-07-27T01:15:00Z',
        '--agent-release-url=https://releases.example.test/node-agent',
        `--agent-release-sha256=${'a'.repeat(64)}`,
        '--node-config=/etc/a3s-cloud/node.acl',
        '--idempotency-key=fleet:bootstrap:worker-1',
      ])
    ).toEqual(
      expect.objectContaining({
        enrollmentTokenStdin: true,
        expiresAt: '2026-07-27T01:15:00Z',
        agentReleaseUrl: 'https://releases.example.test/node-agent',
        agentReleaseSha256: 'a'.repeat(64),
        nodeConfig: '/etc/a3s-cloud/node.acl',
      })
    );
  });

  it('parses explicit Gateway rollout thresholds', () => {
    expect(
      parseArguments([
        'gateway-scopes',
        'create',
        '019c0000-0000-7000-8000-000000000004',
        '019c0000-0000-7000-8000-000000000005',
        '--min-ready=1',
        '--max-unavailable',
        '1',
        '--idempotency-key=route:scope-42',
      ])
    ).toEqual(
      expect.objectContaining({
        minReady: '1',
        maxUnavailable: '1',
        idempotencyKey: 'route:scope-42',
      })
    );
  });

  it('parses an explicit Source build recipe', () => {
    expect(
      parseArguments([
        'source-revisions',
        'resolve',
        'https://github.com/A3S-Lab/Cloud.git',
        'branch',
        'main',
        '--context-path=services/api',
        '--dockerfile-path',
        'Dockerfile',
        '--target=release',
        '--platforms=linux/amd64,linux/arm64',
        '--idempotency-key=source:resolve-42',
      ])
    ).toEqual(
      expect.objectContaining({
        contextPath: 'services/api',
        dockerfilePath: 'Dockerfile',
        target: 'release',
        platforms: 'linux/amd64,linux/arm64',
        idempotencyKey: 'source:resolve-42',
      })
    );
  });

  it('parses explicit standard-input Secret material without accepting a value', () => {
    expect(
      parseArguments([
        'secrets',
        'create',
        'Database URL',
        '--value-stdin',
        '--idempotency-key=secret:create-42',
      ])
    ).toEqual(
      expect.objectContaining({
        valueStdin: true,
        idempotencyKey: 'secret:create-42',
      })
    );
  });

  it.each([
    [['--token', 'secret', 'organizations', 'list'], 'API tokens are accepted only'],
    [['--token=secret', 'organizations', 'list'], 'API tokens are accepted only'],
    [['--unknown', 'value'], 'unknown option'],
    [['--output'], 'requires a value'],
    [['--output', 'json', '--output', 'table'], 'may be specified only once'],
    [['secrets', 'create', 'Database URL', '--value-stdin', '--value-stdin'], 'may be specified only once'],
    [['secrets', 'create', 'Database URL', '--value-stdin=plaintext'], 'does not accept a value'],
    [
      ['nodes', 'bootstrap', 'worker-1', '--enrollment-token-stdin', '--enrollment-token-stdin'],
      'may be specified only once',
    ],
    [['nodes', 'bootstrap', 'worker-1', '--enrollment-token-stdin=value'], 'does not accept a value'],
  ])('rejects unsafe or ambiguous arguments %#', (argv, message) => {
    expect(() => parseArguments(argv)).toThrow(message);
    try {
      parseArguments(argv);
    } catch (error) {
      expect(error).toBeInstanceOf(CliError);
      expect((error as CliError).exitCode).toBe(2);
    }
  });
});
