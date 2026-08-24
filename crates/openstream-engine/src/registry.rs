//! Action registry: typed registration with capability-scope declarations.
//!
//! Every executable action type registers exactly once against the runtime
//! (`TECHNICAL_SPEC` §3: `openstream-engine` owns the deterministic action
//! graph; adapter contracts stay typed and closed). A registration declares:
//!
//! - its **capability scopes** — the exact capabilities it is able to
//!   exercise (the manifest layer of the taxonomy §2 intersection); graphs
//!   may only request capabilities covered by a declared scope, and grants
//!   are evaluated against these declarations immediately before dispatch;
//! - its **idempotency class** — required by `retry` nodes and by replay
//!   after `outcome_unknown` (`OSCP_MESSAGES.md` §7);
//! - whether the adapter declares **safe compensation** — required before
//!   failure policy `compensate` or any compensation link may run
//!   (`TECHNICAL_SPEC` §5).
//!
//! Internal-only capabilities (`secret.*`, taxonomy §4) reject at
//! registration: they are never manifest-declarable.

use crate::error::ConfigError;
use crate::port::EffectPort;
use openstream_domain::capability::Capability;
use openstream_domain::grant::ManifestDeclaration;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Maximum byte length of one registered action name.
pub const MAX_ACTION_NAME_BYTES: usize = 64;

/// Adapter-declared idempotency class. Non-idempotent adapters receive no
/// automatic retry and no replay after `outcome_unknown`
/// (`PROTOCOL.md`; `OSCP_MESSAGES.md` §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdempotencyClass {
    /// The effect may not be safely repeated; OpenStream claims no
    /// exactly-once behavior for it.
    #[default]
    NonIdempotent,
    /// The adapter accepts a stable idempotency key and collapses duplicate
    /// applications itself.
    Idempotent,
}

impl IdempotencyClass {
    /// True when the adapter declared idempotency.
    #[must_use]
    pub const fn is_declared(self) -> bool {
        matches!(self, Self::Idempotent)
    }
}

impl fmt::Display for IdempotencyClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonIdempotent => "non_idempotent",
            Self::Idempotent => "idempotent",
        })
    }
}

/// One typed registration: name, capability scopes, idempotency class,
/// safe-compensation declaration, and the port that performs effects.
#[derive(Debug, Clone)]
pub struct ActionRegistration {
    name: String,
    scopes: Vec<Capability>,
    manifest: ManifestDeclaration,
    idempotency: IdempotencyClass,
    safe_compensation: bool,
    port: Arc<dyn EffectPort>,
}

impl ActionRegistration {
    /// Validates and assembles a registration.
    ///
    /// # Errors
    /// [`ConfigError::InvalidActionName`] off-grammar names;
    /// [`ConfigError::InternalCapabilityScope`] when any scope is an
    /// internal-only capability.
    pub fn try_new(
        name: &str,
        scopes: Vec<Capability>,
        idempotency: IdempotencyClass,
        safe_compensation: bool,
        port: Arc<dyn EffectPort>,
    ) -> Result<Self, ConfigError> {
        if !crate::identifiers::validate_identifier(name, true, MAX_ACTION_NAME_BYTES) {
            return Err(ConfigError::InvalidActionName);
        }
        if scopes.iter().any(Capability::is_internal) {
            return Err(ConfigError::InternalCapabilityScope);
        }
        let manifest = ManifestDeclaration::try_new(scopes.clone())
            .map_err(|_| ConfigError::InternalCapabilityScope)?;
        Ok(Self {
            name: name.to_string(),
            scopes,
            manifest,
            idempotency,
            safe_compensation,
            port,
        })
    }

    /// The registered action name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Declared capability scopes in declaration order.
    #[must_use]
    pub fn scopes(&self) -> &[Capability] {
        &self.scopes
    }

    /// Precomputed manifest declaration backing grant evaluation.
    #[must_use]
    pub const fn manifest(&self) -> &ManifestDeclaration {
        &self.manifest
    }

    /// Declared idempotency class.
    #[must_use]
    pub const fn idempotency(&self) -> IdempotencyClass {
        self.idempotency
    }

    /// Whether the adapter declared safe compensation.
    #[must_use]
    pub const fn safe_compensation(&self) -> bool {
        self.safe_compensation
    }

    /// The dispatch port.
    #[must_use]
    pub fn port(&self) -> Arc<dyn EffectPort> {
        Arc::clone(&self.port)
    }

    /// True when some declared scope covers `requested` (same kind, request
    /// qualifiers within scope restrictions).
    #[must_use]
    pub fn scopes_cover(&self, requested: &Capability) -> bool {
        self.scopes.iter().any(|scope| scope.covers(requested))
    }
}

/// Closed set of registered action types. Duplicate names reject;
/// iteration order is deterministic (name-sorted). Cloning shares ports.
#[derive(Debug, Clone, Default)]
pub struct ActionRegistry {
    entries: BTreeMap<String, ActionRegistration>,
}

impl ActionRegistry {
    /// Empty registry: every graph referencing an action type rejects at
    /// validation until something registers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one action type.
    ///
    /// # Errors
    /// [`ConfigError::DuplicateActionName`] on re-registration;
    /// [`ActionRegistration::try_new`] failures otherwise.
    pub fn register(&mut self, registration: ActionRegistration) -> Result<(), ConfigError> {
        if self.entries.contains_key(registration.name()) {
            return Err(ConfigError::DuplicateActionName);
        }
        self.entries
            .insert(registration.name().to_string(), registration);
        Ok(())
    }

    /// Looks up a registration by action name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&ActionRegistration> {
        self.entries.get(name)
    }

    /// Number of registered action types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Registered names in sorted order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}
