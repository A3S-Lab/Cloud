use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::{canonical_digest, generate_acl, parse_acl};

mod codec;
mod model;
mod validation;

pub use model::{
    WorkflowStepBindingKind, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistrySpec,
    WorkflowStepDescriptorSpec, WorkflowStepExecutionClass, WorkflowStepFailureContract,
    WorkflowStepFallbackMode, WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepPresentationSpec, WorkflowStepRetryClassification,
};
use validation::normalize_registry_spec;

pub const WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA: &str =
    "cloud.workflow.step-descriptor-registry.v1";
pub const WORKFLOW_STEP_DESCRIPTOR_SEMANTIC_SCHEMA: &str =
    "cloud.workflow.step-descriptor-semantic.v1";
pub const WORKFLOW_STEP_PRESENTATION_SCHEMA: &str = "cloud.workflow.step-presentation.v1";
pub const WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepPresentation {
    spec: WorkflowStepPresentationSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowStepPresentation {
    pub const fn spec(&self) -> &WorkflowStepPresentationSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepDescriptorRevision {
    spec: WorkflowStepDescriptorSpec,
    semantic_acl: String,
    semantic_digest: Sha256Digest,
    presentation: WorkflowStepPresentation,
}

impl WorkflowStepDescriptorRevision {
    pub fn id(&self) -> &str {
        &self.spec.id
    }

    pub fn revision(&self) -> &str {
        &self.spec.revision
    }

    pub const fn spec(&self) -> &WorkflowStepDescriptorSpec {
        &self.spec
    }

    pub fn semantic_acl(&self) -> &str {
        &self.semantic_acl
    }

    pub const fn semantic_digest(&self) -> &Sha256Digest {
        &self.semantic_digest
    }

    pub const fn presentation(&self) -> &WorkflowStepPresentation {
        &self.presentation
    }

    pub const fn supports_compiler_schema_version(&self, version: u32) -> bool {
        version >= self.spec.minimum_compiler_schema_version
            && version <= self.spec.maximum_compiler_schema_version
    }

    pub const fn admitted(&self) -> bool {
        matches!(
            self.spec.admission,
            WorkflowStepDescriptorAdmission::Admitted
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepDescriptorRegistry {
    id: String,
    revision: String,
    compiler_schema_version: u32,
    descriptors: Vec<WorkflowStepDescriptorRevision>,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowStepDescriptorRegistry {
    pub fn from_spec(spec: WorkflowStepDescriptorRegistrySpec) -> Result<Self, String> {
        let spec = normalize_registry_spec(spec)?;
        let document = codec::registry_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES {
            return Err("Workflow descriptor registry ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Workflow descriptor ACL is invalid: {error}"))?;
        let digest = digest_document(&reparsed, "Workflow descriptor registry")?;
        let descriptors = spec
            .descriptors
            .into_iter()
            .map(build_descriptor)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: spec.id,
            revision: spec.revision,
            compiler_schema_version: spec.compiler_schema_version,
            descriptors,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES {
            return Err("Workflow descriptor registry ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Workflow descriptor registry contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let registry = Self::from_spec(codec::parse_registry_spec(&normalized)?)?;
        if registry.canonical_acl != normalized {
            return Err("Workflow descriptor registry ACL is not canonical".into());
        }
        Ok(registry)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let registry = Self::parse_acl(source)?;
        if registry.digest.as_str() != stored_digest {
            return Err("stored Workflow descriptor registry and digest do not match".into());
        }
        Ok(registry)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub const fn compiler_schema_version(&self) -> u32 {
        self.compiler_schema_version
    }

    pub fn descriptors(&self) -> &[WorkflowStepDescriptorRevision] {
        &self.descriptors
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn resolve(&self, id: &str, revision: &str) -> Option<&WorkflowStepDescriptorRevision> {
        self.descriptors
            .binary_search_by(|descriptor| {
                descriptor
                    .id()
                    .cmp(id)
                    .then_with(|| descriptor.revision().cmp(revision))
            })
            .ok()
            .map(|index| &self.descriptors[index])
    }

    pub fn resolve_for_compiler(
        &self,
        id: &str,
        revision: &str,
        compiler_schema_version: u32,
    ) -> Result<&WorkflowStepDescriptorRevision, String> {
        let descriptor = self
            .resolve(id, revision)
            .ok_or_else(|| format!("Workflow descriptor {id:?}@{revision:?} is not registered"))?;
        if !descriptor.admitted() {
            let reason = descriptor
                .spec()
                .unavailable_reason
                .as_deref()
                .unwrap_or("no unavailable reason was recorded");
            return Err(format!(
                "Workflow descriptor {id:?}@{revision:?} is unavailable: {reason}"
            ));
        }
        if !descriptor.supports_compiler_schema_version(compiler_schema_version) {
            return Err(format!(
                "Workflow descriptor {id:?}@{revision:?} does not support compiler schema {compiler_schema_version}"
            ));
        }
        Ok(descriptor)
    }
}

fn build_descriptor(
    spec: WorkflowStepDescriptorSpec,
) -> Result<WorkflowStepDescriptorRevision, String> {
    let semantic_document = codec::semantic_document(&spec);
    let semantic_acl = generate_acl(&semantic_document);
    let semantic_digest = digest_document(&semantic_document, "Workflow descriptor semantics")?;
    let presentation_document =
        codec::presentation_document(&spec.id, &spec.revision, &spec.presentation);
    let presentation = WorkflowStepPresentation {
        spec: spec.presentation.clone(),
        canonical_acl: generate_acl(&presentation_document),
        digest: digest_document(&presentation_document, "Workflow descriptor presentation")?,
    };
    Ok(WorkflowStepDescriptorRevision {
        spec,
        semantic_acl,
        semantic_digest,
        presentation,
    })
}

fn digest_document(document: &a3s_acl::Document, label: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(
        canonical_digest(document)
            .map_err(|error| format!("{label} is not canonicalizable: {error}"))?,
    )
}
