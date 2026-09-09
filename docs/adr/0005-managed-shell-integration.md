# ADR 0005: Manage Shell Startup Integration Explicitly

- Status: Accepted
- Date: 2026-09-05

## Context

Interactive zsh and fish sessions must receive the same registered Keychain values. Startup files can already contain user-maintained configuration and legacy token definitions. Installation must preserve unrelated content and must not silently replace a changed integration.

## Decision

Implement `aiw env zsh` and `aiw env fish` with alias `en`. Emit shell-safe assignments for accounts that currently have values and skip missing accounts.

Implement `aiw shell status SHELL` and `aiw shell install SHELL`, with `sl`, `st`, and `in` aliases. Report integration as missing, current, drifted, or malformed.

Manage a marked block inside `~/.zshrc`. Manage the complete `~/.config/fish/conf.d/ai-world.fish` file for fish. Ask for confirmation before initial installation or drift replacement. Store the complete previous target in a dated Keychain entry and replace the target atomically. Refuse automatic replacement when zsh markers are malformed or the target is a symbolic link. Recheck the target after confirmation and immediately before the atomic rename.

Do not install through `~/.zshenv`. Do not publish values through `launchctl` in this stage.

## Consequences

Shell startup remains predictable and reversible. The installer can update its own code without replacing unrelated zsh configuration. Missing Keychain values do not produce startup errors. A newly updated value becomes visible only after the shell reloads the integration or starts again.
