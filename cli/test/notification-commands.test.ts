import { describe, expect, it } from 'bun:test';
import type {
  CloudFetch,
  Notification,
  NotificationAlertPolicy,
  NotificationPage,
  OutboundNotificationSubscription,
} from '@a3s/cloud-client';
import { runCli } from '../src/cli';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const NOTIFICATION_ID = '019c0000-0000-7000-8000-000000000002';
const REQUEST_ID = '019c0000-0000-7000-8000-000000000003';
const SUBSCRIPTION_ID = '019c0000-0000-7000-8000-000000000006';
const POLICY_ID = '019c0000-0000-7000-8000-00000000000c';

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

const ALERT_POLICY: NotificationAlertPolicy = {
  organizationId: ORGANIZATION_ID,
  policyId: POLICY_ID,
  source: 'edge.gateway-certificate-renewal-status.v1',
  projectId: '019c0000-0000-7000-8000-000000000007',
  environmentId: '019c0000-0000-7000-8000-000000000008',
  notifyOnRecovery: true,
  definitionSchema: 'cloud.notification.alert-policy.v1',
  definitionAcl: 'schema = "cloud.notification.alert-policy.v1"\n',
  definitionDigest: `sha256:${'b'.repeat(64)}`,
  state: 'active',
  aggregateVersion: 1,
  createdBy: '019c0000-0000-7000-8000-00000000000b',
  createdAt: '2026-08-14T01:02:03Z',
  revokedAt: null,
};

