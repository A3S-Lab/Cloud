import { Download, LoaderCircle, Network } from 'lucide-react';
import { useRef, useState } from 'react';
import { useI18n } from '../../lib/i18n';
import { statusBadgeState } from '../../lib/status-badge';
import {
  CAPABILITY_GROUPS,
  CAPABILITY_STATES,
  localize,
  PRODUCT_PILLARS,
  ROADMAP_SNAPSHOT,
  type CapabilityState,
} from '../project/project-catalog';
import { exportElementAsPng } from './architecture-export';

const EXPORT_FILENAME = 'a3s-cloud-module-architecture.png';
const HARNESS_COMMAND = '/usr/bin/a3s code harness --manifest /app/.a3s/asset.acl';

const AUDIENCES = [
  'Agent applications',
  'Application services',
  'Developers',
  'Platform operators',
  'Enterprise automation',
];
const ACCESS_SURFACES = [
  'Web console',
  'a3s-cloud CLI',
  'REST / OpenAPI',
  'TypeScript SDK',
  'Management MCP',
];
const ORCHESTRATION = [
  'Commands / Queries',
  'PostgreSQL desired state',
  'Operations + A3S Flow',
  'Outbox + A3S Event',
  'Workloads / Fleet',
];
const NODE_PATH = [
  'Fleet node_commands',
  'Leases / Claims / receipts',
  'Outbound-only Node Agent',
  'A3S Runtime Task / Service',
  'A3S Box',
];
const FOUNDATIONS = [
  'PostgreSQL + A3S ORM',
  'Immutable objects + fenced mutable volumes',
  'OCI Registry',
  'mTLS + A3S ACL',
  'Compatibility lock',
];
const LEGEND_STATES: readonly CapabilityState[] = ['verified', 'in-progress', 'recertification', 'planned'];

export function ArchitectureOverview() {
  return (
    <section id='architecture' className='home-section home-architecture'>
      <ArchitecturePanel />
    </section>
  );
}

export function ArchitectureSection() {
  return (
    <section
      id='console-architecture-panel'
      className='console-section architecture-section'
      role='tabpanel'
      aria-labelledby='console-architecture-tab'
    >
      <ArchitecturePanel />
    </section>
  );
}

