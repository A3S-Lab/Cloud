import { Ban, Boxes, Hammer, RotateCcw, ShieldCheck, SquareTerminal } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { BuildRun, BuildRunStatus } from '../../types/api';
import { compactDigest, shortId, statusBadgeState } from './console-format';

interface BuildRunPanelProps {
  buildRuns: BuildRun[];
  selectedBuildRunId: string | null;
  cancellingBuildRunId: string | null;
  retryingBuildRunId: string | null;
  onSelect: (buildRunId: string) => void;
  onCancel: (buildRunId: string) => void;
  onRetry: (buildRunId: string) => void;
}

const TERMINAL_STATUSES = new Set<BuildRunStatus>(['succeeded', 'failed', 'cancelled']);

export function BuildRunPanel({
  buildRuns,
  selectedBuildRunId,
  cancellingBuildRunId,
  retryingBuildRunId,
  onSelect,
  onCancel,
  onRetry,
}: BuildRunPanelProps) {
  const { formatRelative, label, t } = useI18n();
  const ordered = [...buildRuns].sort((left, right) => right.requestedAt.localeCompare(left.requestedAt));

  return (
    <section className='card surface build-run-panel' data-size='sm' aria-label={t('Build runs')}>
      <header className='surface-heading'>
        <div>
          <h2>{t('Build runs')}</h2>
          <p>{t('Immutable source to OCI')}</p>
        </div>
        <span className='badge panel-count card-action' data-variant='secondary'>
          <Hammer size={14} /> {buildRuns.length}
        </span>
      </header>
      <section>
        {ordered.length === 0 ? (
          <div className='empty detail-empty'>
            <figure>
              <Boxes size={22} />
            </figure>
            <header>
              <h3>{t('No build runs')}</h3>
              <p>{t('Accepted source revisions and their authoritative build state will appear here.')}</p>
            </header>
          </div>
        ) : (
          <div className='item-group build-run-list'>
            {ordered.map((buildRun) => {
              const terminal = TERMINAL_STATUSES.has(buildRun.status);
              const cancelling = cancellingBuildRunId === buildRun.id || buildRun.status === 'cancelling';
              const retryable = buildRun.status === 'failed' || buildRun.status === 'cancelled';
              const retrying = retryingBuildRunId === buildRun.id;
              const selected = selectedBuildRunId === buildRun.id;
              return (
                <article
                  className={`item build-run-item${selected ? ' selected' : ''}`}
                  data-size='sm'
                  data-variant={selected ? 'muted' : 'outline'}
                  key={buildRun.id}
                >
                  <header className='build-run-heading'>
                    <div>
                      <strong>
                        {t('Build {id} · Attempt {attempt}', {
                          id: shortId(buildRun.id),
                          attempt: buildRun.attempt,
                        })}
                      </strong>
                      <small>
                        {t('source {source} · {time}', {
                          source: shortId(buildRun.sourceRevisionId),
                          time: formatRelative(buildRun.updatedAt),
                        })}
                      </small>
                    </div>
                    <span
                      className='status-badge'
                      data-state={statusBadgeState(buildRun.status)}
                      data-size='sm'
                      data-indicator
                    >
                      {label(buildRun.status)}
                    </span>
                  </header>
                  <section>
                    <dl className='property-list build-run-facts' data-size='sm'>
                      <div>
                        <dt>{t('Operation')}</dt>
                        <dd>{shortId(buildRun.operationId)}</dd>
                      </div>
                      <div>
                        <dt>{t('Source digest')}</dt>
                        <dd>
                          {buildRun.sourceContentDigest
                            ? compactDigest(buildRun.sourceContentDigest)
                            : t('Preparing input')}
                        </dd>
                      </div>
                      <div>
                        <dt>{t('Platform')}</dt>
                        <dd>{buildRun.output?.platforms.join(', ') ?? t('Pending')}</dd>
                      </div>
                      <div>
                        <dt>{t('Artifact')}</dt>
                        <dd>
                          {buildRun.publishedArtifact
                            ? compactDigest(buildRun.publishedArtifact.digest)
                            : t('Not published')}
                        </dd>
                      </div>
                    </dl>
                    {buildRun.publishedArtifact ? (
                      <code className='build-artifact-uri'>{buildRun.publishedArtifact.uri}</code>
                    ) : null}
                    {buildRun.evidenceSummary ? (
                      <div className='build-run-evidence-summary'>
                        <span>
                          <ShieldCheck size={13} /> {t('Verified evidence')}
                        </span>
                        <dl className='property-list' data-size='sm'>
                          <div>
                            <dt>SBOM</dt>
                            <dd title={buildRun.evidenceSummary.sbomDigest}>
                              {compactDigest(buildRun.evidenceSummary.sbomDigest)}
                            </dd>
                          </div>
                          <div>
                            <dt>{t('Provenance')}</dt>
                            <dd title={buildRun.evidenceSummary.provenanceDigest}>
                              {compactDigest(buildRun.evidenceSummary.provenanceDigest)}
                            </dd>
                          </div>
                          <div>
                            <dt>{t('Signing key')}</dt>
                            <dd title={buildRun.evidenceSummary.signingKeyId}>
                              {compactDigest(buildRun.evidenceSummary.signingKeyId)}
                              {buildRun.evidenceSummary.signingKeyVersion === null
                                ? ''
                                : ` · v${buildRun.evidenceSummary.signingKeyVersion}`}
                            </dd>
                          </div>
                        </dl>
                      </div>
                    ) : null}
                    {buildRun.failure ? (
                      <output className='build-run-failure'>{buildRun.failure}</output>
                    ) : null}
                  </section>
                  <footer className='build-run-actions'>
                    <button
                      className='btn secondary-button compact'
                      data-size='xs'
                      data-variant='outline'
                      type='button'
                      aria-pressed={selected}
                      onClick={() => onSelect(buildRun.id)}
                    >
                      <SquareTerminal size={13} />
                      {selected ? t('Inspecting run') : t('Inspect run')}
                    </button>
                    {!terminal ? (
                      <button
                        className='btn danger-button compact'
                        data-size='xs'
                        data-variant='destructive'
                        type='button'
                        disabled={cancelling}
                        onClick={() => onCancel(buildRun.id)}
                      >
                        <Ban size={13} /> {cancelling ? t('Cancelling') : t('Cancel build')}
                      </button>
                    ) : null}
                    {retryable ? (
                      <button
                        className='btn secondary-button compact'
                        data-size='xs'
                        data-variant='outline'
                        type='button'
                        disabled={retrying}
                        onClick={() => onRetry(buildRun.id)}
                      >
                        <RotateCcw size={13} /> {retrying ? t('Retrying') : t('Retry build')}
                      </button>
                    ) : null}
                  </footer>
                </article>
              );
            })}
          </div>
        )}
      </section>
    </section>
  );
}
