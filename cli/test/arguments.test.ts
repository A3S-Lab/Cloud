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

  it.each([
    [['--token', 'secret', 'organizations', 'list'], 'API tokens are accepted only'],
    [['--token=secret', 'organizations', 'list'], 'API tokens are accepted only'],
    [['--unknown', 'value'], 'unknown option'],
    [['--output'], 'requires a value'],
    [['--output', 'json', '--output', 'table'], 'may be specified only once'],
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
