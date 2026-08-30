# Workload identity provider conformance

This WI1 gate exercises Cloud's production `spiffe_https_web` adapter against
a deterministic local provider over real TLS. The fixture uses an exact pinned
CA, serves a standards-compatible SPIFFE trust-bundle JWK document, and changes
the observed bundle on the same content-addressed provider profile.

The gate proves HTTPS transport, exact CA digest binding, strict and bounded
bundle parsing, X.509-SVID and JWT-SVID capability observation, exact
TrustDomain revision admission, bundle-drift rejection, and rejection of an
unknown provider profile. It does not claim SPIRE registration, workload
attestation, credential issuance, revocation epochs, or federation; those stay
behind later workload-identity milestones.

Run it from the Cloud repository root:

```bash
bash tools/workload-identity-conformance/run_provider_gate.sh \
  /absolute/evidence/directory
```
