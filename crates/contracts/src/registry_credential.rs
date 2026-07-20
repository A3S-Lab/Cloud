use serde::Deserialize;
use zeroize::Zeroize;

const MAXIMUM_MATERIAL_BYTES: usize = 32 * 1024;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryCredentialMaterial {
    schema: String,
    username: String,
    password: String,
}

impl RegistryCredentialMaterial {
    pub const SCHEMA: &'static str = "a3s.cloud.registry-credential.v1";

    pub fn parse(material: &[u8]) -> Result<Self, String> {
        if material.is_empty() || material.len() > MAXIMUM_MATERIAL_BYTES {
            return Err(invalid_material());
        }
        let credential: Self = serde_json::from_slice(material).map_err(|_| invalid_material())?;
        credential.validate()?;
        Ok(credential)
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA
            || !valid_field(&self.username, 255)
            || self.username.contains(':')
            || !valid_field(&self.password, 16 * 1024)
        {
            return Err(invalid_material());
        }
        Ok(())
    }
}

impl std::fmt::Debug for RegistryCredentialMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted-registry-credential>")
    }
}

impl Drop for RegistryCredentialMaterial {
    fn drop(&mut self) {
        self.schema.zeroize();
        self.username.zeroize();
        self.password.zeroize();
    }
}

fn valid_field(value: &str, maximum_length: usize) -> bool {
    !value.is_empty() && value.len() <= maximum_length && !value.contains(['\0', '\r', '\n'])
}

fn invalid_material() -> String {
    "registry credential Secret material is invalid".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_is_strict_bounded_and_redacted() {
        let credential = RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v1","username":"registry-user","password":"registry-password"}"#,
        )
        .expect("registry credential");
        assert_eq!(credential.username(), "registry-user");
        assert_eq!(credential.password(), "registry-password");
        assert_eq!(format!("{credential:?}"), "<redacted-registry-credential>");

        let error = RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v1","username":"registry-user","password":"never-leak-this","extra":true}"#,
        )
        .expect_err("unknown credential field");
        assert!(!format!("{error:?}").contains("never-leak-this"));
        assert!(RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v0","username":"registry-user","password":"registry-password"}"#,
        )
        .is_err());
        assert!(RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v1","username":"invalid:user","password":"registry-password"}"#,
        )
        .is_err());
        assert!(
            RegistryCredentialMaterial::parse(&vec![b'x'; MAXIMUM_MATERIAL_BYTES + 1]).is_err()
        );
    }
}
