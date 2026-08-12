import { useState } from 'react';
import { useI18n } from '../../lib/i18n';
import type { Asset, AssetRelease, Workload } from '../../types/api';
import { shortId } from './console-format';
import { isWorkloadReadyForReplacement } from './workload-view-model';

interface SkillBindingsPanelProps {
  workload: Workload | undefined;
  assets: Asset[];
  releases: AssetRelease[];
  onBind: (skillAssetId: string, skillAssetReleaseId: string, idempotencyKey: string) => Promise<void>;
  onUnbind: (skillAssetId: string, idempotencyKey: string) => Promise<void>;
}

interface MutationAttempt {
  identity: string;
  idempotencyKey: string;
}

export function SkillBindingsPanel({
  workload,
  assets,
  releases,
  onBind,
  onUnbind,
}: SkillBindingsPanelProps) {
  const { t } = useI18n();
  const [selectedAssetId, setSelectedAssetId] = useState('');
  const [selectedReleaseId, setSelectedReleaseId] = useState('');
  const [bindAttempt, setBindAttempt] = useState<MutationAttempt | null>(null);
  const [unbindAttempts, setUnbindAttempts] = useState<Record<string, MutationAttempt>>({});
  const [submitting, setSubmitting] = useState<string | null>(null);

  const revision = workload?.desiredRevision;
  if (!workload || !revision?.agentBinding) return null;

  const skillAssets = assets
    .filter((asset) => asset.kind === 'skill' && asset.state === 'active')
    .sort((left, right) => left.name.localeCompare(right.name));
  const effectiveAssetId = skillAssets.some((asset) => asset.id === selectedAssetId)
    ? selectedAssetId
    : (skillAssets[0]?.id ?? '');
  const publishedReleases = releases
    .filter(
      (release) =>
        release.assetId === effectiveAssetId &&
        release.state === 'published' &&
        release.artifact?.kind === 'skill_bundle'
    )
    .sort((left, right) => right.version.localeCompare(left.version, undefined, { numeric: true }));
  const effectiveReleaseId = publishedReleases.some((release) => release.id === selectedReleaseId)
    ? selectedReleaseId
    : (publishedReleases[0]?.id ?? '');
  const ready = isWorkloadReadyForReplacement(workload);
  const exactBindingExists = revision.skillBindings.some(
    (binding) => binding.assetId === effectiveAssetId && binding.assetReleaseId === effectiveReleaseId
  );

  const bind = async () => {
    if (!ready || !effectiveAssetId || !effectiveReleaseId || exactBindingExists) return;
    const identity = `${workload.id}:${effectiveAssetId}:${effectiveReleaseId}`;
    const attempt =
      bindAttempt?.identity === identity
        ? bindAttempt
        : { identity, idempotencyKey: mutationKey('bind', workload.id, effectiveAssetId) };
    setBindAttempt(attempt);
    setSubmitting(`bind:${effectiveAssetId}`);
    try {
      await onBind(effectiveAssetId, effectiveReleaseId, attempt.idempotencyKey);
      setBindAttempt(null);
    } catch {
      // The shared console banner owns the error. Retain this key for an exact retry.
    } finally {
      setSubmitting(null);
    }
  };

  const unbind = async (assetId: string) => {
    if (!ready) return;
    const identity = `${workload.id}:${assetId}`;
    const current = unbindAttempts[assetId];
    const attempt =
      current?.identity === identity
        ? current
        : { identity, idempotencyKey: mutationKey('unbind', workload.id, assetId) };
    setUnbindAttempts((attempts) => ({ ...attempts, [assetId]: attempt }));
    setSubmitting(`unbind:${assetId}`);
    try {
      await onUnbind(assetId, attempt.idempotencyKey);
      setUnbindAttempts((attempts) => {
        const next = { ...attempts };
        delete next[assetId];
        return next;
      });
    } catch {
      // The shared console banner owns the error. Retain this key for an exact retry.
    } finally {
      setSubmitting(null);
    }
  };

  return (
    <article className='card surface skill-bindings-card' data-size='sm'>
      <header className='surface-heading'>
        <div>
          <h2>{t('Skill bindings')}</h2>
          <p>{t('Agent inputs')}</p>
        </div>
        <span className='badge panel-count card-action' data-variant='secondary'>
          {revision.skillBindings.length}
        </span>
      </header>
      <section>
        <p className='surface-note'>
          {t(
            'Each change creates a new immutable Agent workload revision. Skill bundles are mounted read-only and are never scheduled as separate services.'
          )}
        </p>

        {revision.skillBindings.length > 0 ? (
          <ul className='item-group skill-binding-list'>
            {revision.skillBindings.map((binding) => {
              const asset = assets.find((item) => item.id === binding.assetId);
              const release = releases.find((item) => item.id === binding.assetReleaseId);
              return (
                <li className='item' data-size='sm' data-variant='outline' key={binding.assetId}>
                  <section data-item-content>
                    <strong>{asset?.name ?? shortId(binding.assetId)}</strong>
                    <span>
                      {release?.version ?? shortId(binding.assetReleaseId)} · {binding.mountTarget}
                    </span>
                    <code>{binding.artifactDigest.slice(0, 23)}</code>
                  </section>
                  <aside data-item-actions>
                    <button
                      className='btn secondary-button compact'
                      data-size='xs'
                      data-variant='destructive'
                      type='button'
                      disabled={!ready || submitting !== null}
                      title={
                        ready ? t('Create a new revision without this Skill') : t(replacementUnavailable)
                      }
                      onClick={() => void unbind(binding.assetId)}
                    >
                      {submitting === `unbind:${binding.assetId}` ? t('Unbinding...') : t('Unbind')}
                    </button>
                  </aside>
                </li>
              );
            })}
          </ul>
        ) : (
          <div className='empty empty-skill-bindings'>
            <header>
              <p>{t('No Skill release is bound to this revision.')}</p>
            </header>
          </div>
        )}

        <div className='skill-binding-form'>
          <div className='field'>
            <label htmlFor='skill-asset-binding'>{t('Skill Asset')}</label>
            <select
              className='select'
              id='skill-asset-binding'
              value={effectiveAssetId}
              disabled={!ready || submitting !== null || skillAssets.length === 0}
              onChange={(event) => {
                setSelectedAssetId(event.target.value);
                setSelectedReleaseId('');
                setBindAttempt(null);
              }}
            >
              {skillAssets.length === 0 ? <option value=''>{t('No active Skill Assets')}</option> : null}
              {skillAssets.map((asset) => (
                <option key={asset.id} value={asset.id}>
                  {asset.name}
                </option>
              ))}
            </select>
          </div>
          <div className='field'>
            <label htmlFor='skill-release-binding'>{t('Published release')}</label>
            <select
              className='select'
              id='skill-release-binding'
              value={effectiveReleaseId}
              disabled={!ready || submitting !== null || publishedReleases.length === 0}
              onChange={(event) => {
                setSelectedReleaseId(event.target.value);
                setBindAttempt(null);
              }}
            >
              {publishedReleases.length === 0 ? <option value=''>{t('No published releases')}</option> : null}
              {publishedReleases.map((release) => (
                <option key={release.id} value={release.id}>
                  {release.version} · {shortId(release.id)}
                </option>
              ))}
            </select>
          </div>
          <button
            className='btn primary-action'
            data-size='sm'
            type='button'
            disabled={
              !ready || submitting !== null || !effectiveAssetId || !effectiveReleaseId || exactBindingExists
            }
            title={!ready ? t(replacementUnavailable) : undefined}
            onClick={() => void bind()}
          >
            {submitting?.startsWith('bind:')
              ? t('Binding...')
              : exactBindingExists
                ? t('Already bound')
                : t('Bind release')}
          </button>
        </div>
      </section>
    </article>
  );
}

function mutationKey(action: 'bind' | 'unbind', workloadId: string, assetId: string): string {
  return `web-skill-${action}:${workloadId}:${assetId}:${crypto.randomUUID()}`;
}

const replacementUnavailable =
  'Skill bindings unlock after the desired Agent revision is the active deployment';
