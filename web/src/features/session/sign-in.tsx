import { ArrowRight, KeyRound, ShieldCheck } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { CloudApi } from '../../lib/api';
import type { Organization } from '../../types/api';

interface SignInProps {
  onAuthenticated: (token: string, organizations: Organization[]) => void;
}

export function SignIn({ onAuthenticated }: SignInProps) {
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
      <section className='signin-story' aria-labelledby='signin-title'>
        <div className='brand-lockup'>
          <span className='brand-mark' aria-hidden='true'>
            A3
          </span>
          <span>A3S Cloud</span>
        </div>
        <div className='story-copy'>
          <p className='eyebrow'>Operator-owned infrastructure</p>
          <h1 id='signin-title'>Desired state in. Proven convergence out.</h1>
          <p>
            Operate applications, Agents, MCP servers, and Skills through one durable control plane—without
            surrendering the nodes they run on.
          </p>
        </div>
        <div className='trust-row'>
          <span>
            <ShieldCheck size={17} /> PostgreSQL authority
          </span>
          <span>
            <ShieldCheck size={17} /> Flow-backed operations
          </span>
        </div>
      </section>

      <section className='signin-panel' aria-label='Sign in to A3S Cloud'>
        <div className='signin-card'>
          <div className='field-icon' aria-hidden='true'>
            <KeyRound size={22} />
          </div>
          <p className='eyebrow'>Control plane access</p>
          <h2>Sign in with an API token</h2>
          <p className='muted'>
            The token stays in this browser tab and is sent only as a Bearer credential.
          </p>
          <form onSubmit={submit}>
            <label htmlFor='api-token'>Organization API token</label>
            <input
              id='api-token'
              type='password'
              autoComplete='off'
              spellCheck={false}
              placeholder='a3s_••••••••••••••••'
              value={token}
              onChange={(event) => setToken(event.target.value)}
              aria-invalid={Boolean(error)}
              aria-describedby={error ? 'signin-error' : undefined}
            />
            {error ? (
              <p className='form-error' id='signin-error' role='alert'>
                {error}
              </p>
            ) : null}
            <button className='primary-button' type='submit' disabled={submitting}>
              {submitting ? 'Verifying…' : 'Open control plane'}
              <ArrowRight size={17} />
            </button>
          </form>
          <p className='signin-footnote'>
            Initial credentials are created through the protected bootstrap API.
          </p>
        </div>
      </section>
    </main>
  );
}
