import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CloudApi } from '../../lib/api';
import type { Deployment, ServiceTemplate, Workload } from '../../types/api';
import { useConsoleActions } from './use-console-actions';

type ConsoleActions = ReturnType<typeof useConsoleActions>;
type ConsoleActionsOptions = Parameters<typeof useConsoleActions>[0];

let root: Root | null = null;
let currentActions: ConsoleActions | null = null;

beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(async () => {
  if (root) {
    await act(async () => root?.unmount());
    root = null;
  }
  currentActions = null;
  vi.restoreAllMocks();
});

describe('useConsoleActions', () => {
  it('coordinates existing Cloud mutations, refresh, and replay keys', async () => {
    const api = mutationApi();
    const refresh = vi.fn().mockResolvedValue(undefined);
    const onBuildRunSelected = vi.fn();
    const onError = vi.fn();
    const onSuccess = vi.fn();
    await renderActions({
      api,
      organizationId: 'organization-1',
      workload: { id: 'workload-1' } as Workload,
      deployment: { id: 'deployment-1' } as Deployment,
      refresh,
      onBuildRunSelected,
      onError,
      onSuccess,
    });

    const template = serviceTemplate();
    await act(async () => current().updateSelectedWorkload(template, 'update-key'));
    await act(async () => current().rollbackSelectedWorkload('revision-1', 'rollback-key'));
    await act(async () => current().cancelLatestDeployment());
    await act(async () => current().stopSelectedWorkload());
    await act(async () => current().cancelBuildRun('build-run-1'));
    await act(async () => current().retryBuildRun('build-run-1'));

    expect(api.updateWorkload).toHaveBeenCalledWith('organization-1', 'workload-1', template, 'update-key');
    expect(api.rollbackWorkload).toHaveBeenCalledWith(
      'organization-1',
      'workload-1',
      'revision-1',
      'rollback-key'
    );
    expect(api.cancelDeployment).toHaveBeenCalledWith(
      'organization-1',
      'deployment-1',
      'web-cancel:deployment-1'
    );
    expect(api.stopWorkload).toHaveBeenCalledWith('organization-1', 'workload-1', 'web-stop:workload-1');
    expect(api.cancelBuildRun).toHaveBeenCalledWith(
      'organization-1',
      'build-run-1',
      'web-cancel-build:build-run-1'
    );
    expect(api.retryBuildRun).toHaveBeenCalledWith(
      'organization-1',
      'build-run-1',
      'web-retry-build:build-run-1'
    );
    expect(onBuildRunSelected).toHaveBeenCalledWith('build-run-retry');
    expect(refresh).toHaveBeenCalledTimes(6);
    expect(onSuccess).toHaveBeenCalledTimes(6);
    expect(onError).not.toHaveBeenCalled();
  });

  it('reports a missing selection and preserves mutation failures', async () => {
    const api = mutationApi();
    vi.mocked(api.cancelBuildRun).mockRejectedValueOnce(new Error('cancel rejected'));
    const onError = vi.fn();
    const onSuccess = vi.fn();
    await renderActions({
      api,
      organizationId: 'organization-1',
      workload: undefined,
      deployment: undefined,
      refresh: vi.fn().mockResolvedValue(undefined),
      onBuildRunSelected: vi.fn(),
      onError,
      onSuccess,
    });

    let missingSelection: unknown;
    await act(async () => {
      try {
        await current().updateSelectedWorkload(serviceTemplate(), 'update-key');
      } catch (cause) {
        missingSelection = cause;
      }
    });
    await act(async () => current().cancelBuildRun('build-run-1'));

    expect(missingSelection).toEqual(new Error('Choose a workload before updating it.'));
    expect(onError).toHaveBeenNthCalledWith(1, missingSelection);
    expect(onError).toHaveBeenNthCalledWith(2, new Error('cancel rejected'));
    expect(onSuccess).not.toHaveBeenCalled();
    expect(current().cancellingBuildRunId).toBeNull();
  });
});

function ActionHarness({ options }: { options: ConsoleActionsOptions }) {
  currentActions = useConsoleActions(options);
  return null;
}

async function renderActions(options: ConsoleActionsOptions) {
  const host = document.getElementById('root');
  if (!host) throw new Error('test root is missing');
  root = createRoot(host);
  await act(async () => root?.render(<ActionHarness options={options} />));
}

function current(): ConsoleActions {
  if (!currentActions) throw new Error('console actions are unavailable');
  return currentActions;
}

function mutationApi(): CloudApi {
  return {
    updateWorkload: vi.fn().mockResolvedValue({}),
    rollbackWorkload: vi.fn().mockResolvedValue({}),
    cancelDeployment: vi.fn().mockResolvedValue({}),
    stopWorkload: vi.fn().mockResolvedValue({}),
    cancelBuildRun: vi.fn().mockResolvedValue({}),
    retryBuildRun: vi.fn().mockResolvedValue({ buildRunId: 'build-run-retry' }),
  } as unknown as CloudApi;
}

function serviceTemplate(): ServiceTemplate {
  return {
    artifact: { uri: 'registry.example.com/app@sha256:1234', expectedDigest: null },
    process: { command: ['/app'], args: [], workingDirectory: null, environment: {} },
    secrets: [],
    resources: { cpuMillis: 100, memoryBytes: 128_000_000, pids: 32, ephemeralStorageBytes: null },
    ports: [],
    health: {
      portName: 'http',
      path: '/health',
      intervalMs: 1_000,
      timeoutMs: 500,
      healthyThreshold: 1,
      unhealthyThreshold: 3,
      stabilizationWindowMs: 5_000,
    },
  };
}
