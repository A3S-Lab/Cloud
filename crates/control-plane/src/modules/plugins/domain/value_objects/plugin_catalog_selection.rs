use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_use_core::{PluginPackageId, PluginSurfaceRef};
use serde::{Deserialize, Serialize};

/// Exact verified catalog selection retained by one Cloud assignment generation.
///
/// Digests and the package identity come from a trusted A3S Use catalog record.
/// Cloud never stores package bytes or a second catalog schema here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogSelection {
    pub package_id: PluginPackageId,
    pub catalog_record_digest: Sha256Digest,
    pub version: String,
    pub package_digest: Sha256Digest,
    pub manifest_digest: Sha256Digest,
    pub selected_surfaces: Vec<PluginSurfaceRef>,
}

impl PluginCatalogSelection {
    pub fn validate(&self) -> Result<(), String> {
        if self.version.trim().is_empty()
            || self.version.len() > 128
            || self.selected_surfaces.is_empty()
            || self.selected_surfaces.len() > 256
        {
            return Err("plugin catalog selection version or surfaces are invalid".into());
        }
        let mut seen = self.selected_surfaces.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != self.selected_surfaces.len() {
            return Err("plugin catalog selection surfaces must be unique".into());
        }
        for surface in &self.selected_surfaces {
            if surface.id.trim().is_empty() || surface.id.len() > 128 {
                return Err("plugin catalog selection surface id is invalid".into());
            }
        }
        PluginPackageId::parse(self.package_id.as_str())
            .map_err(|_| "plugin catalog selection package id is invalid".to_owned())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PluginCatalogSelection;
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use a3s_use_core::{PluginPackageId, PluginSurfaceKind, PluginSurfaceRef};

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn selection() -> PluginCatalogSelection {
        PluginCatalogSelection {
            package_id: PluginPackageId::parse("a3s/registry-selftest").expect("package"),
            catalog_record_digest: digest('a'),
            version: "0.1.0".into(),
            package_digest: digest('b'),
            manifest_digest: digest('c'),
            selected_surfaces: vec![PluginSurfaceRef {
                kind: PluginSurfaceKind::Skill,
                id: "selftest".into(),
            }],
        }
    }

    #[test]
    fn accepts_one_exact_signed_catalog_selection() {
        selection().validate().expect("selection");
    }

    #[test]
    fn rejects_empty_or_duplicate_surfaces() {
        let mut empty = selection();
        empty.selected_surfaces.clear();
        assert!(empty.validate().is_err());

        let mut duplicate = selection();
        duplicate.selected_surfaces.push(PluginSurfaceRef {
            kind: PluginSurfaceKind::Skill,
            id: "selftest".into(),
        });
        assert!(duplicate.validate().is_err());
    }
}
