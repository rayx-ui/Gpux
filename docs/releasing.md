# Releasing gpux

A release is one version tag `vX.Y.Z` on `main` for the **whole workspace**: every own crate
(`gpux`, every `gpux-*` package outside `vendor/`) has the same version, and one root
[CHANGELOG.md](../CHANGELOG.md) section and one GitHub Release describe it. Nothing is published
to a registry; consumers depend on the tag (see [installation-and-usage.md](installation-and-usage.md#add-gpux-to-a-rust-project)),
and the manifests stay publishable to crates.io.

[release-plz](https://release-plz.dev) does the work in git-only mode
([release-plz.toml](../release-plz.toml), [release-plz.yml](../.github/workflows/release-plz.yml)):
it turns Conventional Commits into a release pull request and, once that is merged, tags the
version and creates the GitHub Release. The vendored forks (`gpux-wgpu*`, `gpux-accesskit-macos`)
keep their own `-gpux.N` versions and are never released, tagged or bumped.

## How a release happens

1. Commit straight to `main` with Conventional Commits (implementation never goes through pull
   requests; see the push policy below). The type decides the next version:

   | Commit | Release |
   | --- | --- |
   | `fix:`, `perf:` | patch (`1.0.0` → `1.0.1`) |
   | `feat:` | minor (`1.0.0` → `1.1.0`), breaking work and Zed imports included (the 1.x policy) |
   | `docs:`, `test:`, `ci:`, `build:`, `chore:`, `refactor:` | none on their own (`release_commits` in `release-plz.toml` lists `feat`, `fix` and `perf`); they appear in the changelog of the next release |

   `chore(agentx)`, `chore(release)`, `chore(deps)` and merge commits never appear in the changelog.
   A release-type commit whose files are all outside the crates (only the root `Cargo.toml`, only
   `Cargo.lock`, only `vendor/**`, only a top-level file such as the README) starts no release
   either: release-plz looks at the files of each package.
2. On every push to `main` the Release-plz workflow runs two jobs, `release` and then `release-pr`.
   `release-pr` opens or updates the release pull request: `[workspace.package] version`, every own
   version requirement (the `[workspace.dependencies]` entries and `crates/gpui_mobile`'s inline
   `gpux` requirement), the own `Cargo.lock` entries and a new `## [X.Y.Z] - YYYY-MM-DD` section in
   `CHANGELOG.md` listing the commits of every own crate. Edit the changelog text in the pull request
   if a line reads badly.
3. Check the release pull request (below), then merge it.
4. The merge is a push to `main`: `release` finds a workspace version without a tag, tags `vX.Y.Z`
   at that commit and creates the GitHub Release `vX.Y.Z` with the new changelog section as its
   notes. `release-pr` waits for `release`, so it never opens a pull request for the version the
   same push tags.

The first release, `v1.0.0`, needs no pull request: the workspace says `1.0.0`, `CHANGELOG.md`
already has its hand-written `1.0.0` section, and in git-only mode release-plz treats a package
without a matching tag as an initial release, so `release` tags `v1.0.0` on the push that brings the
workflow to `main`, and `release-pr` then finds nothing to release (history before the first tag
never counts).

### Before merging a release pull request

The released commits reached `main` without pull-request CI, so check all of these first:

- its CI is green: a release pull request runs `check` on ubuntu and `package` only
  ([ci.md](ci.md));
- the latest nightly run on `main` is green (it covers what the commits were never tested with);
- it changes only `Cargo.toml` files, `Cargo.lock` and `CHANGELOG.md`;
- the proposed version is still `1.x`.

A pull request that proposes `2.0.0` means a stray breaking marker: do not merge it (the 1.x
policy, below).

## The 1.x policy

While the owner (RayX) is the only consumer of gpux, releases stay on 1.x. release-plz proposes a
major version for a `!` commit or a `BREAKING CHANGE:` footer and cannot cap it, so:

- No commit is marked breaking: no `!` after the type or scope (`feat!:`, `refactor(gpui)!:`) and
  no `BREAKING CHANGE:` footer.
- A break, every Zed import that breaks the API included, is committed as `feat` (or `fix` when it
  is a fix) whose body states what breaks, with a `Migration:` footer that tells RayX what to
  change, for example:

  ```text
  feat(gpux): import Zed GPUI closure 0123abcdef

  `Window::foo` is now `Window::bar`, and `Foo::new` takes a `&mut App`.

  Migration: rename `Window::foo` calls to `Window::bar`; pass `cx` to `Foo::new`.
  ```

  The changelog prints each `Migration:` footer as a nested line under its commit's entry. A break
  is therefore a minor bump (`1.3.0` → `1.4.0`).
- A release pull request that proposes `2.0.0` means a stray marker. Do not merge it: find the
  commit (`git log -E --grep='^BREAKING' --grep='!:' v<last>..origin/main`), keep a local ref at the
  current head, reword the commit, then push with the push policy's safe form and read the pull
  request again (release-plz recomputes it).
- `semver_check = false` and `release_commits = "^(feat|fix|perf)"` stay as they are:
  cargo-semver-checks would find a break in every Zed import.

`tools/testkit/tests/release_config.rs` asserts the workspace version's major is 1. The policy ends
when a second consumer appears; then `!` and major bumps return, and that assertion changes with it.

## The push policy

Commits go straight to `main`, never through a feature branch or an implementation pull request;
pull requests exist only as release-plz's release pull requests. Push only in the safe form, so a
force push never drops a remote commit (a merged release pull request, a commit a tag points to):

```bash
git fetch origin && git rebase origin/main && git push --force-with-lease origin main
```

The branch ruleset lets the owner's account through. Gpux runs no CI on pushes to `main`;
`release-plz.yml` is the only workflow a push starts, and the nightly run covers `main`.

## Tags

Tags are `vX.Y.Z` (`git_tag_name` in `release-plz.toml`); only `gpux` creates them, so a crate
missing from `release-plz.toml` can never tag by itself. The repository ruleset **Release tags**
targets `refs/tags/v*` with the rules `update`, `deletion` and `non_fast_forward` and no bypass:
a tag never moves or disappears, while creating a new one stays allowed. A defect found after a
release is fixed by the next patch release, never by moving a tag. Consumers' `Cargo.lock` records
the tag's commit.

## A new crate

A crate added to the workspace (usually by a Zed import) joins `release-plz.toml`: a `[[package]]`
entry with `version_group = "gpux"`, and its name in gpux's `changelog_include`. It inherits
`version.workspace = true`, and its `[workspace.dependencies]` entry carries the workspace version.
`release_config.rs` fails until all three hold. A new vendored fork gets `release = false`.

## The `RELEASE_PLZ_TOKEN` secret

Events made with the workflow's default `GITHUB_TOKEN` start no other workflows, so with it the
release pull request would get no CI. release-plz therefore uses a fine-grained personal access
token stored as the repository secret `RELEASE_PLZ_TOKEN`. It is the token shared with RayX.CLI
(and the other RayX libraries): resource owner `rayx-ui`, repository permissions **Contents** read
and write and **Pull requests** read and write (Metadata read comes automatically). **Its
repository access must include `Gpux`**; otherwise both jobs fail at checkout or push.

Release pull requests, tags and release commits then appear as the token's owner.

**When it expires, nothing fails loudly:** the `release` and `release-pr` jobs fail at checkout or
push (red Release-plz runs in the Actions tab), no release pull request is updated and no tag is
created; releases silently stop until the token is replaced. Rotate before the expiry date:
generate a new token with the same settings and repository access, and replace the secret's value
in every repository that shares it. Nothing else changes.

## By-hand fallback

If release-plz cannot cut a release (a broken workflow, an expired token you cannot replace yet),
release by hand on an up-to-date `main`:

1. Set `[workspace.package] version` in the root `Cargo.toml` to `X.Y.Z`, and the `version` of
   every own `[workspace.dependencies]` entry (each one with a `path` outside `vendor/`) and of
   `crates/gpui_mobile/Cargo.toml`'s inline `gpui = { package = "gpux", ... }` requirement to the
   same value. Leave the forks' `=…-gpux.N` requirements alone.
2. Refresh only the workspace's own lockfile entries: `cargo update --workspace --offline`.
3. Add a `## [X.Y.Z] - YYYY-MM-DD` section under the header of `CHANGELOG.md`.
4. Run `cargo test -p gpux-testkit --test release_config` and `cargo package --workspace`.
5. Commit (`chore(release): release vX.Y.Z`), rebase, tag the rebased commit and push the branch
   and the tag together. A push of the version without its tag would start Release-plz, whose
   `release` job tags any untagged version; the ruleset would then reject a second tag.

   ```bash
   git fetch origin && git rebase origin/main
   git tag -a vX.Y.Z -m "chore: Release package gpux version X.Y.Z"
   git push --atomic --force-with-lease origin main vX.Y.Z
   ```

6. Create the GitHub Release unless a working `release` job already did
   (`gh release view vX.Y.Z --repo rayx-ui/Gpux`):

   ```bash
   gh release create vX.Y.Z --repo rayx-ui/Gpux --title vX.Y.Z --notes-file <the section's body>
   ```

release-plz continues from the by-hand tag on its next run.

## How the configuration was proven

release-plz documents neither a git-only version group sharing one tag nor `version.workspace`
inheritance, so the configuration was proven before it landed (2026-10-18, epic `gpux-releases`)
with release-plz **0.3.172**, the version `release-plz.yml` pins (`version:` of
`release-plz/action@v0.5`). In a scratch clone whose `origin` was a local bare repository, these
held:

1. With no tag, `release` created exactly one tag, `v1.0.0`, at HEAD and one GitHub Release, for
   `gpux` only; a second run created nothing.
2. After a `fix(gpui_platform)` commit, `update` (the release pull request's content) set the
   workspace version, all 30 own `[workspace.dependencies]` requirements, `crates/gpui_mobile`'s
   inline requirement and the 31 own lockfile entries to `1.0.1`, left the forks and every other
   lockfile line alone, and added one `## [1.0.1]` section to the root `CHANGELOG.md` only. A release
   pull request therefore changes exactly `Cargo.toml`, `Cargo.lock`, `crates/gpui_mobile/Cargo.toml`
   and `CHANGELOG.md`.
3. A `fix(gpui_mobile)` commit, in a crate `gpux` does not depend on, did the same, and `release`
   then created `v1.0.1`.
4. `feat` gave `1.1.0`; a `feat(gpux): import Zed GPUI closure <sha>` with a break in its body and a
   `Migration:` footer gave `1.1.0` too, with the footer as a nested `- Migration:` line.
5. Only `docs`, `test`, `ci`, `chore(agentx)`, `refactor` and `build` commits changed nothing.
6. After the release pull request's commit, `release` created exactly `v1.0.1` with that section as
   the Release notes; a second run and a following `update` did nothing.

Also observed: `feat!`, or `fix` with a `BREAKING CHANGE:` footer, proposes `2.0.0`; a
`refactor(gpui)!:` alone starts no release but turns any `fix` or `feat` beside it into `2.0.0`;
`update` on the untagged `1.0.0` tree changes nothing (the history before the first tag, which has
`!` commits, does not count); release-plz compiles nothing, so the workflow installs no system
packages; the default GitHub Release name is `gpux-vX.Y.Z`, hence `git_release_name`.
`release`'s JSON output lists every own package as released under the one tag, but only `gpux`
creates the tag and the Release.

### Rehearsing a configuration change

Never point an experiment at `github.com/rayx-ui/Gpux`. Rehearse in a scratch clone under the
ignored `artifacts-temp/`:

```bash
git clone --no-local . artifacts-temp/release-rehearsal/gpux
git init --bare artifacts-temp/release-rehearsal/remote.git
git -C artifacts-temp/release-rehearsal/gpux remote set-url origin "$PWD/artifacts-temp/release-rehearsal/remote.git"
git -C artifacts-temp/release-rehearsal/gpux push origin main
```

Use the release-plz version the workflow pins (the `version:` of both release-plz steps in
`release-plz.yml`); a rehearsal on another version proves nothing about the pinned one:

```bash
cargo install release-plz --version 0.3.172 --locked
```

(release-plz may need a newer Rust than the repository's `rust-toolchain.toml`; run the install
outside the repository, with `cargo +<newer toolchain> install ...` if needed.)

Then, in the clone, make commits with the subjects you want to test and run `release-plz update`
(what the release pull request would change; inspect `git diff`). `release-plz release` needs a git
forge even with `--dry-run`; pass `--forge gitea --repo-url http://127.0.0.1:<port>/scratch/gpux
--git-token dummy` and serve that address with a local stub, never a real forge. Run without
`GITHUB_TOKEN` in the environment. The stub answers the three calls `release` makes:

- `GET /api/v1/repos/scratch/gpux/commits/<sha>/pull`: answer `404` (no release pull request; the
  `release` job then releases whatever version has no tag);
- `POST /api/v1/repos/scratch/gpux/tags` with `{"tag_name", "target", "message"}`: create the
  annotated tag in the bare remote (`git -C remote.git tag -a <tag_name> -m <message> <target>`) and
  answer `201`;
- `POST /api/v1/repos/scratch/gpux/releases` with `{"tag_name", "name", "body", "draft",
  "prerelease"}`: record it (that is the GitHub Release `release` would create) and answer `201`.

After each `release`, `git fetch --tags origin` in the clone, as the workflow's checkout does.
