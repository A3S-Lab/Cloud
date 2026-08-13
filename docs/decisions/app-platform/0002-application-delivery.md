# 0002: One Application Delivery Path

Status: Accepted

## Context

Chatbot, Text Generator, classic Agent, New Agent, Chatflow, and Workflow have
different authoring experiences but share identity, authorization, execution,
session, publication, and observability requirements.

## Decision

Applications owns `Application`, immutable `ApplicationRelease`, end-user,
session, ordered message, conversation-variable, feedback, annotation, and
publication policy. Every `ApplicationRelease` binds exactly one immutable
`WorkflowRevision`. Preset compilers and user-authored modes both produce that
same target.

Web, blocking API, streaming API, embed, internal invocation, and hosted MCP
resolve the same release, authorization policy, schemas, session semantics,
Workflow revision, cursor stream, limits, and audit identity. Gateway owns live
routing; the bounded delivery role calls the same Applications commands and
queries as maintained management interfaces.

## Consequences

There are no mode-specific controllers, session stores, execution engines, or
channel-specific business rules. New Agent reuses an exact Assets/Agents release
and the AR0 execution path rather than introducing an Applications sandbox.
