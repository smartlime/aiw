# Repository Instructions

## Safety

- Never store, log, snapshot, or commit secret values.
- Never pass secret values through command-line arguments.
- Treat macOS Keychain as the only persistent source for user-managed secrets.
- Use an in-memory fake secret store in tests.
- Redact values in errors, traces, process descriptions, and test fixtures.
- Do not edit shell files, application configuration, or Keychain entries without an explicit migration step and a recoverable backup.
- Do not add a Git remote unless the user explicitly approves the repository publication policy.

## Architecture

- Implement the core in Rust.
- Keep Python only for vendored upstream MCP servers and skill helper scripts that have not been rewritten.
- Keep runtime operation independent from external source checkouts or mounts.
- Update external sources only through an explicit synchronization command.
- Export registered source paths into staging, record the source revision and checksums, test the candidate, and promote it only after validation.
- Never overwrite a native Rust implementation from upstream automatically. Report upstream drift instead.
- Expose tools through the Tool Catalog interface and keep host profiles static for the MCP server process lifetime.
- Add an adapter only at a real seam with at least two implementations or a production implementation and a test fake.
- Test modules through their interfaces.

## Command Interface

- Give every public full command a two-letter alias.
- Keep `list` and `ls` value-free.
- Make `show --all` and `show -a` display all registered values without an additional reveal flag.
- Do not add a `run` command. Child processes inherit secrets from the environment.

## Documentation

- Write repository documentation in English.
- Record durable architectural choices as Architecture Decision Records.
- Distinguish implemented behavior from accepted but unverified design decisions.

## Git Development

- Use `https://github.com/smartlime/aiw.git` as the canonical `origin` repository.
- Use `dev` as the long-lived integration branch. Base every normal change on the latest `dev` and implement it in a short-lived branch. Agent-owned branches use the `codex/` prefix.
- Merge reviewed and validated work branches into `dev`. Do not commit ordinary development work directly to `dev` or `main`.
- Keep `main` for released states only. Do not merge `dev` into `main` outside an explicit release operation.
- Do not force-push shared branches or rewrite published release history.
- After a release, merge the released `main` state back into `dev` so both branches contain the release metadata and changelog.

## Releases

- Start a release only when the user explicitly requests it and supplies the version. Use the supplied version string exactly for the Git tag; do not infer, increment, normalize, or prefix it.
- Create a release branch from `dev`. Update package version metadata, release automation, and `CHANGELOG.md` on that branch. Record the release version, date, and user-visible changes in the changelog.
- Validate the release artifacts before merging the release branch into `main`. Push `main` and the version tag only after the release commit is final.
- Create a GitHub Release for the tag and attach the final artifacts. A pushed tag alone does not complete a release.
- Use release notes supplied by the user. When the user does not supply notes, derive concise release notes from the changelog and the commits since the previous release.
- Never put secret values, private configuration, or internal acquisition links into the changelog, release notes, artifacts, or formula.

## Homebrew Releases

- Maintain reproducible formula generation or assembly support in this repository before the first release. Do not depend on a developer's untracked files to produce the formula.
- Generate the Homebrew formula only from the final published GitHub Release artifact. Calculate the SHA-256 checksum from that exact artifact after publication.
- Publish the formula to `https://github.com/smartlime/homebrew-aiw.git`. Follow the tap's existing layout; use `Formula/aiw.rb` when initializing an empty tap.
- Make the formula reference the immutable release URL, exact version, and verified checksum. Test installation and a basic `aiw` invocation through Homebrew before considering the release complete.
- Keep a release incomplete when the GitHub Release or Homebrew formula has not been published and verified. Report the exact unfinished stage instead of claiming completion.
