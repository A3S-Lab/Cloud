import { Ban, CircleStop } from 'lucide-react';
import type { DeploymentStatus, Route, ServiceTemplate, Workload } from '../../types/api';
import { humanize } from './console-format';
import { WorkloadActions } from './workload-actions';

interface WorkloadOverviewProps {
  workload: Workload | undefined;
  routes: Route[];
  cancelling: boolean;
  stopping: boolean;
  onCancel: () => Promise<void>;
  onStop: () => Promise<void>;
  onUpdate: (template: ServiceTemplate, idempotencyKey: string) => Promise<void>;
  onRollback: (revisionId: string, idempotencyKey: string) => Promise<void>;
}

export function WorkloadOverview({
  workload,
  routes,
  cancelling,
  stopping,
  onCancel,
  onStop,
  onUpdate,
  onRollback,
}: WorkloadOverviewProps) {
  const latestDeployment = workload?.deployments[0];
  const observedRuntime = latestDeployment?.observedRuntime;
  const cancellationNotice = deploymentCancellationNotice(latestDeployment?.status);
  const stopNotice = workloadStopNotice(workload);

  return (
    <article className='surface convergence-card'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Convergence</p>
          <h2>{workload?.name ?? 'Deployment state'}</h2>
        </div>
        <div className='surface-actions'>
          <span className={`state-badge ${latestDeployment?.status ?? 'neutral'}`}>
            {latestDeployment ? humanize(latestDeployment.status) : 'Awaiting workload'}
          </span>
          {workload ? (
            <WorkloadActions workload={workload} onUpdate={onUpdate} onRollback={onRollback} />
          ) : null}
          {latestDeployment && canCancel(latestDeployment.status) ? (
            <button className='danger-button compact' type='button' disabled={cancelling} onClick={onCancel}>
              <Ban size={14} />
              {cancelling ? 'Requesting…' : 'Cancel'}
            </button>
          ) : null}
          {workload && canStop(workload) ? (
            <button className='danger-button compact' type='button' disabled={stopping} onClick={onStop}>
              <CircleStop size={14} />
              {stopping ? 'Stopping…' : 'Stop'}
            </button>
          ) : null}
        </div>
      </div>
      <ol className='convergence-track' aria-label='Deployment convergence stages'>
        {deploymentStages(latestDeployment?.status, latestDeployment?.revision.id, routes).map(
          (stage, index) => (
            <li className={`convergence-step ${stage.state}`} key={stage.name}>
              <span>{index + 1}</span>
              <div>
                <strong>{stage.name}</strong>
                <small>{stage.label}</small>
              </div>
            </li>
          )
        )}
      </ol>
      {workload ? (
        <dl className='deployment-facts'>
          <div>
            <dt>Desired revision</dt>
            <dd>{revisionLabel(workload.desiredRevision)}</dd>
          </div>
          <div>
            <dt>Active revision</dt>
            <dd>{revisionLabel(workload.activeRevision)}</dd>
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
  );
}

function deploymentStages(
  status: DeploymentStatus | undefined,
  revisionId: string | undefined,
  routes: Route[]
): Array<{
  name: string;
  label: string;
  state: 'pending' | 'current' | 'complete' | 'failed';
}> {
  const stages = [
    { name: 'Desired state', threshold: 0 },
    { name: 'Runtime apply', threshold: 3 },
    { name: 'Health proof', threshold: 5 },
  ];
  const rank: Record<DeploymentStatus, number> = {
    queued: 0,
    resolving: 1,
    scheduled: 2,
    applying: 3,
    verifying: 5,
    retiring: 6,
    cancelling: 5,
    cleanup_pending: 5,
    active: 6,
    failed: 6,
    orphaned: 6,
    cancelled: 6,
  };
  const current = status ? rank[status] : -1;
  const projected = stages.map((stage, index) => {
    if (
      (status === 'failed' || status === 'orphaned' || status === 'cancelled') &&
      stage.threshold >= current &&
      index < stages.length
    ) {
      return { ...stage, label: status, state: 'failed' as const };
    }
    if (current > stage.threshold) {
      return { ...stage, label: 'Complete', state: 'complete' as const };
    }
    if (current === stage.threshold) {
      return { ...stage, label: status ?? 'Not requested', state: 'current' as const };
    }
    return { ...stage, label: 'Pending', state: 'pending' as const };
  });
  return [...projected, routeStage(revisionId, routes)];
}

function routeStage(
  revisionId: string | undefined,
  routes: Route[]
): {
  name: string;
  label: string;
  state: 'pending' | 'current' | 'complete' | 'failed';
} {
  const revisionRoutes = revisionId ? routes.filter((route) => route.workloadRevisionId === revisionId) : [];
  if (revisionRoutes.some((route) => route.state === 'rejected')) {
    return { name: 'Route active', label: 'Rejected', state: 'failed' };
  }
  if (revisionRoutes.length > 0 && revisionRoutes.every((route) => route.state === 'active')) {
    return {
      name: 'Route active',
      label: `${revisionRoutes.length} acknowledged`,
      state: 'complete',
    };
  }
  if (revisionRoutes.some((route) => route.state === 'publishing')) {
    return { name: 'Route active', label: 'Publishing', state: 'current' };
  }
  if (revisionRoutes.some((route) => route.state === 'pending')) {
    return { name: 'Route active', label: 'Pending', state: 'current' };
  }
  if (routes.some((route) => route.state === 'active')) {
    return { name: 'Route active', label: 'Prior revision active', state: 'pending' };
  }
  return { name: 'Route active', label: 'No route projection', state: 'pending' };
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

function revisionLabel(revision: Workload['desiredRevision']): string {
  return revision ? `Generation ${revision.generation}` : 'None';
}
