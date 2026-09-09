# ADR 0003: Expose Curated MCP Profiles and Import Upstream Sources Explicitly

- Status: Accepted
- Date: 2026-09-05

## Context

Passing every tool from every MCP server to every agent wastes context and makes tool selection less predictable. Some useful MCP implementations live in external source repositories, but normal use should not require a mounted checkout.

## Decision

Expose tools through a Tool Catalog with explicit groups and allowlists. Each MCP server process starts with one named profile and keeps its visible tool set static for its lifetime.

Support native Rust tools, local MCP subprocesses, and remote MCP endpoints through separate Adapters. Reimplement selected thin API wrappers in Rust. Preserve complex upstream behavior behind a subprocess Adapter when rewriting would create unnecessary ownership.

Keep external source checkouts out of the runtime path. An explicit synchronization command fetches an immutable revision, exports registered paths into staging, records the source revision and checksums, and validates the candidate before promotion.

Promote a tested snapshot for a vendored upstream MCP. For a native Rust implementation, report upstream drift and require a reviewed manual update. Never overwrite native code from upstream automatically.

## Consequences

Agents receive smaller and deterministic tool surfaces. The local repository continues to work without an external source mount. Updates become deliberate and auditable, but the maintainer must resolve drift and local patches during synchronization.
