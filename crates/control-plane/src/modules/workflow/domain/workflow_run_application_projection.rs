use super::workflow_run_contract::{
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5,
};
use super::{
    WorkflowApplicationFrameAuthority, WorkflowPlan, WorkflowStepKind, WorkflowVariableContract,
    WorkflowVariableScope,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRunApplicationProjection {
    pub schema: String,
    pub final_output_step_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answer_step_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variable_step_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variable_assignment_step_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_authority: Option<WorkflowApplicationFrameAuthority>,
}

impl WorkflowRunApplicationProjection {
    pub fn from_plan(plan: &WorkflowPlan) -> Result<Self, String> {
        let outputs = plan
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Output)
            .collect::<Vec<_>>();
        let [output] = outputs.as_slice() else {
            return Err("Application WorkflowRun requires exactly one final Output step".into());
        };
        let projection = Self {
            schema: WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA.into(),
            final_output_step_id: output.id.clone(),
            answer_step_ids: Vec::new(),
            variable_step_ids: Vec::new(),
            variable_assignment_step_ids: Vec::new(),
            frame_authority: None,
        };
        projection.validate(plan)?;
        Ok(projection)
    }

    pub(crate) fn from_application_outputs(
        plan: &WorkflowPlan,
        final_output_step_id: String,
        answer_step_ids: Vec<String>,
    ) -> Result<Self, String> {
        let projection = Self {
            schema: WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2.into(),
            final_output_step_id,
            answer_step_ids,
            variable_step_ids: Vec::new(),
            variable_assignment_step_ids: Vec::new(),
            frame_authority: None,
        };
        projection.validate(plan)?;
        Ok(projection)
    }

    pub(crate) fn from_application_variables(
        plan: &WorkflowPlan,
        final_output_step_id: String,
        answer_step_ids: Vec<String>,
        variable_step_ids: Vec<String>,
        variable_assignment_step_ids: Vec<String>,
    ) -> Result<Self, String> {
        let projection = Self {
            schema: WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3.into(),
            final_output_step_id,
            answer_step_ids,
            variable_step_ids,
            variable_assignment_step_ids,
            frame_authority: None,
        };
        projection.validate(plan)?;
        Ok(projection)
    }

    pub(crate) fn from_application_frame(
        plan: &WorkflowPlan,
        final_output_step_id: String,
        answer_step_ids: Vec<String>,
        frame_authority: WorkflowApplicationFrameAuthority,
    ) -> Result<Self, String> {
        frame_authority.validate_for_child(
            frame_authority.organization_id,
            frame_authority.project_id,
            frame_authority.child_workflow_run_id,
            plan,
        )?;
        let projection = Self {
            schema: WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4.into(),
            final_output_step_id,
            answer_step_ids,
            variable_step_ids: Vec::new(),
            variable_assignment_step_ids: Vec::new(),
            frame_authority: Some(frame_authority),
        };
        projection.validate(plan)?;
        Ok(projection)
    }

    pub(crate) fn from_application_composite(
        plan: &WorkflowPlan,
        final_output_step_id: String,
        answer_step_ids: Vec<String>,
        variable_step_ids: Vec<String>,
        variable_assignment_step_ids: Vec<String>,
    ) -> Result<Self, String> {
        let projection = Self {
            schema: WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5.into(),
            final_output_step_id,
            answer_step_ids,
            variable_step_ids,
            variable_assignment_step_ids,
            frame_authority: None,
        };
        projection.validate(plan)?;
        Ok(projection)
    }

    pub fn validate(&self, plan: &WorkflowPlan) -> Result<(), String> {
        super::validation::validate_identifier(
            "WorkflowRun Application final Output step",
            &self.final_output_step_id,
        )?;
        for answer_step_id in &self.answer_step_ids {
            super::validation::validate_identifier(
                "WorkflowRun Application Answer step",
                answer_step_id,
            )?;
        }
        for variable_step_id in &self.variable_step_ids {
            super::validation::validate_identifier(
                "WorkflowRun Application variable step",
                variable_step_id,
            )?;
        }
        for assignment_step_id in &self.variable_assignment_step_ids {
            super::validation::validate_identifier(
                "WorkflowRun Application variable assignment step",
                assignment_step_id,
            )?;
        }
        let outputs = plan
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Output)
            .collect::<Vec<_>>();
        match self.schema.as_str() {
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA
                if self.answer_step_ids.is_empty()
                    && self.variable_step_ids.is_empty()
                    && self.variable_assignment_step_ids.is_empty()
                    && self.frame_authority.is_none()
                    && matches!(outputs.as_slice(), [output] if output.id == self.final_output_step_id)
                    && plan_step_uses_descriptor(
                        plan,
                        &self.final_output_step_id,
                        WorkflowStepKind::Output,
                        "workflow.output",
                    ) =>
            {
                Ok(())
            }
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2
                if !self.answer_step_ids.is_empty()
                    && self.variable_step_ids.is_empty()
                    && self.variable_assignment_step_ids.is_empty()
                    && self.frame_authority.is_none()
                    && outputs
                        .iter()
                        .any(|output| output.id == self.final_output_step_id)
                    && outputs
                        .iter()
                        .filter(|output| output.id != self.final_output_step_id)
                        .map(|output| output.id.as_str())
                        .eq(self.answer_step_ids.iter().map(String::as_str))
                    && plan_step_uses_descriptor(
                        plan,
                        &self.final_output_step_id,
                        WorkflowStepKind::Output,
                        "workflow.output",
                    )
                    && self.answer_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Output,
                            "application.answer",
                        )
                    }) =>
            {
                Ok(())
            }
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                if !self.variable_step_ids.is_empty()
                    && self.frame_authority.is_none()
                    && outputs
                        .iter()
                        .any(|output| output.id == self.final_output_step_id)
                    && outputs
                        .iter()
                        .filter(|output| output.id != self.final_output_step_id)
                        .map(|output| output.id.as_str())
                        .eq(self.answer_step_ids.iter().map(String::as_str))
                    && plan
                        .steps
                        .iter()
                        .filter(|step| self.variable_step_ids.contains(&step.id))
                        .map(|step| step.id.as_str())
                        .eq(self.variable_step_ids.iter().map(String::as_str))
                    && plan
                        .steps
                        .iter()
                        .filter(|step| self.variable_assignment_step_ids.contains(&step.id))
                        .map(|step| step.id.as_str())
                        .eq(self.variable_assignment_step_ids.iter().map(String::as_str))
                    && self
                        .variable_assignment_step_ids
                        .iter()
                        .all(|step_id| self.variable_step_ids.contains(step_id))
                    && plan_step_uses_descriptor(
                        plan,
                        &self.final_output_step_id,
                        WorkflowStepKind::Output,
                        "workflow.output",
                    )
                    && self.answer_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Output,
                            "application.answer",
                        )
                    })
                    && self.variable_assignment_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Service,
                            "application.conversation-variable-assign",
                        )
                    })
                    && self.variable_step_ids.iter().all(|step_id| {
                        self.variable_assignment_step_ids.contains(step_id)
                            || self.answer_step_ids.contains(step_id)
                    }) =>
            {
                Ok(())
            }
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
                if self.variable_step_ids.is_empty()
                    && self.variable_assignment_step_ids.is_empty()
                    && self.frame_authority.is_some()
                    && outputs
                        .iter()
                        .any(|output| output.id == self.final_output_step_id)
                    && outputs
                        .iter()
                        .filter(|output| output.id != self.final_output_step_id)
                        .map(|output| output.id.as_str())
                        .eq(self.answer_step_ids.iter().map(String::as_str))
                    && plan_step_uses_descriptor(
                        plan,
                        &self.final_output_step_id,
                        WorkflowStepKind::Output,
                        "workflow.output",
                    )
                    && self.answer_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Output,
                            "application.answer",
                        )
                    }) =>
            {
                self.frame_authority
                    .as_ref()
                    .expect("guarded frame authority")
                    .validate()
            }
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
                if self.frame_authority.is_none()
                    && outputs
                        .iter()
                        .any(|output| output.id == self.final_output_step_id)
                    && outputs
                        .iter()
                        .filter(|output| output.id != self.final_output_step_id)
                        .map(|output| output.id.as_str())
                        .eq(self.answer_step_ids.iter().map(String::as_str))
                    && plan
                        .steps
                        .iter()
                        .filter(|step| self.variable_step_ids.contains(&step.id))
                        .map(|step| step.id.as_str())
                        .eq(self.variable_step_ids.iter().map(String::as_str))
                    && plan
                        .steps
                        .iter()
                        .filter(|step| self.variable_assignment_step_ids.contains(&step.id))
                        .map(|step| step.id.as_str())
                        .eq(self.variable_assignment_step_ids.iter().map(String::as_str))
                    && self
                        .variable_assignment_step_ids
                        .iter()
                        .all(|step_id| self.variable_step_ids.contains(step_id))
                    && plan_step_uses_descriptor(
                        plan,
                        &self.final_output_step_id,
                        WorkflowStepKind::Output,
                        "workflow.output",
                    )
                    && self.answer_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Output,
                            "application.answer",
                        )
                    })
                    && self.variable_assignment_step_ids.iter().all(|step_id| {
                        plan_step_uses_descriptor(
                            plan,
                            step_id,
                            WorkflowStepKind::Service,
                            "application.conversation-variable-assign",
                        )
                    })
                    && self.variable_step_ids.iter().all(|step_id| {
                        self.variable_assignment_step_ids.contains(step_id)
                            || self.answer_step_ids.contains(step_id)
                    }) =>
            {
                Ok(())
            }
            _ => Err("WorkflowRun Application projection drifted from its exact descriptor-bound final Output, Answer, or variable steps".into()),
        }
    }

    pub(crate) fn validate_variable_contract(
        &self,
        plan: &WorkflowPlan,
        contract: &WorkflowVariableContract,
    ) -> Result<(), String> {
        self.validate(plan)?;
        if !matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) {
            return Err(
                "WorkflowRun Application variable material requires a variable-capable projection"
                    .into(),
            );
        }
        let application_variables = contract
            .spec()
            .declarations
            .iter()
            .filter(|declaration| declaration.scope == WorkflowVariableScope::Application)
            .map(|declaration| declaration.name.as_str())
            .collect::<BTreeSet<_>>();
        if application_variables.is_empty() {
            return Err(
                "WorkflowRun Application projection v3 has no Application-scoped variables".into(),
            );
        }
        let mut variable_steps = BTreeSet::new();
        let mut assignment_steps = BTreeSet::new();
        for read in &contract.spec().reads {
            if application_variables.contains(read.variable.as_str()) {
                variable_steps.insert(read.consumer_step_id.as_str());
            }
        }
        for assignment in &contract.spec().assignments {
            if application_variables.contains(assignment.source_variable.as_str())
                || assignment
                    .expected_revision_variable
                    .as_deref()
                    .is_some_and(|name| application_variables.contains(name))
                || assignment
                    .idempotency_key_variable
                    .as_deref()
                    .is_some_and(|name| application_variables.contains(name))
            {
                variable_steps.insert(assignment.writer_step_id.as_str());
            }
            if application_variables.contains(assignment.target_variable.as_str()) {
                variable_steps.insert(assignment.writer_step_id.as_str());
                assignment_steps.insert(assignment.writer_step_id.as_str());
            }
        }
        let ordered_variable_steps = plan
            .steps
            .iter()
            .filter(|step| variable_steps.contains(step.id.as_str()))
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        let ordered_assignment_steps = plan
            .steps
            .iter()
            .filter(|step| assignment_steps.contains(step.id.as_str()))
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        if ordered_variable_steps
            != self
                .variable_step_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
            || ordered_assignment_steps
                != self
                    .variable_assignment_step_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
        {
            return Err(
                "WorkflowRun Application variable projection drifted from its exact variable contract"
                    .into(),
            );
        }
        for step_id in &self.variable_step_ids {
            let step = plan
                .steps
                .iter()
                .find(|step| step.id == *step_id)
                .ok_or_else(|| {
                    format!("WorkflowRun Application variable step {step_id:?} disappeared")
                })?;
            if !matches!(
                step.kind,
                WorkflowStepKind::Output | WorkflowStepKind::Service
            ) {
                return Err(format!(
                    "WorkflowRun Application variable step {step_id:?} has an unsupported kind"
                ));
            }
        }
        for step_id in &self.variable_assignment_step_ids {
            if !plan
                .steps
                .iter()
                .any(|step| step.id == *step_id && step.kind == WorkflowStepKind::Service)
            {
                return Err(format!(
                    "WorkflowRun Application variable assignment step {step_id:?} is not a Service"
                ));
            }
        }
        Ok(())
    }

    pub fn is_answer_step(&self, step_id: &str) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) && self
            .answer_step_ids
            .iter()
            .any(|answer_step_id| answer_step_id == step_id)
    }

    pub fn is_variable_step(&self, step_id: &str) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) && self
            .variable_step_ids
            .iter()
            .any(|variable_step_id| variable_step_id == step_id)
    }

    pub fn is_variable_assignment_step(&self, step_id: &str) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) && self
            .variable_assignment_step_ids
            .iter()
            .any(|assignment_step_id| assignment_step_id == step_id)
    }

    pub fn projects_application_lifecycle(&self) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) && self.frame_authority.is_none()
    }

    pub fn supports_application_frames(&self) -> bool {
        matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        )
    }
}

fn plan_step_uses_descriptor(
    plan: &WorkflowPlan,
    step_id: &str,
    kind: WorkflowStepKind,
    descriptor_id: &str,
) -> bool {
    plan.steps.iter().any(|step| {
        step.id == step_id
            && step.kind == kind
            && step
                .descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.descriptor_id == descriptor_id)
    })
}
