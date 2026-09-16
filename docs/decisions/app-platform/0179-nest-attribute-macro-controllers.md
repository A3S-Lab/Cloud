# 0179. Prefer Nest-style attribute macros for Boot controllers

- Status: Accepted
- Date: 2026-09-15
- Gate: platform-control-plane

## Context

Cloud control-plane registered every HTTP controller through fluent
`ControllerDefinition` builders. `a3s-boot` already ships Nest-style attribute
macros (`#[controller]`, `#[get]`, `#[metadata]`, ?? behind the `macros`
feature, but the Cloud workspace did not enable that feature and had zero
macro-authored controllers.

Builder-only style remains valid, but the product direction is Nest-first:
attribute macros are the preferred presentation surface for new and thin
controllers. Leaving the thinnest public module (`platform`) on builders
blocks that preference without a first-principles reason.

## Decision

1. **Enable** workspace `a3s-boot` feature `macros`.
2. **Convert** `PlatformModule` `/platform` GET to Nest attribute macros with
   public metadata `auth.public = true`, registered via
   `Arc::new(...).controller()?`.
3. **Prefer** Nest macros for new controllers and for thin existing ones when
   touched. Keep fluent builders where a conversion would invent behavior or
   obscure a large existing surface without a claim-path need.
4. **Refuse** overfitting: do not mass-rewrite every controller in one PR;
   convert by need and thinness.

## Consequences

- Establishes an in-tree Nest-macro precedent and compile-time gate.
- Does not declare production release complete.
- Broader Nest migration remains incremental.

## Evidence

- This ADR; `Cargo.toml` `macros` feature; `modules/platform.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_platform_controller`
