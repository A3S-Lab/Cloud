use super::super::types::{AttestStepInput, AttestStepOutput};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build, load_revision};
use crate::modules::artifacts::domain::{
    BuildEvidence, BuildEvidenceGenerationError, BuildRunStatus,
};
use a3s_flow::FlowError;
use chrono::Utc;

pub(super) async fn attest(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: AttestStepInput,
) -> a3s_flow::Result<AttestStepOutput> {
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    if build.published_artifact.as_ref() != Some(&input.artifact) {
        return Err(FlowError::Runtime(
            "build attestation changed the published OCI artifact".into(),
        ));
    }
    if let Some(reason) = &build.failure {
        return Ok(AttestStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if let Some(evidence) = &build.evidence {
        return ready_output(evidence, &input.artifact);
    }
    if !build.evidence_required {
        return Ok(AttestStepOutput::Failed {
            reason: "cloud.build@3 requires supply-chain evidence for every published artifact"
                .into(),
        });
    }
    if matches!(
        build.status,
        BuildRunStatus::Publishing | BuildRunStatus::Cancelling
    ) {
        let expected = build.aggregate_version;
        build
            .begin_attestation(Utc::now().max(build.updated_at))
            .map_err(|error| flow_error("could not begin build attestation", error))?;
        build = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist build attestation", error))?;
    } else if build.status != BuildRunStatus::Attesting {
        return Err(FlowError::Runtime(format!(
            "build cannot attest output from {}",
            build.status.as_str()
        )));
    }

    let revision = load_revision(runtime, &build).await?;
    let attested_at = Utc::now().max(build.updated_at);
    let evidence = match runtime
        .evidence
        .generate(&build, &revision, attested_at)
        .await
    {
        Ok(evidence) => evidence,
        Err(
            error @ (BuildEvidenceGenerationError::Unavailable(_)
            | BuildEvidenceGenerationError::Storage(_)),
        ) => return Err(flow_error("build evidence is not ready", error)),
        Err(error) => {
            return Ok(AttestStepOutput::Failed {
                reason: bounded_reason(error.to_string()),
            })
        }
    };
    let expected = build.aggregate_version;
    if let Err(error) = build.record_evidence(evidence.clone(), attested_at) {
        return Ok(AttestStepOutput::Failed {
            reason: bounded_reason(format!(
                "build evidence failed durable binding validation: {error}"
            )),
        });
    }
    let build = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist verified build evidence", error))?;
    ready_output(
        build
            .evidence
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("attesting build omitted its evidence".into()))?,
        &input.artifact,
    )
}

fn ready_output(
    evidence: &BuildEvidence,
    artifact: &crate::modules::artifacts::domain::PublishedOciArtifact,
) -> a3s_flow::Result<AttestStepOutput> {
    if &evidence.artifact != artifact {
        return Err(FlowError::Runtime(
            "verified build evidence changed its published artifact".into(),
        ));
    }
    Ok(AttestStepOutput::Ready {
        sbom_digest: evidence.sbom_digest.clone(),
        provenance_digest: evidence.provenance_digest.clone(),
        key_id: evidence.signing_key.key_id.clone(),
        attested_at: evidence.attested_at,
    })
}
