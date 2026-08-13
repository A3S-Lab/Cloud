/// Stable capability inventory categories required by the v1 parity baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AppPlatformCapabilityCategory {
    ApplicationMode,
    AuthoringToolkit,
    Node,
    Plugin,
    Knowledge,
    PublicationChannel,
    Monitoring,
    Enterprise,
}

impl AppPlatformCapabilityCategory {
    /// Returns the canonical ACL value for this category.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApplicationMode => "application_mode",
            Self::AuthoringToolkit => "authoring_toolkit",
            Self::Node => "node",
            Self::Plugin => "plugin",
            Self::Knowledge => "knowledge",
            Self::PublicationChannel => "publication_channel",
            Self::Monitoring => "monitoring",
            Self::Enterprise => "enterprise",
        }
    }

    pub(super) const fn id_prefix(self) -> &'static str {
        match self {
            Self::ApplicationMode => "application.",
            Self::AuthoringToolkit => "toolkit.",
            Self::Node => "node.",
            Self::Plugin => "plugin.",
            Self::Knowledge => "knowledge.",
            Self::PublicationChannel => "publication.",
            Self::Monitoring => "monitoring.",
            Self::Enterprise => "enterprise.",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "application_mode" => Ok(Self::ApplicationMode),
            "authoring_toolkit" => Ok(Self::AuthoringToolkit),
            "node" => Ok(Self::Node),
            "plugin" => Ok(Self::Plugin),
            "knowledge" => Ok(Self::Knowledge),
            "publication_channel" => Ok(Self::PublicationChannel),
            "monitoring" => Ok(Self::Monitoring),
            "enterprise" => Ok(Self::Enterprise),
            _ => Err(format!(
                "unknown application-platform capability category {value:?}"
            )),
        }
    }
}

/// Delivery state of a gate referenced by one or more parity capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPlatformGateState {
    Planned,
    InProgress,
    Implemented,
    Verified,
}

impl AppPlatformGateState {
    /// Returns the canonical ACL value for this state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "in_progress",
            Self::Implemented => "implemented",
            Self::Verified => "verified",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "planned" => Ok(Self::Planned),
            "in_progress" => Ok(Self::InProgress),
            "implemented" => Ok(Self::Implemented),
            "verified" => Ok(Self::Verified),
            _ => Err(format!("unknown application-platform gate state {value:?}")),
        }
    }
}

/// Visibility of one capability at the frozen baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPlatformCapabilityAvailability {
    Unavailable,
    Internal,
    Public,
}

impl AppPlatformCapabilityAvailability {
    /// Returns the canonical ACL value for this availability.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
            Self::Public => "public",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "unavailable" => Ok(Self::Unavailable),
            "internal" => Ok(Self::Internal),
            "public" => Ok(Self::Public),
            _ => Err(format!(
                "unknown application-platform capability availability {value:?}"
            )),
        }
    }
}

/// One immutable public source used to define this comparison baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPlatformReference {
    pub(super) id: String,
    pub(super) observed_on: String,
    pub(super) url: String,
}

impl AppPlatformReference {
    /// Returns the stable source identifier referenced by capability entries.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the date on which this source contributed to the frozen baseline.
    pub fn observed_on(&self) -> &str {
        &self.observed_on
    }

    /// Returns the exact public source URL frozen into this manifest revision.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// One roadmap gate referenced by the parity inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPlatformGate {
    pub(super) id: String,
    pub(super) state: AppPlatformGateState,
    pub(super) evidence: Vec<String>,
}

impl AppPlatformGate {
    /// Returns the roadmap gate identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the current gate state frozen into this manifest revision.
    pub const fn state(&self) -> AppPlatformGateState {
        self.state
    }

    /// Returns typed repository evidence references.
    pub fn evidence(&self) -> &[String] {
        &self.evidence
    }
}

/// One application-platform capability and its single semantic owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPlatformCapability {
    pub(super) id: String,
    pub(super) category: AppPlatformCapabilityCategory,
    pub(super) label: String,
    pub(super) owner: String,
    pub(super) gate: String,
    pub(super) dependencies: Vec<String>,
    pub(super) availability: AppPlatformCapabilityAvailability,
    pub(super) evidence: Vec<String>,
    pub(super) references: Vec<String>,
}

impl AppPlatformCapability {
    /// Returns the stable capability identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the baseline inventory category.
    pub const fn category(&self) -> AppPlatformCapabilityCategory {
        self.category
    }

    /// Returns the public display label recorded by the baseline.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the sole semantic owner.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Returns the owning delivery gate.
    pub fn gate(&self) -> &str {
        &self.gate
    }

    /// Returns additional gate dependencies.
    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    /// Returns the capability visibility frozen into this manifest revision.
    pub const fn availability(&self) -> AppPlatformCapabilityAvailability {
        self.availability
    }

    /// Returns typed repository evidence references.
    pub fn evidence(&self) -> &[String] {
        &self.evidence
    }

    /// Returns frozen public-reference identifiers supporting this inventory item.
    pub fn references(&self) -> &[String] {
        &self.references
    }
}

/// Strict, canonical A3S ACL representation of the frozen parity baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPlatformParityManifest {
    pub(super) baseline: String,
    pub(super) public_claim_gate: String,
    pub(super) parity_claim: bool,
    pub(super) references: Vec<AppPlatformReference>,
    pub(super) gates: Vec<AppPlatformGate>,
    pub(super) capabilities: Vec<AppPlatformCapability>,
    pub(super) canonical_acl: String,
    pub(super) digest: String,
}

impl AppPlatformParityManifest {
    /// Returns the reference-product baseline date.
    pub fn baseline(&self) -> &str {
        &self.baseline
    }

    /// Returns the gate that authorizes a full public parity claim.
    pub fn public_claim_gate(&self) -> &str {
        &self.public_claim_gate
    }

    /// Reports whether this manifest revision makes the complete parity claim.
    pub const fn parity_claim(&self) -> bool {
        self.parity_claim
    }

    /// Returns the frozen public-source registry in stable identifier order.
    pub fn references(&self) -> &[AppPlatformReference] {
        &self.references
    }

    /// Returns every referenced roadmap gate in stable identifier order.
    pub fn gates(&self) -> &[AppPlatformGate] {
        &self.gates
    }

    /// Returns the exact required capability inventory in stable identifier order.
    pub fn capabilities(&self) -> &[AppPlatformCapability] {
        &self.capabilities
    }

    /// Returns canonical ACL bytes normalized to LF line endings.
    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    /// Returns the canonical semantic SHA-256 digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
