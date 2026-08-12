import { Globe2, ShieldCheck } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { GatewayCertificate, Route, Workload } from '../../types/api';
import { compactDigest, shortId, statusBadgeState } from './console-format';

interface EdgeStatusPanelProps {
  workload: Workload | undefined;
  routes: Route[];
  certificates: GatewayCertificate[];
}

export function EdgeStatusPanel({ workload, routes, certificates }: EdgeStatusPanelProps) {
  const { formatTimestamp, label, t } = useI18n();
  const certificateById = new Map(certificates.map((certificate) => [certificate.id, certificate]));

  return (
    <section
      className='card surface edge-status-panel'
      data-size='sm'
      aria-label={t('Route and certificate state')}
    >
      <header className='surface-heading'>
        <div>
          <h2>{t('Routes and certificates')}</h2>
          <p>{t('Authoritative edge projection')}</p>
        </div>
        <span className='badge panel-count card-action' data-variant='secondary'>
          <Globe2 size={14} /> {routes.length}
        </span>
      </header>
      <section>
        {!workload || routes.length === 0 ? (
          <div className='empty detail-empty'>
            <figure>
              <Globe2 size={21} />
            </figure>
            <header>
              <h3>{t('No route projection')}</h3>
              <p>{t('Reachability appears only after Cloud owns a route for this workload.')}</p>
            </header>
          </div>
        ) : (
          <div className='item-group edge-route-list'>
            {routes.map((route) => {
              const certificate = route.gatewayCertificateId
                ? certificateById.get(route.gatewayCertificateId)
                : undefined;
              const generation = workload.deployments.find(
                (deployment) => deployment.revision.id === route.workloadRevisionId
              )?.revision.generation;
              return (
                <article className='item edge-route' data-size='sm' data-variant='outline' key={route.id}>
                  <header className='edge-route-heading'>
                    <div>
                      <strong>
                        {route.gatewayCertificateId ? 'https' : 'http'}://{route.hostname}
                        {route.pathPrefix}
                      </strong>
                      <small>
                        {generation
                          ? t('Generation {generation}', { generation })
                          : shortId(route.workloadRevisionId)}
                      </small>
                    </div>
                    <span
                      className='status-badge'
                      data-state={statusBadgeState(route.state)}
                      data-size='sm'
                      data-indicator
                    >
                      {label(route.state)}
                    </span>
                  </header>
                  <section>
                    <dl className='property-list edge-facts' data-size='sm'>
                      <div>
                        <dt>{t('Gateway node')}</dt>
                        <dd>{shortId(route.gatewayNodeId)}</dd>
                      </div>
                      <div>
                        <dt>{t('Gateway revision')}</dt>
                        <dd>{route.gatewayRevision ?? t('Not acknowledged')}</dd>
                      </div>
                      <div>
                        <dt>{t('Activated')}</dt>
                        <dd>{formatTimestamp(route.activatedAt)}</dd>
                      </div>
                      <div>
                        <dt>{t('Snapshot')}</dt>
                        <dd>
                          {route.snapshotDigest ? compactDigest(route.snapshotDigest) : t('Not published')}
                        </dd>
                      </div>
                    </dl>
                    {route.failure ? <output className='edge-failure'>{route.failure}</output> : null}
                    <CertificateProjection
                      certificateId={route.gatewayCertificateId}
                      certificate={certificate}
                    />
                  </section>
                </article>
              );
            })}
          </div>
        )}
      </section>
    </section>
  );
}

function CertificateProjection({
  certificateId,
  certificate,
}: {
  certificateId: string | null;
  certificate: GatewayCertificate | undefined;
}) {
  const { formatTimestamp, label, t } = useI18n();
  if (!certificateId) {
    return (
      <article className='item certificate-projection unbound' data-size='sm' data-variant='muted'>
        <figure>
          <ShieldCheck size={16} />
        </figure>
        <section>
          <h3>{t('No managed certificate bound')}</h3>
          <p>{t('This route projection does not reference a Gateway certificate.')}</p>
        </section>
      </article>
    );
  }
  if (!certificate) {
    return (
      <article className='item certificate-projection missing' data-size='sm' data-variant='muted'>
        <figure>
          <ShieldCheck size={16} />
        </figure>
        <section>
          <h3>{t('Certificate projection unavailable')}</h3>
          <p>
            {t('Referenced certificate {id} is absent from this snapshot.', { id: shortId(certificateId) })}
          </p>
        </section>
      </article>
    );
  }
  return (
    <article className='item certificate-projection' data-size='sm' data-variant='muted'>
      <figure>
        <ShieldCheck size={16} />
      </figure>
      <section>
        <header className='certificate-title'>
          <h3>{certificate.dnsNames.join(', ')}</h3>
          <span
            className='status-badge'
            data-state={statusBadgeState(certificate.state)}
            data-size='sm'
            data-indicator
          >
            {label(certificate.state)}
          </span>
        </header>
        <p>
          {t('Fingerprint {fingerprint} · expires {expires}', {
            fingerprint: certificate.fingerprint ? compactDigest(certificate.fingerprint) : t('Not issued'),
            expires: formatTimestamp(certificate.expiresAt),
          })}
        </p>
        {certificate.failure ? <p className='certificate-failure'>{certificate.failure}</p> : null}
      </section>
    </article>
  );
}
