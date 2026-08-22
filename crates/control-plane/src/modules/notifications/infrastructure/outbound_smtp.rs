use crate::infrastructure::{
    PreparedSmtpSubmission, SmtpCredentials, SmtpPreparationError, SmtpSubmissionOutcome,
    SmtpTlsPolicy, SmtpTransport, SmtpTransportOptions,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::notifications::domain::{
    IOutboundNotificationSmtpDeliveryService, IPreparedOutboundNotificationSmtpDelivery,
    OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationSmtpPreparationError, OutboundNotificationSmtpProviderOutcome,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::SecondsFormat;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

const MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpOutboundNotificationTlsPolicy {
    RequiredStartTls,
    Implicit,
}

pub struct SmtpOutboundNotificationCredentials {
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for SmtpOutboundNotificationCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpOutboundNotificationCredentials")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

pub struct SmtpOutboundNotificationDeliveryOptions {
    pub host: String,
    pub port: u16,
    pub tls_policy: SmtpOutboundNotificationTlsPolicy,
    pub hello_name: String,
    pub ca_certificate_file: String,
    pub sender: RecipientEmailAddress,
    pub credentials: SmtpOutboundNotificationCredentials,
    pub connect_timeout: Duration,
    pub command_timeout: Duration,
}

pub struct SmtpOutboundNotificationDeliveryService {
    sender: RecipientEmailAddress,
    transport: Arc<SmtpTransport>,
}

impl SmtpOutboundNotificationDeliveryService {
    pub fn from_options(options: SmtpOutboundNotificationDeliveryOptions) -> Result<Self, String> {
        let tls_policy = match options.tls_policy {
            SmtpOutboundNotificationTlsPolicy::RequiredStartTls => SmtpTlsPolicy::RequiredStartTls,
            SmtpOutboundNotificationTlsPolicy::Implicit => SmtpTlsPolicy::Implicit,
        };
        let transport = SmtpTransport::new(SmtpTransportOptions {
            host: options.host,
            port: options.port,
            tls_policy,
            hello_name: options.hello_name,
            ca_certificate_file: options.ca_certificate_file,
            credentials: SmtpCredentials {
                username: options.credentials.username,
                password: options.credentials.password,
            },
            connect_timeout: options.connect_timeout,
            command_timeout: options.command_timeout,
        })?;
        Ok(Self::new(options.sender, Arc::new(transport)))
    }

    pub(crate) fn new(sender: RecipientEmailAddress, transport: Arc<SmtpTransport>) -> Self {
        Self { sender, transport }
    }
}

impl std::fmt::Debug for SmtpOutboundNotificationDeliveryService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpOutboundNotificationDeliveryService")
            .field("sender", &"[REDACTED]")
            .field("transport", &self.transport)
            .finish()
    }
}

struct PreparedOutboundNotificationSmtpDelivery {
    submission: PreparedSmtpSubmission,
}

impl std::fmt::Debug for PreparedOutboundNotificationSmtpDelivery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedOutboundNotificationSmtpDelivery")
            .field("submission", &self.submission)
            .finish()
    }
}

#[async_trait]
impl IOutboundNotificationSmtpDeliveryService for SmtpOutboundNotificationDeliveryService {
    async fn prepare(
        &self,
        delivery: &OutboundNotificationDelivery,
        address: RecipientEmailAddress,
    ) -> Result<
        Box<dyn IPreparedOutboundNotificationSmtpDelivery>,
        OutboundNotificationSmtpPreparationError,
    > {
        delivery
            .validate()
            .map_err(|_| OutboundNotificationSmtpPreparationError::Invalid)?;
        if delivery.channel() != OutboundNotificationChannel::Smtp
            || delivery.recipient_contact_id().is_none()
        {
            return Err(OutboundNotificationSmtpPreparationError::Invalid);
        }
        let message = build_message(&self.sender, &address, delivery)
            .map_err(|_| OutboundNotificationSmtpPreparationError::Invalid)?;
        let session = self
            .transport
            .prepare()
            .await
            .map_err(|error| match error {
                SmtpPreparationError::Unavailable => {
                    OutboundNotificationSmtpPreparationError::Unavailable
                }
            })?;
        let submission = session
            .prepare_submission(self.sender.as_str(), address.as_str(), message)
            .map_err(|_| OutboundNotificationSmtpPreparationError::Invalid)?;
        Ok(Box::new(PreparedOutboundNotificationSmtpDelivery {
            submission,
        }))
    }
}

#[async_trait]
impl IPreparedOutboundNotificationSmtpDelivery for PreparedOutboundNotificationSmtpDelivery {
    async fn deliver(self: Box<Self>) -> OutboundNotificationSmtpProviderOutcome {
        match self.submission.submit().await {
            SmtpSubmissionOutcome::Accepted => OutboundNotificationSmtpProviderOutcome::Accepted,
            SmtpSubmissionOutcome::PermanentRejected => {
                OutboundNotificationSmtpProviderOutcome::Rejected
            }
            SmtpSubmissionOutcome::TransientRejected => {
                OutboundNotificationSmtpProviderOutcome::Retryable
            }
            SmtpSubmissionOutcome::Indeterminate => {
                OutboundNotificationSmtpProviderOutcome::Indeterminate
            }
        }
    }
}

