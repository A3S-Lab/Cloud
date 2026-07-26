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
