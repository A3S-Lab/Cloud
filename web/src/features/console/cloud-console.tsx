import { CircleDot, RotateCw } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { CloudApi } from '../../lib/api';
import type {
  BuildRun,
  Environment,
  GatewayCertificate,
  Operation,
  Organization,
  Project,
  Route,
  SearchResult,
  Workload,
} from '../../types/api';
import { useOperationStream } from '../operations/use-operation-stream';
import { parseCloudLocation, selectionFromSearchResult, type CloudLocation } from '../search/cloud-location';
import { ConsoleNavigation, sectionForResourceKind } from './console-navigation';
import { DeliverySection, EdgeSection, OverviewSection, WorkloadsSection } from './console-sections';
import { ContextBar } from './context-bar';
import { ConsoleTopbar } from './console-topbar';
import { EnvironmentHeading } from './environment-summary';
import { OperationDrawer } from './operation-drawer';
import { useConsoleActions } from './use-console-actions';
import { isTerminalOperation } from './workload-view-model';

interface CloudConsoleProps {
  token: string;
  initialOrganizations: Organization[];
  onSignOut: () => void;
}

const ORGANIZATION_KEY = 'a3s-cloud.organization';
const PROJECT_KEY = 'a3s-cloud.project';
const ENVIRONMENT_KEY = 'a3s-cloud.environment';
const PROJECTION_REFRESH_MS = 5_000;

