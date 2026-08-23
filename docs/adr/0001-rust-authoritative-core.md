# ADR-0001: Rust authoritative core

Status: proposed  
Date: 2026-08-23

## Context

Desktop, Cloud, mobile, browser, plugins, and future hardware must share one definition of valid decks, permissions, commands, and execution outcomes.

## Decision

Stable pinned Rust owns domain validation, OSCP, action execution, permission evaluation, persistence/sync semantics, pairing/crypto, and mobile shared logic. TypeScript, Swift, and Kotlin are platform UI/binding languages. Python does not ship in the product.

## Consequences

Core invariants are shared and testable across surfaces. FFI/code generation adds build complexity, so exposed boundaries remain narrow and versioned.

## Security impact

Authority cannot be reimplemented optimistically in a UI. Unsafe code is forbidden by default and isolated at proven platform boundaries.

## Reversal

A replacement requires compatible golden fixtures, migration proof, independent security review, and a human hard-stop decision.
