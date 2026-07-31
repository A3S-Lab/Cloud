## What changed

Describe the control-plane, Runtime, Studio, or documentation change.

## Invariants

- [ ] Every executable workflow node still crosses the A3S Runtime boundary.
- [ ] PostgreSQL remains the authoritative workflow and queue store.
- [ ] Runtime policies and unsupported capabilities fail closed.
- [ ] Stateless-node provider/pool placement remains independently scalable.

## Verification

- [ ] `cargo test --workspace --all-targets`
- [ ] Bun type check, test, and production build
- [ ] `a3s-test` end-to-end manifest (for observable changes)
