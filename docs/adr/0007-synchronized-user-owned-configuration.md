# ADR 0007: Synchronize User-Owned Configuration Before Bootstrap

- Status: Accepted
- Date: 2026-09-07

## Context

The public repository cannot contain organization-specific token inventories or acquisition links. Requiring every user to create and maintain the same TOML file manually does not scale. Runtime operation must remain independent from external source checkouts or mounts.

## Decision

Add `aiw sync` with the `sy` alias. Accept an HTTPS URL, a local file, or standard input through the `ConfigSource` interface. Record repeatable sources in `~/.config/ai-world/sources.d/` and install validated profiles in `~/.config/ai-world/secrets.d/`. Do not record standard input as a repeatable source.

Validate a candidate before installation. Limit its size, require UTF-8, reject secret-value fields, and reject duplicate environment names or Keychain accounts across all active profiles. Show account-level additions, changes, and removals before confirmation. Back up a replaced profile, record the SHA-256 checksum, refuse symbolic-link replacement, and write atomically.

Add `aiw bootstrap` with the `bs` alias as a linear terminal wizard. Synchronize configured sources, prompt for a source when no profile exists, acquire only missing Keychain accounts, detect zsh and fish, and invoke the existing shell installer. Derive progress from current state instead of storing wizard state.

Keep organization-specific source addresses outside this repository. A private publication process can export an organization-owned profile from an immutable revision to an HTTPS endpoint. That publication process is separate from the public runtime implementation.

## Consequences

A new user can start from one private source address. Existing users can refresh metadata without changing Keychain values. Local files and pasted TOML provide offline and authentication fallback paths. HTTPS publication and its access policy must be implemented and validated by the source owner.