export function ArchitecturePanel() {
  const { language, t } = useI18n();
  const diagram = useRef<HTMLDivElement>(null);
  const [exporting, setExporting] = useState(false);
  const [exportStatus, setExportStatus] = useState('PNG is generated from this live HTML diagram.');
  const [exportFailed, setExportFailed] = useState(false);

  const exportPng = async () => {
    if (!diagram.current || exporting) return;
    setExporting(true);
    setExportFailed(false);
    setExportStatus('Rendering the architecture PNG...');
    try {
      await exportElementAsPng(diagram.current, EXPORT_FILENAME);
      setExportStatus('Architecture PNG exported.');
    } catch (cause) {
      setExportFailed(true);
      setExportStatus(cause instanceof Error ? cause.message : 'Architecture PNG export failed.');
    } finally {
      setExporting(false);
    }
  };

  return (
    <article className='card surface architecture-surface'>
      <header className='toolbar architecture-toolbar' data-wrap='true'>
        <div>
          <h2>{t('A3S OS architecture')}</h2>
          <p>
            {t(
              'The complete 19-gate portfolio shares one control path from A3S OS intent to Runtime, Gateway, and one provider-neutral Agent execution contract.'
            )}
          </p>
        </div>
        <fieldset className='architecture-export-controls'>
          <legend className='sr-only'>{t('Export PNG')}</legend>
          <button
            className='btn'
            data-size='sm'
            data-variant='outline'
            type='button'
            disabled={exporting}
            onClick={() => void exportPng()}
          >
            {exporting ? <LoaderCircle className='spinning' size={16} /> : <Download size={16} />}
            {exporting ? t('Exporting...') : t('Export PNG')}
          </button>
          <p className={exportFailed ? 'export-error' : undefined} role={exportFailed ? 'alert' : 'status'}>
            {t(exportStatus)}
          </p>
        </fieldset>
      </header>

      <section className='architecture-scroll' aria-label={t('Scrollable A3S OS architecture diagram')}>
        <div ref={diagram} className='architecture-diagram'>
          <header className='architecture-diagram-heading'>
            <span className='architecture-title-mark' aria-hidden='true'>
              <Network size={23} />
            </span>
            <div>
              <p>A3S OS</p>
              <h3>{t('Complete module architecture')}</h3>
            </div>
            <span className='architecture-version'>
              {t('Roadmap snapshot')} {ROADMAP_SNAPSHOT}
            </span>
          </header>

          <ModuleBand title='Users and application scenarios' tone='audience' items={AUDIENCES} />
          <section
            className='architecture-band architecture-band-products'
            aria-labelledby='products-band-title'
          >
            <h4 id='products-band-title'>{t('External application products')}</h4>
            <ul className='architecture-product-grid'>
              {PRODUCT_PILLARS.map((product) => (
                <li key={product.id}>
                  <strong>{localize(product.title, language)}</strong>
                  <small>
                    {t('Built on')} {product.basedOn}
                  </small>
                </li>
              ))}
            </ul>
          </section>
          <ModuleBand title='Unified access and experience' tone='access' items={ACCESS_SURFACES} />
          <ModuleBand title='Cloud orchestration and control' tone='control' items={ORCHESTRATION} />

          <section
            className='architecture-band architecture-band-business'
            aria-labelledby='business-band-title'
          >
            <div className='architecture-band-title-row'>
              <h4 id='business-band-title'>{t('Complete Cloud product portfolio')}</h4>
              <ul className='architecture-legend' aria-label={t('Roadmap state legend')}>
                {LEGEND_STATES.map((state) => (
                  <li
                    className={`status-badge architecture-legend-${state}`}
                    data-state={statusBadgeState(state)}
                    data-size='sm'
                    key={state}
                  >
                    {localize(CAPABILITY_STATES[state].label, language)}
                  </li>
                ))}
              </ul>
            </div>
            <div className='architecture-business-groups'>
              {CAPABILITY_GROUPS.map((group) => (
                <article className='architecture-business-group' key={group.id}>
                  <h5>{localize(group.title, language)}</h5>
                  <ul>
                    {group.gates.map((capability) => (
                      <li className={`architecture-module-${capability.state}`} key={capability.code}>
                        <code>{capability.code}</code>
                        <span>{localize(capability.title, language)}</span>
                        {capability.unavailable ? <small>{t('Unavailable')}</small> : null}
                      </li>
                    ))}
                  </ul>
                </article>
              ))}
            </div>
          </section>

          <section
            className='architecture-band architecture-band-runtime'
            aria-labelledby='runtime-band-title'
          >
            <h4 id='runtime-band-title'>{t('Node convergence and execution plane')}</h4>
            <ol
              className='stepper architecture-runtime-path'
              aria-label={t('Node convergence and execution plane')}
              // biome-ignore lint/a11y/noNoninteractiveTabindex: Overflowing steps must remain keyboard-scrollable.
              tabIndex={0}
            >
              {NODE_PATH.map((item, index) => (
                <li key={item}>
                  <span data-step-marker aria-hidden='true'>
                    {index + 1}
                  </span>
                  <section>
                    <h5>{t(item)}</h5>
                  </section>
                </li>
              ))}
            </ol>
          </section>

          <section
            className='architecture-band architecture-band-payload'
            aria-labelledby='payload-band-title'
          >
            <h4 id='payload-band-title'>{t('Runtime services and payload ownership')}</h4>
            <div className='architecture-payload-grid'>
              <article className='architecture-payload-card'>{t('Applications / Hosted MCP')}</article>
              <article className='architecture-payload-card architecture-harness-card'>
                <span>{t('A3S Code Core / Native Agent execution provider')}</span>
                <code>{HARNESS_COMMAND}</code>
                <strong>{t('One Cloud lifecycle and provider conformance contract')}</strong>
              </article>
              <article className='architecture-payload-card architecture-module-planned'>
                {t('A3S Power / Inference planned')}
              </article>
            </div>
          </section>

          <ModuleBand title='Infrastructure and trust boundaries' tone='foundation' items={FOUNDATIONS} />
        </div>
      </section>
    </article>
  );
}

function ModuleBand({ title, tone, items }: { title: string; tone: string; items: readonly string[] }) {
  const { t } = useI18n();
  const titleId = `architecture-${tone}-title`;
  return (
    <section className={`architecture-band architecture-band-${tone}`} aria-labelledby={titleId}>
      <h4 id={titleId}>{t(title)}</h4>
      <ul className={`architecture-module-grid architecture-module-grid-${items.length}`}>
        {items.map((item) => (
          <li key={item}>{t(item)}</li>
        ))}
      </ul>
    </section>
  );
}
