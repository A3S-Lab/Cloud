import { Ban, CircleStop } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { DeploymentStatus, Route, ServiceTemplate, Workload } from '../../types/api';
import { shortId, statusBadgeState } from './console-format';
import { WorkloadActions } from './workload-actions';
import { routeStage } from './workload-view-model';

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
  const { label, t } = useI18n();
  const latestDeployment = workload?.deployments[0];
  const observedRuntime = latestDeployment?.observedRuntime;
  const cancellationNotice = deploymentCancellationNotice(latestDeployment?.status);
  const stopNotice = workloadStopNotice(workload);

  return (
    <article className='card surface convergence-card' data-size='sm'>
      <header className='surface-heading'>
        <div>
          <h2>{workload?.name ?? t('Deployment state')}</h2>
          <p>{t('Convergence')}</p>
        </div>
        <div className='surface-actions card-action'>
          <span
            className='status-badge'
            data-state={statusBadgeState(latestDeployment?.status ?? 'neutral')}
            data-size='sm'
            data-indicator
          >
            {latestDeployment ? label(latestDeployment.status) : t('Awaiting workload')}
          </span>
          {workload ? (
            <WorkloadActions workload={workload} onUpdate={onUpdate} onRollback={onRollback} />
          ) : null}
          {latestDeployment && canCancel(latestDeployment.status) ? (
            <button
              className='btn danger-button compact'
              data-size='xs'
              data-variant='destructive'
              type='button'
              disabled={cancelling}
              onClick={onCancel}
            >
              <Ban size={14} />
              {cancelling ? t('Requesting...') : t('Cancel')}
            </button>
          ) : null}
          {workload && canStop(workload) ? (
            <button
              className='btn danger-button compact'
              data-size='xs'
              data-variant='destructive'
              type='button'
              disabled={stopping}
              onClick={onStop}
            >
              <CircleStop size={14} />
              {stopping ? t('Stopping...') : t('Stop')}
            </button>
          ) : null}
        </div>
      </header>
      {/* biome-ignore lint/a11y/noNoninteractiveTabindex: Overflowing steps must remain keyboard-scrollable. */}
      <ol className='stepper convergence-track' aria-label={t('Deployment convergence stages')} tabIndex={0}>
        {deploymentStages(latestDeployment?.status, latestDeployment?.revision.id, routes).map(
          (stage, index) => (
            <li
              className={`convergence-step ${stage.state}`}
              data-state={stepperStageState(stage.state)}
              aria-current={stage.state === 'current' ? 'step' : undefined}
              key={stage.name}
            >
              <span data-step-marker aria-hidden='true'>
                {index + 1}
              </span>
              <section>
                <strong>{t(stage.name)}</strong>
                <small>{localizedStageLabel(stage.label, label, t)}</small>
              </section>
            </li>
          )
        )}
      </ol>
      {workload ? (
        <dl className='property-list deployment-facts' data-size='sm'>
          <div>
            <dt>{t('Desired revision')}</dt>
            <dd>
              {workload.desiredRevision
                ? t('Generation {generation}', { generation: workload.desiredRevision.generation })
                : t('None')}
            </dd>
          </div>
          <div>
            <dt>{t('Active revision')}</dt>
            <dd>
              {workload.activeRevision
                ? t('Generation {generation}', { generation: workload.activeRevision.generation })
                : t('None')}
            </dd>
          </div>
          <div>
            <dt>{t('Observed generation')}</dt>
            <dd>
              {observedRuntime
                ? t('Generation {generation}', { generation: observedRuntime.generation })
                : t('No evidence')}
            </dd>
          </div>
          <div>
            <dt>{t('Runtime / health')}</dt>
            <dd>
              {observedRuntime
                ? `${label(observedRuntime.state)} / ${
                    observedRuntime.healthState ? label(observedRuntime.healthState) : t('Not reported')
                  }`
                : t('Not observed')}
            </dd>
          </div>
          <div>
            <dt>{t('Release binding')}</dt>
            <dd>{releaseBindingLabel(workload.desiredRevision, t)}</dd>
          </div>
        </dl>
      ) : (
        <p className='surface-note'>
          {t('A deployment appears here only after its committed operation is observable.')}
        </p>
      )}
      {cancellationNotice ? (
        <output className={`deployment-notice ${cancellationNotice.tone}`}>
          <strong>{t(cancellationNotice.title)}</strong>
          <span>{t(cancellationNotice.detail)}</span>
        </output>
      ) : null}
      {stopNotice ? (
        <output className={`deployment-notice ${stopNotice.tone}`}>
          <strong>{t(stopNotice.title)}</strong>
          <span>{t(stopNotice.detail)}</span>
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

function stepperStageState(
  state: 'pending' | 'current' | 'complete' | 'failed'
): 'active' | 'success' | 'danger' | undefined {
  if (state === 'current') return 'active';
  if (state === 'complete') return 'success';
  if (state === 'failed') return 'danger';
  return undefined;
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

function releaseBindingLabel(
  revision: Workload['desiredRevision'],
  t: (message: string, values?: Record<string, string | number>) => string
): string {
  if (revision?.agentBinding) {
    const skills = revision.skillBindings.length;
    return `Agent ${shortId(revision.agentBinding.assetReleaseId)}${
      skills > 0 ? ` · ${t('Skills {skills}', { skills })}` : ''
    }`;
  }
  if (revision?.mcpBinding) {
    return `MCP ${shortId(revision.mcpBinding.assetReleaseId)}`;
  }
  return t('Ordinary Workload');
}

function localizedStageLabel(
  value: string,
  label: (input: string) => string,
  t: (message: string, values?: Record<string, string | number>) => string
): string {
  const acknowledged = /^(\d+) acknowledged$/.exec(value);
  if (acknowledged) return t('{count} acknowledged', { count: acknowledged[1] });
  return t(label(value));
}
