# Contributing

Thanks for helping build A3S Workflow. Open an issue before a large protocol,
storage, or user-interface change so the invariants in [AGENTS.md](AGENTS.md)
remain explicit.

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

cd web
bun install --frozen-lockfile
bun run check
bun test
bun run build
```

End-to-end changes must also pass the official A3S Test manifest:

```bash
scripts/e2e.sh
```

Keep pull requests focused, include tests for observable behavior, and call
out changes to the node protocol, PostgreSQL schema, Runtime placement, or
security policy.
