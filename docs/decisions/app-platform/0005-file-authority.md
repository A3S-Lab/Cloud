# 0005: Separate File Lifecycles

Status: Accepted

## Context

Published application uploads, immutable Agent capability files, Knowledge
sources, build artifacts, and runtime working files have different ownership,
retention, mutability, and authorization semantics.

## Decision

Files owns user upload sessions, metadata, scan state, quota, retention, and
typed references. Immutable bytes use the shared immutable-object client and an
admitted `S0` provider. Assets owns immutable release files. Knowledge owns
document and chunk lineage. Build Artifacts remain build outputs. Runtime/AR0
owns ephemeral task working files and exports only typed immutable references.

Secrets are references only; plaintext never enters file metadata, Workflow
ACL, histories, events, or logs.

## Consequences

No application-local blob table, Files-specific object client, Build Artifact
substitute, or persisted sandbox directory is introduced. Cross-context use is
by immutable typed reference and owning command, not table writes.
