use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitReference, GitRepository, GithubAccountId, GithubAccountKind,
    GithubConnectionLifecycleChange, GithubInstallationAccount, GithubInstallationId, GithubLogin,
    ISourceWebhookVerifier, SourceWebhookVerificationError, SourceWebhookVerificationRequest,
    VerifiedGithubConnectionLifecycle, VerifiedSourcePush, VerifiedSourceWebhook,
    WebhookDeliveryId,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

const MINIMUM_SECRET_BYTES: usize = 32;
const MAXIMUM_SECRET_BYTES: usize = 512;

pub struct GithubWebhookVerifier {
    secret: WebhookSecret,
    maximum_body_bytes: usize,
}

enum WebhookSecret {
    Environment(String),
    #[cfg(test)]
    Fixed(Zeroizing<String>),
}

impl GithubWebhookVerifier {
    pub fn new(
        secret_environment: impl Into<String>,
        maximum_body_bytes: usize,
    ) -> Result<Self, String> {
        let secret_environment = secret_environment.into();
        if !valid_environment_name(&secret_environment) {
            return Err(
                "GitHub webhook secret reference must be an uppercase environment variable name"
                    .into(),
            );
        }
        validate_body_limit(maximum_body_bytes)?;
        Ok(Self {
            secret: WebhookSecret::Environment(secret_environment),
            maximum_body_bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(secret: &str, maximum_body_bytes: usize) -> Result<Self, String> {
        validate_secret(secret)?;
        validate_body_limit(maximum_body_bytes)?;
        Ok(Self {
            secret: WebhookSecret::Fixed(Zeroizing::new(secret.to_owned())),
            maximum_body_bytes,
        })
    }

    fn secret(&self) -> Result<Zeroizing<String>, SourceWebhookVerificationError> {
        let value = match &self.secret {
            WebhookSecret::Environment(name) => std::env::var(name).map_err(|_| {
                SourceWebhookVerificationError::Unavailable(
                    "configured secret material is not available".into(),
                )
            })?,
            #[cfg(test)]
            WebhookSecret::Fixed(value) => value.to_string(),
        };
        validate_secret(&value).map_err(SourceWebhookVerificationError::Unavailable)?;
        Ok(Zeroizing::new(value))
    }

    fn authenticate(
        &self,
        signature: &str,
        body: &[u8],
    ) -> Result<(), SourceWebhookVerificationError> {
        let signature =
            decode_signature(signature).ok_or(SourceWebhookVerificationError::Authentication)?;
        let secret = self.secret()?;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| {
            SourceWebhookVerificationError::Unavailable(
                "configured secret material is invalid".into(),
            )
        })?;
        mac.update(body);
        mac.verify_slice(&signature)
            .map_err(|_| SourceWebhookVerificationError::Authentication)
    }

    fn parse_push(
        &self,
        delivery_id: &str,
        body: &[u8],
    ) -> Result<VerifiedSourceWebhook, SourceWebhookVerificationError> {
        let payload: GithubPushPayload = serde_json::from_slice(body)
            .map_err(|_| invalid("body is not a valid GitHub push payload"))?;
        if payload.deleted {
            return Ok(VerifiedSourceWebhook::Ignored);
        }
        let Some(branch) = payload.git_reference.strip_prefix("refs/heads/") else {
            return Ok(VerifiedSourceWebhook::Ignored);
        };
        let delivery_id =
            WebhookDeliveryId::parse(delivery_id).map_err(|_| invalid("delivery ID is invalid"))?;
        let repository = GitRepository::parse(GitProvider::Github, &payload.repository.html_url)
            .map_err(|_| invalid("repository URL is invalid"))?;
        let (owner, name) = repository
            .owner_and_name()
            .ok_or_else(|| invalid("repository coordinates are unavailable"))?;
        if !payload
            .repository
            .full_name
            .eq_ignore_ascii_case(&format!("{owner}/{name}"))
        {
            return Err(invalid("repository identity does not match its URL"));
        }
        let installation_id = GithubInstallationId::parse(payload.installation.id)
            .map_err(|_| invalid("installation ID is invalid"))?;
        let reference = GitReference::parse("branch", branch.to_owned())
            .map_err(|_| invalid("branch reference is invalid"))?;
        let commit_sha = GitCommitSha::parse(payload.after)
            .map_err(|_| invalid("commit object ID is invalid"))?;
        if commit_sha.as_str().bytes().all(|byte| byte == b'0') {
            return Err(invalid("commit object ID cannot be the deletion sentinel"));
        }
        Ok(VerifiedSourceWebhook::Push(VerifiedSourcePush {
            provider: GitProvider::Github,
            delivery_id,
            repository,
            installation_id,
            reference,
            commit_sha,
            payload_digest: payload_digest(body),
        }))
    }

    fn parse_connection_lifecycle(
        &self,
        event: &str,
        delivery_id: &str,
        body: &[u8],
    ) -> Result<VerifiedSourceWebhook, SourceWebhookVerificationError> {
        let action: GithubActionPayload = serde_json::from_slice(body)
            .map_err(|_| invalid("body is not a valid GitHub lifecycle payload"))?;
        let change = match (event, action.action.as_str()) {
            ("installation", "suspend" | "unsuspend" | "deleted") => {
                let payload: GithubInstallationLifecyclePayload = serde_json::from_slice(body)
                    .map_err(|_| invalid("GitHub installation lifecycle payload is invalid"))?;
                let installation_id = GithubInstallationId::parse(payload.installation.id)
                    .map_err(|_| invalid("installation ID is invalid"))?;
                let account = parse_account(payload.installation.account)?;
                match payload.action.as_str() {
                    "suspend" => GithubConnectionLifecycleChange::InstallationSuspended {
                        installation_id,
                        account,
                    },
                    "unsuspend" => GithubConnectionLifecycleChange::InstallationUnsuspended {
                        installation_id,
                        account,
                    },
                    "deleted" => GithubConnectionLifecycleChange::InstallationDeleted {
                        installation_id,
                        account,
                    },
                    _ => {
                        return Err(invalid(
                            "GitHub installation lifecycle action changed while decoding",
                        ))
                    }
                }
            }
            ("installation_target", "renamed") => {
                let payload: GithubInstallationTargetPayload = serde_json::from_slice(body)
                    .map_err(|_| invalid("GitHub installation-target payload is invalid"))?;
                let installation_id = GithubInstallationId::parse(payload.installation.id)
                    .map_err(|_| invalid("installation ID is invalid"))?;
                let target_kind = GithubAccountKind::parse(&payload.target_type)
                    .map_err(|_| invalid("installation target type is invalid"))?;
                let account = parse_account(payload.account)?;
                if account.kind != target_kind {
                    return Err(invalid(
                        "installation target type does not match the account identity",
                    ));
                }
                GithubConnectionLifecycleChange::InstallationTargetRenamed {
                    installation_id,
                    account,
                    previous_login: GithubLogin::parse(payload.changes.login.from)
                        .map_err(|_| invalid("previous installation login is invalid"))?,
                }
            }
            ("github_app_authorization", "revoked") => {
                let payload: GithubAuthorizationLifecyclePayload = serde_json::from_slice(body)
                    .map_err(|_| invalid("GitHub authorization lifecycle payload is invalid"))?;
                GithubConnectionLifecycleChange::UserAuthorizationRevoked {
                    user_id: GithubAccountId::parse(payload.sender.id)
                        .map_err(|_| invalid("verifying user ID is invalid"))?,
                    user_login: GithubLogin::parse(payload.sender.login)
                        .map_err(|_| invalid("verifying user login is invalid"))?,
                }
            }
            ("installation" | "installation_target" | "github_app_authorization", _) => {
                return Ok(VerifiedSourceWebhook::Ignored)
            }
            _ => return Ok(VerifiedSourceWebhook::Ignored),
        };
        Ok(VerifiedSourceWebhook::GithubConnectionLifecycle(
            VerifiedGithubConnectionLifecycle {
                provider: GitProvider::Github,
                delivery_id: WebhookDeliveryId::parse(delivery_id)
                    .map_err(|_| invalid("delivery ID is invalid"))?,
                change,
                payload_digest: payload_digest(body),
            },
        ))
    }
}

impl fmt::Debug for GithubWebhookVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubWebhookVerifier")
            .field("maximum_body_bytes", &self.maximum_body_bytes)
            .finish_non_exhaustive()
    }
}

