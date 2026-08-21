# 0034: Compile Application presets through Workflow publication authority

Status: Accepted

## Context

Decision 0026 requires every Application release, including Chatbot, Text
Generator, classic Agent, and New Agent, to bind one ordinary immutable
Workflow revision. Decision 0033 composes an already published release into an
ordinary WorkflowRun, but Applications still had no deterministic path for
creating the wrapper Workflow used by preset experiences.

Letting Applications write Workflow tables would create a second graph and
revision authority. Calling the public Workflow controller from inside the
process would duplicate authorization and transport rules. Random wrapper
identities would also make recovery ambiguous if the process stopped after
Workflow committed but before an Application release recorded the binding.

Chatflow and Workflow are user-authored modes and must not be silently reduced
to a preset. The preset compiler also cannot invent mutable provider, prompt,
model, Tool, Agent, Secret, or sandbox configuration. Those inputs must first
become an exact immutable capability in their owning context.

## Decision

Workflow exposes one internal definition-publication application port. The
port accepts only canonical Workflow ACL strings plus caller-selected strong
definition and revision identities. It checks that the Project exists before
parsing the ACLs, reparses every definition, payload, descriptor binding,
descriptor registry, and variable contract through the existing Workflow
domain, and then creates the aggregate, immutable initial revision,
idempotency record, audit metadata, and Outbox event through the existing
Workflow repository. The public create command and preset compiler both use
this port; neither writes Workflow storage directly.

Applications defines one project-authorized internal preset command and a
typed target union. Chatbot and Text Generator require one exact
`ModelRevision`; classic Agent and New Agent require one exact
`AgentRelease`. Chatflow and Workflow fail closed because they require an exact
user-authored Workflow revision.

For each Organization, Project, Application, and positive Application release
number, Applications derives a stable Workflow definition UUIDv5 from the
versioned `cloud.application.preset-workflow.v1` identity. The initial revision
UUIDv5 is derived from that definition. A retry therefore selects the same
identities even after process restart. Reusing the release slot with changed
capability evidence conflicts both under the original idempotency key and
under a different key.

The compiler emits one canonical three-step graph:

```text
Workflow Input -> exact Model or Agent capability -> Workflow Output
```

It also emits the exact configuration and object-schema payloads, a complete
descriptor snapshot and bindings, and direct immutable variable reads from
invocation input to the target and from target output to final output. Model
steps use the Inference-owned `model.llm` profile. Agent execution uses the
Agents-owned application port while its exact release identity remains owned
by Assets; classic and New Agent retain distinct `agent.classic` and
`agent.release` profiles. All generated material is canonical A3S ACL.

The compiler returns only the existing Applications-owned immutable Workflow
evidence and verifies that replayed Workflow content exactly matches its
deterministic compilation. It adds no public route, table, queue, provider
call, Flow command, or mode-specific runner.

## Consequences

- Preset wrapper identities, ACL bytes, payloads, semantic contracts, and
  release evidence are stable across retries and independent process state.
- Workflow remains the sole definition/revision publication and persistence
  authority; Applications owns only the preset request and resulting exact
  binding evidence.
- Chatbot/Text Generator/classic Agent authoring-profile publication and all
  owning-context execution ports remain gated by their later Inference,
  Assets, Agents, Use, and `W0.4` dependencies.
- `APP0.2-C4` is component-only. Public session/invocation, cancellation,
  replay, streaming/blocking delivery, remaining message/file/feedback state,
  and recovery evidence are still required before `APP0.2` is available.
