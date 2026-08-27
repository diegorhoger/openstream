# Artifact Checksums (v0.1.0-alpha)

Generated during CI packaging. Every artifact has an individual .sha256 file. This file serves as the combined manifest.

## Format
- <filename> followed by <sha256>
- Each line represents one artifact/checksum pair
- Combined manifest is produced by concatenating all .sha256 files

## Verification
`ash
sha256sum -c <artifact>.sha256
`

## Note
Production artifacts are BLOCKED. Only CI test artifacts with self-signed test signatures are produced in this pipeline (signing/signing.md).
