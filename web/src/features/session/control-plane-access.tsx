import { ArrowRight, KeyRound } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { CloudApi } from '../../lib/api';
import { useI18n } from '../../lib/i18n';
import type { Organization } from '../../types/api';

interface ControlPlaneAccessProps {
  onAuthenticated: (token: string, organizations: Organization[]) => void;
}

export function ControlPlaneAccess({ onAuthenticated }: ControlPlaneAccessProps) {
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
    <section id='access' className='signin-card' aria-label={t('Sign in to A3S OS')}>
      <div className='signin-card-heading'>
        <span className='field-icon' aria-hidden='true'>
          <KeyRound size={21} />
        </span>
        <div>
          <h2>{t('Open A3S Web')}</h2>
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
          {submitting ? t('Verifying...') : t('Open A3S Web')}
          <ArrowRight size={17} />
        </button>
      </form>
    </section>
  );
}