fn build_message(
    sender: &RecipientEmailAddress,
    recipient: &RecipientEmailAddress,
    delivery: &OutboundNotificationDelivery,
) -> Result<Zeroizing<Vec<u8>>, String> {
    delivery.validate()?;
    if delivery.channel() != OutboundNotificationChannel::Smtp {
        return Err("outbound SMTP message requires an SMTP delivery".into());
    }

    let occurred_at = delivery
        .occurred_at()
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut body = Zeroizing::new(String::with_capacity(4096));
    writeln!(body, "A3S Cloud notification").map_err(|error| error.to_string())?;
    writeln!(body).map_err(|error| error.to_string())?;
    writeln!(body, "Severity: {}", delivery.severity().as_str())
        .map_err(|error| error.to_string())?;
    writeln!(body, "Occurred at: {occurred_at}").map_err(|error| error.to_string())?;
    writeln!(body).map_err(|error| error.to_string())?;
    writeln!(body, "{}", delivery.title()).map_err(|error| error.to_string())?;
    writeln!(body).map_err(|error| error.to_string())?;
    writeln!(body, "{}", delivery.body()).map_err(|error| error.to_string())?;
    let encoded = Zeroizing::new(STANDARD.encode(body.as_bytes()));

    let mut message = Zeroizing::new(Vec::with_capacity(encoded.len() + 1024));
    for part in [
        "From: ",
        sender.as_str(),
        "\r\nTo: ",
        recipient.as_str(),
        "\r\nSubject: A3S Cloud notification\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "Auto-Submitted: auto-generated\r\n\r\n",
    ] {
        message.extend_from_slice(part.as_bytes());
    }
    for line in encoded.as_bytes().chunks(76) {
        message.extend_from_slice(line);
        message.extend_from_slice(b"\r\n");
    }
    if message.len() > MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_MESSAGE_BYTES || !message.is_ascii() {
        return Err("outbound SMTP notification message is invalid".into());
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{SmtpCredentials, SmtpTlsPolicy, SmtpTransportOptions};
    use crate::modules::notifications::domain::{
        Notification, NotificationScope, NotificationSeverity,
        OutboundNotificationSubscriptionDefinition,
    };
    use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
    use chrono::Utc;
    use std::time::Duration;
    use uuid::Uuid;

    fn smtp_delivery() -> OutboundNotificationDelivery {
        let now = Utc::now();
        let notification = Notification::project(
            OrganizationId::new(),
            PrincipalId::new(),
            Uuid::now_v7(),
            "identity.membership.role-changed".into(),
            1,
            Uuid::now_v7(),
            1,
            Uuid::now_v7(),
            NotificationSeverity::Warning,
            "Permissions updated".into(),
            "Your organization role is now member.".into(),
            NotificationScope::Organization,
            now,
            now,
        )
        .expect("notification");
        OutboundNotificationSubscriptionDefinition::from_smtp_spec(
            RecipientContactId::new(),
            NotificationSeverity::Information,
            2,
            None,
        )
        .expect("SMTP definition")
        .delivery_for(&notification)
        .expect("SMTP delivery")
    }

    #[test]
    fn message_is_fixed_bounded_base64_and_debug_surfaces_are_redacted() {
        let delivery = smtp_delivery();
        let sender = RecipientEmailAddress::parse("no-reply@example.test").expect("sender");
        let recipient = RecipientEmailAddress::parse("private@example.test").expect("recipient");
        let message = build_message(&sender, &recipient, &delivery).expect("message");
        assert!(message.len() < MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_MESSAGE_BYTES);
        assert!(message.is_ascii());
        let rendered = String::from_utf8(message.to_vec()).expect("ASCII message");
        assert!(rendered.contains("Subject: A3S Cloud notification\r\n"));
        assert!(rendered.contains("Content-Transfer-Encoding: base64\r\n"));
        assert!(!rendered.contains(delivery.title()));
        assert!(!rendered.contains(delivery.body()));
    }

    #[tokio::test]
    #[ignore = "requires the checksum-pinned authenticated TLS Mailpit H0 fixture"]
    async fn real_mailpit_accepts_one_outbound_notification_submission() {
        let host = required_environment("A3S_CLOUD_TEST_SMTP_HOST");
        let port = required_environment("A3S_CLOUD_TEST_SMTP_PORT")
            .parse::<u16>()
            .expect("SMTP fixture port");
        let ca_certificate_file = required_environment("A3S_CLOUD_TEST_SMTP_CA_FILE");
        let username = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_USERNAME"));
        let password = Zeroizing::new(required_environment("A3S_CLOUD_TEST_SMTP_PASSWORD"));
        let api = required_environment("A3S_CLOUD_TEST_MAILPIT_API");
        let sender = RecipientEmailAddress::parse("no-reply@example.test").expect("sender");
        let recipient =
            RecipientEmailAddress::parse("n5e-real-relay@example.test").expect("recipient");
        let transport = SmtpTransport::new(SmtpTransportOptions {
            host,
            port,
            tls_policy: SmtpTlsPolicy::RequiredStartTls,
            hello_name: "cloud.test.invalid".into(),
            ca_certificate_file,
            credentials: SmtpCredentials { username, password },
            connect_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(10),
        })
        .expect("SMTP transport");
        let service = SmtpOutboundNotificationDeliveryService::new(sender, Arc::new(transport));
        let delivery = smtp_delivery();

        let outcome = service
            .prepare(&delivery, recipient)
            .await
            .expect("authenticated required-STARTTLS session")
            .deliver()
            .await;
        assert_eq!(outcome, OutboundNotificationSmtpProviderOutcome::Accepted);

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Mailpit API client");
        let view = format!(
            "{}/view/latest.txt?query=to%3An5e-real-relay%40example.test",
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
        let captured = captured.expect("Mailpit captured the outbound notification");
        assert!(captured.contains("A3S Cloud notification"));
        println!(
            "A3S_CLOUD_C0_3_N5E_MAILPIT_CERTIFIED tls=required_starttls auth=plain submissions=1 outcome=accepted"
        );
    }

    fn required_environment(name: &str) -> String {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("required test environment {name} is not set"))
    }
}
