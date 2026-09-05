use super::validation::{
    acl_integer, exact_shape, optional_digest, required_digest, required_string,
    required_string_list, required_u64, required_uuid,
};
use super::*;
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{Block, Document};

pub(super) fn definition_document(spec: &AutomationDefinitionSpecV1) -> Result<Document, String> {
    let authorization = BlockBuilder::new("authorization")
        .attr("policy_digest", string(&spec.authorization.policy_digest))
        .attr(
            "required_grants",
            list(
                spec.authorization
                    .required_grants
                    .iter()
                    .map(|grant| string(grant))
                    .collect(),
            ),
        )
        .build();
    let policy = BlockBuilder::new("policy")
        .attr(
            "concurrency_mode",
            string(spec.policy.concurrency.mode.as_str()),
        )
        .attr(
            "deduplication_scope",
            string(spec.policy.deduplication.scope.as_str()),
        )
        .attr(
            "deduplication_template",
            string(&spec.policy.deduplication.key_template),
        )
        .attr(
            "deduplication_window_ms",
            acl_integer(
                "deduplication_window_ms",
                spec.policy.deduplication.window_ms,
            )?,
        )
        .attr(
            "maximum_concurrency",
            acl_integer("maximum_concurrency", spec.policy.concurrency.maximum)?,
        )
        .attr(
            "misfire_grace_ms",
            acl_integer("misfire_grace_ms", spec.policy.misfire.grace_ms)?,
        )
        .attr("misfire_mode", string(spec.policy.misfire.mode.as_str()))
        .build();
    let root = BlockBuilder::new("automation_definition")
        .attr("automation_id", string(&spec.automation_id.to_string()))
        .attr("environment_id", string(&spec.environment_id.to_string()))
        .attr("name", string(&spec.name))
        .attr("organization_id", string(&spec.organization_id.to_string()))
        .attr("project_id", string(&spec.project_id.to_string()))
        .attr("schema", string(AUTOMATION_DEFINITION_SCHEMA_V1))
        .nested_block(authorization)
        .nested_block(policy)
        .nested_block(target_block(&spec.target))
        .nested_block(trigger_block(&spec.trigger)?);
    // Keep the nested order explicit; the ACL generator performs canonical
    // attribute ordering while preserving this semantic block order.
    let document = Document {
        blocks: vec![root.build()],
    };
    Ok(document)
}

fn target_block(target: &AutomationTargetV1) -> Block {
    match target {
        AutomationTargetV1::ApplicationRelease(target) => BlockBuilder::new("application_release")
            .attr("application_id", string(&target.application_id.to_string()))
            .attr(
                "application_release_id",
                string(&target.application_release_id.to_string()),
            )
            .attr("release_digest", string(&target.release_digest))
            .build(),
        AutomationTargetV1::WorkflowRevision(target) => BlockBuilder::new("workflow_revision")
            .attr(
                "workflow_definition_id",
                string(&target.workflow_definition_id.to_string()),
            )
            .attr(
                "workflow_revision_id",
                string(&target.workflow_revision_id.to_string()),
            )
            .attr("revision_digest", string(&target.revision_digest))
            .build(),
        AutomationTargetV1::Task(target) => BlockBuilder::new("task")
            .attr(
                "task_profile_id",
                string(&target.task_profile_id.to_string()),
            )
            .attr(
                "task_revision_id",
                string(&target.task_revision_id.to_string()),
            )
            .attr("revision_digest", string(&target.revision_digest))
            .build(),
    }
}

fn trigger_block(trigger: &AutomationTriggerV1) -> Result<Block, String> {
    let block = match trigger {
        AutomationTriggerV1::Schedule(trigger) => BlockBuilder::new("schedule")
            .attr("expression", string(&trigger.expression))
            .attr("timezone", string(&trigger.timezone))
            .build(),
        AutomationTriggerV1::Webhook(trigger) => BlockBuilder::new("webhook")
            .attr(
                "request_schema_digest",
                string(&trigger.request_schema_digest),
            )
            .attr(
                "subscription_id",
                string(&trigger.subscription.subscription_id.to_string()),
            )
            .attr(
                "subscription_revision_digest",
                string(&trigger.subscription.revision_digest),
            )
            .build(),
        AutomationTriggerV1::PluginEvent(trigger) => event_trigger_block("plugin_event", trigger)?,
        AutomationTriggerV1::SourceEvent(trigger) => event_trigger_block("source_event", trigger)?,
    };
    Ok(block)
}

fn event_trigger_block(name: &str, trigger: &AutomationEventTriggerV1) -> Result<Block, String> {
    let mut block = BlockBuilder::new(name)
        .attr("event_key", string(&trigger.event_key))
        .attr(
            "subscription_id",
            string(&trigger.subscription.subscription_id.to_string()),
        )
        .attr(
            "subscription_revision_digest",
            string(&trigger.subscription.revision_digest),
        );
    if let Some(filter_digest) = &trigger.filter_digest {
        block = block.attr("filter_digest", string(filter_digest));
    }
    Ok(block.build())
}