impl ISourceWebhookVerifier for GithubWebhookVerifier {
    fn verify(
        &self,
        request: SourceWebhookVerificationRequest<'_>,
    ) -> Result<VerifiedSourceWebhook, SourceWebhookVerificationError> {
        if request.body.len() > self.maximum_body_bytes {
            return Err(SourceWebhookVerificationError::PayloadTooLarge {
                maximum_bytes: self.maximum_body_bytes,
            });
        }
        self.authenticate(request.signature, request.body)?;
        match request.event {
            "push" => self.parse_push(request.delivery_id, request.body),
            "installation" | "installation_target" | "github_app_authorization" => {
                self.parse_connection_lifecycle(request.event, request.delivery_id, request.body)
            }
            _ => Ok(VerifiedSourceWebhook::Ignored),
        }
    }
}

#[derive(Deserialize)]
struct GithubActionPayload {
    action: String,
}

#[derive(Deserialize)]
struct GithubInstallationLifecyclePayload {
    action: String,
    installation: GithubLifecycleInstallation,
}

#[derive(Deserialize)]
struct GithubLifecycleInstallation {
    id: u64,
    account: GithubLifecycleAccount,
}

#[derive(Deserialize)]
struct GithubLifecycleInstallationIdentity {
    id: u64,
}

