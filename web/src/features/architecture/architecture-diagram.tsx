import { Download, LoaderCircle, Network } from 'lucide-react';
import { useRef, useState } from 'react';
import { useI18n } from '../../lib/i18n';
import { exportElementAsPng } from './architecture-export';

const EXPORT_FILENAME = 'a3s-cloud-module-architecture.png';
const HARNESS_COMMAND = '/usr/bin/a3s code harness --manifest /app/.a3s/asset.acl';

const AUDIENCES = [
  'Agent applications',
  'Developers',
  'Platform operators',
  'Automation',
  'Enterprise integration',
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
  'Operations + A3S Flow',
  'Workloads scheduling',
  'Outbox + A3S Event',
];
const BUSINESS_GROUPS = [
  {
    title: 'Platform and resources',
    items: [
      'Identity / Tenants',
      'Projects / Environments',
      'Assets / Artifacts',
      'Search / Audit',
      'Plugins (planned)',
    ],
  },
  {
    title: 'Delivery and services',
    items: [
      'Sources / Builds',
      'Generic Executions',
      'Workloads / Deployments',
      'Secrets',
      'Edge / Gateway',
      'Data / Inference (planned)',
    ],
  },
  {
    title: 'Agent platform',
    items: [
      'Agent Release',
      'Conversations / Executions',
      'Semantic event stream',
      'Skill / MCP bindings',
      'Approvals / checkpoints (planned)',
    ],
  },
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
  'Immutable object storage',
  'OCI Registry',
  'mTLS + A3S ACL',
  'Compatibility lock',
];

export function ArchitectureSection() {
  const { t } = useI18n();
  const diagram = useRef<HTMLDivElement>(null);
  const [exporting, setExporting] = useState(false);
  const [exportStatus, setExportStatus] = useState('PNG is generated from this live HTML diagram.');
  const [exportFailed, setExportFailed] = useState(false);

  const exportPng = async () => {
    if (!diagram.current || exporting) return;
    setExporting(true);
    setExportFailed(false);
    setExportStatus('Rendering the architecture PNG…');
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
    <section
      id='console-architecture-panel'
      className='console-section architecture-section'
      role='tabpanel'
      aria-labelledby='console-architecture-tab'
    >
      <article className='surface architecture-surface'>
        <header className='architecture-toolbar'>
          <div>
            <h2>{t('A3S Cloud architecture')}</h2>
            <p>
              {t(
                'One control path from Cloud intent to the sole A3S Code Harness. Every exported image is rendered from the HTML below.'
              )}
            </p>
          </div>
          <div className='architecture-export-controls'>
            <button type='button' disabled={exporting} onClick={() => void exportPng()}>
              {exporting ? <LoaderCircle className='spinning' size={16} /> : <Download size={16} />}
              {exporting ? t('Exporting...') : t('Export PNG')}
            </button>
            <p className={exportFailed ? 'export-error' : undefined} role={exportFailed ? 'alert' : 'status'}>
              {t(exportStatus.replace('…', '...'))}
            </p>
          </div>
        </header>

        <section
          className='architecture-scroll'
          aria-label={t('Scrollable A3S Cloud architecture diagram')}
        >
          <div ref={diagram} className='architecture-diagram'>
            <header className='architecture-diagram-heading'>
              <span className='architecture-title-mark' aria-hidden='true'>
                <Network size={23} />
              </span>
              <div>
                <p>A3S Cloud</p>
                <h3>{t('Module architecture')}</h3>
              </div>
              <span className='architecture-version'>{t('Control and execution authority')}</span>
            </header>

            <ModuleBand title='Users and application scenarios' tone='audience' items={AUDIENCES} />
            <ModuleBand title='Unified access and experience' tone='access' items={ACCESS_SURFACES} />
            <ModuleBand title='Cloud orchestration and control' tone='control' items={ORCHESTRATION} />

            <section
              className='architecture-band architecture-band-business'
              aria-labelledby='business-band-title'
            >
              <h4 id='business-band-title'>{t('Cloud business modules')}</h4>
              <div className='architecture-business-groups'>
                {BUSINESS_GROUPS.map((group) => (
                  <article className='architecture-business-group' key={group.title}>
                    <h5>{t(group.title)}</h5>
                    <ul>
                      {group.items.map((item) => (
                        <li
                          className={item.endsWith('(planned)') ? 'architecture-module-planned' : undefined}
                          key={item}
                        >
                          {t(item)}
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
              <h4 id='runtime-band-title'>{t('Node and runtime plane')}</h4>
              <ol className='architecture-runtime-path'>
                {NODE_PATH.map((item, index) => (
                  <li key={item}>
                    <span>{t(item)}</span>
                    {index < NODE_PATH.length - 1 ? <b aria-hidden='true'>→</b> : null}
                  </li>
                ))}
              </ol>
            </section>

            <section
              className='architecture-band architecture-band-payload'
              aria-labelledby='payload-band-title'
            >
              <h4 id='payload-band-title'>{t('Runtime payloads')}</h4>
              <div className='architecture-payload-grid'>
                <article className='architecture-payload-card'>{t('Application / MCP')}</article>
                <article className='architecture-payload-card architecture-harness-card'>
                  <span>{t('A3S Code Core · Sole Agent execution Harness')}</span>
                  <code>{HARNESS_COMMAND}</code>
                  <strong>{t('Cloud only orchestrates and transports')}</strong>
                </article>
                <article className='architecture-payload-card architecture-module-planned'>
                  {t('A3S Power (planned)')}
                </article>
              </div>
            </section>

            <ModuleBand title='Infrastructure and trust boundaries' tone='foundation' items={FOUNDATIONS} />
          </div>
        </section>
      </article>
    </section>
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
