import { describe, expect, it } from 'bun:test';
import type { CloudFetch, Notification, NotificationPage } from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const NOTIFICATION_ID = '019c0000-0000-7000-8000-000000000002';
const REQUEST_ID = '019c0000-0000-7000-8000-000000000003';

const NOTIFICATION: Notification = {
  id: NOTIFICATION_ID,
  organizationId: ORGANIZATION_ID,
  sourceEventId: '019c0000-0000-7000-8000-000000000004',
  sourceEventKey: 'identity.membership.role-changed',
  sourceAggregateId: '019c0000-0000-7000-8000-000000000005',
  severity: 'information',
  title: 'Organization role changed',
  body: 'Your organization role is now member.',
  scope: { kind: 'organization' },
  occurredAt: '2026-08-14T01:02:03Z',
  deliveredAt: '2026-08-14T01:02:03Z',
  aggregateVersion: 1,
  readAt: null,
};

const PAGE: NotificationPage = {
  notifications: [NOTIFICATION],
  nextCursor: `v1:1786678923000000:${NOTIFICATION_ID}`,
};

function envelope(data: unknown): Response {
  return new Response(
    JSON.stringify({
      code: 200,
      message: 'Success',
      data,
      requestId: REQUEST_ID,
      timestamp: '2026-08-14T01:02:04Z',
    }),
    { status: 200 }
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

function environment() {
  return {
    A3S_CLOUD_TOKEN: 'token',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
  };
}

describe('notification commands', () => {
  it('lists the personal inbox with bounded unread pagination', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(['notifications', 'list', '--unread-only', '--limit=25'], {
      ...output.runtime,
      environment: environment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(PAGE);
      },
    });

    expect(exitCode).toBe(0);
    expect(String(calls[0]?.[0])).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notifications?unreadOnly=true&limit=25`
    );
    expect(output.stdout()).toContain('OCCURRED AT');
    expect(output.stdout()).toContain('Organization role changed');
    expect(output.stdout()).toContain('Next cursor: v1:');
    expect(output.stderr()).toBe('');
  });

  it('marks one exact notification read with concurrency and idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const read = {
      ...NOTIFICATION,
      aggregateVersion: 2,
      readAt: '2026-08-14T01:03:00Z',
    };
    const exitCode = await runCli(
      [
        'notifications',
        'read',
        NOTIFICATION_ID,
        '--expected-version=1',
        '--idempotency-key=cli:notification:read',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: environment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ notification: read, replayed: false });
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notifications/${NOTIFICATION_ID}/read`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:notification:read' }),
        body: JSON.stringify({ expectedVersion: 1 }),
      })
    );
    expect(output.stdout()).toContain('"aggregateVersion": 2');
    expect(output.stdout()).toContain('"replayed": false');
    expect(output.stderr()).toBe('');
  });

  it.each([
    [['notifications', 'list', '--limit=201'], 'notification limit must be between 1 and 200'],
    [['organizations', 'list', '--unread-only'], '--unread-only is valid only for notifications list'],
    [
      ['notifications', 'get', NOTIFICATION_ID, '--unread-only'],
      '--unread-only is valid only for notifications list',
    ],
  ])('rejects invalid or misplaced options before transport %#', async (argv, message) => {
    let called = false;
    const output = capture();
    const exitCode = await runCli(argv, {
      ...output.runtime,
      environment: environment(),
      fetch: async () => {
        called = true;
        return envelope(PAGE);
      },
    });
    expect(exitCode).toBe(2);
    expect(called).toBe(false);
    expect(output.stderr()).toContain(message);
  });
});
