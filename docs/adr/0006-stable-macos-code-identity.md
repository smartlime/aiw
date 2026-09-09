# ADR 0006: Use a Stable macOS Code Identity for Keychain Access

- Status: Accepted
- Date: 2026-09-07

## Context

The macOS linker gives an unconfigured Rust executable an ad-hoc signature whose designated requirement contains the executable hash. Rebuilding `aiw` changes that hash. Keychain then treats the rebuilt executable as a different application and asks for access to every independent secret item.

## Decision

Sign development binaries with a stable code-signing identity and the identifier `com.smartlime.aiw`. On macOS, Cargo uses `scripts/cargo-runner.sh`. The runner signs only the `aiw` binary immediately before execution and passes test executables through unchanged. Release artifact signing remains part of the separate release assembly process.

Use `aiw Local Code Signing` as the default local identity. A different machine or release environment can select its provisioned identity through `AIW_CODESIGN_IDENTITY`. The repository never contains a certificate, private key, identity export, or password.

Migrate existing Keychain item access control once outside the application. Do not add an application command that weakens or rewrites Keychain access control. Keep every secret in its independent Keychain item.

## Consequences

Rebuilt development binaries retain the same designated requirement while the signing identity remains unchanged. Keychain can remember access across Cargo runs. A developer must provision the local signing identity before running `aiw` through Cargo on macOS.
