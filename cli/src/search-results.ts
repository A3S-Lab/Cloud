import type { SearchResult } from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

export function searchResultsResult(rows: SearchResult[]): CommandResult {
  return {
    json: rows,
    table: renderTable(rows, [
      { header: 'KIND', value: (row) => row.kind },
      { header: 'TITLE', value: (row) => row.title },
      { header: 'STATE', value: (row) => row.state },
      { header: 'DESCRIPTION', value: (row) => row.description },
      { header: 'ID', value: (row) => row.id },
      { header: 'HREF', value: (row) => row.href },
      { header: 'UPDATED AT', value: (row) => row.updatedAt },
    ]),
  };
}
