# 0101: Publish preset authoring profiles through C4 wrapper evidence

Status: Accepted

## Context

`APP0.2-C4` already compiles Chatbot, Text Generator, classic Agent, and New
Agent experiences into deterministic three-step Workflow wrappers over one exact
`ModelRevision` or `AgentRelease`. Decision 0034 still left Applications-owned
authoring-profile publication open: callers had no closed product profile that
bound experience, audience, delivery policy, presentation digest, and the exact
capability target before asking C4 to publish wrapper evidence and before an
`ApplicationReleaseContract` could be assembled.

Inventing owning-context Model/Agent execution ports, a second Workflow writer,
or management REST for this slice would reopen the C4 boundary. Identity-issued
credentials, file/citation toolkit pins, regeneration, Gateway, and SSE remain
later APP0.2 / APP0.3 / APP0.4 work.

## Decision

Applications owns one immutable `ApplicationAuthoringProfile` for the four
preset experiences only. Chatflow and Workflow fail closed. Chatbot and Text
Generator require one exact `ModelRevision` target; classic Agent and New Agent
require one exact `AgentRelease` target. Audience, delivery policy, and a
canonical presentation digest travel with the profile. Identity is deterministic
UUIDv5 material over organization, project, application, positive release
number, experience, audience, delivery, target capability evidence, and
presentation digest.

One project-authorized internal command publishes that profile by:

1. validating and materializing the profile
2. calling the existing C4 `IApplicationPresetWorkflowPort` as the sole wrapper
   publication path
3. assembling one canonical `ApplicationReleaseContract` from the profile policy
   plus the returned `ApplicationWorkflowBinding`

The command returns profile, C4 evidence/result, and the release contract. It
does not write Application aggregates, mint Identity credentials, open sessions,
or expose REST/OpenAPI/client/CLI/MCP.

## Consequences

- Preset authoring publication reuses C4 identities and fails closed on capability
  drift under the same release slot exactly as C4.
- APP0.1 `PublishApplicationRelease` remains the Application head writer; C29 only
  produces the contract ACL/evidence callers may later submit.
- Identity issuance, file/citation, blocking/streaming parity beyond the profile's
  admitted response modes, regeneration, Gateway, anonymous public routes, and
  management delivery stay later numbered slices.
