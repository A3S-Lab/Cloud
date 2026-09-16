# 0163. Proven APP0.2 toolkit production claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C57

## Context

Gate `APP0.2` remains `in_progress` while several authoring-toolkit capabilities
stay `unavailable` in the parity manifest even though their Verified slices
already ship Applications-owned CQRS and management REST under `/organizations`:

- `toolkit.annotation-reply` ? APP0.2-C21..C24 / ADR `0093`-`0096`
- `toolkit.more-like-this` ? APP0.2-C25..C28 / ADR `0097`-`0100`
- `toolkit.file-input` ? APP0.2-C31..C34 / ADR `0075`, `0103`-`0106`
- `toolkit.citations` ? APP0.2-C35..C38 / ADR `0107`-`0110`

Leaving proven surfaces `unavailable` blocks honest APP0.2 production progress.
Inventing `toolkit.opener` / `toolkit.follow-up` domain models, or flipping
`application.*` mode capabilities that APP0.4 owns to complete, would be
overfit or scope substitution.

## Decision

1. **Production claim path** for the four proven toolkit capabilities is the
   Applications management traffic owner: session-scoped create/get/list routes
   under `/organizations/.../sessions/{session_id}/` for `feedbacks`,
   `annotations`, `message-variants`, `message-file-references`, and
   `message-citations`.

2. **Parity availability:** mark those four capabilities `internal` under gate
   `APP0.2` with ADR/test/implementation evidence. Keep `toolkit.opener` and
   `toolkit.follow-up` `unavailable` until first-principles domain slices exist.
   Keep `application.chatbot` / `text-generator` / `chatflow` / `workflow` (and
   sibling modes) `unavailable` for APP0.4 behavior completion.

3. **Gate `APP0.2`:** remain `in_progress` after this slice. Closing the gate
   still requires opener/follow-up and/or an accepted deferral plus remaining
   streaming/delivery epic work?not a paperwork-only Verified stamp.

4. **Focused conformance:** add a Cloud lib test that freezes management
   controllers expose the five route fragments above under `/organizations`.

## Consequences

- Restores truthfulness between Verified APP0.2 toolkit slices and parity.
- Does not declare APP0.2 production-complete or invent missing toolkit domains.
- Application-mode experiences stay APP0.4-gated work.

## Evidence

- This ADR; plan row `APP0.2-C57`; architecture + parity updates
- Prior delivery ADRs listed above; focused
  `cargo test -p a3s-cloud-control-plane --lib toolkit_claim_path`

