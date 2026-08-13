use super::*;
use crate::modules::shared_kernel::domain::Sha256Digest;

const BINDINGS_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.3/step-descriptor-bindings.acl"
));

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn spec() -> WorkflowStepDescriptorBindingsSpec {
    WorkflowStepDescriptorBindingsSpec {
        id: "support.triage".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: vec![
            WorkflowStepDescriptorBinding {
                step_id: "triage".into(),
                descriptor_id: "workflow.transform".into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: digest('b'),
            },
            WorkflowStepDescriptorBinding {
                step_id: "input".into(),
                descriptor_id: "workflow.input".into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: digest('a'),
            },
        ],
    }
}

#[test]
fn descriptor_bindings_are_canonical_digest_addressed_and_restorable() {
    let bindings = WorkflowStepDescriptorBindings::from_spec(spec()).expect("bindings");
    assert_eq!(bindings.id(), "support.triage");
    assert_eq!(bindings.revision(), "1.0.0");
    assert_eq!(bindings.compiler_schema_version(), 2);
    assert_eq!(bindings.bindings()[0].step_id, "input");
    assert!(bindings.canonical_acl().ends_with('\n'));
    assert_eq!(
        WorkflowStepDescriptorBindings::restore(
            bindings.canonical_acl(),
            bindings.digest().as_str(),
        )
        .expect("restored"),
        bindings
    );
}

#[test]
fn checked_in_descriptor_bindings_fixture_matches_the_domain_generator() {
    let generated = WorkflowStepDescriptorBindings::from_spec(spec()).expect("bindings");
    assert_eq!(
        BINDINGS_FIXTURE.replace("\r\n", "\n"),
        generated.canonical_acl()
    );
    assert_eq!(
        WorkflowStepDescriptorBindings::parse_acl(BINDINGS_FIXTURE).expect("fixture"),
        generated
    );
}

#[test]
fn descriptor_bindings_reject_floating_or_duplicate_authority() {
    let mut floating = spec();
    floating.bindings[0].descriptor_revision = "latest".into();
    assert!(WorkflowStepDescriptorBindings::from_spec(floating).is_err());

    let mut duplicate = spec();
    duplicate.bindings[1].step_id = duplicate.bindings[0].step_id.clone();
    assert!(WorkflowStepDescriptorBindings::from_spec(duplicate).is_err());
}
