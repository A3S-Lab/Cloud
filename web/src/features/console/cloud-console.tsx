import { CircleDot, LogOut, PanelRightClose, PanelRightOpen, Radio, RotateCw } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { CloudApi } from '../../lib/api';
import type {
  Environment,
  GatewayCertificate,
  Operation,
  Organization,
  Project,
  Route,
  ServiceTemplate,
  Workload,
} from '../../types/api';
import { LiveLogPanel } from '../logs/live-log-panel';
import { useOperationStream } from '../operations/use-operation-stream';
import { streamLabel } from './console-format';
import { ContextBar } from './context-bar';
import { DeploymentTimeline } from './deployment-timeline';
import { EdgeStatusPanel } from './edge-status-panel';
import { AssetCatalogCard, EnvironmentHeading, InfrastructureCard } from './environment-summary';
import { OperationDrawer } from './operation-drawer';
import { isTerminalOperation } from './workload-view-model';
import { WorkloadList } from './workload-list';
import { WorkloadOverview } from './workload-overview';

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
  const [organizations, setOrganizations] = useState(initialOrganizations);
  const [organizationId, setOrganizationId] = useState(() => sessionStorage.getItem(ORGANIZATION_KEY) ?? '');
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState(() => sessionStorage.getItem(PROJECT_KEY) ?? '');
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [environmentId, setEnvironmentId] = useState(() => sessionStorage.getItem(ENVIRONMENT_KEY) ?? '');
  const [operations, setOperations] = useState<Operation[]>([]);
  const [dismissedOperationIds, setDismissedOperationIds] = useState<ReadonlySet<string>>(() => new Set());
  const [workloads, setWorkloads] = useState<Workload[]>([]);
  const [routes, setRoutes] = useState<Route[]>([]);
  const [certificates, setCertificates] = useState<GatewayCertificate[]>([]);
  const [workloadId, setWorkloadId] = useState('');
  const [drawerOpen, setDrawerOpen] = useState(() => !window.matchMedia('(max-width: 780px)').matches);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [cancellingDeploymentId, setCancellingDeploymentId] = useState<string | null>(null);
  const [stoppingWorkloadId, setStoppingWorkloadId] = useState<string | null>(null);

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
        const [workloadItems, routeItems] = await Promise.all([
          api.listWorkloads(organizationId, projectId, environmentId, controller.signal),
          api.listRoutes(organizationId, projectId, environmentId, controller.signal),
        ]);
        if (stopped) return;
        setWorkloads(workloadItems);
        setRoutes(routeItems);
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
    const [workloadItems, routeItems, certificateItems, operationItems] = await Promise.all([
      api.listWorkloads(organizationId, projectId, environmentId),
      api.listRoutes(organizationId, projectId, environmentId),
      api.listGatewayCertificates(organizationId),
      api.listOperations(organizationId),
    ]);
    setWorkloads(workloadItems);
    setRoutes(routeItems);
    setCertificates(certificateItems);
    setOperations(operationItems);
    setWorkloadId((current) => selectExisting(current, workloadItems));
  }, [api, environmentId, organizationId, projectId]);

  const selectedOrganization = organizations.find((item) => item.id === organizationId);
  const selectedProject = projects.find((item) => item.id === projectId);
  const selectedEnvironment = environments.find((item) => item.id === environmentId);
  const selectedWorkload = workloads.find((item) => item.id === workloadId);
  const latestDeployment = selectedWorkload?.deployments[0];
  const selectedRoutes = routes.filter((route) => route.workloadId === selectedWorkload?.id);
  const logRevision =
    selectedWorkload?.activeRevision ?? latestDeployment?.revision ?? selectedWorkload?.desiredRevision;
  const activeOperations = operations.filter((operation) => !isTerminalOperation(operation)).length;

  const updateSelectedWorkload = async (template: ServiceTemplate, idempotencyKey: string) => {
    if (!organizationId || !selectedWorkload) {
      const cause = new Error('Choose a workload before updating it.');
      setError(cause.message);
      throw cause;
    }
    try {
      await api.updateWorkload(organizationId, selectedWorkload.id, template, idempotencyKey);
      await refreshAuthoritativeProjections();
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
      throw cause;
    }
  };

  const rollbackSelectedWorkload = async (revisionId: string, idempotencyKey: string) => {
    if (!organizationId || !selectedWorkload) {
      const cause = new Error('Choose a workload before rolling it back.');
      setError(cause.message);
      throw cause;
    }
    try {
      await api.rollbackWorkload(organizationId, selectedWorkload.id, revisionId, idempotencyKey);
      await refreshAuthoritativeProjections();
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
      throw cause;
    }
  };

  const cancelLatestDeployment = async () => {
    if (!organizationId || !latestDeployment) return;
    setCancellingDeploymentId(latestDeployment.id);
    try {
      await api.cancelDeployment(organizationId, latestDeployment.id, `web-cancel:${latestDeployment.id}`);
      await refreshAuthoritativeProjections();
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
    } finally {
      setCancellingDeploymentId(null);
    }
  };

  const stopSelectedWorkload = async () => {
    if (!organizationId || !selectedWorkload) return;
    setStoppingWorkloadId(selectedWorkload.id);
    try {
      await api.stopWorkload(organizationId, selectedWorkload.id, `web-stop:${selectedWorkload.id}`);
      await refreshAuthoritativeProjections();
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
    } finally {
      setStoppingWorkloadId(null);
    }
  };

  const dismissTerminalOperations = (operationIds: string[]) => {
    setDismissedOperationIds((current) => {
      const next = new Set(current);
      for (const operationId of operationIds) next.add(operationId);
      return next;
    });
  };

  return (
    <div className={drawerOpen ? 'console-shell drawer-visible' : 'console-shell'}>
      <header className='topbar'>
        <div className='brand-lockup compact'>
          <span className='brand-mark' aria-hidden='true'>
            A3
          </span>
          <div>
            <strong>A3S Cloud</strong>
            <span>Control plane</span>
          </div>
        </div>
        <div className='topbar-actions'>
          <span className={`connection-pill ${streamState}`}>
            <Radio size={14} /> {streamLabel(streamState)}
          </span>
          <button className='icon-button' type='button' onClick={() => setDrawerOpen((open) => !open)}>
            {drawerOpen ? <PanelRightClose size={18} /> : <PanelRightOpen size={18} />}
            <span className='sr-only'>{drawerOpen ? 'Close operations' : 'Open operations'}</span>
          </button>
          <button className='quiet-button' type='button' onClick={onSignOut}>
            <LogOut size={16} /> Sign out
          </button>
        </div>
      </header>

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

        <section className='dashboard-grid' aria-label='Environment status'>
          <WorkloadOverview
            workload={selectedWorkload}
            routes={selectedRoutes}
            cancelling={cancellingDeploymentId === latestDeployment?.id}
            stopping={stoppingWorkloadId === selectedWorkload?.id}
            onCancel={cancelLatestDeployment}
            onStop={stopSelectedWorkload}
            onUpdate={updateSelectedWorkload}
            onRollback={rollbackSelectedWorkload}
          />
          <InfrastructureCard deployment={latestDeployment} routes={selectedRoutes} />
          <AssetCatalogCard />
        </section>

        <section className='workload-detail-grid' aria-label='Selected workload details'>
          <DeploymentTimeline workload={selectedWorkload} operations={operations} />
          <EdgeStatusPanel workload={selectedWorkload} routes={selectedRoutes} certificates={certificates} />
        </section>

        <LiveLogPanel
          api={api}
          organizationId={organizationId || null}
          workloadId={selectedWorkload?.id ?? null}
          revisionId={logRevision?.id ?? null}
          generation={logRevision?.generation ?? null}
        />

        <WorkloadList
          workloads={workloads}
          selectedWorkloadId={workloadId}
          environment={selectedEnvironment}
          onSelect={setWorkloadId}
        />
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
