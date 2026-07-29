import { useCallback, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { Deployment, ServiceTemplate, Workload } from '../../types/api';

interface ConsoleActionsOptions {
  api: CloudApi;
  organizationId: string;
  workload: Workload | undefined;
  deployment: Deployment | undefined;
  refresh: () => Promise<void>;
  onBuildRunSelected: (buildRunId: string) => void;
  onError: (cause: unknown) => void;
  onSuccess: () => void;
}

export function useConsoleActions({
  api,
  organizationId,
  workload,
  deployment,
  refresh,
  onBuildRunSelected,
  onError,
  onSuccess,
}: ConsoleActionsOptions) {
  const [cancellingDeploymentId, setCancellingDeploymentId] = useState<string | null>(null);
  const [cancellingBuildRunId, setCancellingBuildRunId] = useState<string | null>(null);
  const [retryingBuildRunId, setRetryingBuildRunId] = useState<string | null>(null);
  const [stoppingWorkloadId, setStoppingWorkloadId] = useState<string | null>(null);

  const updateSelectedWorkload = useCallback(
    async (template: ServiceTemplate, idempotencyKey: string) => {
      if (!organizationId || !workload) {
        const cause = new Error('Choose a workload before updating it.');
        onError(cause);
        throw cause;
      }
      try {
        await api.updateWorkload(organizationId, workload.id, template, idempotencyKey);
        await refresh();
        onSuccess();
      } catch (cause) {
        onError(cause);
        throw cause;
      }
    },
    [api, onError, onSuccess, organizationId, refresh, workload]
  );

  const rollbackSelectedWorkload = useCallback(
    async (revisionId: string, idempotencyKey: string) => {
      if (!organizationId || !workload) {
        const cause = new Error('Choose a workload before rolling it back.');
        onError(cause);
        throw cause;
      }
      try {
        await api.rollbackWorkload(organizationId, workload.id, revisionId, idempotencyKey);
        await refresh();
        onSuccess();
      } catch (cause) {
        onError(cause);
        throw cause;
      }
    },
    [api, onError, onSuccess, organizationId, refresh, workload]
  );

  const cancelLatestDeployment = useCallback(async () => {
    if (!organizationId || !deployment) return;
    setCancellingDeploymentId(deployment.id);
    try {
      await api.cancelDeployment(organizationId, deployment.id, `web-cancel:${deployment.id}`);
      await refresh();
      onSuccess();
    } catch (cause) {
      onError(cause);
    } finally {
      setCancellingDeploymentId(null);
    }
  }, [api, deployment, onError, onSuccess, organizationId, refresh]);

  const stopSelectedWorkload = useCallback(async () => {
    if (!organizationId || !workload) return;
    setStoppingWorkloadId(workload.id);
    try {
      await api.stopWorkload(organizationId, workload.id, `web-stop:${workload.id}`);
      await refresh();
      onSuccess();
    } catch (cause) {
      onError(cause);
    } finally {
      setStoppingWorkloadId(null);
    }
  }, [api, onError, onSuccess, organizationId, refresh, workload]);

  const cancelBuildRun = useCallback(
    async (buildRunId: string) => {
      if (!organizationId) return;
      setCancellingBuildRunId(buildRunId);
      try {
        await api.cancelBuildRun(organizationId, buildRunId, `web-cancel-build:${buildRunId}`);
        await refresh();
        onSuccess();
      } catch (cause) {
        onError(cause);
      } finally {
        setCancellingBuildRunId(null);
      }
    },
    [api, onError, onSuccess, organizationId, refresh]
  );

  const retryBuildRun = useCallback(
    async (buildRunId: string) => {
      if (!organizationId) return;
      setRetryingBuildRunId(buildRunId);
      try {
        const retry = await api.retryBuildRun(organizationId, buildRunId, `web-retry-build:${buildRunId}`);
        await refresh();
        onBuildRunSelected(retry.buildRunId);
        onSuccess();
      } catch (cause) {
        onError(cause);
      } finally {
        setRetryingBuildRunId(null);
      }
    },
    [api, onBuildRunSelected, onError, onSuccess, organizationId, refresh]
  );

  return {
    cancelBuildRun,
    cancelLatestDeployment,
    cancellingBuildRunId,
    cancellingDeploymentId,
    retryBuildRun,
    retryingBuildRunId,
    rollbackSelectedWorkload,
    stopSelectedWorkload,
    stoppingWorkloadId,
    updateSelectedWorkload,
  };
}
