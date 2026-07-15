import {
  Activity,
  Ban,
  Bot,
  Box,
  Braces,
  ChevronRight,
  CircleDot,
  CircleStop,
  Database,
  LogOut,
  PanelRightClose,
  PanelRightOpen,
  Radio,
  RotateCw,
  Server,
  Sparkles,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { CloudApi } from '../../lib/api';
import type { Environment, Operation, Organization, Project } from '../../types/api';
import type { DeploymentStatus, Workload } from '../../types/api';
import { type StreamState, useOperationStream } from '../operations/use-operation-stream';

interface CloudConsoleProps {
  token: string;
  initialOrganizations: Organization[];
  onSignOut: () => void;
}

const ORGANIZATION_KEY = 'a3s-cloud.organization';
const PROJECT_KEY = 'a3s-cloud.project';
const ENVIRONMENT_KEY = 'a3s-cloud.environment';

export function CloudConsole({ token, initialOrganizations, onSignOut }: CloudConsoleProps) {
  const api = useMemo(() => new CloudApi(token), [token]);
  const [organizations, setOrganizations] = useState(initialOrganizations);
  const [organizationId, setOrganizationId] = useState(() => sessionStorage.getItem(ORGANIZATION_KEY) ?? '');
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState(() => sessionStorage.getItem(PROJECT_KEY) ?? '');
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [environmentId, setEnvironmentId] = useState(() => sessionStorage.getItem(ENVIRONMENT_KEY) ?? '');
  const [operations, setOperations] = useState<Operation[]>([]);
  const [workloads, setWorkloads] = useState<Workload[]>([]);
  const [workloadId, setWorkloadId] = useState('');
  const [drawerOpen, setDrawerOpen] = useState(true);
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
      .catch((cause) => setError(messageFrom(cause)))
      .finally(() => setLoading(false));
    return () => controller.abort();
  }, [api, initialOrganizations]);

  useEffect(() => {
    if (!organizationId) {
      setProjects([]);
      setOperations([]);
      setWorkloads([]);
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
      .catch((cause) => setError(messageFrom(cause)));
    return () => controller.abort();
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
      .catch((cause) => setError(messageFrom(cause)));
    return () => controller.abort();
  }, [api, organizationId, projectId]);

  useEffect(() => {
    if (environmentId) {
      sessionStorage.setItem(ENVIRONMENT_KEY, environmentId);
    }
  }, [environmentId]);

  useEffect(() => {
    if (!organizationId || !projectId || !environmentId) {
      setWorkloads([]);
      setWorkloadId('');
      return;
    }
    let stopped = false;
    const controller = new AbortController();
    const refresh = () => {
      api
        .listWorkloads(organizationId, projectId, environmentId, controller.signal)
        .then((items) => {
          if (stopped) return;
          setWorkloads(items);
          setWorkloadId((current) => selectExisting(current, items));
          setError(null);
        })
        .catch((cause) => {
          if (!controller.signal.aborted) setError(messageFrom(cause));
        });
    };
    refresh();
    const interval = window.setInterval(refresh, 5_000);
    return () => {
      stopped = true;
      window.clearInterval(interval);
      controller.abort();
    };
  }, [api, environmentId, organizationId, projectId]);

  const selectedOrganization = organizations.find((item) => item.id === organizationId);
  const selectedProject = projects.find((item) => item.id === projectId);
  const selectedEnvironment = environments.find((item) => item.id === environmentId);
  const selectedWorkload = workloads.find((item) => item.id === workloadId);
  const latestDeployment = selectedWorkload?.deployments[0];
  const observedRuntime = latestDeployment?.observedRuntime;
  const activeOperations = operations.filter((operation) => !isTerminal(operation.status)).length;
  const cancellationNotice = deploymentCancellationNotice(latestDeployment?.status);
  const stopNotice = workloadStopNotice(selectedWorkload);

  const cancelLatestDeployment = async () => {
    if (!organizationId || !selectedWorkload || !latestDeployment || !canCancel(latestDeployment.status)) {
      return;
    }
    setCancellingDeploymentId(latestDeployment.id);
    try {
      await api.cancelDeployment(organizationId, latestDeployment.id, `web-cancel:${latestDeployment.id}`);
      const [refreshedWorkload, refreshedOperations] = await Promise.all([
        api.getWorkload(organizationId, selectedWorkload.id),
        api.listOperations(organizationId),
      ]);
      setWorkloads((current) =>
        current.map((workload) => (workload.id === refreshedWorkload.id ? refreshedWorkload : workload))
      );
      setOperations(refreshedOperations);
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
    } finally {
      setCancellingDeploymentId(null);
    }
  };

  const stopSelectedWorkload = async () => {
    if (!organizationId || !selectedWorkload || !canStop(selectedWorkload)) return;
    setStoppingWorkloadId(selectedWorkload.id);
    try {
      await api.stopWorkload(organizationId, selectedWorkload.id, `web-stop:${selectedWorkload.id}`);
      const [refreshedWorkload, refreshedOperations] = await Promise.all([
        api.getWorkload(organizationId, selectedWorkload.id),
        api.listOperations(organizationId),
      ]);
      setWorkloads((current) =>
        current.map((workload) => (workload.id === refreshedWorkload.id ? refreshedWorkload : workload))
      );
      setOperations(refreshedOperations);
      setError(null);
    } catch (cause) {
      setError(messageFrom(cause));
    } finally {
      setStoppingWorkloadId(null);
    }
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
        <nav className='context-bar' aria-label='Cloud context'>
          <ContextSelect
            label='Organization'
            value={organizationId}
            items={organizations}
            disabled={loading}
            onChange={(value) => {
              setOrganizationId(value);
              setProjectId('');
              setEnvironmentId('');
            }}
          />
          <ChevronRight size={15} aria-hidden='true' />
          <ContextSelect label='Project' value={projectId} items={projects} onChange={setProjectId} />
          <ChevronRight size={15} aria-hidden='true' />
          <ContextSelect
            label='Environment'
            value={environmentId}
            items={environments}
            onChange={setEnvironmentId}
          />
        </nav>

        {error ? (
          <div className='error-banner' role='alert'>
            <CircleDot size={16} />
            <span>{error}</span>
            <button type='button' onClick={() => window.location.reload()}>
              <RotateCw size={15} /> Retry
            </button>
          </div>
        ) : null}

        <section className='environment-heading'>
          <div>
            <p className='eyebrow'>Observed workspace</p>
            <h1>
              {selectedEnvironment?.name ?? selectedProject?.name ?? selectedOrganization?.name ?? 'Cloud'}
            </h1>
            <p>
              {selectedEnvironment
                ? `${selectedOrganization?.name} / ${selectedProject?.name} / ${selectedEnvironment.name}`
                : 'Choose a project and environment to inspect its desired state.'}
            </p>
          </div>
          <div className='heading-facts'>
            <span>
              <Activity size={15} /> {activeOperations} active operation{activeOperations === 1 ? '' : 's'}
            </span>
            <span>
              <Box size={15} /> {workloads.length} workload{workloads.length === 1 ? '' : 's'}
            </span>
            <span>
              <Database size={15} /> desired state authoritative
            </span>
          </div>
        </section>

        <section className='dashboard-grid' aria-label='Environment status'>
          <article className='surface convergence-card'>
            <div className='surface-heading'>
              <div>
                <p className='eyebrow'>Convergence</p>
                <h2>{selectedWorkload?.name ?? 'Deployment state'}</h2>
              </div>
              <div className='surface-actions'>
                <span className={`state-badge ${latestDeployment?.status ?? 'neutral'}`}>
                  {latestDeployment ? humanize(latestDeployment.status) : 'Awaiting workload'}
                </span>
                {latestDeployment && canCancel(latestDeployment.status) ? (
                  <button
                    className='danger-button compact'
                    type='button'
                    disabled={cancellingDeploymentId === latestDeployment.id}
                    onClick={cancelLatestDeployment}
                  >
                    <Ban size={14} />
                    {cancellingDeploymentId === latestDeployment.id ? 'Requesting…' : 'Cancel'}
                  </button>
                ) : null}
                {selectedWorkload && canStop(selectedWorkload) ? (
                  <button
                    className='danger-button compact'
                    type='button'
                    disabled={stoppingWorkloadId === selectedWorkload.id}
                    onClick={stopSelectedWorkload}
                  >
                    <CircleStop size={14} />
                    {stoppingWorkloadId === selectedWorkload.id ? 'Stopping…' : 'Stop'}
                  </button>
                ) : null}
              </div>
            </div>
            <ol className='convergence-track' aria-label='Deployment convergence stages'>
              {deploymentStages(latestDeployment?.status).map((stage, index) => (
                <li className={`convergence-step ${stage.state}`} key={stage.name}>
                  <span>{index + 1}</span>
                  <div>
                    <strong>{stage.name}</strong>
                    <small>{stage.label}</small>
                  </div>
                </li>
              ))}
            </ol>
            {selectedWorkload ? (
              <dl className='deployment-facts'>
                <div>
                  <dt>Desired revision</dt>
                  <dd>{revisionLabel(selectedWorkload.desiredRevision)}</dd>
                </div>
                <div>
                  <dt>Active revision</dt>
                  <dd>{revisionLabel(selectedWorkload.activeRevision)}</dd>
                </div>
                <div>
                  <dt>Observed generation</dt>
                  <dd>{observedRuntime ? `Generation ${observedRuntime.generation}` : 'No evidence'}</dd>
                </div>
                <div>
                  <dt>Runtime / health</dt>
                  <dd>
                    {observedRuntime
                      ? `${observedRuntime.state} / ${observedRuntime.healthState ?? 'not reported'}`
                      : 'Not observed'}
                  </dd>
                </div>
              </dl>
            ) : (
              <p className='surface-note'>
                A deployment appears here only after its committed operation is observable.
              </p>
            )}
            {cancellationNotice ? (
              <output className={`deployment-notice ${cancellationNotice.tone}`}>
                <strong>{cancellationNotice.title}</strong>
                <span>{cancellationNotice.detail}</span>
              </output>
            ) : null}
            {stopNotice ? (
              <output className={`deployment-notice ${stopNotice.tone}`}>
                <strong>{stopNotice.title}</strong>
                <span>{stopNotice.detail}</span>
              </output>
            ) : null}
          </article>

          <article className='surface infrastructure-card'>
            <div className='surface-heading'>
              <div>
                <p className='eyebrow'>Execution boundary</p>
                <h2>Infrastructure</h2>
              </div>
              <Server size={20} />
            </div>
            <dl className='fact-list'>
              <div>
                <dt>Runtime</dt>
                <dd>Task + Service</dd>
              </div>
              <div>
                <dt>Operation authority</dt>
                <dd>A3S Flow</dd>
              </div>
              <div>
                <dt>Node</dt>
                <dd>{latestDeployment?.nodeId ? shortId(latestDeployment.nodeId) : 'Not scheduled'}</dd>
              </div>
              <div>
                <dt>Edge</dt>
                <dd>Not published</dd>
              </div>
            </dl>
          </article>

          <article className='surface assets-card'>
            <div className='surface-heading'>
              <div>
                <p className='eyebrow'>Release catalog</p>
                <h2>A3S assets</h2>
              </div>
              <Sparkles size={20} />
            </div>
            <div className='asset-kinds'>
              <AssetKind icon={<Bot size={18} />} name='Agent' />
              <AssetKind icon={<Braces size={18} />} name='MCP' />
              <AssetKind icon={<Box size={18} />} name='Skill' />
            </div>
            <p className='surface-note'>
              Immutable releases will use the common workload and deployment path.
            </p>
          </article>
        </section>

        <section className='workload-section' aria-label='Workloads'>
          <div className='section-heading'>
            <div>
              <p className='eyebrow'>Desired and observed state</p>
              <h2>Workloads</h2>
            </div>
            <span>{selectedEnvironment ? selectedEnvironment.name : 'Select an environment'}</span>
          </div>
          {workloads.length === 0 ? (
            <div className='surface workload-empty'>
              <Box size={22} />
              <strong>No workloads in this environment</strong>
              <p>Create a digest-bound Service deployment to start convergence.</p>
            </div>
          ) : (
            <div className='workload-list'>
              {workloads.map((workload) => {
                const deployment = workload.deployments[0];
                return (
                  <button
                    className={workload.id === workloadId ? 'workload-row selected' : 'workload-row'}
                    type='button'
                    key={workload.id}
                    onClick={() => setWorkloadId(workload.id)}
                  >
                    <span className={`workload-state ${deployment?.status ?? 'neutral'}`} />
                    <span className='workload-identity'>
                      <strong>{workload.name}</strong>
                      <small>
                        {workload.desiredRevision?.artifactUri ??
                          workload.desiredRevision?.artifactSourceUri ??
                          'No desired revision'}
                      </small>
                    </span>
                    <span>
                      <small>Desired</small>
                      <strong>{revisionLabel(workload.desiredRevision)}</strong>
                    </span>
                    <span>
                      <small>Observed</small>
                      <strong>
                        {deployment?.observedRuntime
                          ? `${deployment.observedRuntime.state} · ${deployment.observedRuntime.healthState ?? 'no health'}`
                          : 'No evidence'}
                      </strong>
                    </span>
                    <span>
                      <small>Operation</small>
                      <strong>{deployment?.operation?.status ?? deployment?.status ?? 'queued'}</strong>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </section>
      </main>

      {drawerOpen ? <OperationDrawer operations={operations} streamState={streamState} /> : null}
    </div>
  );
}

interface NamedItem {
  id: string;
  name: string;
}

function ContextSelect({
  label,
  value,
  items,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  items: NamedItem[];
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label className='context-select'>
      <span>{label}</span>
      <select
        value={value}
        disabled={disabled || items.length === 0}
        onChange={(event) => onChange(event.target.value)}
      >
        {items.length === 0 ? <option value=''>None yet</option> : null}
        {items.map((item) => (
          <option value={item.id} key={item.id}>
            {item.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function AssetKind({ icon, name }: { icon: React.ReactNode; name: string }) {
  return (
    <div>
      <span>{icon}</span>
      <strong>{name}</strong>
      <small>No releases</small>
    </div>
  );
}

function OperationDrawer({ operations, streamState }: { operations: Operation[]; streamState: StreamState }) {
  return (
    <aside className='operation-drawer' aria-label='Operations'>
      <div className='drawer-heading'>
        <div>
          <p className='eyebrow'>Durable timeline</p>
          <h2>Operations</h2>
        </div>
        <output className={`stream-dot ${streamState}`} aria-label={streamLabel(streamState)} />
      </div>
      <div className='operation-list'>
        {operations.length === 0 ? (
          <div className='empty-operations'>
            <Activity size={22} />
            <strong>No operations yet</strong>
            <p>Committed deployment, rollback, build, and repair work will appear here.</p>
          </div>
        ) : (
          operations.map((operation) => (
            <article className='operation-item' key={operation.id}>
              <span className={`operation-status ${operation.status}`} />
              <div>
                <div className='operation-title'>
                  <strong>{humanize(operation.subjectKind)}</strong>
                  <span>{operation.status}</span>
                </div>
                <p>{operation.workflowName}</p>
                <small>
                  seq {operation.lastSequence} · {formatRelative(operation.updatedAt)}
                </small>
                {operation.error ? <em>{operation.error}</em> : null}
              </div>
            </article>
          ))
        )}
      </div>
    </aside>
  );
}

function selectExisting<T extends { id: string }>(current: string, items: T[]): string {
  return items.some((item) => item.id === current) ? current : (items[0]?.id ?? '');
}

function isTerminal(status: Operation['status']): boolean {
  return status === 'succeeded' || status === 'failed' || status === 'cancelled';
}

function streamLabel(state: StreamState): string {
  if (state === 'live') return 'Live';
  if (state === 'retrying') return 'Reconnecting';
  if (state === 'connecting') return 'Connecting';
  return 'Idle';
}

function messageFrom(cause: unknown): string {
  return cause instanceof Error ? cause.message : 'Cloud state could not be loaded.';
}

function humanize(value: string): string {
  return value.replaceAll('_', ' ').replace(/^./, (character) => character.toUpperCase());
}

function formatRelative(value: string): string {
  const elapsed = Math.max(0, Date.now() - new Date(value).getTime());
  if (elapsed < 60_000) return 'just now';
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`;
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value));
}

function revisionLabel(revision: Workload['desiredRevision']): string {
  return revision ? `Generation ${revision.generation}` : 'None';
}

function shortId(value: string): string {
  return value.slice(0, 8);
}

function deploymentStages(status?: DeploymentStatus): Array<{
  name: string;
  label: string;
  state: 'pending' | 'current' | 'complete' | 'failed';
}> {
  const stages = [
    { name: 'Desired state', threshold: 0 },
    { name: 'Runtime apply', threshold: 3 },
    { name: 'Health proof', threshold: 5 },
    { name: 'Route active', threshold: 7 },
  ];
  const rank: Record<DeploymentStatus, number> = {
    queued: 0,
    resolving: 1,
    scheduled: 2,
    applying: 3,
    verifying: 5,
    cancelling: 5,
    cleanup_pending: 5,
    active: 6,
    failed: 6,
    orphaned: 6,
    cancelled: 6,
  };
  const current = status ? rank[status] : -1;
  return stages.map((stage, index) => {
    if (
      (status === 'failed' || status === 'orphaned' || status === 'cancelled') &&
      index < 3 &&
      stage.threshold >= current
    ) {
      return { ...stage, label: status, state: 'failed' as const };
    }
    if (stage.name === 'Route active') {
      return { ...stage, label: 'E0 not published', state: 'pending' as const };
    }
    if (current > stage.threshold) return { ...stage, label: 'Complete', state: 'complete' as const };
    if (current === stage.threshold)
      return { ...stage, label: status ?? 'Not requested', state: 'current' as const };
    return { ...stage, label: 'Pending', state: 'pending' as const };
  });
}

function canCancel(status: DeploymentStatus): boolean {
  return (
    status === 'queued' ||
    status === 'resolving' ||
    status === 'scheduled' ||
    status === 'applying' ||
    status === 'verifying'
  );
}

function canStop(workload: Workload): boolean {
  return (
    workload.desiredState === 'running' &&
    workload.activeRevision !== null &&
    workload.deployments.some((deployment) => deployment.status === 'active')
  );
}

function workloadStopNotice(workload?: Workload): {
  title: string;
  detail: string;
  tone: 'pending' | 'complete';
} | null {
  if (!workload || workload.desiredState !== 'stopped') return null;
  if (workload.activeRevision) {
    return {
      title: 'Stop requested',
      detail: 'The active revision remains selected until Runtime reports stopped or absent.',
      tone: 'pending',
    };
  }
  return {
    title: 'Workload stopped',
    detail: 'Runtime stop evidence was persisted and no active revision remains selected.',
    tone: 'complete',
  };
}

function deploymentCancellationNotice(status?: DeploymentStatus): {
  title: string;
  detail: string;
  tone: 'pending' | 'danger' | 'complete';
} | null {
  if (status === 'cancelling') {
    return {
      title: 'Cancellation requested',
      detail: 'The operation is checking whether a Runtime child must be stopped.',
      tone: 'pending',
    };
  }
  if (status === 'cleanup_pending') {
    return {
      title: 'Runtime cleanup pending',
      detail: 'The operation remains non-terminal until stopped or absent Runtime evidence is persisted.',
      tone: 'pending',
    };
  }
  if (status === 'orphaned') {
    return {
      title: 'Cleanup could not be proven',
      detail: 'Operator action is required because the Runtime child may still exist.',
      tone: 'danger',
    };
  }
  if (status === 'cancelled') {
    return {
      title: 'Cancellation complete',
      detail: 'No active Runtime child remains for this deployment.',
      tone: 'complete',
    };
  }
  return null;
}
