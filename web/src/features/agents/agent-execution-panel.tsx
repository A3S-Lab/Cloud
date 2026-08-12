import { Ban, Bot, MessageSquarePlus, Play } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import { useI18n } from '../../lib/i18n';
import type { StreamState } from '../../lib/sse';
import type {
  AgentConversation,
  AgentExecution,
  AgentExecutionEventKind,
  AgentExecutionStatus,
  Asset,
  AssetRelease,
} from '../../types/api';
import { useAgentEventStream } from './use-agent-event-stream';

interface AgentExecutionPanelProps {
  api: CloudApi;
  organizationId: string | null;
  projectId: string | null;
  environmentId: string | null;
  conversations: AgentConversation[];
  selectedConversationId: string;
  assets: Asset[];
  releases: AssetRelease[];
  onSelectConversation: (conversationId: string) => void;
  onConversationChanged: (conversation: AgentConversation) => void;
  onError: (cause: unknown) => void;
}

const PROJECTION_REFRESH_MS = 5_000;

export function AgentExecutionPanel({
  api,
  organizationId,
  projectId,
  environmentId,
  conversations,
  selectedConversationId,
  assets,
  releases,
  onSelectConversation,
  onConversationChanged,
  onError,
}: AgentExecutionPanelProps) {
  const { formatTimestamp, label, t } = useI18n();
  const [executions, setExecutions] = useState<AgentExecution[]>([]);
  const [bindingKey, setBindingKey] = useState('');
  const [creating, setCreating] = useState(false);
  const [starting, setStarting] = useState(false);
  const [cancellingExecutionId, setCancellingExecutionId] = useState<string | null>(null);
  const bindings = useMemo(() => publishedAgentBindings(assets, releases), [assets, releases]);
  const selectedConversation = conversations.find((item) => item.id === selectedConversationId);
  const eventStream = useAgentEventStream(api, organizationId, selectedConversationId || null);

  useEffect(() => {
    setBindingKey((current) => (bindings.some((binding) => binding.key === current) ? current : ''));
  }, [bindings]);

  useEffect(() => {
    if (!organizationId || !selectedConversationId) {
      setExecutions([]);
      return;
    }
    const controller = new AbortController();
    let refreshing = false;
    const refresh = async () => {
      if (refreshing) return;
      refreshing = true;
      try {
        setExecutions(
          await api.listAgentExecutions(organizationId, selectedConversationId, controller.signal)
        );
      } catch (cause) {
        if (!controller.signal.aborted) onError(cause);
      } finally {
        refreshing = false;
      }
    };
    void refresh();
    const interval = window.setInterval(refresh, PROJECTION_REFRESH_MS);
    return () => {
      window.clearInterval(interval);
      controller.abort();
    };
  }, [api, onError, organizationId, selectedConversationId]);

  const createConversation = async () => {
    if (!organizationId || !projectId || !environmentId || creating) return;
    setCreating(true);
    try {
      const mutation = await api.createAgentConversation(
        organizationId,
        projectId,
        environmentId,
        `web:agent-conversation:${crypto.randomUUID()}`
      );
      onConversationChanged(mutation.conversation);
      onSelectConversation(mutation.conversation.id);
    } catch (cause) {
      onError(cause);
    } finally {
      setCreating(false);
    }
  };

  const startExecution = async () => {
    const binding = bindings.find((candidate) => candidate.key === bindingKey);
    if (!organizationId || !selectedConversation || !binding || starting) return;
    setStarting(true);
    try {
      const mutation = await api.startAgentExecution(
        organizationId,
        selectedConversation.id,
        { agentAssetId: binding.asset.id, agentAssetReleaseId: binding.release.id },
        `web:agent-execution:${crypto.randomUUID()}`
      );
      setExecutions((current) => [
        mutation.execution,
        ...current.filter((candidate) => candidate.id !== mutation.execution.id),
      ]);
      onConversationChanged(mutation.conversation);
    } catch (cause) {
      onError(cause);
    } finally {
      setStarting(false);
    }
  };

  const cancelExecution = async (execution: AgentExecution) => {
    if (
      !organizationId ||
      cancellingExecutionId !== null ||
      !['pending', 'running'].includes(execution.status)
    ) {
      return;
    }
    setCancellingExecutionId(execution.id);
    try {
      const mutation = await api.cancelAgentExecution(
        organizationId,
        execution.id,
        `web:agent-execution-cancel:${crypto.randomUUID()}`
      );
      setExecutions((current) =>
        current.map((candidate) => (candidate.id === mutation.execution.id ? mutation.execution : candidate))
      );
      onConversationChanged(mutation.conversation);
    } catch (cause) {
      onError(cause);
    } finally {
      setCancellingExecutionId(null);
    }
  };

  return (
    <section className='agent-workbench agent-workspace' aria-label={t('Agent execution workbench')}>
      <article className='card surface agent-conversation-panel' data-size='sm' data-agent-context>
        <header className='surface-heading'>
          <div>
            <h2>{t('Agent conversations')}</h2>
            <p>{t('Durable context')}</p>
          </div>
          <button
            className='btn secondary-action card-action'
            data-size='sm'
            data-variant='outline'
            type='button'
            disabled={!organizationId || !projectId || !environmentId || creating}
            onClick={createConversation}
          >
            <MessageSquarePlus size={15} /> {creating ? t('Creating...') : t('New conversation')}
          </button>
        </header>
        <section>
          <div
            className='item-group agent-conversation-list'
            role='listbox'
            aria-label={t('Agent conversations')}
          >
            {conversations.length === 0 ? (
              <div className='empty agent-empty'>
                <header>
                  <p>{t('Create a conversation to start an immutable Agent release.')}</p>
                </header>
              </div>
            ) : (
              conversations.map((conversation) => {
                const selected = conversation.id === selectedConversationId;
                return (
                  <button
                    type='button'
                    role='option'
                    aria-selected={selected}
                    className={`item${selected ? ' selected' : ''}`}
                    data-size='sm'
                    data-variant={selected ? 'muted' : 'outline'}
                    onClick={() => onSelectConversation(conversation.id)}
                    key={conversation.id}
                  >
                    <span data-item-content>
                      <strong>{shortId(conversation.id)}</strong>
                      <small>{formatTimestamp(conversation.updatedAt)}</small>
                    </span>
                    <span data-item-actions>
                      <span className='status-badge' data-state='neutral' data-size='sm'>
                        {t('{count} events', { count: conversation.lastEventSequence })}
                      </span>
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </section>
      </article>

      <article className='card surface agent-execution-panel' data-size='sm' data-agent-canvas>
        <header className='surface-heading'>
          <div>
            <h2>{t('Executions')}</h2>
            <p>{t('Exact release binding')}</p>
          </div>
          <Bot className='card-action' size={20} aria-hidden='true' />
        </header>
        <section>
          <div className='agent-start-controls'>
            <div className='field'>
              <label htmlFor='agent-release-binding'>{t('Published Agent release')}</label>
              <select
                className='select'
                id='agent-release-binding'
                value={bindingKey}
                onChange={(event) => setBindingKey(event.target.value)}
              >
                <option value=''>{t('Choose a release')}</option>
                {bindings.map((binding) => (
                  <option value={binding.key} key={binding.key}>
                    {binding.asset.name} · {binding.release.version}
                  </option>
                ))}
              </select>
            </div>
            <button
              className='btn primary-action'
              data-size='sm'
              type='button'
              disabled={!selectedConversation || !bindingKey || starting}
              onClick={startExecution}
            >
              <Play size={15} /> {starting ? t('Starting...') : t('Start execution')}
            </button>
          </div>
          <ul className='item-group agent-execution-list'>
            {executions.length === 0 ? (
              <li className='empty agent-empty'>
                <header>
                  <p>{t('No executions recorded for this conversation.')}</p>
                </header>
              </li>
            ) : (
              executions.map((execution) => (
                <li className='item' data-size='sm' data-variant='outline' key={execution.id}>
                  <span data-item-content>
                    <strong>{shortId(execution.id)}</strong>
                    <small>
                      {t('release {release}', { release: shortId(execution.agent.assetReleaseId) })}
                    </small>
                  </span>
                  <span data-item-actions>
                    <span
                      className='status-badge agent-status'
                      data-state={agentExecutionStatusState(execution.status)}
                      data-size='sm'
                      data-indicator
                    >
                      {label(execution.status)}
                    </span>
                    {['pending', 'running', 'cancelling'].includes(execution.status) ? (
                      <button
                        className='btn agent-cancel-action'
                        data-size='xs'
                        data-variant='destructive'
                        type='button'
                        disabled={execution.status === 'cancelling' || cancellingExecutionId !== null}
                        onClick={() => cancelExecution(execution)}
                      >
                        <Ban size={12} />
                        {execution.status === 'cancelling' || cancellingExecutionId === execution.id
                          ? t('Cancelling')
                          : t('Cancel')}
                      </button>
                    ) : null}
                  </span>
                </li>
              ))
            )}
          </ul>
        </section>
      </article>

      <article className='card surface agent-event-panel' data-size='sm' data-agent-activity>
        <header className='surface-heading'>
          <div>
            <h2>{t('Execution events')}</h2>
            <p>{t('Monotonic semantic history')}</p>
          </div>
          <span
            className='status-badge agent-stream-state card-action'
            data-state={agentStreamStatusState(eventStream.state)}
            data-indicator
          >
            {label(eventStream.state)}
          </span>
        </header>
        <section>
          {eventStream.error ? (
            <p className='agent-stream-error' role='alert'>
              {t(eventStream.error)}
            </p>
          ) : null}
          <ol className='timeline agent-event-list' aria-label={t('Execution events')} reversed>
            {eventStream.records.length === 0 ? (
              <li>
                <div className='empty agent-empty'>
                  <header>
                    <p>{t('Select a conversation to follow its semantic event stream.')}</p>
                  </header>
                </div>
              </li>
            ) : (
              [...eventStream.records].reverse().map((event) => (
                <li
                  key={event.sequence}
                  data-marker={event.sequence}
                  data-state={agentEventStatusState(event.kind)}
                >
                  <article className='item' data-size='sm' data-variant='outline'>
                    <section>
                      <h3>{label(event.kind)}</h3>
                      <p>
                        {t('{time} · {count} bytes', {
                          time: formatTimestamp(event.occurredAt),
                          count: event.contentSizeBytes,
                        })}
                      </p>
                      <pre>{boundedJson(event.content)}</pre>
                    </section>
                  </article>
                </li>
              ))
            )}
          </ol>
        </section>
      </article>
    </section>
  );
}

interface AgentBinding {
  key: string;
  asset: Asset;
  release: AssetRelease;
}

function publishedAgentBindings(assets: Asset[], releases: AssetRelease[]): AgentBinding[] {
  const agents = new Map(
    assets
      .filter((asset) => asset.kind === 'agent' && asset.state === 'active')
      .map((asset) => [asset.id, asset])
  );
  return releases
    .filter(
      (release) => release.state === 'published' && release.artifact !== null && agents.has(release.assetId)
    )
    .map((release) => ({
      key: `${release.assetId}:${release.id}`,
      asset: agents.get(release.assetId) as Asset,
      release,
    }))
    .sort((left, right) =>
      `${left.asset.name}:${left.release.version}`.localeCompare(
        `${right.asset.name}:${right.release.version}`
      )
    );
}

function shortId(value: string): string {
  return value.length <= 12 ? value : `${value.slice(0, 8)}…${value.slice(-4)}`;
}

function boundedJson(value: unknown): string {
  const encoded = JSON.stringify(value, null, 2) ?? 'null';
  return encoded.length > 4_096 ? `${encoded.slice(0, 4_096)}\n…` : encoded;
}

type StatusBadgeState = 'neutral' | 'active' | 'success' | 'warning' | 'danger';

export function agentExecutionStatusState(status: AgentExecutionStatus): StatusBadgeState {
  if (status === 'running') return 'active';
  if (status === 'succeeded') return 'success';
  if (status === 'cancelling') return 'warning';
  if (status === 'failed' || status === 'cancelled') return 'danger';
  return 'neutral';
}

export function agentStreamStatusState(state: StreamState): StatusBadgeState {
  if (state === 'live') return 'active';
  if (state === 'connecting' || state === 'retrying') return 'warning';
  return 'neutral';
}

export function agentEventStatusState(kind: AgentExecutionEventKind): StatusBadgeState {
  if (kind === 'execution_completed') return 'success';
  if (kind === 'execution_failed' || kind === 'execution_cancelled') return 'danger';
  if (kind === 'model_output') return 'active';
  return 'neutral';
}