pub(super) fn revision_document(spec: &AutomationRevisionSpecV1) -> Result<Document, String> {
    let definition_block = definition_document(&spec.definition)?
        .blocks
        .into_iter()
        .next()
        .ok_or_else(|| "Automation definition block is missing".to_owned())?;
    let mut root = BlockBuilder::new("automation_revision")
        .attr("revision_id", string(&spec.revision_id.to_string()))
        .attr(
            "revision_number",
            acl_integer("revision_number", spec.revision_number)?,
        )
        .attr("schema", string(AUTOMATION_REVISION_SCHEMA_V1))
        .nested_block(definition_block);
    if let Some(parent_revision_id) = spec.parent_revision_id {
        root = root.attr(
            "parent_revision_id",
            string(&parent_revision_id.to_string()),
        );
    }
    if let Some(parent_digest) = &spec.parent_digest {
        root = root.attr("parent_digest", string(parent_digest));
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

pub(super) fn parse_definition(document: &Document) -> Result<AutomationDefinitionSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("Automation definition must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        "automation_definition",
        &[
            "automation_id",
            "environment_id",
            "name",
            "organization_id",
            "project_id",
            "schema",
        ],
        &[
            "authorization",
            "policy",
            "application_release",
            "workflow_revision",
            "task",
            "schedule",
            "webhook",
            "plugin_event",
            "source_event",
        ],
    )?;
    let authorization = parse_authorization(super::validation::one_child(root, "authorization")?)?;
    let policy = parse_policy(super::validation::one_child(root, "policy")?)?;
    let target = parse_target(root)?;
    let trigger = parse_trigger(root)?;
    Ok(AutomationDefinitionSpecV1 {
        schema: required_string(root, "schema")?,
        automation_id: required_uuid(root, "automation_id")?,
        organization_id: required_uuid(root, "organization_id")?,
        project_id: required_uuid(root, "project_id")?,
        environment_id: required_uuid(root, "environment_id")?,
        name: required_string(root, "name")?,
        trigger,
        target,
        policy,
        authorization,
    })
}

fn parse_authorization(block: &Block) -> Result<AutomationAuthorizationPolicyV1, String> {
    exact_shape(
        block,
        "authorization",
        &["policy_digest", "required_grants"],
        &[],
    )?;
    Ok(AutomationAuthorizationPolicyV1 {
        policy_digest: required_digest(block, "policy_digest")?,
        required_grants: required_string_list(block, "required_grants")?,
    })
}

fn parse_policy(block: &Block) -> Result<AutomationTriggerPolicyV1, String> {
    exact_shape(
        block,
        "policy",
        &[
            "concurrency_mode",
            "deduplication_scope",
            "deduplication_template",
            "deduplication_window_ms",
            "maximum_concurrency",
            "misfire_grace_ms",
            "misfire_mode",
        ],
        &[],
    )?;
    Ok(AutomationTriggerPolicyV1 {
        deduplication: AutomationDeduplicationPolicyV1 {
            scope: AutomationDeduplicationScopeV1::parse(&required_string(
                block,
                "deduplication_scope",
            )?)?,
            key_template: required_string(block, "deduplication_template")?,
            window_ms: required_u64(block, "deduplication_window_ms")?,
        },
        concurrency: AutomationConcurrencyPolicyV1 {
            maximum: required_u64(block, "maximum_concurrency")?,
            mode: AutomationConcurrencyModeV1::parse(&required_string(block, "concurrency_mode")?)?,
        },
        misfire: AutomationMisfirePolicyV1 {
            mode: AutomationMisfireModeV1::parse(&required_string(block, "misfire_mode")?)?,
            grace_ms: required_u64(block, "misfire_grace_ms")?,
        },
    })
}

fn parse_target(root: &Block) -> Result<AutomationTargetV1, String> {
    let blocks = root
        .blocks
        .iter()
        .filter(|block| {
            ["application_release", "workflow_revision", "task"].contains(&block.name.as_str())
        })
        .collect::<Vec<_>>();
    if blocks.len() != 1 {
        return Err("Automation definition must contain exactly one target block".into());
    }
    let block = blocks[0];
    match block.name.as_str() {
        "application_release" => {
            exact_shape(
                block,
                "application_release",
                &["application_id", "application_release_id", "release_digest"],
                &[],
            )?;
            Ok(AutomationTargetV1::ApplicationRelease(
                AutomationApplicationTargetV1 {
                    application_id: required_uuid(block, "application_id")?,
                    application_release_id: required_uuid(block, "application_release_id")?,
                    release_digest: required_digest(block, "release_digest")?,
                },
            ))
        }
        "workflow_revision" => {
            exact_shape(
                block,
                "workflow_revision",
                &[
                    "revision_digest",
                    "workflow_definition_id",
                    "workflow_revision_id",
                ],
                &[],
            )?;
            Ok(AutomationTargetV1::WorkflowRevision(
                AutomationWorkflowTargetV1 {
                    workflow_definition_id: required_uuid(block, "workflow_definition_id")?,
                    workflow_revision_id: required_uuid(block, "workflow_revision_id")?,
                    revision_digest: required_digest(block, "revision_digest")?,
                },
            ))
        }
        "task" => {
            exact_shape(
                block,
                "task",
                &["revision_digest", "task_profile_id", "task_revision_id"],
                &[],
            )?;
            Ok(AutomationTargetV1::Task(AutomationTaskTargetV1 {
                task_profile_id: required_uuid(block, "task_profile_id")?,
                task_revision_id: required_uuid(block, "task_revision_id")?,
                revision_digest: required_digest(block, "revision_digest")?,
            }))
        }
        _ => Err("Automation target block is unsupported".into()),
    }
}

fn parse_trigger(root: &Block) -> Result<AutomationTriggerV1, String> {
    let names = ["schedule", "webhook", "plugin_event", "source_event"];
    let blocks = root
        .blocks
        .iter()
        .filter(|block| names.contains(&block.name.as_str()))
        .collect::<Vec<_>>();
    if blocks.len() != 1 {
        return Err("Automation definition must contain exactly one trigger block".into());
    }
    let block = blocks[0];
    match block.name.as_str() {
        "schedule" => {
            exact_shape(block, "schedule", &["expression", "timezone"], &[])?;
            Ok(AutomationTriggerV1::Schedule(AutomationScheduleTriggerV1 {
                expression: required_string(block, "expression")?,
                timezone: required_string(block, "timezone")?,
            }))
        }
        "webhook" => {
            exact_shape(
                block,
                "webhook",
                &[
                    "request_schema_digest",
                    "subscription_id",
                    "subscription_revision_digest",
                ],
                &[],
            )?;
            Ok(AutomationTriggerV1::Webhook(AutomationWebhookTriggerV1 {
                subscription: AutomationSubscriptionReferenceV1 {
                    subscription_id: required_uuid(block, "subscription_id")?,
                    revision_digest: required_digest(block, "subscription_revision_digest")?,
                },
                request_schema_digest: required_digest(block, "request_schema_digest")?,
            }))
        }
        "plugin_event" | "source_event" => {
            let mut attributes = vec![
                "event_key",
                "subscription_id",
                "subscription_revision_digest",
            ];
            if block.attributes.contains_key("filter_digest") {
                attributes.push("filter_digest");
            }
            exact_shape(block, block.name.as_str(), &attributes, &[])?;
            let trigger = AutomationEventTriggerV1 {
                subscription: AutomationSubscriptionReferenceV1 {
                    subscription_id: required_uuid(block, "subscription_id")?,
                    revision_digest: required_digest(block, "subscription_revision_digest")?,
                },
                event_key: required_string(block, "event_key")?,
                filter_digest: optional_digest(block, "filter_digest")?,
            };
            if block.name == "plugin_event" {
                Ok(AutomationTriggerV1::PluginEvent(trigger))
            } else {
                Ok(AutomationTriggerV1::SourceEvent(trigger))
            }
        }
        _ => Err("Automation trigger block is unsupported".into()),
    }
}

pub(super) fn parse_revision(document: &Document) -> Result<AutomationRevisionSpecV1, String> {
    if document.blocks.len() != 1 {
        return Err("Automation revision must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    let mut attributes = vec!["revision_id", "revision_number", "schema"];
    if root.attributes.contains_key("parent_revision_id") {
        attributes.push("parent_revision_id");
    }
    if root.attributes.contains_key("parent_digest") {
        attributes.push("parent_digest");
    }
    exact_shape(
        root,
        "automation_revision",
        &attributes,
        &["automation_definition"],
    )?;
    if root
        .blocks
        .iter()
        .filter(|block| block.name == "automation_definition")
        .count()
        != 1
    {
        return Err("Automation revision must contain exactly one definition block".into());
    }
    let definition = root
        .blocks
        .iter()
        .find(|block| block.name == "automation_definition")
        .ok_or_else(|| "Automation revision definition block is missing".to_owned())?;
    let definition = parse_definition(&Document {
        blocks: vec![definition.clone()],
    })?;
    Ok(AutomationRevisionSpecV1 {
        schema: required_string(root, "schema")?,
        revision_id: required_uuid(root, "revision_id")?,
        revision_number: required_u64(root, "revision_number")?,
        parent_revision_id: super::validation::optional_uuid(root, "parent_revision_id")?,
        parent_digest: optional_digest(root, "parent_digest")?,
        definition,
    })
}