export function CloudConsole({ token, initialOrganizations, onSignOut }: CloudConsoleProps) {
  const api = useMemo(() => new CloudApi(token), [token]);
  const initialLocation = useMemo(() => parseCloudLocation(window.location.hash), []);
  const [activeSection, setActiveSection] = useState(() =>
    sectionForResourceKind(initialLocation?.resourceKind ?? null)
  );
  const [organizations, setOrganizations] = useState(initialOrganizations);
  const [organizationId, setOrganizationId] = useState(
    () => initialLocation?.organizationId ?? sessionStorage.getItem(ORGANIZATION_KEY) ?? ''
  );
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState(
    () => initialLocation?.projectId ?? sessionStorage.getItem(PROJECT_KEY) ?? ''
  );
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [environmentId, setEnvironmentId] = useState(
    () => initialLocation?.environmentId ?? sessionStorage.getItem(ENVIRONMENT_KEY) ?? ''
  );
  const [operations, setOperations] = useState<Operation[]>([]);
  const [buildRuns, setBuildRuns] = useState<BuildRun[]>([]);
  const [selectedBuildRunId, setSelectedBuildRunId] = useState(() =>
    initialLocation?.resourceKind === 'build_run' ? (initialLocation.resourceId ?? '') : ''
  );
  const [dismissedOperationIds, setDismissedOperationIds] = useState<ReadonlySet<string>>(() => new Set());
  const [workloads, setWorkloads] = useState<Workload[]>([]);
  const [routes, setRoutes] = useState<Route[]>([]);
  const [certificates, setCertificates] = useState<GatewayCertificate[]>([]);
  const [workloadId, setWorkloadId] = useState(() =>
    initialLocation?.resourceKind === 'workload' ? (initialLocation.resourceId ?? '') : ''
  );
  const [drawerOpen, setDrawerOpen] = useState(
    () => initialLocation?.resourceKind === 'operation' || !window.matchMedia('(max-width: 780px)').matches
  );
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const applyLocation = useCallback((location: CloudLocation) => {
    setOrganizationId(location.organizationId);
    setProjectId(location.projectId ?? '');
    setEnvironmentId(location.environmentId ?? '');
    setWorkloadId(location.resourceKind === 'workload' ? (location.resourceId ?? '') : '');
    setSelectedBuildRunId(location.resourceKind === 'build_run' ? (location.resourceId ?? '') : '');
    setActiveSection(sectionForResourceKind(location.resourceKind));
    if (location.resourceKind === 'operation') setDrawerOpen(true);
  }, []);

  useEffect(() => {
    const onHashChange = () => {
      const location = parseCloudLocation(window.location.hash);
      if (location) applyLocation(location);
    };
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, [applyLocation]);

  const acceptSnapshot = useCallback((snapshot: Operation[]) => {
    setOperations(snapshot);
  }, []);
  const streamState = useOperationStream(api, organizationId || null, acceptSnapshot);

  useEffect(() => {
    if (initialOrganizations.length > 0) {
      setOrganizations(initialOrganizations);
      setOrganizationId((current) => selectExisting(current, initialOrganizations));
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    setLoading(true);
    api
      .listOrganizations(controller.signal)
      .then((items) => {
        setOrganizations(items);
        setOrganizationId((current) => selectExisting(current, items));
        setError(null);
      })
      .catch((cause) => {
        if (!controller.signal.aborted) setError(messageFrom(cause));
      })
      .finally(() => setLoading(false));
    return () => controller.abort();
  }, [api, initialOrganizations]);

  useEffect(() => {
    if (!organizationId) {
      setProjects([]);
      setOperations([]);
      setBuildRuns([]);
      setSelectedBuildRunId('');
      setCertificates([]);
      setWorkloads([]);
      setRoutes([]);
      setWorkloadId('');
      return;
    }
    sessionStorage.setItem(ORGANIZATION_KEY, organizationId);
    const controller = new AbortController();
    Promise.all([
      api.listProjects(organizationId, controller.signal),
      api.listOperations(organizationId, controller.signal),
    ])
      .then(([projectItems, operationItems]) => {
        setProjects(projectItems);
        setProjectId((current) => selectExisting(current, projectItems));
        setOperations(operationItems);
        setError(null);
      })
      .catch((cause) => {
        if (!controller.signal.aborted) setError(messageFrom(cause));
      });
    return () => controller.abort();
  }, [api, organizationId]);

  useEffect(() => {
    if (!organizationId) {
      setCertificates([]);
      return;
    }
    let stopped = false;
    let refreshing = false;
    const controller = new AbortController();
    const refresh = async () => {
      if (refreshing) return;
      refreshing = true;
      try {
        const items = await api.listGatewayCertificates(organizationId, controller.signal);
        if (!stopped) setCertificates(items);
      } catch (cause) {
        if (!controller.signal.aborted) setError(messageFrom(cause));
      } finally {
        refreshing = false;
      }
    };
    void refresh();
    const interval = window.setInterval(refresh, PROJECTION_REFRESH_MS);
    return () => {
      stopped = true;
      window.clearInterval(interval);
      controller.abort();
    };
  }, [api, organizationId]);

  useEffect(() => {
    if (!organizationId || !projectId) {
      setEnvironments([]);
      setEnvironmentId('');
      return;
    }
    sessionStorage.setItem(PROJECT_KEY, projectId);
    const controller = new AbortController();
    api
      .listEnvironments(organizationId, projectId, controller.signal)
      .then((items) => {
        setEnvironments(items);
        setEnvironmentId((current) => selectExisting(current, items));
        setError(null);
      })
      .catch((cause) => {
        if (!controller.signal.aborted) setError(messageFrom(cause));
      });
    return () => controller.abort();
  }, [api, organizationId, projectId]);

  useEffect(() => {
    if (environmentId) sessionStorage.setItem(ENVIRONMENT_KEY, environmentId);
  }, [environmentId]);

  useEffect(() => {
    if (!organizationId || !projectId || !environmentId) {
      setWorkloads([]);
      setRoutes([]);
      setBuildRuns([]);
      setSelectedBuildRunId('');
      setWorkloadId('');
      return;
    }
    let stopped = false;
    let refreshing = false;
    const controller = new AbortController();
    const refresh = async () => {
      if (refreshing) return;
      refreshing = true;
      try {
        const [workloadItems, routeItems, buildItems] = await Promise.all([
          api.listWorkloads(organizationId, projectId, environmentId, controller.signal),
          api.listRoutes(organizationId, projectId, environmentId, controller.signal),
          api.listBuildRuns(organizationId, projectId, environmentId, controller.signal),
        ]);
        if (stopped) return;
        setWorkloads(workloadItems);
        setRoutes(routeItems);
        setBuildRuns(buildItems);
        setSelectedBuildRunId((current) => selectExisting(current, buildItems));
        setWorkloadId((current) => selectExisting(current, workloadItems));
        setError(null);
      } catch (cause) {
        if (!controller.signal.aborted) setError(messageFrom(cause));
      } finally {
        refreshing = false;
      }
    };
    void refresh();
    const interval = window.setInterval(refresh, PROJECTION_REFRESH_MS);
    return () => {
      stopped = true;
      window.clearInterval(interval);
      controller.abort();
    };
  }, [api, environmentId, organizationId, projectId]);

  const refreshAuthoritativeProjections = useCallback(async () => {
    if (!organizationId || !projectId || !environmentId) {
      throw new Error('Choose an organization, project, and environment first.');
    }
    const [workloadItems, routeItems, buildItems, certificateItems, operationItems] = await Promise.all([
      api.listWorkloads(organizationId, projectId, environmentId),
      api.listRoutes(organizationId, projectId, environmentId),
      api.listBuildRuns(organizationId, projectId, environmentId),
      api.listGatewayCertificates(organizationId),
      api.listOperations(organizationId),
    ]);
    setWorkloads(workloadItems);
    setRoutes(routeItems);
    setBuildRuns(buildItems);
    setSelectedBuildRunId((current) => selectExisting(current, buildItems));
    setCertificates(certificateItems);
    setOperations(operationItems);
    setWorkloadId((current) => selectExisting(current, workloadItems));
  }, [api, environmentId, organizationId, projectId]);

  const selectedOrganization = organizations.find((item) => item.id === organizationId);
  const selectedProject = projects.find((item) => item.id === projectId);
  const selectedEnvironment = environments.find((item) => item.id === environmentId);
  const selectedWorkload = workloads.find((item) => item.id === workloadId);
  const selectedBuildRun = buildRuns.find((item) => item.id === selectedBuildRunId) ?? null;
  const latestDeployment = selectedWorkload?.deployments[0];
  const selectedRoutes = routes.filter((route) => route.workloadId === selectedWorkload?.id);
  const logRevision =
    selectedWorkload?.activeRevision ?? latestDeployment?.revision ?? selectedWorkload?.desiredRevision;
  const activeOperations = operations.filter((operation) => !isTerminalOperation(operation)).length;
  const reportError = useCallback((cause: unknown) => setError(messageFrom(cause)), []);
  const clearError = useCallback(() => setError(null), []);
  const {
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
  } = useConsoleActions({
    api,
    organizationId,
    workload: selectedWorkload,
    deployment: latestDeployment,
    refresh: refreshAuthoritativeProjections,
    onBuildRunSelected: setSelectedBuildRunId,
    onError: reportError,
    onSuccess: clearError,
  });

  const dismissTerminalOperations = (operationIds: string[]) => {
    setDismissedOperationIds((current) => {
      const next = new Set(current);
      for (const operationId of operationIds) next.add(operationId);
      return next;
    });
  };

  const selectSearchResult = useCallback((result: SearchResult) => {
    const selection = selectionFromSearchResult(result);
    setOrganizationId(selection.organizationId);
    setProjectId(selection.projectId ?? '');
    setEnvironmentId(selection.environmentId ?? '');
    setWorkloadId(selection.workloadId ?? '');
    setSelectedBuildRunId(selection.buildRunId ?? '');
    setActiveSection(sectionForResourceKind(result.kind));
    if (selection.openOperations) setDrawerOpen(true);
    if (selection.href) window.history.pushState(null, '', selection.href);
  }, []);

  return (
    <div className={drawerOpen ? 'console-shell drawer-visible' : 'console-shell'}>
      <ConsoleTopbar
        api={api}
        organizationId={organizationId || null}
        streamState={streamState}
        drawerOpen={drawerOpen}
        onSelectSearchResult={selectSearchResult}
        onToggleDrawer={() => setDrawerOpen((open) => !open)}
        onSignOut={onSignOut}
      />

      <main className='workspace'>
        <ContextBar
          organizationId={organizationId}
          organizations={organizations}
          organizationLoading={loading}
          projectId={projectId}
          projects={projects}
          environmentId={environmentId}
          environments={environments}
          onOrganizationChange={(value) => {
            setOrganizationId(value);
            setProjectId('');
            setEnvironmentId('');
          }}
          onProjectChange={(value) => {
            setProjectId(value);
            setEnvironmentId('');
          }}
          onEnvironmentChange={setEnvironmentId}
        />

        {error ? (
          <div className='error-banner' role='alert'>
            <CircleDot size={16} />
            <span>{error}</span>
            <button type='button' onClick={() => window.location.reload()}>
              <RotateCw size={15} /> Retry
            </button>
          </div>
        ) : null}

        <EnvironmentHeading
          organization={selectedOrganization}
          project={selectedProject}
          environment={selectedEnvironment}
          activeOperations={activeOperations}
          workloadCount={workloads.length}
        />

        <ConsoleNavigation
          activeSection={activeSection}
          counts={{
            workloads: workloads.length,
            delivery: buildRuns.length,
            edge: routes.length,
            operations: activeOperations,
          }}
          onSelect={setActiveSection}
        />

        {activeSection === 'overview' ? (
          <OverviewSection
            activeOperations={activeOperations}
            buildRunCount={buildRuns.length}
            deployment={latestDeployment}
            routes={routes}
            workloadCount={workloads.length}
          />
        ) : null}

        {activeSection === 'workloads' ? (
          <WorkloadsSection
            api={api}
            organizationId={organizationId || null}
            environment={selectedEnvironment}
            workloads={workloads}
            workload={selectedWorkload}
            routes={selectedRoutes}
            operations={operations}
            selectedWorkloadId={workloadId}
            cancelling={cancellingDeploymentId === latestDeployment?.id}
            stopping={stoppingWorkloadId === selectedWorkload?.id}
            logRevisionId={logRevision?.id ?? null}
            logGeneration={logRevision?.generation ?? null}
            onSelectWorkload={setWorkloadId}
            onCancel={cancelLatestDeployment}
            onStop={stopSelectedWorkload}
            onUpdate={updateSelectedWorkload}
            onRollback={rollbackSelectedWorkload}
          />
        ) : null}

        {activeSection === 'delivery' ? (
          <DeliverySection
            api={api}
            organizationId={organizationId || null}
            buildRuns={buildRuns}
            selectedBuildRun={selectedBuildRun}
            selectedBuildRunId={selectedBuildRunId || null}
            cancellingBuildRunId={cancellingBuildRunId}
            retryingBuildRunId={retryingBuildRunId}
            onSelect={setSelectedBuildRunId}
            onCancel={cancelBuildRun}
            onRetry={retryBuildRun}
          />
        ) : null}

        {activeSection === 'edge' ? (
          <EdgeSection
            environment={selectedEnvironment}
            workloads={workloads}
            workload={selectedWorkload}
            routes={selectedRoutes}
            certificates={certificates}
            selectedWorkloadId={workloadId}
            onSelectWorkload={setWorkloadId}
          />
        ) : null}
      </main>

      {drawerOpen ? (
        <OperationDrawer
          operations={operations}
          dismissedOperationIds={dismissedOperationIds}
          streamState={streamState}
          onDismissTerminal={dismissTerminalOperations}
        />
      ) : null}
    </div>
  );
}

function selectExisting<T extends { id: string }>(current: string, items: T[]): string {
  return items.some((item) => item.id === current) ? current : (items[0]?.id ?? '');
}

function messageFrom(cause: unknown): string {
  return cause instanceof Error ? cause.message : 'Cloud state could not be loaded.';
}
