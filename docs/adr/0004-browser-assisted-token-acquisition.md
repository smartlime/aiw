# ADR 0004: Acquire Interactive Tokens Through Browser Handoff

- Status: Accepted
- Date: 2026-09-05

## Context

Some token inventories contain OAuth implicit-flow links and service-specific token pages. These flows depend on an authenticated browser session, consent, and service-specific UI. Several environment variables can intentionally share one Keychain account and value.

The runtime must not depend on the document from which the registry was initially assembled. Private acquisition links should remain local rather than become part of repository history. Secret values must not enter Git, process arguments, terminal history, or plaintext configuration.

## Decision

Load user-specific registry fragments from `~/.config/ai-world/secrets.d/*.toml`. Keep only an anonymized schema example in Git. Treat the local registry as the maintained inventory and update it explicitly when a token is added or changed.

Use the TOML table name as the Keychain account under the fixed `ai-world` service. Use `env` for one environment variable and `envs` for aliases that share the account value. Reject entries that define both fields, duplicate an environment name, or repeat an account across files.

Implement `aiw update NAME` and alias `aiw up NAME` for one token. Implement `aiw update --all`, `aiw update -a`, and their `up` forms for the complete registry.

Open the acquisition URL for each selected Keychain account in the default browser. Read the resulting token through hidden terminal input and write it through the Secret Store Interface. Acquire all required values before the first Keychain write. Persist every previous value under the `ai-world.backup` Keychain service before changing primary values. Provide `aiw rollback` and alias `aiw rb` to restore one or all latest backups.

Do not automate the authenticated browser session or call undocumented token-issuance APIs. The generic verification-code entry still requires the device flow that requested the code.

## Consequences

The workflow remains interactive, but it works across OAuth and service-specific pages without storing browser credentials. The registry stays independent from its original documentation and keeps private links outside repository history. A complete refresh opens one browser page for each selected Keychain account. The primary `ai-world` service retains one authoritative value per account. The separate backup service preserves one previous generation per account and never publishes that generation to the environment.
