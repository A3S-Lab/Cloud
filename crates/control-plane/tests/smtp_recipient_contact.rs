use a3s_cloud_control_plane::modules::identity::domain::services::{
    IRecipientContactVerificationDeliveryService, RecipientContactVerificationDeliveryRequest,
    RecipientContactVerificationProviderOutcome,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::RecipientEmailAddress;
use a3s_cloud_control_plane::modules::identity::infrastructure::{
    SmtpRecipientContactVerificationCredentials, SmtpRecipientContactVerificationDeliveryOptions,
    SmtpRecipientContactVerificationDeliveryService, SmtpRecipientContactVerificationTlsPolicy,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::RecipientContactVerificationId;
use chrono::{Duration as ChronoDuration, Utc};
use std::time::Duration;
use zeroize::Zeroizing;

#[tokio::test]
#[ignore = "requires the checksum-pinned authenticated TLS Mailpit H0 fixture"]
async fn real_mailpit_accepts_one_authenticated_required_starttls_submission() {
    let host = required_environment("A3S_CLOUD_TEST_SMTP_HOST");
    let port = required_environment("A3S_CLOUD_TEST_SMTP_PORT")
        .parse::<u16>()
        .expect("SMTP fixture port");
    let ca_certificate_file = required_environment("A3S_CLOUD_TEST_SMTP_CA_FILE");
    let username = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_USERNAME"));
    let password = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_PASSWORD"));
    let api = required_environment("A3S_CLOUD_TEST_MAILPIT_API");
    let recipient =
        RecipientEmailAddress::parse("n5c-real-relay@example.test").expect("recipient address");
    let proof = Zeroizing::new("a3srcv1.mailpit-conformance.synthetic-proof".to_owned());
    let service = SmtpRecipientContactVerificationDeliveryService::new(
        SmtpRecipientContactVerificationDeliveryOptions {
            host,
            port,
            tls_policy: SmtpRecipientContactVerificationTlsPolicy::RequiredStartTls,
            hello_name: "cloud.test.invalid".into(),
            ca_certificate_file,
            sender: RecipientEmailAddress::parse("no-reply@example.test").expect("sender"),
            credentials: SmtpRecipientContactVerificationCredentials { username, password },
            connect_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(10),
        },
    )
    .expect("SMTP delivery service");

    let outcome = service
        .prepare()
        .await
        .expect("authenticated required-STARTTLS session")
        .deliver(RecipientContactVerificationDeliveryRequest {
            verification_id: RecipientContactVerificationId::new(),
            address: recipient,
            proof: proof.clone(),
            expires_at: Utc::now() + ChronoDuration::minutes(10),
        })
        .await;
    assert_eq!(
        outcome,
        RecipientContactVerificationProviderOutcome::Delivered
    );

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Mailpit API client");
    let view = format!(
        "{}/view/latest.txt?query=to%3An5c-real-relay%40example.test",
        api.trim_end_matches('/')
    );
    let mut captured = None;
    for _ in 0..20 {
        if let Ok(response) = client.get(&view).send().await {
            if response.status().is_success() {
                captured = response.text().await.ok();
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let captured = captured.expect("Mailpit captured the SMTP submission");
    assert!(captured.contains(proof.as_str()));
    assert!(captured.contains("Expires at:"));
    println!(
        "A3S_CLOUD_C0_3_N5C_MAILPIT_CERTIFIED tls=required_starttls auth=plain submissions=1 outcome=delivered"
    );
}

fn required_environment(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("required test environment {name} is not set"))
}
