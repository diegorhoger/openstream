//! Integration tests for the real Windows Credential Manager backend
//! (issue #8). These run only on Windows and exercise the live OS store;
//! every entry uses a unique synthetic reference and is cleaned up.
//! No fixture contains real credentials.

#![cfg(windows)]

use openstream_domain::secret::{SecretRef, SecretValue};
use openstream_persistence::vault::{
    CredentialVault, VaultError, VaultOperation, WindowsCredentialVault,
};
use uuid::Uuid;

/// Unique per-run synthetic reference under the test namespace; the run ID
/// keeps parallel/failed runs from colliding or cleaning each other up.
/// Segments must start with a letter, hence the `r` prefix on the hex id.
fn unique_ref(label: &str) -> SecretRef {
    let run = Uuid::now_v7();
    SecretRef::try_new(&format!("obs.test.{label}.r{}", run.simple())).expect("valid test ref")
}

fn value(material: &str) -> SecretValue {
    SecretValue::try_new(material.to_string()).expect("in-range synthetic value")
}

#[test]
fn store_load_delete_round_trip_against_credential_manager() {
    let vault = WindowsCredentialVault::new();
    let reference = unique_ref("roundtrip");
    // Cleanup even on assertion failure paths below.
    let cleanup = DropDelete {
        vault,
        reference: reference.clone(),
    };

    vault
        .store(&reference, &value("osv-it-value-9f3a"))
        .expect("store into Windows Credential Manager");
    let loaded = vault.load(&reference).expect("load back");
    assert_eq!(loaded.expose(), "osv-it-value-9f3a");

    vault.delete(&reference).expect("delete");
    drop(cleanup);
}

#[test]
fn missing_entry_loads_as_not_found_and_double_delete_fails() {
    let vault = WindowsCredentialVault::new();
    let reference = unique_ref("missing");

    assert_eq!(
        vault.load(&reference).unwrap_err(),
        VaultError::NotFound {
            operation: VaultOperation::Load,
        }
    );
    // Deleting a never-stored entry is an honest NotFound, not silent Ok.
    assert_eq!(
        vault.delete(&reference).unwrap_err(),
        VaultError::NotFound {
            operation: VaultOperation::Delete,
        }
    );
}

#[test]
fn overwrite_keeps_only_the_latest_value() {
    let vault = WindowsCredentialVault::new();
    let reference = unique_ref("overwrite");
    let cleanup = DropDelete {
        vault,
        reference: reference.clone(),
    };

    vault
        .store(&reference, &value("osv-it-first-1a2b"))
        .unwrap();
    vault
        .store(&reference, &value("osv-it-second-3c4d"))
        .unwrap();
    let loaded = vault.load(&reference).unwrap();
    assert_eq!(loaded.expose(), "osv-it-second-3c4d");

    drop(cleanup);
}

#[test]
fn distinct_references_do_not_collide() {
    let vault = WindowsCredentialVault::new();
    let a = unique_ref("collide");
    let b = unique_ref("collide");
    let cleanup_a = DropDelete {
        vault,
        reference: a.clone(),
    };
    let cleanup_b = DropDelete {
        vault,
        reference: b.clone(),
    };

    vault.store(&a, &value("osv-it-a-5e6f")).unwrap();
    let loaded_b = vault.load(&b);
    assert!(
        matches!(loaded_b, Err(VaultError::NotFound { .. })),
        "distinct references must map to distinct entries"
    );

    drop(cleanup_a);
    drop(cleanup_b);
}

#[test]
fn empty_values_are_rejected_before_touching_the_store() {
    let vault = WindowsCredentialVault::new();
    let reference = unique_ref("empty");
    assert!(SecretValue::try_new(String::new()).is_err());
    // Nothing reached the OS store, so loading reports plain NotFound.
    assert_eq!(
        vault.load(&reference).unwrap_err(),
        VaultError::NotFound {
            operation: VaultOperation::Load,
        }
    );
}

#[test]
fn oversized_values_are_rejected_by_the_shared_bound() {
    let vault = WindowsCredentialVault::new();
    let reference = unique_ref("oversize");
    let oversized = "x".repeat(openstream_domain::limits::MAX_SECRET_VALUE_BYTES + 1);
    assert!(SecretValue::try_new(oversized).is_err());
    assert_eq!(
        vault.load(&reference).unwrap_err(),
        VaultError::NotFound {
            operation: VaultOperation::Load,
        }
    );
}

/// Best-effort deletion guard so failed tests do not leave entries behind.
struct DropDelete {
    vault: WindowsCredentialVault,
    reference: SecretRef,
}

impl Drop for DropDelete {
    fn drop(&mut self) {
        // Best effort only; a failure here must not mask a test result.
        let _ = self.vault.delete(&self.reference);
    }
}