#[derive(Deserialize)]
struct GithubLifecycleAccount {
    id: u64,
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct GithubInstallationTargetPayload {
    installation: GithubLifecycleInstallationIdentity,
    account: GithubLifecycleAccount,
    changes: GithubLoginChanges,
    target_type: String,
}

#[derive(Deserialize)]
struct GithubLoginChanges {
    login: GithubPreviousLogin,
}

#[derive(Deserialize)]
struct GithubPreviousLogin {
    from: String,
}

#[derive(Deserialize)]
struct GithubAuthorizationLifecyclePayload {
    sender: GithubLifecycleUser,
}

#[derive(Deserialize)]
struct GithubLifecycleUser {
    id: u64,
    login: String,
}

#[derive(Deserialize)]
struct GithubPushPayload {
    #[serde(rename = "ref")]
    git_reference: String,
    after: String,
    deleted: bool,
    repository: GithubRepositoryPayload,
    installation: GithubInstallationPayload,
}

#[derive(Deserialize)]
struct GithubRepositoryPayload {
    full_name: String,
    html_url: String,
}

#[derive(Deserialize)]
struct GithubInstallationPayload {
    id: u64,
}

fn parse_account(
    account: GithubLifecycleAccount,
) -> Result<GithubInstallationAccount, SourceWebhookVerificationError> {
    Ok(GithubInstallationAccount {
        id: GithubAccountId::parse(account.id)
            .map_err(|_| invalid("installation account ID is invalid"))?,
        login: GithubLogin::parse(account.login)
            .map_err(|_| invalid("installation account login is invalid"))?,
        kind: GithubAccountKind::parse(&account.kind)
            .map_err(|_| invalid("installation account type is invalid"))?,
    })
}

fn payload_digest(body: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(body))
}

fn decode_signature(value: &str) -> Option<[u8; 32]> {
    let encoded = value.strip_prefix("sha256=")?;
    if encoded.len() != 64 {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = hex_nibble(pair[0])?
            .checked_mul(16)?
            .checked_add(hex_nibble(pair[1])?)?;
    }
    Some(decoded)
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn validate_secret(value: &str) -> Result<(), String> {
    if !(MINIMUM_SECRET_BYTES..=MAXIMUM_SECRET_BYTES).contains(&value.len()) {
        return Err(format!(
            "GitHub webhook secret must contain {MINIMUM_SECRET_BYTES} to {MAXIMUM_SECRET_BYTES} bytes"
        ));
    }
    Ok(())
}

fn validate_body_limit(value: usize) -> Result<(), String> {
    if !(1024..=2 * 1024 * 1024).contains(&value) {
        return Err("GitHub webhook body limit must be between 1024 bytes and 2 MiB".into());
    }
    Ok(())
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn invalid(message: impl Into<String>) -> SourceWebhookVerificationError {
    SourceWebhookVerificationError::Invalid(message.into())
}

#[cfg(test)]
#[path = "github_webhook_verifier_tests.rs"]
mod tests;
