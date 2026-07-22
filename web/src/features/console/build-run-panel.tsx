import { Ban, Boxes, Hammer } from 'lucide-react';
import type { BuildRun, BuildRunStatus } from '../../types/api';
import { compactDigest, formatRelative, humanize, shortId } from './console-format';

interface BuildRunPanelProps {
  buildRuns: BuildRun[];
  cancellingBuildRunId: string | null;
  onCancel: (buildRunId: string) => void;
}

const TERMINAL_STATUSES = new Set<BuildRunStatus>(['succeeded', 'failed', 'cancelled']);

export function BuildRunPanel({ buildRuns, cancellingBuildRunId, onCancel }: BuildRunPanelProps) {
  const ordered = [...buildRuns].sort((left, right) => right.requestedAt.localeCompare(left.requestedAt));

  return (
    <section className='surface build-run-panel' aria-label='Build runs'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Immutable source to OCI</p>
          <h2>Build runs</h2>
        </div>
        <span className='panel-count'>
          <Hammer size={14} /> {buildRuns.length}
        </span>
      </div>
      {ordered.length === 0 ? (
        <div className='detail-empty'>
          <Boxes size={22} />
          <strong>No build runs</strong>
          <p>Accepted source revisions and their authoritative build state will appear here.</p>
        </div>
      ) : (
        <div className='build-run-list'>
          {ordered.map((buildRun) => {
            const terminal = TERMINAL_STATUSES.has(buildRun.status);
            const cancelling = cancellingBuildRunId === buildRun.id || buildRun.status === 'cancelling';
            return (
              <article className='build-run-item' key={buildRun.id}>
                <div className='build-run-heading'>
                  <div>
                    <strong>Build {shortId(buildRun.id)}</strong>
                    <small>
                      source {shortId(buildRun.sourceRevisionId)} · {formatRelative(buildRun.updatedAt)}
                    </small>
                  </div>
                  <span className={`state-badge ${buildRun.status}`}>{humanize(buildRun.status)}</span>
                </div>
                <dl className='build-run-facts'>
                  <div>
                    <dt>Operation</dt>
                    <dd>{shortId(buildRun.operationId)}</dd>
                  </div>
                  <div>
                    <dt>Source digest</dt>
                    <dd>
                      {buildRun.sourceContentDigest
                        ? compactDigest(buildRun.sourceContentDigest)
                        : 'Preparing input'}
                    </dd>
                  </div>
                  <div>
                    <dt>Platform</dt>
                    <dd>{buildRun.output?.platforms.join(', ') ?? 'Pending'}</dd>
                  </div>
                  <div>
                    <dt>Artifact</dt>
                    <dd>
                      {buildRun.publishedArtifact
                        ? compactDigest(buildRun.publishedArtifact.digest)
                        : 'Not published'}
                    </dd>
                  </div>
                </dl>
                {buildRun.publishedArtifact ? (
                  <code className='build-artifact-uri'>{buildRun.publishedArtifact.uri}</code>
                ) : null}
                {buildRun.failure ? <output className='build-run-failure'>{buildRun.failure}</output> : null}
                {!terminal ? (
                  <div className='build-run-actions'>
                    <button
                      className='danger-button compact'
                      type='button'
                      disabled={cancelling}
                      onClick={() => onCancel(buildRun.id)}
                    >
                      <Ban size={13} /> {cancelling ? 'Cancelling' : 'Cancel build'}
                    </button>
                  </div>
                ) : null}
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
