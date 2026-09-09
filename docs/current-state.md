# Current Implementation State

This document records implemented behavior without private configuration or secret values.

The Secret Registry, macOS Keychain Adapter, in-memory test Adapter, environment renderer, configuration synchronizer, bootstrap wizard, and shell installer are implemented. The public command interface includes `bootstrap`, `sync`, `list`, `show`, `update`, `rollback`, `env`, and `shell` with two-letter aliases.

The runtime loads user-owned registry files from `~/.config/ai-world/secrets.d/`. Repeatable configuration sources remain under `~/.config/ai-world/sources.d/`. The repository contains only an anonymized registry example and does not publish private acquisition links or source addresses.

A confirmed bootstrap run verified managed zsh and fish integration. A separate verification confirmed that a rebuilt, consistently signed development binary can read previously authorized Keychain items without repeated dialogs. These checks establish local behavior and do not claim cross-machine results.

The launch environment, Tool Catalog, MCP gateway, Skill Catalog, and external-source snapshot workflow remain planned rather than implemented.

## Intended End State

- Keychain stores one authoritative value for every user-managed secret.
- The repository stores registry schemas and policies without values.
- zsh and fish load the same registered values into their environments.
- The user launch environment supplies future graphical applications.
- Application-owned sessions remain separate.
- Legacy plaintext copies are removed only through an explicit migration with recoverable backups.
