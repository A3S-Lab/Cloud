# AI Application Platform Decisions

These accepted decisions define the authority boundaries used by the versioned
AI application platform parity manifest.

| Decision | Outcome |
| --- | --- |
| [0001](0001-flow-preservation.md) | 0001: Preserve A3S Flow | Accepted |
| [0002](0002-application-delivery.md) | 0002: One Application Delivery Path | Accepted |
| [0003](0003-step-descriptor-registry.md) | 0003: Immutable Semantic Step Descriptors | Accepted |
| [0004](0004-trigger-authority.md) | 0004: Automations Own New Invocations | Accepted |
| [0005](0005-file-authority.md) | 0005: Separate File Lifecycles | Accepted |
| [0006](0006-knowledge-authority.md) | 0006: Knowledge Owns Corpus Semantics | Accepted |
| [0007](0007-typed-variable-scopes.md) | 0007: Workflow-owned typed variable scopes | Accepted |
| [0008](0008-revision-bound-plan-v2.md) | 0008: Revision-bound semantic contracts and Plan v2 | Accepted |
| [0009](0009-workflow-node-catalog-projection.md) | 0009: Read-only Built-in Workflow Node Catalog Projection | Accepted |
| [0010](0010-flow-derived-variable-inspection.md) | 0010: Flow-derived Workflow Variable Inspection | Accepted |
| [0011](0011-digest-bound-variable-defaults.md) | 0011: Bind Workflow defaults as immutable revision material | Accepted |
| [0012](0012-revision-bound-composite-region-policies.md) | 0012: Bind composite-region policy to WorkflowRevision | Accepted |
| [0013](0013-single-flow-dag-compiler.md) | 0013: Reuse Flow as the sole portable DAG structural compiler | Accepted |
| [0014](0014-exact-flow-runtime-registry.md) | 0014: Route Flow work through one exact runtime registry | Accepted |
| [0015](0015-versioned-flow-runtime-builds.md) | 0015: Version every deployed Flow runtime generation | Accepted |
| [0016](0016-deterministic-composite-frames.md) | 0016: Reduce composite child results through deterministic frames | Accepted |
| [0017](0017-ordered-composite-region-results.md) | 0017: Reconstruct composite regions by stable ordinal | Accepted |
| [0018](0018-authority-bound-composite-child-workflow-runs.md) | 0018: Coordinate composite frames through authority-bound WorkflowRuns | Accepted |
| [0019](0019-descriptor-bound-execution-failure-routes.md) | 0019: Route typed Execution failures through descriptor-bound DAG edges | Accepted |
| [0020](0020-flow-owned-connector-attempt-waits.md) | 0020: Keep Connector attempt and wait decisions in Flow | Accepted |
| [0021](0021-connector-immutable-response-objects.md) | 0021: Compose Connector responses through immutable objects | Accepted |
| [0022](0022-terminal-evidence-authorized-connector-response-reads.md) | 0022: Authorize Connector response reads through terminal evidence | Accepted |
| [0023](0023-descriptor-bound-default-output-fallback.md) | 0023: Fold terminal Execution failure into an exact default output | Accepted |
| [0024](0024-schema-bound-connector-json-response-projection.md) | 0024: Project Connector JSON through one typed Flow step | Accepted |
| [0025](0025-descriptor-bound-connector-failure-routes.md) | 0025: Route Connector failures through descriptor-bound DAG edges | Accepted |
| [0026](0026-single-application-release-authority.md) | 0026: Keep one immutable Application release authority | Accepted |
| [0027](0027-atomic-application-release-persistence.md) | 0027: Persist Application releases atomically | Accepted |
| [0028](0028-authorized-application-release-cqrs.md) | 0028: Authorize Application release CQRS before replay | Accepted |
| [0029](0029-single-application-management-interface.md) | 0029: Expose one Application management authority | Accepted |
| [0030](0030-canonical-user-file-admission.md) | 0030: Admit user files through one canonical lifecycle | Accepted |
| [0031](0031-single-application-session-authority.md) | 0031: Keep one Application session and semantic-effect authority | Accepted |
| [0032](0032-atomic-application-session-persistence.md) | 0032: Persist Application session semantics atomically | Accepted |
| [0033](0033-typed-application-workflow-run-composition.md) | 0033: Compose Application invocations into ordinary WorkflowRuns | Accepted |
| [0034](0034-deterministic-application-preset-workflows.md) | 0034: Compile Application presets through Workflow publication authority | Accepted |
| [0035](0035-durable-application-invocation-execution-authority.md) | 0035: Persist Application invocation execution authority before composition | Accepted |
| [0036](0036-authorized-application-delivery-cqrs.md) | 0036: Authorize Application delivery before replay and compose through existing owners | Accepted |
| [0037](0037-workflow-application-semantic-effect-port.md) | 0037: Apply Workflow semantic effects through the single Applications authority | Accepted |
| [0038](0038-project-member-application-delivery-admission.md) | 0038: Admit project-member Application invocations through one owned path | Accepted |
| [0039](0039-versioned-application-workflow-lifecycle-projection.md) | 0039: Project Application WorkflowRun lifecycle through a versioned reconciliation contract | Accepted |
| [0040](0040-descriptor-bound-application-answer-effects.md) | 0040: Dispatch descriptor-bound Answer effects before resuming Workflow | Accepted |
| [0041](0041-descriptor-bound-application-variable-effects.md) | 0041: Dispatch descriptor-bound Application variable effects through snapshot and CAS hooks | Accepted |
| [0042](0042-project-member-application-lifecycle-replay.md) | 0042: Expose project-member Application lifecycle and replay through C6 | Accepted |
| [0043](0043-root-bound-repeated-application-answer-frames.md) | 0043: Bind repeated Application Answer frames to one root invocation | Accepted |
| [0044](0044-replay-pinned-bounded-flow-step-retries.md) | 0044: Pin bounded infrastructure step retries to new Flow histories | Accepted |
| [0045](0045-descriptor-bound-application-variable-failure-routes.md) | 0045: Route Application variable failures through descriptor-bound DAG edges | Accepted |
| [0046](0046-descriptor-bound-application-answer-failure-routes.md) | 0046: Route Application Answer failures through descriptor-bound DAG edges | Accepted |
| [0047](0047-descriptor-bound-transform-failure-routes.md) | 0047: Route Workflow-local Transform failures through descriptor-bound DAG edges | Accepted |
| [0048](0048-descriptor-bound-output-failure-routes.md) | 0048: Route Workflow-local Output failures through descriptor-bound DAG edges | Accepted |
| [0049](0049-descriptor-bound-branch-failure-routes.md) | 0049: Route Workflow-local Branch failures through descriptor-bound DAG edges | Accepted |
| [0050](0050-descriptor-bound-composite-failure-routes.md) | 0050: Route composite-region failures through descriptor-bound DAG edges | Accepted |
| [0051](0051-workflow-local-variable-aggregation.md) | 0051: Aggregate branch variables through one typed Workflow-local step | Accepted |
| [0052](0052-workflow-local-list-operations.md) | 0052: Process typed lists through one Workflow-local step | Accepted |
| [0053](0053-bounded-parallel-iteration-waves.md) | 0053: Execute bounded Iteration frames in authority-bound waves | Accepted |
| [0054](0054-internal-connector-workflow-capability.md) | 0054: Advertise the exact Connector Workflow step as internal | Accepted |
| [0055](0055-ordinary-durable-connector-compensation.md) | 0055: Compose Connector compensation from ordinary durable steps | Accepted |
| [0056](0056-terminal-connector-wait-projection.md) | 0056: Project terminated Connector waits from the terminal Flow event | Accepted |
| [0057](0057-exact-connector-revision-revocation.md) | 0057: Serialize exact Connector revision revocation with dispatch admission | Accepted |
| [0058](0058-terminal-connector-attempt-resolution.md) | 0058: Close expired Connector dispatches with an exact indeterminate resolution | Accepted |
| [0059](0059-single-sources-pull-request-fact-boundary.md) | 0059: Publish pull-request changes through the single Sources delivery boundary | Accepted |
| [0060](0060-exact-composite-profile-and-evidence-authority.md) | 0060: Bind composite profiles and evidence to exact runtime authority | Accepted |
| [0061](0061-single-developer-preview-projection-authority.md) | 0061: Project pull-request Preview lifecycle through one consumer authority | Accepted |
| [0062](0062-exact-finite-execution-dispatch-authority.md) | 0062: Admit only the exact finite Execution profile | Accepted |
| [0063](0063-single-preview-environment-owner-handoff.md) | 0063: Preview Environment handoff through the existing owner authorities | Accepted |
| [0064](0064-flow-owned-connector-cancellation-compensation.md) | 0064: Compensate accepted Connector effects during Flow cancellation | Accepted |
| [0065](0065-single-accepted-workload-profile-production-compilation.md) | 0065: Compose one exact accepted workload-profile compilation path | Accepted |
| [0066](0066-single-build-plan-detection-production-composition.md) | 0066: Compose one closed BuildPlan detection path | Accepted |
| [0067](0067-single-owner-authorized-build-plan-acceptance-composition.md) | 0067: Compose one owner-authorized BuildPlan acceptance path | Accepted |
| [0068](0068-single-owner-authorized-workload-profile-acceptance-composition.md) | 0068: Compose one owner-authorized workload-profile acceptance path | Accepted |
| [0069](0069-single-owner-authorized-preview-policy-acceptance-composition.md) | 0069: Compose one owner-authorized Preview Policy acceptance path | Accepted |
| [0070](0070-single-authorized-source-layout-acquisition.md) | 0070: Acquire one authorized trusted BuildPlan source layout | Accepted |
| [0071](0071-single-build-plan-management-interface.md) | 0071: Expose one BuildPlan management interface | Accepted |
| [0072](0072-single-workload-profile-management-interface.md) | 0072: Expose one WorkloadProfile revision management interface | Accepted |
| [0073](0073-single-preview-management-interface.md) | 0073: Expose one pull-request Preview management interface | Accepted |
| [0074](0074-single-transient-source-discovery-authority.md) | 0074: Discover GitHub source state through one transient Sources authority | Accepted |
| [0075](0075-single-user-file-lifecycle-authority.md) | 0075: Persist and expose UserFile through one lifecycle authority | Accepted |
| [0076](0076-single-agent-flow-function-runtime-authority.md) | 0076: Compose Agent, Workflow, and Function runtimes from one authority per concern | Accepted |
| [0077](0077-single-elastic-workload-authority.md) | 0077: Use one elastic Workload authority with explicit state-safety semantics | Accepted |
| [0078](0078-separate-git-oci-use-registry-authorities.md) | 0078: Keep Git, OCI, A3S Use Registry, and model supply as separate authorities | Accepted |
| [0079](0079-identity-owned-workload-trust-contract.md) | 0079: Keep workload trust in Identity and execution evidence with existing owners | Accepted |
| [0080](0080-identity-owned-platform-scope-and-rbac.md) | 0080: Keep installation scope and platform RBAC in one Identity authority | Accepted |
| [0081](0081-one-privileged-authorization-evidence-model.md) | 0081: Use one privileged authorization evidence model | Accepted |
| [0082](0082-one-installation-scoped-fact-rail.md) | 0082: Use one Installation-scoped fact rail | Accepted |
| [0083](0083-one-platform-rbac-persistence-authority.md) | 0083: Serialize platform RBAC through one Identity persistence authority | Accepted |
| [0084](0084-one-tenant-support-approval-authority.md) | 0084: Persist one actual tenant-support approval authority | Accepted |
| [0085](0085-one-atomic-privileged-authorization-authority.md) | 0085: Issue privileged allows through one atomic Identity authority | Accepted |
| [0086](0086-one-atomic-identity-bootstrap-authority.md) | 0086: Bootstrap tenant identity and platform authority atomically | Accepted |
| [0087](0087-one-workload-runtime-evidence-authority.md) | 0087: One workload Runtime evidence authority | Accepted |
| [0088](0088-application-delivery-credential-binding.md) | 0088. Applications-owned anonymous delivery credential binding | Accepted |
| [0089](0089-application-delivery-credential-persistence.md) | 0089. Persist Applications-owned anonymous delivery credentials | Accepted |
| [0090](0090-anonymous-delivery-session-admission.md) | 0090. Anonymous delivery session admission | Accepted |
| [0091](0091-anonymous-delivery-invocation-admission.md) | 0091. Anonymous delivery invocation admission | Accepted |
| [0092](0092-application-delivery-credential-lifecycle.md) | 0092. Application delivery credential lifecycle CQRS | Accepted |
| [0093](0093-application-feedback-annotation-authority.md) | 0093. Application feedback and annotation authority | Accepted |
| [0094](0094-application-feedback-annotation-persistence.md) | 0094. Persist Applications-owned feedback and annotations | Accepted |
| [0095](0095-application-feedback-annotation-cqrs.md) | 0095. Project-authorized feedback and annotation CQRS | Accepted |
| [0096](0096-application-feedback-annotation-delivery.md) | 0096. Management feedback and annotation delivery | Accepted |
| [0097](0097-application-message-variant-authority.md) | 0097. Application message-variant authority | Accepted |
| [0098](0098-application-message-variant-persistence.md) | 0098. Persist Applications-owned message variants | Accepted |
| [0099](0099-application-message-variant-cqrs.md) | 0099. Project-authorized message-variant CQRS | Accepted |
| [0100](0100-application-message-variant-delivery.md) | 0100. Management message-variant delivery | Accepted |
| [0101](0101-application-authoring-profile-publication.md) | 0101: Publish preset authoring profiles through C4 wrapper evidence | Accepted |
| [0102](0102-application-delivery-credential-issuance.md) | 0102. Application delivery credential Identity/Secrets issuance | Accepted |
| [0103](0103-application-message-file-reference-authority.md) | 0103. Application message file-reference authority | Accepted |
| [0104](0104-application-message-file-reference-persistence.md) | 0104. Persist Applications-owned message file references | Accepted |
| [0105](0105-application-message-file-reference-cqrs.md) | 0105. Project-authorized message file-reference CQRS | Accepted |
| [0106](0106-application-message-file-reference-delivery.md) | 0106. Message file-reference management delivery | Accepted |
| [0107](0107-application-message-citation-authority.md) | 0107. Application message citation authority | Accepted |
| [0108](0108-application-message-citation-persistence.md) | 0108. Persist Applications-owned message citations | Accepted |
| [0109](0109-application-message-citation-cqrs.md) | 0109. Project-authorized message citation CQRS | Accepted |
| [0110](0110-application-message-citation-delivery.md) | 0110. Message citation management delivery | Accepted |
| [0111](0111-application-blocking-invocation-observation.md) | 0111. Application blocking invocation observation | Accepted |
| [0112](0112-application-blocking-observation-cqrs.md) | 0112. Project-authorized blocking observation CQRS | Accepted |
| [0113](0113-application-blocking-observation-delivery.md) | 0113. Application blocking observation delivery | Accepted |
| [0114](0114-application-streaming-observation.md) | 0114. Application streaming invocation observation | Accepted |
| [0115](0115-application-streaming-observation-cqrs.md) | 0115. Project-authorized streaming observation CQRS | Accepted |
| [0116](0116-application-streaming-observation-delivery.md) | 0116. Application streaming observation delivery | Accepted |
| [0117](0117-application-asynchronous-observation.md) | 0117. Application asynchronous invocation observation | Accepted |
| [0118](0118-application-asynchronous-observation-cqrs.md) | 0118. Project-authorized asynchronous observation CQRS | Accepted |
| [0119](0119-application-asynchronous-observation-delivery.md) | 0119. Application asynchronous observation delivery | Accepted |
| [0121](0121-application-anonymous-blocking-observation-cqrs.md) | 0121. Anonymous-credential blocking observation CQRS | Accepted |
| [0122](0122-application-anonymous-streaming-observation-cqrs.md) | 0122. Anonymous-credential streaming observation CQRS | Accepted |
| [0123](0123-application-anonymous-asynchronous-observation-cqrs.md) | 0123. Anonymous-credential asynchronous observation CQRS | Accepted |
| [0124](0124-application-delivery-credential-read-cqrs.md) | 0124. Application delivery credential get/list CQRS | Accepted |
| [0125](0125-application-delivery-credential-management-delivery.md) | 0125. Application delivery credential management delivery | Accepted |
| [0126](0126-application-anonymous-delivery-admission.md) | 0126. Application anonymous delivery admission | Accepted |
| [0127](0127-application-anonymous-observation-delivery.md) | 0127-application-anonymous-observation-delivery.md | Accepted |
| [0128](0128-application-anonymous-lifecycle-cqrs.md) | 0128. Application anonymous session close and invocation cancel CQRS | Accepted |
| [0129](0129-application-anonymous-lifecycle-delivery.md) | 0129. Application anonymous session close and invocation cancel delivery | Accepted |
| [0130](0130-application-resource-grant-scope.md) | 0130. Exact Application Resource Grant scope for delivery (Rule 8) | Accepted |
| [0131](0131-process-role-delivery.md) | 0131. ProcessRole::Delivery capability boundary | Accepted |
| [0132](0132-process-role-delivery-http.md) | 0132. Delivery process public anonymous delivery HTTP | Accepted |
| [0133](0133-application-invoke-api-token-scope.md) | 0133. Admit Identity `application:invoke` on Principal-bound API tokens | Accepted |
| [0134](0134-delivery-authenticated-http.md) | 0134. Delivery process authenticated published-application HTTP | Accepted |
| [0134](0134-process-role-delivery-authenticated-http.md) | 0134. Process role Delivery authenticated published-application HTTP | Accepted |
| [0135](0135-process-role-delivery-authenticated-observation.md) | 0135. Process role Delivery authenticated observation polls | Accepted |
| [0136](0136-delivery-authenticated-openapi.md) | 0136. Authenticated `/delivery` OpenAPI contract | Accepted |
| [0136](0136-process-role-delivery-authenticated-openapi.md) | 0136. Authenticated `/delivery` OpenAPI contract | Accepted |
| [0137](0137-delivery-process-drain.md) | 0137. Delivery process drain | Accepted |
| [0137](0137-process-role-delivery-drain.md) | 0137. Delivery process drain and readiness admission gate | Accepted |
| [0138](0138-delivery-authenticated-client.md) | 0138. Authenticated Delivery client and CLI | Accepted |
| [0138](0138-process-role-delivery-authenticated-client.md) | 0138. Authenticated Delivery client and CLI | Accepted |
| [0139](0139-application-publication-route-intent.md) | 0139. Application publication route intent | Accepted |
| [0139](0139-publication-route-intent.md) | 0139. Application publication route intent | Accepted |
| [0140](0140-application-publication-route-intent-persistence.md) | 0140. Application publication route intent persistence | Accepted |
| [0140](0140-publication-route-intent-persistence.md) | 0140. Application publication route intent persistence | Accepted |
| [0141](0141-application-publication-route-intent-cqrs.md) | 0141. Application publication route intent CQRS | Accepted |
| [0142](0142-application-publication-route-intent-rest.md) | 0142. Application publication route intent REST | Accepted |
| [0142](0142-publication-route-intent-rest.md) | 0142. Application publication route intent REST | Accepted |
| [0143](0143-application-publication-route-intent-client-cli.md) | 0143. Application publication route intent client and CLI | Accepted |
| [0143](0143-publication-route-intent-client-cli.md) | 0143. Application publication route intent client and CLI | Accepted |
| [0144](0144-application-publication-route-intent-edge-projection.md) | 0144. Application publication route intent Edge projection | Accepted |
| [0144](0144-publication-route-intent-edge-projection.md) | 0144. Application publication route intent Edge projection | Accepted |
| [0145](0145-application-publication-route-intent-gateway-snapshot-compile.md) | 0145. Application publication route intent Gateway snapshot compile | Accepted |
| [0145](0145-publication-route-intent-gateway-snapshot-compile.md) | 0145. Application publication route intent Gateway snapshot compile | Accepted |
| [0146](0146-application-publication-route-intent-desired-state-load.md) | 0146. Application publication route intent desired-state load | Accepted |
| [0147](0147-mcp-gateway-publication-route-intent-acl-load.md) | 0147. MCP Gateway publication route intent ACL load | Accepted |
| [0148](0148-publication-route-intent-empty-compile-site-audit.md) | 0148. Publication route intent empty compile-site audit | Accepted |
| [0149](0149-gateway-rate-shaping-profile-compile-binding.md) | 0149. Gateway rate-shaping profile compile binding | Accepted |
| [0150](0150-gateway-runtime-publication-rate-shaping-apply.md) | 0150. Gateway runtime apply of publication rate-shaping ACL | Accepted |
| [0151](0151-gateway-rate-shaping-catalog-registration.md) | 0151. Cloud Edge Gateway rate-shaping catalog registration | Accepted |
| [0152](0152-gateway-rate-shaping-config-acl-seed.md) | 0152. Cloud Edge Gateway rate-shaping catalog config ACL seed | Accepted |
| [0153](0153-gateway-exact-release-publication-route-binding.md) | 0153. Gateway exact-release publication route binding table | Accepted |
| [0154](0154-gateway-publication-http-admit.md) | 0154. Gateway HTTP publication admit (resolve + rate catalog) | Accepted |
| [0155](0155-gateway-publication-admit-reject-metadata.md) | 0155. Gateway publication admit reject response metadata | Accepted |
| [0156](0156-gateway-edge-request-headers-before-publication-admit.md) | 0156. Gateway applies Edge `request_headers` before publication admit | Accepted |
| [0157](0157-gateway-rate-shaping-durable-postgres-catalog.md) | 0157. Durable Postgres Gateway rate-shaping profile catalog | Accepted |
| [0158](0158-retained-rollback-publication-route-intent-acl.md) | 0158. Retained/rollback Gateway snapshots retain publication route intent ACL | Accepted |
| [0159](0159-managed-rollback-publication-route-intent-acl.md) | 0159. Managed rollout rollback publications retain publication route intent ACL | Accepted |
| [0160](0160-app03-production-release-audit.md) | 0160. APP0.3 production-release audit (first principles) | Accepted |
| [0161](0161-delivery-traffic-owner-publication-api-claim-path.md) | 0161. Delivery traffic-owner claim path for publication API channels | Accepted |
| [0162](0162-app03-production-release-api-channels-embed-web-deferral.md) | 0162. APP0.3 production release for API channels with embed/web deferral | Accepted |
| [0163](0163-app02-proven-toolkit-claim-path.md) | 0163. Proven APP0.2 toolkit production claim path | Accepted |
| [0164](0164-app02-production-foundation-opener-follow-up-deferral.md) | 0164. APP0.2 production foundation with opener/follow-up deferral | Accepted |
| [0165](0165-app04-proven-application-mode-claim-path.md) | 0165. Proven APP0.4 application-mode claim path (four presets) | Accepted |
| [0166](0166-app04-chatflow-workflow-claim-path.md) | 0166. Proven APP0.4 Chatflow/Workflow claim path (exact Workflow release) | Accepted |
| [0167](0167-app04-production-foundation-toolkit-channel-deferral.md) | 0167. APP0.4 production foundation with toolkit and channel deferral | Accepted |
| [0168](0168-app05-feedback-annotation-review-claim-path.md) | 0168. Proven APP0.5 feedback/annotation review claim path | Accepted |
| [0169](0169-app05-usage-cost-showback-claim-path.md) | 0169. Proven APP0.5 usage/cost showback claim path | Accepted |
| [0170](0170-app05-production-foundation-ops-monitoring-deferral.md) | 0170. APP0.5 production foundation with ops monitoring deferral | Accepted |
| [0171](0171-app06-custom-domain-claim-path.md) | 0171. Proven APP0.6 custom-domain claim path | Accepted |
| [0172](0172-app06-isolation-quota-retention-claim-path.md) | 0172. Proven APP0.6 isolation-quota-retention claim path | Accepted |
| [0173](0173-app06-production-foundation.md) | 0173. APP0.6 production foundation for claimable enterprise | Accepted |
| [0174](0174-c05-oidc-federation-claim-path.md) | 0174. Proven C0.5 OIDC federation claim path | Accepted |
| [0175](0175-c05-audit-security-claim-path.md) | 0175. Proven C0.5 audit-security claim path | Accepted |
| [0176](0176-c05-production-foundation.md) | 0176. C0.5 production foundation for claimable enterprise identity surfaces | Accepted |
| [0177](0177-c03-external-identity-claim-path.md) | 0177. C0.3 external-identity claim path via Resource Grants and Workload Trust | Accepted |
| [0178](0178-c03-production-foundation.md) | 0178. C0.3 production foundation for claimable enterprise identity access surfaces | Accepted |
| [0179](0179-nest-attribute-macro-controllers.md) | 0179. Prefer Nest-style attribute macros for Boot controllers | Accepted |
| [0180](0180-h05-ha-disaster-recovery-claim-path.md) | 0180. Proven H0.5 HA / disaster-recovery claim path | Accepted |
| [0181](0181-h05-production-foundation.md) | 0181. H0.5 production foundation for claimable HA / disaster-recovery | Accepted |
| [0182](0182-nest-organizations-query-controller.md) | 0182. Nest-macro organizations list query controller | Accepted |
| [0183](0183-nest-usage-retention-controller.md) | 0183. Nest-macro inference usage retention query controller | Accepted |
| [0184](0184-nest-gateway-scope-queries-controller.md) | 0184. Nest-macro gateway scope query controller | Accepted |
| [0185](0185-nest-source-revision-queries-controller.md) | 0185. Nest-macro source revision query controller | Accepted |
| [0186](0186-nest-search-controller.md) | 0186. Nest-macro organization search query controller | Accepted |
| [0187](0187-nest-github-repository-subscription-queries-controller.md) | 0187. Nest-macro GitHub repository subscription query controller | Accepted |
| [0188](0188-nest-blocking-observation-controller.md) | 0188. Nest-macro blocking observation query controller | Accepted |
| [0189](0189-nest-asynchronous-observation-controller.md) | 0189. Nest-macro asynchronous observation query controller | Accepted |
| [0190](0190-nest-streaming-observation-controller.md) | 0190. Nest-macro streaming observation query controller | Accepted |
| [0191](0191-nest-node-queries-controller.md) | 0191. Nest-macro fleet node query controller | Accepted |
| [0192](0192-nest-node-pool-queries-controller.md) | 0192. Nest-macro fleet node-pool query controller | Accepted |
| [0193](0193-nest-usage-queries-controller.md) | 0193. Nest-macro inference usage query controller | Accepted |
| [0194](0194-nest-inference-route-queries-controller.md) | 0194. Nest-macro inference route query controller | Accepted |
| [0195](0195-nest-project-queries-controller.md) | 0195. Nest-macro project and environment query controllers | Accepted |
| [0196](0196-nest-authenticated-observation-controller.md) | 0196. Nest-macro authenticated delivery observation controller | Accepted |
| [0197](0197-nest-anonymous-observation-controller.md) | 0197. Nest-macro anonymous delivery observation controller | Accepted |
| [0198](0198-nest-audit-query-controller.md) | 0198. Nest-macro audit query controller | Accepted |
| [0199](0199-nest-plugin-plan-projection-controller.md) | 0199. Nest-macro plugin plan projection controller | Accepted |
| [0200](0200-nest-plugin-registry-queries-controller.md) | 0200. Nest-macro plugin registry queries controller | Accepted |
| [0201](0201-nest-api-token-controller.md) | 0201. Nest-macro API token controller | Accepted |
| [0202](0202-nest-plugin-assignment-controller.md) | 0202. Nest-macro plugin assignment controller | Accepted |
| [0203](0203-nest-workload-profile-controller.md) | 0203. Nest-macro workload profile controller | Accepted |
| [0204](0204-nest-gateway-scope-commands-controller.md) | 0204. Nest-macro gateway scope commands controller | Accepted |
| [0205](0205-nest-plugin-registry-commands-controller.md) | 0205. Nest-macro plugin registry commands controller | Accepted |
| [0206](0206-nest-organization-controller.md) | 0206. Nest-macro organization create controller | Accepted |
| [0207](0207-nest-inference-route-commands-controller.md) | 0207. Nest-macro inference route commands controller | Accepted |
| [0208](0208-nest-recipient-contact-controller.md) | 0208. Nest-macro recipient contact controller | Accepted |
| [0209](0209-nest-routes-controller.md) | 0209. Nest-macro routes publish controller | Accepted |
| [0210](0210-nest-mcp-route-policy-commands-controller.md) | 0210. Nest-macro MCP route policy commands controller | Accepted |
| [0211](0211-nest-source-revisions-controller.md) | 0211. Nest-macro source revisions controller | Accepted |
| [0212](0212-nest-enrollment-controller.md) | 0212. Nest-macro node enrollment controller | Accepted |
| [0213](0213-nest-domain-claim-commands-controller.md) | 0213. Nest-macro domain claim commands controller | Accepted |
| [0214](0214-nest-mcp-credential-commands-controller.md) | 0214. Nest-macro MCP credential commands controller | Accepted |
| [0215](0215-nest-projects-commands-controller.md) | 0215. Nest-macro project and environment command controllers | Accepted |
| [0216](0216-nest-github-repository-subscription-commands-controller.md) | 0216. Nest-macro GitHub repository subscription commands controller | Accepted |
| [0217](0217-nest-node-management-controller.md) | 0217. Nest-macro fleet node management commands controller | Accepted |
| [0218](0218-nest-node-pool-management-controller.md) | 0218. Nest-macro fleet node-pool management commands controller | Accepted |
| [0219](0219-nest-membership-controller.md) | 0219. Nest-macro membership controller | Accepted |
| [0220](0220-nest-resource-grant-controller.md) | 0220. Nest-macro resource grant controller | Accepted |
| [0221](0221-nest-membership-invitation-controller.md) | 0221. Nest-macro membership invitation controllers | Accepted |
| [0222](0222-nest-security-investigation-controller.md) | 0222. Nest-macro security investigation controller | Accepted |
| [0223](0223-nest-automation-webhooks-controller.md) | 0223. Nest-macro automation webhooks controller | Accepted |
| [0224](0224-nest-github-webhooks-controller.md) | 0224. Nest-macro GitHub webhooks controller | Accepted |
| [0225](0225-nest-build-plan-controller.md) | 0225. Nest-macro build plan controllers | Accepted |
| [0226](0226-nest-preview-management-controller.md) | 0226. Nest-macro preview management controllers | Accepted |
| [0227](0227-nest-message-variant-delivery-controller.md) | 0227. Nest-macro application message variant delivery controllers | Accepted |
| [0228](0228-nest-message-citation-delivery-controller.md) | 0228. Nest-macro application message citation delivery controllers | Accepted |
| [0229](0229-nest-message-file-reference-delivery-controller.md) | 0229. Nest-macro application message file-reference delivery controllers | Accepted |
| [0230](0230-nest-feedback-delivery-controller.md) | 0230. Nest-macro application feedback and annotation delivery controllers | Accepted |
| [0231](0231-nest-delivery-credential-controller.md) | 0231. Nest-macro application delivery credential controllers | Accepted |
| [0232](0232-nest-publication-route-intent-controller.md) | 0232. Nest-macro application publication route intent controllers | Accepted |
| [0233](0233-nest-automation-management-controller.md) | 0233. Nest-macro automation management controllers | Accepted |
| [0234](0234-nest-anonymous-delivery-controller.md) | 0234. Nest-macro anonymous application delivery controller | Accepted |
| [0235](0235-nest-authenticated-delivery-controller.md) | 0235. Nest-macro authenticated application delivery controller | Accepted |
| [0236](0236-nest-application-delivery-controller.md) | 0236. Nest-macro organization-scoped application delivery controllers | Accepted |
| [0237](0237-nest-durable-cell-controller.md) | 0237. Nest-macro durable cell controllers | Accepted |
| [0238](0238-nest-connector-controller.md) | 0238. Nest-macro connector controllers | Accepted |
| [0239](0239-nest-application-controller.md) | 0239. Nest-macro application controllers | Accepted |
| [0240](0240-nest-knowledge-controller.md) | 0240. Nest-macro knowledge controllers | Accepted |
| [0241](0241-nest-privileged-management-controller.md) | 0241. Nest-macro privileged management controllers | Accepted |
| [0242](0242-w03-iteration-internal-claim-path.md) | 0242. Advertise Iteration as an internal Workflow node | Accepted |
| [0243](0243-w03-loop-internal-claim-path.md) | 0243. Advertise Loop as an internal Workflow node | Accepted |
| [0244](0244-w03-answer-internal-claim-path.md) | 0244. Advertise Answer as an internal Applications node | Accepted |
| [0245](0245-w03-production-foundation.md) | 0245. W0.3 production foundation for claimable Workflow/Answer nodes | Accepted |
| [0246](0246-aut05-production-foundation.md) | 0246. AUT0.5 production foundation for claimable Connector execution | Accepted |
| [0247](0247-w04-production-foundation.md) | 0247. W0.4 production foundation with foreign-owner node deferral | Accepted |
| [0248](0248-a05-production-foundation.md) | 0248. A0.5 production foundation for Skill bind path without external-provider verification | Accepted |
| [0249](0249-aut02-production-foundation.md) | 0249. AUT0.2 production foundation for Webhook Trigger | Accepted |
| [0250](0250-aut03-production-foundation.md) | 0250. AUT0.3 production foundation for Schedule Trigger | Accepted |
| [0251](0251-production-release-honesty-refusals.md) | 0251. Production-release blockers: refuse inventing AUT0.4, S0, and Knowledge datasource productization | Accepted |
| [0252](0252-a13-production-foundation.md) | 0252. A1.3 production foundation for provider-neutral Agent execution contracts | Accepted |
| [0253](0253-a14-production-foundation.md) | 0253. A1.4 production foundation for Harness invocation profiles | Accepted |
| [0254](0254-i02c-production-foundation.md) | 0254. I0.2c production foundation for usage/cost showback | Accepted |
| [0255](0255-aut04-production-foundation.md) | 0255. AUT0.4 production foundation for normalized event dispatch | Accepted |
| [0256](0256-i02-production-foundation.md) | 0256. I0.2 production foundation for inference route control plane | Accepted |
| [0257](0257-nest-inference-key-controller.md) | 0257. Nest-macro inference key controller | Accepted |
| [0258](0258-nest-edge-route-domain-claim-queries.md) | 0258: Nest-macro Edge route and domain-claim queries | Accepted |
| [0259](0259-nest-secret-queries.md) | 0259: Nest-macro Secrets query list controller | Accepted |
| [0260](0260-nest-edge-mcp-queries.md) | 0260: Nest-macro Edge MCP credential and route-policy queries | Accepted |
| [0261](0261-nest-bootstrap.md) | 0261: Nest-macro Identity bootstrap controller | Accepted |
| [0262](0262-nest-github-connections.md) | 0262: Nest-macro Sources GitHub connections controller | Accepted |
| [0263](0263-nest-github-callbacks.md) | 0263: Nest-macro Sources GitHub connection callbacks | Accepted |
| [0264](0264-nest-oidc.md) | 0264: Nest-macro Identity OIDC controllers | Accepted |
| [0265](0265-nest-ontology-commands.md) | 0265: Nest-macro Workflow ontology create command | Accepted |
| [0266](0266-nest-execution-commands.md) | 0266: Nest-macro Executions create commands | Accepted |
| [0267](0267-nest-secrets-commands.md) | 0267: Nest-macro Secrets create command | Accepted |
| [0268](0268-nest-ontology-queries.md) | 0268: Nest-macro Workflow ontology list query | Accepted |
