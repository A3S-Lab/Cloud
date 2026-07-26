# A3S Cloud TypeScript Client

`@a3s/cloud-client` is the single typed REST client shared by the A3S Cloud web
console and CLI. It contains transport and public response types only; business
rules remain in Cloud application commands and queries.

```typescript
import { CloudApi } from '@a3s/cloud-client';

const api = new CloudApi(process.env.A3S_CLOUD_TOKEN!,
  'https://cloud.example.test/api/v1');
const organizations = await api.listOrganizations();
```

Every request has a finite timeout and expects the standard Cloud success or
error envelope. Invalid JSON, invalid envelopes, network failure, timeout, and
caller cancellation become stable `CloudApiError` values. Tokens are sent only
in authorization headers and never appear in generated stream URLs or error
messages.

The package currently exposes the Web management calls plus `C0.1` tenant,
operational-resource, evidence, and bounded paged-log queries. Its Workload,
deployment, and route types match the current replica/member and Gateway scope
REST projections. It is internal and versioned with Cloud until public package
compatibility and deprecation policy are completed.

Mutating methods require a caller-owned idempotency key. The client accepts a
portable visible-ASCII subset up to the server's 255-byte limit, rejects an
invalid key before transport, and sends the value only in `Idempotency-Key`.
