//! Opaque DirectoryProjection subject references for Cloud-owned Grants.
//!
//! Format: `{issuer}#department/{uuid}` or `{issuer}#group/{uuid}` where
//! `issuer` is a canonical HTTPS partner directory issuer (OidcIssuer).

use super::{parse_partner_directory_issuer, OidcIssuer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryGrantSubjectKind {
    Department,
    Group,
}

impl DirectoryGrantSubjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Department => "department",
            Self::Group => "group",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "department" => Ok(Self::Department),
            "group" => Ok(Self::Group),
            _ => Err("directory grant subject kind must be `department` or `group`".into()),
        }
    }
}

/// Opaque department/group subject reference admitted by DirectoryProjection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DirectoryGrantSubjectRef {
    kind: DirectoryGrantSubjectKind,
    issuer: OidcIssuer,
    subject_id: Uuid,
}

impl DirectoryGrantSubjectRef {
    pub fn new(kind: DirectoryGrantSubjectKind, issuer: OidcIssuer, subject_id: Uuid) -> Self {
        Self {
            kind,
            issuer,
            subject_id,
        }
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.contains('@') {
            return Err(
                "directory grant subject ref must not use email keys; use opaque UUID subjects"
                    .into(),
            );
        }
        let (issuer_raw, rest) = value.split_once('#').ok_or_else(|| {
            "directory grant subject ref must be `{issuer}#department/{uuid}` or `{issuer}#group/{uuid}`"
                .to_owned()
        })?;
        let issuer = parse_partner_directory_issuer(issuer_raw)?;
        let (kind, id_raw) = if let Some(id) = rest.strip_prefix("department/") {
            (DirectoryGrantSubjectKind::Department, id)
        } else if let Some(id) = rest.strip_prefix("group/") {
            (DirectoryGrantSubjectKind::Group, id)
        } else {
            return Err(
                "directory grant subject ref path must be `department/{uuid}` or `group/{uuid}`"
                    .into(),
            );
        };
        if id_raw.is_empty() || id_raw.contains('/') {
            return Err("directory grant subject id must be a single canonical UUID".into());
        }
        let subject_id = Uuid::parse_str(id_raw).map_err(|_| {
            "directory grant subject id must be a canonical UUID string (no email keys)".to_owned()
        })?;
        Ok(Self::new(kind, issuer, subject_id))
    }

    pub fn format_ref(&self) -> String {
        format!(
            "{}#{}/{}",
            self.issuer.as_str(),
            self.kind.as_str(),
            self.subject_id
        )
    }

    pub const fn kind(&self) -> DirectoryGrantSubjectKind {
        self.kind
    }

    pub fn issuer(&self) -> &OidcIssuer {
        &self.issuer
    }

    pub const fn subject_id(&self) -> Uuid {
        self.subject_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_department_and_group_refs() {
        let department = DirectoryGrantSubjectRef::parse(format!(
            "https://kense.example/directory#department/{}",
            Uuid::nil()
        ))
        .expect("department");
        assert_eq!(department.kind(), DirectoryGrantSubjectKind::Department);
        assert_eq!(department.issuer().as_str(), "https://kense.example/directory");
        assert_eq!(department.subject_id(), Uuid::nil());
        assert_eq!(
            DirectoryGrantSubjectRef::parse(department.format_ref()).expect("round-trip"),
            department
        );

        let group_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let group = DirectoryGrantSubjectRef::parse(format!(
            "https://kense.example/directory#group/{group_id}"
        ))
        .expect("group");
        assert_eq!(group.kind(), DirectoryGrantSubjectKind::Group);
        assert_eq!(group.subject_id(), group_id);
        assert_eq!(
            DirectoryGrantSubjectRef::parse(group.format_ref()).expect("round-trip"),
            group
        );
    }

    #[test]
    fn rejects_email_and_malformed_refs() {
        assert!(DirectoryGrantSubjectRef::parse("user@example.com").is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "https://kense.example/directory#department/user@example.com"
        )
        .is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "https://kense.example/directory#member/550e8400-e29b-41d4-a716-446655440000"
        )
        .is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "http://insecure.example/directory#department/550e8400-e29b-41d4-a716-446655440000"
        )
        .is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "https://kense.example/directory#department/"
        )
        .is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "https://kense.example/directory#department/550e8400-e29b-41d4-a716-446655440000/extra"
        )
        .is_err());
        assert!(DirectoryGrantSubjectRef::parse(
            "not-an-issuer#department/550e8400-e29b-41d4-a716-446655440000"
        )
        .is_err());
    }
}
