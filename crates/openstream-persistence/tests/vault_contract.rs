//! Cross-platform credential-vault contract (issue #8). Runs everywhere CI
//! runs; proves the shared trait surface and the honest `Unsupported`
//! capability report on platforms without a shipped backend.

#[cfg(not(target_os = "windows"))]
use openstream_domain::secret::{SecretRef, SecretValue};
use openstream_persistence::vault::{CredentialVault, platform_vault};

#[cfg(not(target_os = "windows"))]
fn reference() -> SecretRef {
    SecretRef::try_new("obs.test.contract").expect("valid structural reference")
}

#[cfg(not(target_os = "windows"))]
fn synthetic_value() -> SecretValue {
    SecretValue::try_new("osv-contract-value-4d7e".to_string()).expect("in-range synthetic value")
}

/// Mirrors the backend's static platform tag for the running host.
#[cfg(not(target_os = "windows"))]
const fn expected_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unsupported-platform"
    }
}

#[test]
fn platform_vault_is_a_debuggable_trait_object() {
    let vault = platform_vault();
    // Object safety: usable as Box<dyn CredentialVault> from the engine.
    let boxed: Box<dyn CredentialVault> = platform_vault();
    assert!(!format!("{vault:?}").is_empty());
    assert!(!format!("{boxed:?}").is_empty());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn unsupported_platform_reports_unsupported_for_every_operation() {
    use openstream_persistence::vault::VaultError;

    let vault = platform_vault();
    let reference = reference();
    let value = synthetic_value();

    let stored = vault.store(&reference, &value).unwrap_err();
    let loaded = vault.load(&reference).unwrap_err();
    let deleted = vault.delete(&reference).unwrap_err();

    for error in [stored, loaded, deleted] {
        assert_eq!(
            error,
            VaultError::Unsupported {
                platform: expected_platform()
            },
            "every operation must fail closed with an honest Unsupported report"
        );
        assert_eq!(error.operation(), None);
    }
}

#[cfg(not(target_os = "windows"))]
#[test]
fn unsupported_errors_never_carry_reference_or_value_material() {
    use openstream_persistence::vault::VaultOperation;

    let vault = platform_vault();
    let reference = reference();
    let value = synthetic_value();

    let stored = vault.store(&reference, &value).unwrap_err();
    let rendered = format!("{stored}");
    assert!(!rendered.contains(reference.as_str()));
    assert!(!rendered.contains(value.expose()));
    assert_eq!(
        VaultOperation::Store.as_str(),
        "store",
        "operation vocabulary stays stable for journaling"
    );
}
