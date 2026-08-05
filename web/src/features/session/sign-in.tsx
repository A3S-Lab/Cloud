import {
  ArrowRight,
  Boxes,
  CheckCircle2,
  Cloud,
  Code2,
  KeyRound,
  Network,
  Route,
  ShieldCheck,
  Workflow,
  type LucideIcon,
} from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { CloudApi } from '../../lib/api';
import { LanguageSwitcher, useI18n } from '../../lib/i18n';
import type { Organization } from '../../types/api';

interface SignInProps {
  onAuthenticated: (token: string, organizations: Organization[]) => void;
}

const AUTHORITY_PATH: ReadonlyArray<{
  label: string;
  detail: string;
  icon: LucideIcon;
}> = [
  { label: 'A3S Cloud', detail: 'Intent, identity, and policy', icon: Cloud },
  { label: 'Operations + A3S Flow', detail: 'Durable orchestration', icon: Workflow },
  { label: 'Outbound-only Node Agent', detail: 'Typed command delivery', icon: Network },
  { label: 'A3S Runtime + Box', detail: 'Execution and isolation', icon: Boxes },
  { label: 'A3S Code Harness', detail: 'Sole Agent run owner', icon: Code2 },
];

const CAPABILITIES: ReadonlyArray<{
  label: string;
  detail: string;
  icon: LucideIcon;
}> = [
  { label: 'Desired state', detail: 'PostgreSQL authority', icon: CheckCircle2 },
  { label: 'Durable operations', detail: 'Flow-backed recovery', icon: Workflow },
  { label: 'Outbound nodes', detail: 'No inbound management ports', icon: Network },
  { label: 'Managed reachability', detail: 'Gateway policy and evidence', icon: Route },
];

export function SignIn({ onAuthenticated }: SignInProps) {
  const { t } = useI18n();
  const [token, setToken] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const candidate = token.trim();
    if (!candidate) {
      setError('Enter an organization API token.');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const organizations = await new CloudApi(candidate).listOrganizations();
      if (organizations.length === 0) {
        throw new Error('This token has no visible organization.');
      }
      onAuthenticated(candidate, organizations);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Cloud could not verify this token.');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className='signin-shell'>
      <header className='signin-topbar'>
        <div className='brand-lockup'>
          <span className='brand-mark' aria-hidden='true'>
            A3
          </span>
          <span>A3S Cloud</span>
        </div>
        <div className='signin-topbar-actions'>
          <span className='signin-product-label'>{t('Self-hosted control plane')}</span>
          <LanguageSwitcher />
        </div>
      </header>

      <section className='signin-hero' aria-labelledby='signin-title'>
        <div className='signin-story'>
          <span className='signin-category'>{t('A3S-native operations')}</span>
          <div className='story-copy'>
            <h1 id='signin-title'>
              <span>A3S Cloud</span>
              {t('Operate Agents on infrastructure you own.')}
            </h1>
            <p>
              {t(
                'Deploy applications and run Agents through one durable control plane for delivery, execution, routing, and authoritative evidence.'
              )}
            </p>
          </div>
          <ul className='trust-row' aria-label={t('Platform trust boundaries')}>
            <li>
              <ShieldCheck size={18} /> {t('Scoped identity')}
            </li>
            <li>
              <ShieldCheck size={18} /> {t('Operator-owned nodes')}
            </li>
            <li>
              <ShieldCheck size={18} /> {t('Durable audit trail')}
            </li>
          </ul>
        </div>

        <div className='signin-panel'>
          <section className='signin-authority-card' aria-labelledby='authority-path-title'>
            <div className='signin-authority-heading'>
              <div>
                <h2 id='authority-path-title'>{t('One control path')}</h2>
                <p>{t('Cloud orchestrates. Existing A3S authorities execute.')}</p>
              </div>
              <span>{t('Live architecture')}</span>
            </div>
            <ol className='signin-authority-path'>
              {AUTHORITY_PATH.map(({ label, detail, icon: Icon }) => (
                <li key={label}>
                  <span className='authority-icon' aria-hidden='true'>
                    <Icon size={19} />
                  </span>
                  <span>
                    <strong>{t(label)}</strong>
                    <small>{t(detail)}</small>
                  </span>
                </li>
              ))}
            </ol>
          </section>

          <section className='signin-card' aria-label={t('Sign in to A3S Cloud')}>
            <div className='signin-card-heading'>
              <span className='field-icon' aria-hidden='true'>
                <KeyRound size={21} />
              </span>
              <div>
                <h2>{t('Open the control plane')}</h2>
                <p>{t('The credential remains in this browser tab.')}</p>
              </div>
            </div>
            <form onSubmit={submit}>
              <label htmlFor='api-token'>{t('Organization API token')}</label>
              <input
                id='api-token'
                type='password'
                autoComplete='off'
                spellCheck={false}
                placeholder='a3s_••••••••••••••••'
                value={token}
                onChange={(event) => setToken(event.target.value)}
                aria-invalid={Boolean(error)}
                aria-describedby={error ? 'signin-error' : 'signin-token-help'}
              />
              <p className='signin-token-help' id='signin-token-help'>
                {t('Sent only as a Bearer credential to the configured Cloud API.')}
              </p>
              {error ? (
                <p className='form-error' id='signin-error' role='alert'>
                  {t(error)}
                </p>
              ) : null}
              <button className='primary-button' type='submit' disabled={submitting}>
                {submitting ? t('Verifying...') : t('Open control plane')}
                <ArrowRight size={17} />
              </button>
            </form>
          </section>
        </div>
      </section>

      <section className='signin-capability-strip' aria-label={t('A3S Cloud platform capabilities')}>
        {CAPABILITIES.map(({ label, detail, icon: Icon }) => (
          <article key={label}>
            <span aria-hidden='true'>
              <Icon size={20} />
            </span>
            <div>
              <strong>{t(label)}</strong>
              <small>{t(detail)}</small>
            </div>
          </article>
        ))}
      </section>
    </main>
  );
}
