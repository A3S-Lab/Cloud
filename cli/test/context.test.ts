import { describe, expect, it } from 'bun:test';
import { parseArguments } from '../src/arguments';
import { normalizeApiUrl, publicContext, resolveContext } from '../src/context';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';

describe('Cloud CLI context', () => {
  it('resolves flags over environment without exposing the token', () => {
    const context = resolveContext(
      parseArguments([
        '--url',
        'https://cloud.example.test/control/api/v1/',
        '--organization',
        ORGANIZATION_ID.toUpperCase(),
        '--output=json',
        'context',
        'show',
      ]),
      {
        A3S_CLOUD_TOKEN: 'a3s_secret',
        A3S_CLOUD_URL: 'https://ignored.example.test/api/v1',
        A3S_CLOUD_OUTPUT: 'table',
      }
    );

    expect(context).toEqual(
      expect.objectContaining({
        baseUrl: 'https://cloud.example.test/control/api/v1',
        token: 'a3s_secret',
        organizationId: ORGANIZATION_ID,
        output: 'json',
        timeoutMs: 30_000,
      })
    );
    expect(publicContext(context)).toEqual(
      expect.objectContaining({ tokenConfigured: true, organizationId: ORGANIZATION_ID })
    );
    expect(JSON.stringify(publicContext(context))).not.toContain('a3s_secret');
  });

  it('resolves the complete environment context', () => {
    const context = resolveContext(parseArguments(['context', 'show']), {
      A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
      A3S_CLOUD_PROJECT_ID: PROJECT_ID,
      A3S_CLOUD_ENVIRONMENT_ID: ENVIRONMENT_ID,
      A3S_CLOUD_TIMEOUT_MS: '1200',
    });

    expect(context).toEqual(
      expect.objectContaining({
        organizationId: ORGANIZATION_ID,
        projectId: PROJECT_ID,
        environmentId: ENVIRONMENT_ID,
        timeoutMs: 1_200,
      })
    );
  });

  it.each([
    ['https://cloud.example.test/api/v1', 'https://cloud.example.test/api/v1'],
    ['http://localhost:8080/api/v1/', 'http://localhost:8080/api/v1'],
    ['http://127.0.0.1:8080/api/v1', 'http://127.0.0.1:8080/api/v1'],
    ['http://[::1]:8080/api/v1', 'http://[::1]:8080/api/v1'],
  ])('accepts a safe API endpoint %s', (input, expected) => {
    expect(normalizeApiUrl(input)).toBe(expected);
  });

  it.each([
    'http://cloud.example.test/api/v1',
    'http://127.1/api/v1',
    'https://user:password@cloud.example.test/api/v1',
    'https://cloud.example.test/api/v2',
    'https://cloud.example.test/api/v1?token=secret',
    '/api/v1',
  ])('rejects an unsafe API endpoint %s', (input) => {
    expect(() => normalizeApiUrl(input)).toThrow();
  });

  it('requires context IDs to be UUIDs and hierarchically complete', () => {
    expect(() => resolveContext(parseArguments(['--project', PROJECT_ID, 'context', 'show']), {})).toThrow(
      'project context requires an organization ID'
    );
    expect(() =>
      resolveContext(parseArguments(['--organization', 'not-a-uuid', 'context', 'show']), {})
    ).toThrow('organization ID must be a UUID');
  });

  it('rejects credentials embedded in the API path', () => {
    expect(() =>
      resolveContext(
        parseArguments(['--url', 'https://cloud.example.test/a3s_secret/api/v1', 'context', 'show']),
        {
          A3S_CLOUD_TOKEN: 'a3s_secret',
        }
      )
    ).toThrow('Cloud API URL cannot contain the configured token');
  });
});
