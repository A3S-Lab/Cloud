import { Box } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { Environment, Workload } from '../../types/api';
import { statusBadgeState } from './console-format';

export function WorkloadList({
  workloads,
  selectedWorkloadId,
  environment,
  onSelect,
}: {
  workloads: Workload[];
  selectedWorkloadId: string;
  environment: Environment | undefined;
  onSelect: (workloadId: string) => void;
}) {
  const { label, t } = useI18n();
  return (
    <section className='card workload-section' data-size='sm' aria-label={t('Workloads')}>
      <header className='section-heading'>
        <div>
          <h2>{t('Workloads')}</h2>
          <p>{t('Desired and observed state')}</p>
        </div>
        <span className='badge card-action' data-variant='secondary'>
          {environment ? environment.name : t('Select an environment')}
        </span>
      </header>
      <section>
        {workloads.length === 0 ? (
          <div className='empty surface workload-empty'>
            <figure>
              <Box size={22} />
            </figure>
            <header>
              <h3>{t('No workloads in this environment')}</h3>
              <p>{t('Create a digest-bound Service deployment to start convergence.')}</p>
            </header>
          </div>
        ) : (
          <div className='item-group workload-list' role='listbox' aria-label={t('Workloads')}>
            {workloads.map((workload) => {
              const deployment = workload.deployments[0];
              const selected = workload.id === selectedWorkloadId;
              const operationStatus = deployment?.operation?.status ?? deployment?.status ?? 'queued';
              return (
                <button
                  className={`item workload-row${selected ? ' selected' : ''}`}
                  data-size='sm'
                  data-variant={selected ? 'muted' : 'outline'}
                  type='button'
                  role='option'
                  aria-selected={selected}
                  key={workload.id}
                  onClick={() => onSelect(workload.id)}
                >
                  <span className={`workload-state ${deployment?.status ?? 'neutral'}`} />
                  <span className='workload-identity' data-item-content>
                    <strong>{workload.name}</strong>
                    <small>
                      {workload.desiredRevision?.artifactUri ??
                        workload.desiredRevision?.artifactSourceUri ??
                        t('No desired revision')}
                    </small>
                  </span>
                  <span>
                    <small>{t('Desired')}</small>
                    <strong>
                      {workload.desiredRevision
                        ? t('Generation {generation}', { generation: workload.desiredRevision.generation })
                        : t('None')}
                    </strong>
                  </span>
                  <span>
                    <small>{t('Observed')}</small>
                    <strong>
                      {deployment?.observedRuntime
                        ? `${label(deployment.observedRuntime.state)} · ${
                            deployment.observedRuntime.healthState
                              ? label(deployment.observedRuntime.healthState)
                              : t('No health')
                          }`
                        : t('No evidence')}
                    </strong>
                  </span>
                  <span data-item-actions>
                    <small>{t('Operation')}</small>
                    <span
                      className='status-badge'
                      data-state={statusBadgeState(operationStatus)}
                      data-size='sm'
                      data-indicator
                    >
                      {label(operationStatus)}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </section>
    </section>
  );
}
