import { ArrowUpRight, LoaderCircle, Search } from 'lucide-react';
import { useEffect, useId, useState } from 'react';
import { DEFAULT_SEARCH_LIMIT, type CloudApi, validateSearchRequest } from '../../lib/api';
import { useI18n } from '../../lib/i18n';
import type { SearchResult } from '../../types/api';

interface ResourceSearchProps {
  api: CloudApi;
  organizationId: string | null;
  onSelect: (result: SearchResult) => void;
}

const SEARCH_DEBOUNCE_MS = 250;

export function ResourceSearch({ api, organizationId, onSelect }: ResourceSearchProps) {
  const { label, t } = useI18n();
  const listboxId = useId();
  const [query, setQuery] = useState('');
  const [settledQuery, setSettledQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const normalizedQuery = query.trim();

  useEffect(() => {
    if (!organizationId || normalizedQuery.length === 0) {
      setResults([]);
      setSettledQuery('');
      setActiveIndex(0);
      setLoading(false);
      setOpen(false);
      setError(null);
      return;
    }

    let validatedQuery: string;
    try {
      validatedQuery = validateSearchRequest(query, DEFAULT_SEARCH_LIMIT);
    } catch (cause) {
      setResults([]);
      setSettledQuery('');
      setLoading(false);
      setOpen(true);
      setError(messageFrom(cause));
      return;
    }

    setResults([]);
    setSettledQuery('');
    setActiveIndex(0);
    setLoading(false);
    setError(null);

    const controller = new AbortController();
    const timeout = window.setTimeout(() => {
      setLoading(true);
      setOpen(true);
      setError(null);
      api
        .searchResources(organizationId, validatedQuery, DEFAULT_SEARCH_LIMIT, controller.signal)
        .then((items) => {
          if (controller.signal.aborted) return;
          setResults(items);
          setSettledQuery(validatedQuery);
          setActiveIndex(0);
          setError(null);
        })
        .catch((cause) => {
          if (controller.signal.aborted) return;
          setResults([]);
          setSettledQuery(validatedQuery);
          setError(messageFrom(cause));
        })
        .finally(() => {
          if (!controller.signal.aborted) setLoading(false);
        });
    }, SEARCH_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [api, normalizedQuery, organizationId, query]);

  const selectResult = (result: SearchResult) => {
    onSelect(result);
    setQuery('');
    setResults([]);
    setOpen(false);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      setOpen(false);
      return;
    }
    if (!open || results.length === 0) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActiveIndex((current) => Math.min(results.length - 1, current + 1));
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((current) => Math.max(0, current - 1));
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      const result = results[activeIndex];
      if (result) selectResult(result);
    }
  };

  return (
    <form
      className='resource-search'
      onSubmit={(event) => event.preventDefault()}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      <Search className='resource-search-icon' size={16} aria-hidden='true' />
      <input
        type='search'
        value={query}
        disabled={!organizationId}
        placeholder={
          organizationId ? t('Search authorized resources') : t('Choose an organization to search')
        }
        aria-label={t('Search authorized Cloud resources')}
        aria-autocomplete='list'
        aria-controls={listboxId}
        aria-expanded={open}
        aria-activedescendant={open && results[activeIndex] ? `${listboxId}-${activeIndex}` : undefined}
        role='combobox'
        onChange={(event) => setQuery(event.target.value)}
        onFocus={() => {
          if (normalizedQuery.length > 0) setOpen(true);
        }}
        onKeyDown={onKeyDown}
      />
      {loading ? (
        <LoaderCircle className='resource-search-spinner' size={15} aria-label={t('Searching')} />
      ) : null}

      {open ? (
        <div className='resource-search-popover'>
          {error ? (
            <p className='resource-search-message error' role='alert'>
              {t(error)}
            </p>
          ) : null}
          {!error && !loading && settledQuery === normalizedQuery && results.length === 0 ? (
            <output className='resource-search-message'>{t('No authorized resources found.')}</output>
          ) : null}
          {results.length > 0 ? (
            <div className='resource-search-results' id={listboxId} role='listbox'>
              {results.map((result, index) => (
                <button
                  className={
                    index === activeIndex ? 'resource-search-result active' : 'resource-search-result'
                  }
                  id={`${listboxId}-${index}`}
                  key={`${result.kind}:${result.id}`}
                  type='button'
                  role='option'
                  aria-selected={index === activeIndex}
                  onMouseEnter={() => setActiveIndex(index)}
                  onClick={() => selectResult(result)}
                >
                  <span className='resource-search-kind'>{label(result.kind)}</span>
                  <span className='resource-search-copy'>
                    <strong>{result.title}</strong>
                    <small>{result.description}</small>
                  </span>
                  {result.state ? <span className='resource-search-state'>{label(result.state)}</span> : null}
                  <ArrowUpRight size={15} aria-hidden='true' />
                </button>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </form>
  );
}

function messageFrom(cause: unknown): string {
  return cause instanceof Error ? cause.message : 'Authorized search is unavailable.';
}
