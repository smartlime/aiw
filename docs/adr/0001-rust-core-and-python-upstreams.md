# ADR 0001: Use Rust for the Core and Python for Preserved Upstreams

- Status: Accepted
- Date: 2026-09-05

## Context

The project will grow from secret management into a universal MCP gateway, skill management, source synchronization, and optional desktop management. The runtime should consume little memory and should remain easy to distribute as a local executable.

Several useful upstream MCP servers and skill helpers already use Python. Rewriting every implementation would increase maintenance cost without improving every integration.

## Decision

Implement the `aiw` core in Rust. Rust owns policy, registries, the command-line interface, Keychain access, environment publication, the MCP server, native integrations, process supervision, validation, and synchronization.

Preserve Python only as a compatibility runtime for selected vendored upstream MCP servers and existing skill helper scripts. Reimplement a tool in Rust when it mainly validates or transforms parameters and calls a stable API. Preserve the upstream process when it contains substantial authentication, pagination, retry, state, or protocol behavior.

Do not use Swift in the initial architecture. A future macOS interface can remain a thin client over the Rust core.

## Consequences

The normal path uses one compiled process and exposes a small curated tool surface. Complex preserved integrations can still incur Python process cost. Process lifecycle optimization remains deferred until measurements justify it.