const SUBSCRIPTION: OutboundNotificationSubscription = {
  organizationId: ORGANIZATION_ID,
  subscriptionId: SUBSCRIPTION_ID,
  channel: 'signed_webhook',
  minimumSeverity: 'warning',
  connectorProjectId: '019c0000-0000-7000-8000-000000000007',
  connectorEnvironmentId: '019c0000-0000-7000-8000-000000000008',
  connectorProfileId: '019c0000-0000-7000-8000-000000000009',
  connectorRevisionId: '019c0000-0000-7000-8000-00000000000a',
  maximumProviderAttempts: 8,
  suppressBefore: null,
  definitionSchema: 'cloud.notification.outbound-subscription.v1',
  definitionAcl: 'schema = "cloud.notification.outbound-subscription.v1"\n',
  definitionDigest: `sha256:${'a'.repeat(64)}`,
  state: 'active',
  aggregateVersion: 1,
  createdBy: '019c0000-0000-7000-8000-00000000000b',
  createdAt: '2026-08-14T01:02:03Z',
  revokedAt: null,
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

  it('manages one ACL-native personal alert policy', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const createOutput = capture();
    const exitCode = await runCli(
      [
        'notification-alert-policies',
        'create',
        '--file=alert-policy.acl',
        '--idempotency-key=cli:notification-alert-policy:create',
        '--output=json',
      ],
      {
        ...createOutput.runtime,
        environment: environment(),
        readFile: async () => new TextEncoder().encode(ALERT_POLICY.definitionAcl),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ policy: ALERT_POLICY, replayed: false });
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notification-alert-policies`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:notification-alert-policy:create',
        }),
        body: ALERT_POLICY.definitionAcl,
      })
    );
    expect(createOutput.stdout()).toContain(`"policyId": "${POLICY_ID}"`);

    const listOutput = capture();
    const listExitCode = await runCli(['notification-alert-policies', 'list', '--limit=25'], {
      ...listOutput.runtime,
      environment: environment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope({ policies: [ALERT_POLICY], nextCursor: null });
      },
    });
    expect(listExitCode).toBe(0);
    expect(listOutput.stdout()).toContain('POLICY ID');
    expect(listOutput.stdout()).toContain('edge.gateway-certificate-renewal-status.v1');

    const getOutput = capture();
    const getExitCode = await runCli(['notification-alert-policies', 'get', POLICY_ID], {
      ...getOutput.runtime,
      environment: environment(),
      fetch: async (...args) => {
        calls.push(args);
        return envelope(ALERT_POLICY);
      },
    });
    expect(getExitCode).toBe(0);
    expect(getOutput.stdout()).toContain(POLICY_ID);

    const revokeOutput = capture();
    const revokeExitCode = await runCli(
      [
        'notification-alert-policies',
        'revoke',
        POLICY_ID,
        '--expected-version=1',
        '--idempotency-key=cli:notification-alert-policy:revoke',
      ],
      {
        ...revokeOutput.runtime,
        environment: environment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({
            policy: { ...ALERT_POLICY, state: 'revoked', aggregateVersion: 2 },
            replayed: false,
          });
        },
      }
    );
    expect(revokeExitCode).toBe(0);
    expect(calls[3]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notification-alert-policies/${POLICY_ID}/revoke`
    );
    expect(calls[3]?.[1]).toEqual(expect.objectContaining({ body: JSON.stringify({ expectedVersion: 1 }) }));
    expect(revokeOutput.stdout()).toContain('revoked');
  });

  it('creates an ACL-native outbound subscription and revokes it through the same authority', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const acl = new TextEncoder().encode(SUBSCRIPTION.definitionAcl);
    const exitCode = await runCli(
      [
        'notification-subscriptions',
        'create',
        '--file=subscription.acl',
        '--idempotency-key=cli:notification-subscription:create',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: environment(),
        readFile: async () => acl,
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ subscription: SUBSCRIPTION, replayed: false });
        },
      }
    );

    expect(exitCode).toBe(0);
    expect(calls[0]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notification-outbound-subscriptions`
    );
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          'Content-Type': 'application/vnd.a3s.acl',
          'Idempotency-Key': 'cli:notification-subscription:create',
        }),
        body: SUBSCRIPTION.definitionAcl,
      })
    );
    expect(output.stdout()).toContain(`"subscriptionId": "${SUBSCRIPTION_ID}"`);

    const getOutput = capture();
    const getExitCode = await runCli(['notification-subscriptions', 'get', SUBSCRIPTION_ID], {
      ...getOutput.runtime,
      environment: environment(),
      fetch: async () =>
        envelope({
          ...SUBSCRIPTION,
          definitionSchema: 'cloud.notification.outbound-subscription.v3',
          maximumProviderAttempts: 3,
          suppressBefore: '2026-08-15T01:02:03Z',
        }),
    });
    expect(getExitCode).toBe(0);
    expect(getOutput.stdout()).toContain('ATTEMPTS');
    expect(getOutput.stdout()).toContain('3');
    expect(getOutput.stdout()).toContain('SUPPRESS BEFORE');
    expect(getOutput.stdout()).toContain('2026-08-15T01:02:03Z');

    const revokeOutput = capture();
    const revokeExitCode = await runCli(
      [
        'notification-subscriptions',
        'revoke',
        SUBSCRIPTION_ID,
        '--expected-version=1',
        '--idempotency-key=cli:notification-subscription:revoke',
      ],
      {
        ...revokeOutput.runtime,
        environment: environment(),
        fetch: async (...args) => {
          calls.push(args);
          return envelope({
            subscription: { ...SUBSCRIPTION, state: 'revoked', aggregateVersion: 2 },
            replayed: false,
          });
        },
      }
    );
    expect(revokeExitCode).toBe(0);
    expect(calls[1]?.[0]).toBe(
      `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/notification-outbound-subscriptions/${SUBSCRIPTION_ID}/revoke`
    );
    expect(calls[1]?.[1]).toEqual(expect.objectContaining({ body: JSON.stringify({ expectedVersion: 1 }) }));
    expect(revokeOutput.stdout()).toContain('revoked');
  });

  it.each([
    [['notifications', 'list', '--limit=201'], 'notification limit must be between 1 and 200'],
    [
      ['notification-alert-policies', 'list', '--limit=201'],
      'notification alert policy limit must be between 1 and 200',
    ],
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
