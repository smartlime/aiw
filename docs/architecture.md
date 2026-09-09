# Architecture

## Goal

`ai-world` provides one local control plane for secrets, MCP tools, agent skills, and selected upstream integrations. The command-line interface is `aiw`.

## System boundary

The repository owns configuration, metadata, adapters, validation, and update workflows. macOS Keychain owns user-managed secret values. External applications continue to own authentication state that they do not expose through a supported interface.

## Modules

### Secret Registry

The Secret Registry is a deep Module. Its Interface maps a Keychain account to one environment name or several aliases and one acquisition link. The Keychain service is fixed as `ai-world`. The registry never contains values.

The runtime loads and merges `~/.config/ai-world/secrets.d/*.toml`. The repository stores only the schema example. Duplicate Keychain accounts across files and duplicate environment names across accounts are invalid.

### Secret Store

The Secret Store Interface reads, writes, and deletes values. `MacOSKeychainAdapter` is the production Adapter. `MemorySecretStore` is the test Adapter.

Values must not appear in command-line arguments. The Keychain Adapter must use standard input or native security APIs for writes and must return values only to the requesting process.

macOS development binaries use a stable code-signing identity when Cargo executes them. Keychain access control therefore recognizes rebuilt versions of `aiw` as the same application instead of binding authorization to an ad-hoc build hash. Release artifact signing belongs to the release assembly process.

### Token Acquisition

The Token Acquisition Interface obtains a value for one or more registry entries that share an acquisition URL. The production Adapter opens the configured page in the default browser and reads the resulting token through hidden terminal input. It does not automate interactive authentication or store browser sessions.

### Configuration Synchronization

The Configuration Synchronization Module reads a registry profile through the `ConfigSource` interface. The production Adapter supports HTTPS, a local file, and standard input. Tests use an in-memory Adapter. Repeatable source descriptors remain in the user-owned `~/.config/ai-world/sources.d/` directory, while active profiles remain in `secrets.d/`.

Synchronization limits input size, requires UTF-8, applies the strict Secret Registry schema, and validates the candidate together with every active profile. It rejects secret-value fields and duplicate environment names or Keychain accounts before writing. A changed profile requires confirmation, receives a protected backup, and is replaced atomically. The source descriptor records the SHA-256 checksum of the installed bytes.

`aiw bootstrap` forms a small orchestration interface over configuration synchronization, missing-secret acquisition, shell detection, and the existing Shell Integration Manager. The linear wizard derives progress from current state, so interrupted or repeated runs do not require a separate state file. It never depends on an external source checkout at runtime. A private system can publish an organization-owned profile through HTTPS without placing its address or contents in this repository.

`aiw update NAME` updates one registered token. `aiw update --all` acquires every required value before changing Keychain. Keychain writes use the Secret Store Interface. Before primary values change, the command stores recoverable copies under the `ai-world.backup` Keychain service. `aiw rollback` restores one or all latest backups.

### Environment Publisher

The Environment Publisher reads the registry and Secret Store and publishes a runtime copy through two Adapters:

- `ShellEnvironmentAdapter` emits shell-safe assignments for zsh and fish startup hooks.
- `LaunchctlEnvironmentAdapter` publishes values to the per-user launch environment for applications started afterward.

The Keychain value remains authoritative. Environment values are disposable copies. Existing shells and applications can remain stale until they reload or restart.

`aiw env zsh` and `aiw env fish` emit shell-specific assignments and skip accounts without values. The Shell Integration Manager detects missing, current, drifted, and malformed startup integration. It asks for confirmation before installation or replacement, stores the complete previous target as a dated Keychain entry, and writes atomically. It edits only a marked block in `~/.zshrc` and owns one generated fish file under `~/.config/fish/conf.d/`. It refuses automatic writes through symbolic links.

The launch environment design remains an experiment until a harmless variable has been verified in ChatGPT, Claude, Cursor, Visual Studio Code, and Obsidian.

### Tool Catalog

The Tool Catalog is the Interface exposed by the universal MCP server. It describes tools, invokes a tool, and reports Adapter health.

The first Adapter families are:

- `NativeToolAdapter` for selected thin HTTP integrations implemented in Rust;
- `McpProcessAdapter` for preserved local upstream MCP implementations;
- `RemoteMcpAdapter` for remote MCP endpoints.

Profiles select explicit tool groups and allowlists. Each host or agent starts a separate server process with a fixed profile. The visible tool set remains static for that process lifetime.

### Skill Catalog

The Skill Catalog stores canonical skill sources and metadata. Target Adapters materialize compatible layouts for Codex, Claude, Cursor, and later hosts. A target Adapter may link, copy, and validate files, but it must not silently fork the canonical source.

Python helper scripts may remain part of a skill. Rust owns orchestration and policy.

### Upstream Sync

Upstream Sync imports registered source paths into staging and records provenance. It does not participate in runtime tool calls.

Each source Adapter performs an explicit update against an immutable source revision, exports only registered paths, records the revision and checksums, and validates the candidate before promotion. A vendored upstream MCP can receive a tested snapshot. A native Rust integration receives a drift report and must be updated manually.

## Runtime flow

1. A shell startup hook asks `aiw` for registered values.
2. `aiw` reads values from Keychain and exports them into the shell environment.
3. Child agents and tools inherit the environment.
4. A future launch-environment command refreshes the per-user environment for graphical applications.
5. An MCP host starts `aiw mcp serve` with a named static profile.
6. The Tool Catalog dispatches each allowed call to its Adapter.

## Implementation boundary

Rust owns the command-line interface, Keychain integration, environment publication, registries, MCP server, native tools, process supervision, validation, and update orchestration.

Python remains a compatibility runtime for vendored MCP servers and existing skill helpers. Swift is not part of the initial architecture. A future native macOS interface can call the same Rust Modules rather than own secret logic.

## Delivery order

1. Implement the Secret Registry, Keychain Adapter, `list`, `show`, and test fakes.
2. Implement zsh and fish publication, then migrate existing secret definitions with backups.
3. Validate launch environment publication with a harmless variable before publishing tokens.
4. Implement the Tool Catalog and one native tool as a vertical slice.
5. Add one vendored MCP Adapter and one external-source synchronization workflow.
6. Add the Skill Catalog and host Adapters.
