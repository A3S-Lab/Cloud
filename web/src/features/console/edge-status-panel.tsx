import { Globe2, ShieldCheck } from 'lucide-react';
import { useI18n } from '../../lib/i18n';
import type { GatewayCertificate, Route, Workload } from '../../types/api';
import { compactDigest, shortId } from './console-format';

interface EdgeStatusPanelProps {
  workload: Workload | undefined;
  routes: Route[];
  certificates: GatewayCertificate[];
}

export function EdgeStatusPanel({ workload, routes, certificates }: EdgeStatusPanelProps) {
  const { formatTimestamp, label, t } = useI18n();
  const certificateById = new Map(certificates.map((certificate) => [certificate.id, certificate]));

  return (
    <section className='surface edge-status-panel' aria-label={t('Route and certificate state')}>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>{t('Authoritative edge projection')}</p>
          <h2>{t('Routes and certificates')}</h2>
        </div>
        <span className='panel-count'>
          <Globe2 size={14} /> {routes.length}
        </span>
      </div>
      {!workload || routes.length === 0 ? (
        <div className='detail-empty'>
          <Globe2 size={21} />
          <strong>{t('No route projection')}</strong>
          <p>{t('Reachability appears only after Cloud owns a route for this workload.')}</p>
        </div>
      ) : (
        <div className='edge-route-list'>
          {routes.map((route) => {
            const certificate = route.gatewayCertificateId
              ? certificateById.get(route.gatewayCertificateId)
              : undefined;
            const generation = workload.deployments.find(
              (deployment) => deployment.revision.id === route.workloadRevisionId
            )?.revision.generation;
            return (
              <article className='edge-route' key={route.id}>
                <div className='edge-route-heading'>
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
                  <span className={`state-badge ${route.state}`}>{label(route.state)}</span>
                </div>
                <dl className='edge-facts'>
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
                    <dd>{route.snapshotDigest ? compactDigest(route.snapshotDigest) : t('Not published')}</dd>
                  </div>
                </dl>
                {route.failure ? <output className='edge-failure'>{route.failure}</output> : null}
                <CertificateProjection certificateId={route.gatewayCertificateId} certificate={certificate} />
              </article>
            );
          })}
        </div>
      )}
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
      <div className='certificate-projection unbound'>
        <ShieldCheck size={16} />
        <span>
          <strong>{t('No managed certificate bound')}</strong>
          <small>{t('This route projection does not reference a Gateway certificate.')}</small>
        </span>
      </div>
    );
  }
  if (!certificate) {
    return (
      <div className='certificate-projection missing'>
        <ShieldCheck size={16} />
        <span>
          <strong>{t('Certificate projection unavailable')}</strong>
          <small>
            {t('Referenced certificate {id} is absent from this snapshot.', { id: shortId(certificateId) })}
          </small>
        </span>
      </div>
    );
  }
  return (
    <div className='certificate-projection'>
      <ShieldCheck size={16} />
      <span>
        <span className='certificate-title'>
          <strong>{certificate.dnsNames.join(', ')}</strong>
          <em className={`state-badge ${certificate.state}`}>{label(certificate.state)}</em>
        </span>
        <small>
          {t('Fingerprint {fingerprint} · expires {expires}', {
            fingerprint: certificate.fingerprint ? compactDigest(certificate.fingerprint) : t('Not issued'),
            expires: formatTimestamp(certificate.expiresAt),
          })}
        </small>
        {certificate.failure ? <small className='certificate-failure'>{certificate.failure}</small> : null}
      </span>
    </div>
  );
}
