# ADR 0002: Store Secrets in Keychain and Distribute Runtime Copies Through the Environment

- Status: Accepted
- Date: 2026-09-05

## Context

Tokens currently exist in multiple shell and application configuration locations. Agents and command-line tools work most predictably when they inherit conventional environment variables. Graphical applications started through the Dock do not inherit an interactive shell environment.

## Decision

Use macOS Keychain as the only persistent authority for user-managed secret values. Store only names, metadata, and policies in Git.

Load all registered values into interactive zsh from `~/.zshrc`. Load the same values into fish from a generated `conf.d` hook. Do not load them from `~/.zshenv`.

Use the per-user `launchctl` environment as a disposable runtime copy for graphical applications started after publication. Validate this path with a harmless variable across the target applications before publishing real tokens.

Define `aiw list` and `aiw ls` as value-free inventory commands. Define `aiw show --all` and `aiw show -a` as the complete inventory with values. Do not require a separate reveal flag. Do not provide `run`; programs inherit the environment normally.

## Consequences

Users get one authoritative value and familiar environment-based consumption. Existing shells and graphical applications can retain stale values until they reload or restart. Environment publication increases the number of live process environments that contain a token, so persistent storage and diagnostics must remain redacted.

Application-owned sessions remain outside this mechanism unless an application documents a supported external secret interface.
