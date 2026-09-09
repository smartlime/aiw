# ai-world

`ai-world` is a local control plane for developer AI tooling on macOS.

The project will centralize four concerns:

- user-managed API tokens and their environment publication;
- a curated Model Context Protocol gateway;
- a canonical skill catalog with adapters for different agents;
- explicit updates of selected external sources.

The project uses the `aiw` command. Every full command must also have a two-letter alias. The confirmed aliases are:

| Command | Alias | Purpose |
| --- | --- | --- |
| `bootstrap` | `bs` | Synchronize configuration, acquire missing secrets, and install shell integration. |
| `sync` | `sy` | Synchronize registered configuration sources without changing secret values. |
| `list` | `ls` | List registered secrets without values. |
| `show` | `sh` | Show one secret or, with `--all` or `-a`, all registered secrets with values. |
| `update` | `up` | Open acquisition pages and update one secret or all registered secrets. |
| `rollback` | `rb` | Restore one secret or all secrets from their latest Keychain backups. |
| `env` | `en` | Emit shell-safe assignments for zsh or fish. |
| `shell` | `sl` | Inspect or install managed zsh and fish startup integration. |

`run` is not part of the interface. Programs consume tokens from their inherited environment.

## Status

The first secret-management slice is implemented:

- the local registry maps Keychain accounts to environment names and acquisition links without values;
- the production secret-store adapter calls macOS Security Framework directly;
- the test adapter keeps fake values in memory;
- `list`, `ls`, `show`, `sh`, `update`, `up`, `rollback`, and `rb` implement the documented interface.

`aiw` loads and merges `~/.config/ai-world/secrets.d/*.toml`. The repository contains only [`config/secrets.example.toml`](config/secrets.example.toml). User-specific and internal links remain outside Git.

`aiw bootstrap --source URL_OR_PATH` starts the linear setup wizard. The source can be an HTTPS URL, a local TOML file, or `-` for TOML pasted through standard input. When no configuration exists and `--source` is omitted, the wizard asks for one of these sources. A profile name normally comes from the source file name and can be set explicitly with `--profile NAME`.

The wizard synchronizes configuration, acquires only missing Keychain accounts, detects available zsh and fish installations, and invokes the existing confirmation-gated shell installer. It finishes with a value-free configuration, secret, and shell status report. Re-running the wizard skips current configuration and shell integration.

Repeatable sources are recorded under `~/.config/ai-world/sources.d/`. `aiw sync` refreshes every recorded source. `aiw sync --source URL_OR_PATH` imports or refreshes one source. Synchronization validates the candidate against every active registry file, shows account additions, changes, and removals before confirmation, records its SHA-256 checksum, backs up a replaced profile, and installs the profile atomically. A pasted source is installed but cannot be synchronized again until it is associated with a repeatable source.

Each TOML table name becomes the Keychain account under the fixed `ai-world` service. Use `env` for one environment variable or `envs` for aliases that share one value:

```toml
[secrets.primary]
env = "PRIMARY_TOKEN"
url = "https://example.invalid/token"

[secrets.shared]
envs = ["FIRST_TOKEN", "SECOND_TOKEN"]
url = "https://example.invalid/shared-token"
```

`aiw show --all` prints every registered environment name and marks absent Keychain values as `<missing>`. `aiw show NAME` still returns an error when that specific value is absent.

`aiw env zsh` emits quoted `export` statements. `aiw env fish` emits quoted `set -gx` statements. Both forms skip registered accounts that do not yet have a Keychain value.

`aiw shell status zsh` and `aiw shell status fish` report `missing`, `current`, `drifted`, or `malformed`. The `status` subcommand has alias `st`. `aiw shell install SHELL` installs or updates the managed integration after confirmation. The `install` subcommand has alias `in`. The zsh installer changes only its marker block in `~/.zshrc`; the fish installer owns `~/.config/fish/conf.d/ai-world.fish`. The complete previous target is saved as a dated Keychain entry before modification. Symbolic links and malformed zsh markers are not replaced automatically.

`aiw update NAME` opens the registered acquisition page, accepts the token through hidden terminal input, and writes it to Keychain. `aiw update --all` acquires every value before writing any value. Before the first primary write, the command saves every previous value under the `ai-world.backup` Keychain service. `aiw rollback NAME` or `aiw rollback --all` restores these backups.

Shell publication, installation, configuration synchronization, and bootstrap commands are implemented. A confirmed bootstrap run verified managed zsh and fish integration without changing secret values. Migration of legacy plaintext copies and the harmless `launchctl` experiment remain separate operations. The repository does not publish a private configuration endpoint.

## Development

On macOS, Cargo runs the `aiw` binary through `scripts/cargo-runner.sh`. The runner signs `aiw` with the stable `aiw Local Code Signing` identity and the identifier `com.smartlime.aiw` before execution. Set `AIW_CODESIGN_IDENTITY` when the machine uses a different provisioned identity. The repository does not contain signing keys or certificates.

## Safety boundary

macOS Keychain is the only persistent store for user-managed secret values. Git stores names, metadata, policies, adapters, and tests, but never secret values, environment snapshots, Keychain exports, or encrypted secret databases.

Application-owned authentication state remains owned by the application unless the application provides a supported external secret interface.

## Repository policy

The canonical repository is `https://github.com/smartlime/aiw.git`. Development is integrated through `dev`; `main` contains released states only.

See [the architecture](docs/architecture.md), [the current implementation state](docs/current-state.md), and [the accepted decisions](docs/adr/).
