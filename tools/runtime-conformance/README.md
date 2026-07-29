# A3S Runtime Revision

`runtime-revision` pins the provider-neutral A3S Runtime contract consumed by
Cloud, A3S Box, and cross-repository recovery gates. It must match the exact
`a3s-runtime` Git revision in the workspace manifest.

The concrete provider conformance suite lives in A3S Box and is invoked by
[`../box-conformance/README.md`](../box-conformance/README.md). Cloud keeps no
second provider implementation, lifecycle fixture, or daemon-specific gate.

Cloud consumer tests continue to exercise the shared Runtime command journal,
replay, recovery, and reconciliation contracts without selecting a provider.
