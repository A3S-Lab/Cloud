import { describe, expect, it } from 'bun:test';
import {
  type CloudFetch,
  USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES,
  type UserFile,
} from '@a3s/cloud-client';
import { runCli } from '../src/cli';
import { ExitCode } from '../src/errors';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const USER_FILE_ID = '019c0000-0000-7000-8000-000000000003';
const UPLOAD_ID = '019c0000-0000-7000-8000-000000000004';
const PRINCIPAL_ID = '019c0000-0000-7000-8000-000000000005';
const DIGEST = `sha256:${'a'.repeat(64)}`;
const ADMISSION_ACL = 'user_file {\n  schema = "cloud.user-file.v1"\n}\n';

describe('a3s-cloud UserFile commands', () => {
  it('reserves only from a bounded .acl file with caller-owned idempotency', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const exitCode = await runCli(
      [
        'user-files',
        'reserve',
        '--file=user-file.acl',
        '--idempotency-key=cli:files:reserve',
        '--output=json',
      ],
      {
        ...output.runtime,
        environment: completeEnvironment(),
        readFile: async (path) => {
          expect(path).toBe('user-file.acl');
          return new TextEncoder().encode(ADMISSION_ACL);
        },
        fetch: async (...args) => {
          calls.push(args);
          return envelope({ file: userFile(), replayed: false }, 201);
        },
      }
    );

    expect(exitCode).toBe(ExitCode.Success);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(`${userFileBase()}`);
    expect(calls[0]?.[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ admissionAcl: ADMISSION_ACL }),
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'Idempotency-Key': 'cli:files:reserve',
        }),
      })
    );
    expect(String(calls[0]?.[0])).not.toContain('upload');
    expect(output.stderr()).toBe('');
  });

  it('lists, gets, tombstones, and reads organization quota through exact routes', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      fetch: async (...args: Parameters<CloudFetch>) => {
        calls.push(args);
        const path = String(args[0]);
        if (path.endsWith('/user-file-quota')) {
          return envelope({
            organizationId: ORGANIZATION_ID,
            limitBytes: 50 * 1024 * 1024 * 1024,
            allocatedBytes: 128,
            availableBytes: 50 * 1024 * 1024 * 1024 - 128,
            revision: 1,
            updatedAt: '2026-08-28T00:00:00.000Z',
          });
        }
        if (path.endsWith('/tombstone')) {
          return envelope({ file: { ...userFile(), state: 'tombstoned' }, replayed: false });
        }
        return envelope(path.includes('?') ? [userFile()] : userFile());
      },
    };

    expect(await runCli(['user-files', 'list', '--limit=25', '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(await runCli(['user-files', 'get', USER_FILE_ID, '--output=json'], runtime)).toBe(
      ExitCode.Success
    );
    expect(
      await runCli(
        [
          'user-files',
          'tombstone',
          USER_FILE_ID,
          '--expected-version=1',
          '--idempotency-key=cli:files:tombstone',
          '--output=json',
        ],
        runtime
      )
    ).toBe(ExitCode.Success);
    expect(await runCli(['user-file-quota', 'get', '--output=json'], runtime)).toBe(ExitCode.Success);

    expect(calls.map(([input, init]) => [input, init?.method])).toEqual([
      [`${userFileBase()}?limit=25`, 'GET'],
      [`${userFileBase()}/${USER_FILE_ID}`, 'GET'],
      [`${userFileBase()}/${USER_FILE_ID}/tombstone`, 'POST'],
      [`http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}/user-file-quota`, 'GET'],
    ]);
    expect(calls[2]?.[1]).toEqual(
      expect.objectContaining({
        body: JSON.stringify({ expectedVersion: 1 }),
        headers: expect.objectContaining({ 'Idempotency-Key': 'cli:files:tombstone' }),
      })
    );
    expect(output.stderr()).toBe('');
  });

  it('rejects non-ACL paths, oversized ACL, list overflow, and missing versions before transport', async () => {
    let called = false;
    const output = capture();
    const runtime = {
      ...output.runtime,
      environment: completeEnvironment(),
      readFile: async () => new Uint8Array(USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES + 1),
      fetch: async () => {
        called = true;
        return envelope({});
      },
    };

    expect(
      await runCli(
        ['user-files', 'reserve', '--file=user-file.json', '--idempotency-key=cli:files:invalid'],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(
      await runCli(
        ['user-files', 'reserve', '--file=user-file.acl', '--idempotency-key=cli:files:oversized'],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(await runCli(['user-files', 'list', '--limit=201'], runtime)).toBe(ExitCode.Usage);
    expect(
      await runCli(
        ['user-files', 'tombstone', USER_FILE_ID, '--idempotency-key=cli:files:no-version'],
        runtime
      )
    ).toBe(ExitCode.Usage);
    expect(output.stderr()).toContain('.acl file');
    expect(output.stderr()).toContain('UserFile admission ACL must contain between');
    expect(output.stderr()).toContain('UserFile list limit must be between 1 and 200');
    expect(output.stderr()).toContain('--expected-version');
    expect(called).toBe(false);
  });
});

function completeEnvironment(): Record<string, string> {
  return {
    A3S_CLOUD_TOKEN: 'a3s_secret',
    A3S_CLOUD_ORGANIZATION_ID: ORGANIZATION_ID,
    A3S_CLOUD_PROJECT_ID: PROJECT_ID,
  };
}

function userFileBase(): string {
  return (
    `http://127.0.0.1:8080/api/v1/organizations/${ORGANIZATION_ID}` + `/projects/${PROJECT_ID}/user-files`
  );
}

function userFile(): UserFile {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    userFileId: USER_FILE_ID,
    uploadId: UPLOAD_ID,
    state: 'awaiting_upload',
    originalName: 'knowledge.txt',
    contractSchema: 'cloud.user-file.v1',
    admissionAcl: ADMISSION_ACL,
    contractDigest: DIGEST,
    objectRef: `organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/files/${USER_FILE_ID}/uploads/${UPLOAD_ID}/sha256/${'a'.repeat(64)}/content`,
    contentDigest: DIGEST,
    sizeBytes: 128,
    mediaType: 'text/plain',
    scanPolicy: 'required',
    uploadExpiresAt: '2026-08-29T00:00:00.000Z',
    retentionUntil: '2026-09-28T00:00:00.000Z',
    scanEvidenceDigest: null,
    rejectionReasonCode: null,
    tombstonedFrom: null,
    aggregateVersion: 1,
    createdBy: PRINCIPAL_ID,
    createdAt: '2026-08-28T00:00:00.000Z',
    uploadedAt: null,
    scannedAt: null,
    expiredAt: null,
    tombstonedAt: null,
    cleanupDueAt: null,
    updatedAt: '2026-08-28T00:00:00.000Z',
  };
}

function envelope(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000010',
      timestamp: '2026-08-28T00:00:00.000Z',
    }),
    { status }
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
