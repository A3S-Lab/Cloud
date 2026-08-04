import type { WorkloadLogStreamFilter } from './types';
import { encodeQueryParameters, encodeSequenceQuery } from './sequence-query';

export interface CloudLogQuery {
  cursor?: string;
  limit?: number;
  stream?: WorkloadLogStreamFilter;
}

export function encodeLogQuery(query: CloudLogQuery): string {
  const parameters = encodeSequenceQuery(query, 'log', 256);
  if (query.stream !== undefined) {
    if (query.stream !== 'stdout' && query.stream !== 'stderr') {
      throw new TypeError('log stream must be stdout or stderr');
    }
    parameters.set('stream', query.stream);
  }
  return encodeQueryParameters(parameters);
}
