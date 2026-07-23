import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudApi } from '../../lib/api';
import type { BuildEvidence, BuildRun } from '../../types/api';
import { BuildEvidencePanel } from './build-evidence-panel';

let root: Root | null = null;
let createObjectUrlDescriptor: PropertyDescriptor | undefined;
let revokeObjectUrlDescriptor: PropertyDescriptor | undefined;

beforeEach(() => {
  createObjectUrlDescriptor = Object.getOwnPropertyDescriptor(URL, 'createObjectURL');
  revokeObjectUrlDescriptor = Object.getOwnPropertyDescriptor(URL, 'revokeObjectURL');
  document.body.innerHTML = '<div id="root"></div>';
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  vi.restoreAllMocks();
  restoreUrlProperty('createObjectURL', createObjectUrlDescriptor);
  restoreUrlProperty('revokeObjectURL', revokeObjectUrlDescriptor);
});

describe('BuildEvidencePanel', () => {
  it('shows the signed summary and loads the full document only on demand', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const api = new CloudApi('token');
    const getBuildEvidence = vi.spyOn(api, 'getBuildEvidence').mockResolvedValue(evidence());
    root = createRoot(host);

    await act(async () => {
      root?.render(<BuildEvidencePanel api={api} organizationId='organization-1' buildRun={buildRun()} />);
    });

    expect(host.textContent).toContain('Verified evidence');
    expect(host.textContent).toContain('sha256:bbbbbbbbbbbb');
    expect(getBuildEvidence).not.toHaveBeenCalled();

    const view = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('View evidence JSON')
    );
    await act(async () => {
      view?.click();
      await Promise.resolve();
    });

    expect(getBuildEvidence).toHaveBeenCalledWith('organization-1', 'build-1', expect.any(AbortSignal));
    expect(host.querySelector('pre')?.textContent).toContain('"schema": "a3s.cloud.build-evidence.v1"');
  });

  it('downloads the exact fetched evidence as JSON', async () => {
    const host = document.getElementById('root');
    if (!host) throw new Error('test root is missing');
    const api = new CloudApi('token');
    vi.spyOn(api, 'getBuildEvidence').mockResolvedValue(evidence());
    const createObjectURL = vi.fn(() => 'blob:a3s-evidence');
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: createObjectURL,
    });
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: revokeObjectURL,
    });
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
    root = createRoot(host);

    await act(async () => {
      root?.render(<BuildEvidencePanel api={api} organizationId='organization-1' buildRun={buildRun()} />);
    });
    const download = [...host.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Download JSON')
    );
    await act(async () => {
      download?.click();
      await Promise.resolve();
    });

    expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));
    expect(click).toHaveBeenCalledOnce();
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:a3s-evidence');
  });
});

function buildRun(): BuildRun {
  return {
    organizationId: 'organization-1',
    projectId: 'project-1',
    environmentId: 'environment-1',
    id: 'build-1',
    sourceRevisionId: 'source-1',
    attempt: 1,
    retryOfBuildRunId: null,
    operationId: 'operation-1',
    status: 'succeeded',
    sourceContentDigest: `sha256:${'a'.repeat(64)}`,
    output: null,
    publicationTarget: null,
    publishedArtifact: null,
    evidenceSummary: {
      schema: 'a3s.cloud.build-evidence.v1',
      verificationState: 'verified',
      sbomDigest: `sha256:${'b'.repeat(64)}`,
      provenanceDigest: `sha256:${'c'.repeat(64)}`,
      signingKeyAlgorithm: 'ed25519',
      signingKeyId: `sha256:${'d'.repeat(64)}`,
      signingKeyVersion: 2,
      attestedAt: '2026-07-23T00:00:00Z',
    },
    failure: null,
    aggregateVersion: 1,
    requestedAt: '2026-07-23T00:00:00Z',
    updatedAt: '2026-07-23T00:01:00Z',
    startedAt: '2026-07-23T00:00:01Z',
    cancellationRequestedAt: null,
    finishedAt: '2026-07-23T00:01:00Z',
  };
}

function evidence(): BuildEvidence {
  return {
    schema: 'a3s.cloud.build-evidence.v1',
    buildRunId: 'build-1',
    operationId: 'operation-1',
    sourceRevisionId: 'source-1',
    attempt: 1,
    repository: 'https://github.com/A3S-Lab/Cloud',
    commitSha: 'a'.repeat(40),
    sourceContentDigest: `sha256:${'a'.repeat(64)}`,
    recipe: { schema: 'a3s.cloud.build-recipe.v1' },
    recipeDigest: `sha256:${'e'.repeat(64)}`,
    runtimeSpecDigest: `sha256:${'f'.repeat(64)}`,
    builder: {
      uri: `oci://docker.io/moby/buildkit@sha256:${'1'.repeat(64)}`,
      digest: `sha256:${'1'.repeat(64)}`,
    },
    platforms: ['linux/amd64'],
    artifact: {
      uri: `oci://registry.example/a3s/builds/build-1@sha256:${'2'.repeat(64)}`,
      digest: `sha256:${'2'.repeat(64)}`,
      mediaType: 'application/vnd.oci.image.manifest.v1+json',
      sizeBytes: 128,
    },
    sbom: { spdxVersion: 'SPDX-2.3' },
    sbomDigest: `sha256:${'b'.repeat(64)}`,
    provenance: { _type: 'https://in-toto.io/Statement/v1' },
    provenanceDigest: `sha256:${'c'.repeat(64)}`,
    envelope: {
      payloadType: 'application/vnd.in-toto+json',
      payload: 'e30=',
      signatures: [{ keyId: `sha256:${'d'.repeat(64)}`, signature: 'signature' }],
    },
    signingKey: {
      algorithm: 'ed25519',
      keyId: `sha256:${'d'.repeat(64)}`,
      publicKey: 'public-key',
      keyVersion: 2,
    },
    verificationState: 'verified',
    attestedAt: '2026-07-23T00:00:00Z',
  };
}

function restoreUrlProperty(name: 'createObjectURL' | 'revokeObjectURL', descriptor?: PropertyDescriptor) {
  if (descriptor) {
    Object.defineProperty(URL, name, descriptor);
  } else {
    Reflect.deleteProperty(URL, name);
  }
}
