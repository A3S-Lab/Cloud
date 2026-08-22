use crate::infrastructure::{
    PreparedSmtpSession, SmtpCredentials, SmtpPreparationError, SmtpSubmissionOutcome,
    SmtpTlsPolicy, SmtpTransport, SmtpTransportOptions,
};
use crate::modules::identity::domain::services::{
    IPreparedRecipientContactVerificationDelivery, IRecipientContactVerificationDeliveryService,
    RecipientContactVerificationDeliveryPreparationError,
    RecipientContactVerificationDeliveryRequest, RecipientContactVerificationProviderOutcome,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use async_trait::async_trait;
use chrono::SecondsFormat;
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_VERIFICATION_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpRecipientContactVerificationTlsPolicy {
    RequiredStartTls,
    Implicit,
}

pub struct SmtpRecipientContactVerificationCredentials {
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for SmtpRecipientContactVerificationCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpRecipientContactVerificationCredentials")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

pub struct SmtpRecipientContactVerificationDeliveryOptions {
    pub host: String,
    pub port: u16,
    pub tls_policy: SmtpRecipientContactVerificationTlsPolicy,
    pub hello_name: String,
    pub ca_certificate_file: String,
    pub sender: RecipientEmailAddress,
    pub credentials: SmtpRecipientContactVerificationCredentials,
    pub connect_timeout: Duration,
    pub command_timeout: Duration,
}

pub struct SmtpRecipientContactVerificationDeliveryService {
    sender: RecipientEmailAddress,
    transport: Arc<SmtpTransport>,
}

impl SmtpRecipientContactVerificationDeliveryService {
    pub fn new(options: SmtpRecipientContactVerificationDeliveryOptions) -> Result<Self, String> {
        if options.sender.as_str().is_empty() {
            return Err("SMTP recipient contact verification options are invalid".into());
        }
        let tls_policy = match options.tls_policy {
            SmtpRecipientContactVerificationTlsPolicy::RequiredStartTls => {
                SmtpTlsPolicy::RequiredStartTls
            }
            SmtpRecipientContactVerificationTlsPolicy::Implicit => SmtpTlsPolicy::Implicit,
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
        Ok(Self::from_transport(options.sender, Arc::new(transport)))
    }

    pub(crate) fn from_transport(
        sender: RecipientEmailAddress,
        transport: Arc<SmtpTransport>,
    ) -> Self {
        Self { sender, transport }
    }
}

impl std::fmt::Debug for SmtpRecipientContactVerificationDeliveryService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpRecipientContactVerificationDeliveryService")
            .field("sender", &"[REDACTED]")
            .field("transport", &self.transport)
            .finish()
    }
}

struct LettrePreparedRecipientContactVerificationDelivery {
    session: PreparedSmtpSession,
    sender: RecipientEmailAddress,
}

#[async_trait]
impl IRecipientContactVerificationDeliveryService
    for SmtpRecipientContactVerificationDeliveryService
{
    async fn prepare(
        &self,
    ) -> Result<
        Box<dyn IPreparedRecipientContactVerificationDelivery>,
        RecipientContactVerificationDeliveryPreparationError,
    > {
        let session = self
            .transport
            .prepare()
            .await
            .map_err(|error| match error {
                SmtpPreparationError::Unavailable => {
                    RecipientContactVerificationDeliveryPreparationError::Unavailable
                }
            })?;
        Ok(Box::new(
            LettrePreparedRecipientContactVerificationDelivery {
                session,
                sender: self.sender.clone(),
            },
        ))
    }
}

#[async_trait]
impl IPreparedRecipientContactVerificationDelivery
    for LettrePreparedRecipientContactVerificationDelivery
{
    async fn deliver(
        self: Box<Self>,
        request: RecipientContactVerificationDeliveryRequest,
    ) -> RecipientContactVerificationProviderOutcome {
        let message = match build_verification_message(&self.sender, &request) {
            Ok(value) => value,
            Err(_) => return RecipientContactVerificationProviderOutcome::Indeterminate,
        };
        let submission = match self.session.prepare_submission(
            self.sender.as_str(),
            request.address.as_str(),
            message,
        ) {
            Ok(value) => value,
            Err(_) => return RecipientContactVerificationProviderOutcome::Indeterminate,
        };
        match submission.submit().await {
            SmtpSubmissionOutcome::Accepted => {
                RecipientContactVerificationProviderOutcome::Delivered
            }
            SmtpSubmissionOutcome::PermanentRejected => {
                RecipientContactVerificationProviderOutcome::Rejected
            }
            SmtpSubmissionOutcome::TransientRejected | SmtpSubmissionOutcome::Indeterminate => {
                RecipientContactVerificationProviderOutcome::Indeterminate
            }
        }
    }
}

fn build_verification_message(
    sender: &RecipientEmailAddress,
    request: &RecipientContactVerificationDeliveryRequest,
) -> Result<Zeroizing<Vec<u8>>, String> {
    if request.proof.is_empty()
        || request.proof.len() > 4096
        || request.proof.contains(['\0', '\r', '\n'])
    {
        return Err("recipient contact verification proof is invalid".into());
    }
    let expires_at = request
        .expires_at
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut message = Zeroizing::new(Vec::with_capacity(8192));
    for part in [
        "From: ",
        sender.as_str(),
        "\r\nTo: ",
        request.address.as_str(),
        "\r\nSubject: Verify your A3S Cloud recipient contact\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "Content-Transfer-Encoding: 7bit\r\n",
        "Auto-Submitted: auto-generated\r\n\r\n",
        "A recipient contact verification was requested for your A3S Cloud account.\r\n\r\n",
        "Verification proof:\r\n",
        request.proof.as_str(),
        "\r\n\r\nExpires at: ",
        expires_at.as_str(),
        "\r\n\r\nIf you did not request this verification, ignore this message.\r\n",
    ] {
        message.extend_from_slice(part.as_bytes());
    }
    if message.len() > MAX_VERIFICATION_MESSAGE_BYTES || !message.is_ascii() {
        return Err("recipient contact verification message is invalid".into());
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::RecipientContactVerificationId;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use chrono::{Duration as ChronoDuration, Utc};
    use rcgen::{generate_simple_self_signed, CertifiedKey};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::task::JoinHandle;
    use tokio_rustls::server::TlsStream;
    use tokio_rustls::TlsAcceptor;

    #[derive(Debug, Clone, Copy)]
    enum SmtpFixtureOutcome {
        Delivered,
        Rejected,
        Indeterminate,
    }

    struct SmtpFixture {
        port: u16,
        ca_certificate_file: String,
        _directory: TempDir,
        server: JoinHandle<SmtpCapture>,
    }

    struct SmtpCapture {
        authenticated: bool,
        mail_commands: usize,
        recipient_commands: usize,
        data_commands: usize,
        message: Vec<u8>,
    }

    impl SmtpFixture {
        async fn start(outcome: SmtpFixtureOutcome) -> Self {
            let CertifiedKey { cert, key_pair } =
                generate_simple_self_signed(vec!["127.0.0.1".into()])
                    .expect("SMTP fixture certificate");
            let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("SMTP fixture TLS protocol versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
            )
            .expect("SMTP fixture TLS server");
            let directory = tempfile::tempdir().expect("SMTP fixture directory");
            let ca_path = directory.path().join("smtp-ca.pem");
            std::fs::write(&ca_path, cert.pem()).expect("SMTP fixture CA file");
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("SMTP fixture listener");
            let port = listener.local_addr().expect("SMTP fixture address").port();
            let acceptor = TlsAcceptor::from(Arc::new(server_config));
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("SMTP fixture connection");
                let tls = acceptor.accept(stream).await.expect("SMTP fixture TLS");
                serve_smtp(tls, outcome).await
            });
            Self {
                port,
                ca_certificate_file: ca_path.to_string_lossy().into_owned(),
                _directory: directory,
                server,
            }
        }

        fn service(&self) -> SmtpRecipientContactVerificationDeliveryService {
            SmtpRecipientContactVerificationDeliveryService::new(
                SmtpRecipientContactVerificationDeliveryOptions {
                    host: "127.0.0.1".into(),
                    port: self.port,
                    tls_policy: SmtpRecipientContactVerificationTlsPolicy::Implicit,
                    hello_name: "cloud.example.test".into(),
                    ca_certificate_file: self.ca_certificate_file.clone(),
                    sender: RecipientEmailAddress::parse("no-reply@example.test").expect("sender"),
                    credentials: SmtpRecipientContactVerificationCredentials {
                        username: Zeroizing::new("smtp-user".into()),
                        password: Zeroizing::new("smtp-password".into()),
                    },
                    connect_timeout: Duration::from_secs(5),
                    command_timeout: Duration::from_secs(5),
                },
            )
            .expect("SMTP service")
        }
    }

    async fn smtp_line(reader: &mut BufReader<TlsStream<TcpStream>>) -> String {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("SMTP fixture command");
        assert!(line.ends_with("\r\n"));
        line
    }

    async fn smtp_reply(reader: &mut BufReader<TlsStream<TcpStream>>, reply: &str) {
        reader
            .get_mut()
            .write_all(reply.as_bytes())
            .await
            .expect("SMTP fixture response");
        reader
            .get_mut()
            .flush()
            .await
            .expect("SMTP fixture response flush");
    }

    async fn serve_smtp(stream: TlsStream<TcpStream>, outcome: SmtpFixtureOutcome) -> SmtpCapture {
        let mut reader = BufReader::new(stream);
        smtp_reply(&mut reader, "220 smtp.fixture.test ESMTP ready\r\n").await;
        assert!(smtp_line(&mut reader).await.starts_with("EHLO "));
        smtp_reply(
            &mut reader,
            "250-smtp.fixture.test\r\n250-AUTH PLAIN\r\n250 PIPELINING\r\n",
        )
        .await;
        let auth = smtp_line(&mut reader).await;
        let encoded = auth
            .strip_prefix("AUTH PLAIN ")
            .and_then(|value| value.strip_suffix("\r\n"))
            .expect("SMTP PLAIN authentication");
        let decoded = STANDARD
            .decode(encoded)
            .expect("SMTP PLAIN authentication payload");
        assert_eq!(decoded, b"\0smtp-user\0smtp-password");
        smtp_reply(&mut reader, "235 2.7.0 authenticated\r\n").await;

        let mail = smtp_line(&mut reader).await;
        assert!(mail.starts_with("MAIL FROM:<no-reply@example.test>"));
        smtp_reply(&mut reader, "250 2.1.0 sender accepted\r\n").await;
        let recipient = smtp_line(&mut reader).await;
        assert!(recipient.starts_with("RCPT TO:<private@example.test>"));
        if matches!(outcome, SmtpFixtureOutcome::Rejected) {
            smtp_reply(&mut reader, "550 5.1.1 recipient rejected\r\n").await;
            return SmtpCapture {
                authenticated: true,
                mail_commands: 1,
                recipient_commands: 1,
                data_commands: 0,
                message: Vec::new(),
            };
        }
        smtp_reply(&mut reader, "250 2.1.5 recipient accepted\r\n").await;
        assert_eq!(smtp_line(&mut reader).await, "DATA\r\n");
        smtp_reply(&mut reader, "354 send message\r\n").await;
        let mut message = Vec::new();
        loop {
            let line = smtp_line(&mut reader).await;
            if line == ".\r\n" {
                break;
            }
            message.extend_from_slice(line.as_bytes());
        }
        if matches!(outcome, SmtpFixtureOutcome::Delivered) {
            smtp_reply(&mut reader, "250 2.0.0 queued\r\n").await;
        }
        SmtpCapture {
            authenticated: true,
            mail_commands: 1,
            recipient_commands: 1,
            data_commands: 1,
            message,
        }
    }

    fn delivery_request() -> RecipientContactVerificationDeliveryRequest {
        RecipientContactVerificationDeliveryRequest {
            verification_id: RecipientContactVerificationId::new(),
            address: RecipientEmailAddress::parse("private@example.test").expect("address"),
            proof: Zeroizing::new("a3srcv1.synthetic.proof".into()),
            expires_at: Utc::now() + ChronoDuration::minutes(10),
        }
    }

    #[test]
    fn message_is_fixed_bounded_and_sensitive_debug_is_redacted() {
        let request = RecipientContactVerificationDeliveryRequest {
            verification_id: RecipientContactVerificationId::new(),
            address: RecipientEmailAddress::parse("private@example.test").expect("address"),
            proof: Zeroizing::new("a3srcv1.payload.authenticator".into()),
            expires_at: Utc::now() + ChronoDuration::minutes(10),
        };
        let sender = RecipientEmailAddress::parse("no-reply@example.test").expect("sender");
        let message = build_verification_message(&sender, &request).expect("message");
        assert!(message.len() < MAX_VERIFICATION_MESSAGE_BYTES);
        assert!(message
            .windows(request.proof.len())
            .any(|value| value == request.proof.as_bytes()));
        let rendered = format!("{request:?}");
        assert!(!rendered.contains("private@example.test"));
        assert!(!rendered.contains(request.proof.as_str()));
    }

    #[test]
    fn adapter_debug_never_exposes_credentials_or_sender() {
        let service = SmtpRecipientContactVerificationDeliveryService::new(
            SmtpRecipientContactVerificationDeliveryOptions {
                host: "smtp.example.test".into(),
                port: 465,
                tls_policy: SmtpRecipientContactVerificationTlsPolicy::Implicit,
                hello_name: "cloud.example.test".into(),
                ca_certificate_file: String::new(),
                sender: RecipientEmailAddress::parse("private-sender@example.test")
                    .expect("sender"),
                credentials: SmtpRecipientContactVerificationCredentials {
                    username: Zeroizing::new("smtp-private-user".into()),
                    password: Zeroizing::new("smtp-private-password".into()),
                },
                connect_timeout: Duration::from_secs(5),
                command_timeout: Duration::from_secs(10),
            },
        )
        .expect("service");
        let rendered = format!("{service:?}");
        assert!(!rendered.contains("smtp-private-user"));
        assert!(!rendered.contains("smtp-private-password"));
        assert!(!rendered.contains("private-sender@example.test"));
    }

    #[tokio::test]
    async fn implicit_tls_authentication_and_one_submission_use_the_live_smtp_protocol() {
        let fixture = SmtpFixture::start(SmtpFixtureOutcome::Delivered).await;
        let outcome = fixture
            .service()
            .prepare()
            .await
            .expect("prepared authenticated SMTP session")
            .deliver(delivery_request())
            .await;
        assert_eq!(
            outcome,
            RecipientContactVerificationProviderOutcome::Delivered
        );
        let capture = fixture.server.await.expect("SMTP fixture task");
        assert!(capture.authenticated);
        assert_eq!(capture.mail_commands, 1);
        assert_eq!(capture.recipient_commands, 1);
        assert_eq!(capture.data_commands, 1);
        let message = String::from_utf8(capture.message).expect("ASCII SMTP message");
        assert!(message.contains("To: private@example.test\r\n"));
        assert!(message.contains("a3srcv1.synthetic.proof"));
    }

    #[tokio::test]
    async fn permanent_rejection_and_lost_final_reply_are_closed_terminal_outcomes() {
        for (fixture_outcome, expected) in [
            (
                SmtpFixtureOutcome::Rejected,
                RecipientContactVerificationProviderOutcome::Rejected,
            ),
            (
                SmtpFixtureOutcome::Indeterminate,
                RecipientContactVerificationProviderOutcome::Indeterminate,
            ),
        ] {
            let fixture = SmtpFixture::start(fixture_outcome).await;
            let outcome = fixture
                .service()
                .prepare()
                .await
                .expect("prepared authenticated SMTP session")
                .deliver(delivery_request())
                .await;
            assert_eq!(outcome, expected);
            let capture = fixture.server.await.expect("SMTP fixture task");
            assert_eq!(capture.mail_commands, 1);
            assert_eq!(capture.recipient_commands, 1);
            assert_eq!(
                capture.data_commands,
                usize::from(matches!(fixture_outcome, SmtpFixtureOutcome::Indeterminate))
            );
        }
    }

    #[tokio::test]
    async fn required_starttls_rejects_a_relay_that_cannot_upgrade() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("plaintext SMTP fixture listener");
        let port = listener
            .local_addr()
            .expect("plaintext SMTP fixture address")
            .port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener
                .accept()
                .await
                .expect("plaintext SMTP fixture connection");
            let mut reader = BufReader::new(stream);
            reader
                .get_mut()
                .write_all(b"220 smtp.fixture.test ESMTP ready\r\n")
                .await
                .expect("plaintext SMTP greeting");
            reader.get_mut().flush().await.expect("greeting flush");
            let mut command = String::new();
            reader
                .read_line(&mut command)
                .await
                .expect("plaintext SMTP EHLO");
            assert!(command.starts_with("EHLO "));
            reader
                .get_mut()
                .write_all(b"250 smtp.fixture.test\r\n")
                .await
                .expect("plaintext SMTP capabilities");
            reader.get_mut().flush().await.expect("capability flush");
            command.clear();
            let bytes = reader
                .read_line(&mut command)
                .await
                .expect("plaintext SMTP connection close");
            (bytes, command)
        });
        let service = SmtpRecipientContactVerificationDeliveryService::new(
            SmtpRecipientContactVerificationDeliveryOptions {
                host: "127.0.0.1".into(),
                port,
                tls_policy: SmtpRecipientContactVerificationTlsPolicy::RequiredStartTls,
                hello_name: "cloud.example.test".into(),
                ca_certificate_file: String::new(),
                sender: RecipientEmailAddress::parse("no-reply@example.test").expect("sender"),
                credentials: SmtpRecipientContactVerificationCredentials {
                    username: Zeroizing::new("smtp-user".into()),
                    password: Zeroizing::new("smtp-password".into()),
                },
                connect_timeout: Duration::from_secs(5),
                command_timeout: Duration::from_secs(5),
            },
        )
        .expect("SMTP service");
        assert!(matches!(
            service.prepare().await,
            Err(RecipientContactVerificationDeliveryPreparationError::Unavailable)
        ));
        let (bytes, command) = server.await.expect("plaintext SMTP fixture task");
        assert_eq!(bytes, 0);
        assert!(command.is_empty());
    }
}
