import { Clock3, GitCommitHorizontal } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { DeploymentStatus, Operation, Workload } from '../../types/api';
import { shortId, statusBadgeState } from './console-format';

interface DeploymentTimelineProps {
  workload: Workload | undefined;
  operations: Operation[];
}

export function DeploymentTimeline({ workload, operations }: DeploymentTimelineProps) {
  const { formatTimestamp, label, t } = useI18n();
  const deployments = [...(workload?.deployments ?? [])].sort(
    (left, right) => Date.parse(right.requestedAt) - Date.parse(left.requestedAt)
  );

  return (
    <section
      className='card surface deployment-timeline'
      data-size='sm'
      aria-label={t('Deployment timeline')}
    >
      <header className='surface-heading'>
        <div>
          <h2>{t('Deployment timeline')}</h2>
          <p>{t('Immutable history')}</p>
        </div>
        <span className='badge panel-count card-action' data-variant='secondary'>
          <Clock3 size={14} /> {deployments.length}
        </span>
      </header>
      <section>
        {deployments.length === 0 ? (
          <div className='empty detail-empty'>
            <figure>
              <GitCommitHorizontal size={21} />
            </figure>
            <header>
              <h3>{t('No deployment projection')}</h3>
              <p>{t('Committed generations appear here with their observed operation state.')}</p>
            </header>
          </div>
        ) : (
          <ol className='timeline deployment-timeline-list'>
            {deployments.map((deployment) => {
              const operation = operations.find((item) => item.id === deployment.operationId);
              const rollbackSource = operation?.rollbackSourceRevisionId
                ? workload?.deployments.find(
                    (item) => item.revision.id === operation.rollbackSourceRevisionId
                  )?.revision
                : undefined;
              const isCurrent = workload?.activeRevision?.id === deployment.revision.id;
              return (
                <li
                  key={deployment.id}
                  data-marker={deployment.revision.generation}
                  data-state={deploymentTimelineState(deployment.status, isCurrent)}
                  aria-current={isCurrent ? 'step' : undefined}
                >
                  <article className='item' data-size='sm' data-variant='outline'>
                    <div className='timeline-title'>
                      <div>
                        <strong>
                          {t('Generation {generation}', { generation: deployment.revision.generation })}
                        </strong>
                        {isCurrent ? <span className='current-label'>{t('Current')}</span> : null}
                      </div>
                      <span
                        className='status-badge'
                        data-state={statusBadgeState(deployment.status)}
                        data-size='sm'
                        data-indicator
                      >
                        {label(deployment.status)}
                      </span>
                    </div>
                    <p className='timeline-artifact'>
                      {deployment.revision.artifactUri ?? deployment.revision.artifactSourceUri}
                    </p>
                    {operation?.rollbackSourceRevisionId ? (
                      <p className='timeline-lineage'>
                        {t('Rollback from {source}', {
                          source: rollbackSource
                            ? t('Generation {generation}', { generation: rollbackSource.generation })
                            : shortId(operation.rollbackSourceRevisionId),
                        })}
                      </p>
                    ) : null}
                    {deployment.revision.externalSourceRevisionId && deployment.revision.buildRunId ? (
                      <p className='timeline-lineage'>
                        {t('Source {source} · build {build}', {
                          source: shortId(deployment.revision.externalSourceRevisionId),
                          build: shortId(deployment.revision.buildRunId),
                        })}
                      </p>
                    ) : null}
                    {deployment.revision.agentBinding ? (
                      <p className='timeline-lineage'>
                        {t('Agent release {release} / build {build}', {
                          release: shortId(deployment.revision.agentBinding.assetReleaseId),
                          build: shortId(deployment.revision.agentBinding.buildRunId),
                        })}
                      </p>
                    ) : null}
                    {deployment.revision.mcpBinding ? (
                      <p className='timeline-lineage'>
                        {t('MCP release {release} / profile {profile}', {
                          release: shortId(deployment.revision.mcpBinding.assetReleaseId),
                          profile: deployment.revision.mcpBinding.profileDigest.slice(0, 15),
                        })}
                      </p>
                    ) : null}
                    {deployment.revision.skillBindings.length > 0 ? (
                      <p className='timeline-lineage'>
                        {t('Skills {skills}', {
                          skills: deployment.revision.skillBindings
                            .map((binding) => shortId(binding.assetReleaseId))
                            .join(', '),
                        })}
                      </p>
                    ) : null}
                    <dl className='property-list timeline-facts' data-size='sm'>
                      <div>
                        <dt>{t('Requested')}</dt>
                        <dd>{formatTimestamp(deployment.requestedAt)}</dd>
                      </div>
                      <div>
                        <dt>{t('Activated')}</dt>
                        <dd>{formatTimestamp(deployment.activatedAt)}</dd>
                      </div>
                      <div>
                        <dt>{t('Node')}</dt>
                        <dd>{deployment.nodeId ? shortId(deployment.nodeId) : t('Not scheduled')}</dd>
                      </div>
                      <div>
                        <dt>{t('Operation')}</dt>
                        <dd>
                          {label(operation?.status ?? deployment.operation?.status ?? deployment.status)}
                        </dd>
                      </div>
                    </dl>
                    {deployment.failure || operation?.error || deployment.operation?.error ? (
                      <output className='timeline-failure'>
                        {deployment.failure ?? operation?.error ?? deployment.operation?.error}
                      </output>
                    ) : null}
                  </article>
                </li>
              );
            })}
          </ol>
        )}
      </section>
    </section>
  );
}

function deploymentTimelineState(
  status: DeploymentStatus,
  isCurrent: boolean
): 'active' | 'success' | 'warning' | 'danger' {
  if (isCurrent) return 'active';
  if (status === 'active') return 'success';
  if (status === 'failed' || status === 'orphaned' || status === 'cancelled') return 'danger';
  return 'warning';
}
