import { Ban, Bot, CircleDot, MessageSquarePlus, Play, Radio } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import type { CloudApi } from '../../lib/api';
import { useI18n } from '../../lib/i18n';
import type { AgentConversation, AgentExecution, Asset, AssetRelease } from '../../types/api';
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
    <div className='agent-workspace'>
      <article className='surface agent-conversation-panel'>
        <div className='surface-heading'>
          <div>
            <p className='eyebrow'>{t('Durable context')}</p>
            <h2>{t('Agent conversations')}</h2>
          </div>
          <button
            className='secondary-action'
            type='button'
            disabled={!organizationId || !projectId || !environmentId || creating}
            onClick={createConversation}
          >
            <MessageSquarePlus size={15} /> {creating ? t('Creating...') : t('New conversation')}
          </button>
        </div>
        <div className='agent-conversation-list' role='listbox' aria-label={t('Agent conversations')}>
          {conversations.length === 0 ? (
            <p className='agent-empty'>{t('Create a conversation to start an immutable Agent release.')}</p>
          ) : (
            conversations.map((conversation) => (
              <button
                type='button'
                role='option'
                aria-selected={conversation.id === selectedConversationId}
                className={conversation.id === selectedConversationId ? 'selected' : undefined}
                onClick={() => onSelectConversation(conversation.id)}
                key={conversation.id}
              >
                <span>
                  <strong>{shortId(conversation.id)}</strong>
                  <small>{formatTimestamp(conversation.updatedAt)}</small>
                </span>
                <em>{t('{count} events', { count: conversation.lastEventSequence })}</em>
              </button>
            ))
          )}
        </div>
      </article>

      <article className='surface agent-execution-panel'>
        <div className='surface-heading'>
          <div>
            <p className='eyebrow'>{t('Exact release binding')}</p>
            <h2>{t('Executions')}</h2>
          </div>
          <Bot size={20} />
        </div>
        <div className='agent-start-controls'>
          <label>
            {t('Published Agent release')}
            <select value={bindingKey} onChange={(event) => setBindingKey(event.target.value)}>
              <option value=''>{t('Choose a release')}</option>
              {bindings.map((binding) => (
                <option value={binding.key} key={binding.key}>
                  {binding.asset.name} · {binding.release.version}
                </option>
              ))}
            </select>
          </label>
          <button
            className='primary-action'
            type='button'
            disabled={!selectedConversation || !bindingKey || starting}
            onClick={startExecution}
          >
            <Play size={15} /> {starting ? t('Starting...') : t('Start execution')}
          </button>
        </div>
        <div className='agent-execution-list'>
          {executions.length === 0 ? (
            <p className='agent-empty'>{t('No executions recorded for this conversation.')}</p>
          ) : (
            executions.map((execution) => (
              <div key={execution.id}>
                <span className={`agent-status ${execution.status}`}>
                  <CircleDot size={12} /> {label(execution.status)}
                </span>
                <strong>{shortId(execution.id)}</strong>
                <small>{t('release {release}', { release: shortId(execution.agent.assetReleaseId) })}</small>
                {['pending', 'running', 'cancelling'].includes(execution.status) ? (
                  <button
                    className='agent-cancel-action'
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
              </div>
            ))
          )}
        </div>
      </article>

      <article className='surface agent-event-panel'>
        <div className='surface-heading'>
          <div>
            <p className='eyebrow'>{t('Monotonic semantic history')}</p>
            <h2>{t('Execution events')}</h2>
          </div>
          <span className={`agent-stream-state ${eventStream.state}`}>
            <Radio size={14} /> {label(eventStream.state)}
          </span>
        </div>
        {eventStream.error ? <p className='agent-stream-error'>{t(eventStream.error)}</p> : null}
        <ol className='agent-event-list'>
          {eventStream.records.length === 0 ? (
            <li className='agent-empty'>{t('Select a conversation to follow its semantic event stream.')}</li>
          ) : (
            [...eventStream.records].reverse().map((event) => (
              <li key={event.sequence}>
                <span>{event.sequence}</span>
                <div>
                  <strong>{label(event.kind)}</strong>
                  <small>
                    {t('{time} · {count} bytes', {
                      time: formatTimestamp(event.occurredAt),
                      count: event.contentSizeBytes,
                    })}
                  </small>
                  <pre>{boundedJson(event.content)}</pre>
                </div>
              </li>
            ))
          )}
        </ol>
      </article>
    </div>
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
