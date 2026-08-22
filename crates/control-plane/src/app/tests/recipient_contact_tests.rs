use super::*;

const RECIPIENT_READ_TOKEN: &str =
    "a3s_5555555555555555555555555555555555555555555555555555555555555555";
const RECIPIENT_WRITE_TOKEN: &str =
    "a3s_6666666666666666666666666666666666666666666666666666666666666666";
const OTHER_RECIPIENT_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777777";

#[tokio::test]
async fn recipient_contact_routes_are_exact_self_scoped_replay_safe_and_redacted() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "recipient-contact-bootstrap", "Recipient contacts").await?;
    let owner_membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "recipient-contact-owner-human",
            json!({
                "principalKind": "human",
                "name": "Mailbox owner",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(owner_membership.status(), 201);
    let owner_principal = response_json(&owner_membership)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("recipient contact principal has no ID".into()))?
        .to_owned();
    for (idempotency_key, name, token, scopes) in [
        (
            "recipient-contact-read-token",
            "recipient-contact-reader",
            RECIPIENT_READ_TOKEN,
            vec![ApiTokenScope::CLOUD_READ],
        ),
        (
            "recipient-contact-write-token",
            "recipient-contact-writer",
            RECIPIENT_WRITE_TOKEN,
            vec![ApiTokenScope::IDENTITY_WRITE],
        ),
    ] {
        let created = app
            .call(post_json(
                format!("/api/v1/organizations/{organization}/api-tokens"),
                idempotency_key,
                json!({
                    "name": name,
                    "token": token,
                    "scopes": scopes,
                    "principalId": owner_principal,
                    "expiresAt": null
                }),
            ))
            .await?;
        assert_eq!(created.status(), 201);
        assert!(!String::from_utf8_lossy(created.body()).contains(token));
    }

    let collection = format!("/api/v1/organizations/{organization}/recipient-contacts");
    let mailbox = "private.owner@example.test";
    let service_cannot_begin = app
        .call(post_json(
            &collection,
            "recipient-contact-service-cannot-begin",
            json!({"address": mailbox}),
        ))
        .await?;
    assert_eq!(service_cannot_begin.status(), 403);
    assert!(!String::from_utf8_lossy(service_cannot_begin.body()).contains(mailbox));

    let read_cannot_begin = app
        .call(post_json_as(
            &collection,
            "recipient-contact-read-cannot-begin",
            json!({"address": mailbox}),
            RECIPIENT_READ_TOKEN,
        ))
        .await?;
    assert_eq!(read_cannot_begin.status(), 403);

    let begun = app
        .call(post_json_as(
            &collection,
            "recipient-contact-begin",
            json!({"address": mailbox}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(begun.status(), 202);
    let begun_body = response_json(&begun)?;
    let begun_data = &begun_body["data"];
    let contact_id = begun_data["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("recipient contact response has no ID".into()))?;
    assert_eq!(begun_data["addressHint"], "***@example.test");
    assert_eq!(begun_data["status"], "pending");
    assert_eq!(begun_data["aggregateVersion"], 1);
    assert_eq!(begun_data["replayed"], false);
    assert!(begun_data["addressDigest"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:") && value.len() == 71));
    assert_recipient_contact_response_is_redacted(&begun_body, mailbox);

    let replayed = app
        .call(post_json_as(
            &collection,
            "recipient-contact-begin",
            json!({"address": mailbox}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed_body = response_json(&replayed)?;
    assert_eq!(replayed_body["data"]["id"], contact_id);
    assert_eq!(replayed_body["data"]["replayed"], true);
    assert_recipient_contact_response_is_redacted(&replayed_body, mailbox);

    let write_cannot_list = app.call(get_as(&collection, RECIPIENT_WRITE_TOKEN)).await?;
    assert_eq!(write_cannot_list.status(), 403);
    let listed = app.call(get_as(&collection, RECIPIENT_READ_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed_body = response_json(&listed)?;
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_body["data"][0]["id"], contact_id);
    assert!(listed_body["data"][0].get("replayed").is_none());
    assert_recipient_contact_response_is_redacted(&listed_body, mailbox);

    let item = format!("{collection}/{contact_id}");
    let fetched = app.call(get_as(&item, RECIPIENT_READ_TOKEN)).await?;
    assert_eq!(fetched.status(), 200);
    let fetched_body = response_json(&fetched)?;
    assert_eq!(fetched_body["data"]["id"], contact_id);
    assert_recipient_contact_response_is_redacted(&fetched_body, mailbox);

    let other_membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "recipient-contact-other-human",
            json!({
                "principalKind": "human",
                "name": "Other mailbox owner",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(other_membership.status(), 201);
    let other_principal = response_json(&other_membership)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("other recipient principal has no ID".into()))?
        .to_owned();
    let other_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "recipient-contact-other-token",
            json!({
                "name": "other-recipient-owner",
                "token": OTHER_RECIPIENT_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::IDENTITY_WRITE],
                "principalId": other_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(other_token.status(), 201);
    let foreign_list = app.call(get_as(&collection, OTHER_RECIPIENT_TOKEN)).await?;
    assert_eq!(foreign_list.status(), 200);
    assert_eq!(response_json(&foreign_list)?["data"], json!([]));
    let foreign_get = app.call(get_as(&item, OTHER_RECIPIENT_TOKEN)).await?;
    assert_eq!(foreign_get.status(), 404);

    let forged_owner = app
        .call(post_json_as(
            &collection,
            "recipient-contact-forged-owner",
            json!({"address": "other@example.test", "principalId": other_principal}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(forged_owner.status(), 400);
    assert!(!String::from_utf8_lossy(forged_owner.body()).contains("other@example.test"));

    let invalid_proof = "a3srcv1.invalid-private-proof";
    let rejected_completion = app
        .call(post_json_as(
            format!("{item}/verification"),
            "recipient-contact-invalid-proof",
            json!({"proof": invalid_proof}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected_completion.status(), 403);
    assert!(!String::from_utf8_lossy(rejected_completion.body()).contains(invalid_proof));

    let revoked = app
        .call(post_json_as(
            format!("{item}/revocation"),
            "recipient-contact-revoke",
            json!({"expectedVersion": 1}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    let revoked_body = response_json(&revoked)?;
    assert_eq!(revoked_body["data"]["status"], "revoked");
    assert_eq!(revoked_body["data"]["aggregateVersion"], 2);
    assert_eq!(revoked_body["data"]["replayed"], false);
    assert_recipient_contact_response_is_redacted(&revoked_body, mailbox);

    let revoke_replay = app
        .call(post_json_as(
            format!("{item}/revocation"),
            "recipient-contact-revoke",
            json!({"expectedVersion": 1}),
            RECIPIENT_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(revoke_replay.status(), 200);
    assert_eq!(response_json(&revoke_replay)?["data"]["replayed"], true);
    Ok(())
}

fn assert_recipient_contact_response_is_redacted(response: &Value, mailbox: &str) {
    let encoded = response.to_string();
    for forbidden in [
        mailbox,
        "challengeId",
        "verificationId",
        "signingKeyId",
        "proof",
    ] {
        assert!(!encoded.contains(forbidden), "response leaked {forbidden}");
    }
}
