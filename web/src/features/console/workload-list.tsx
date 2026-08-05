import { Box } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { Environment, Workload } from '../../types/api';

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
    <section className='workload-section' aria-label={t('Workloads')}>
      <div className='section-heading'>
        <div>
          <p className='eyebrow'>{t('Desired and observed state')}</p>
          <h2>{t('Workloads')}</h2>
        </div>
        <span>{environment ? environment.name : t('Select an environment')}</span>
      </div>
      {workloads.length === 0 ? (
        <div className='surface workload-empty'>
          <Box size={22} />
          <strong>{t('No workloads in this environment')}</strong>
          <p>{t('Create a digest-bound Service deployment to start convergence.')}</p>
        </div>
      ) : (
        <div className='workload-list'>
          {workloads.map((workload) => {
            const deployment = workload.deployments[0];
            return (
              <button
                className={workload.id === selectedWorkloadId ? 'workload-row selected' : 'workload-row'}
                type='button'
                key={workload.id}
                onClick={() => onSelect(workload.id)}
              >
                <span className={`workload-state ${deployment?.status ?? 'neutral'}`} />
                <span className='workload-identity'>
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
                <span>
                  <small>{t('Operation')}</small>
                  <strong>{label(deployment?.operation?.status ?? deployment?.status ?? 'queued')}</strong>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
