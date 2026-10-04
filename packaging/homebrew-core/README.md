# Candidate for homebrew/core

The formula in [tiingo-mcp.rb](tiingo-mcp.rb) is a local candidate for an official Homebrew submission. It builds the v2.2.0 source archive with Rust and Cargo. It is separate from the generated tap formula, which installs prebuilt release binaries.

The source checksum is verified, and the candidate is ready for review and additional Homebrew testing. It has not been submitted to `homebrew/core`.

## Verified source and local checks

On October 4, 2026, the public annotated `v2.2.0` tag resolved to commit `d79c4d55163eadc52068736ed5b8e5d1576422c6`. All 75 files and symlinks in the downloaded source archive matched that commit. `Cargo.toml`, `Cargo.lock`, and the MCPB manifest all declared version `2.2.0`.

The formula uses [the tagged source archive](https://github.com/major7apps/tiingo-mcp/archive/refs/tags/v2.2.0.tar.gz) with SHA-256:

```text
119104f796107956ed4fce81e9e1e5d003c76020f61a65070426ab5618977e0f
```

Local validation on Apple silicon macOS passed:

- The extracted archive built and installed into an isolated directory with `cargo install --locked --root <temporary-prefix> --path <extracted-source>`, using the existing Rust and Cargo 1.88.0 toolchain.
- The installed executable reported `tiingo-mcp 2.2.0`. The formula's exact test block passed through a small assertion harness, checking MCP initialization, the server version, 38 tools, named tools, and clean shutdown without credentials or Tiingo requests.
- `brew style` reported no offenses when the candidate was placed under a `Formula/` directory.

A full `brew install --build-from-source` and the `brew test` command have not been run for this candidate. Homebrew's Rust dependency installation, core audit checks, and source builds on Intel macOS and Linux remain unverified. The local Cargo build and test-block execution do not establish those results.

## Eligibility

On October 4, 2026, the [GitHub repository API](https://api.github.com/repos/major7apps/tiingo-mcp) reported 11 stars, 1 fork, and 0 watchers. The repository was created on April 13, 2026, and uses the MIT license.

Homebrew's [package acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) normally requires 75 stars, 30 forks, or 30 watchers. Owner submissions normally require 225 stars, 90 forks, or 90 watchers. The repository currently falls below both sets of thresholds. Keep the existing tap available, and defer an official submission until the project meets the criteria or Homebrew grants an exception.

The [core formula requirements](https://docs.brew.sh/Acceptable-Formulae) also require a stable release, reproducible sources, a compatible open source license, and working builds and tests on Homebrew's supported platforms. The candidate uses `std_cargo_args`, which includes `--locked`, to install the dependency versions in `Cargo.lock`. Its test starts the installed server without a Tiingo API key, completes MCP initialization, lists all 38 tools, and checks shutdown. It makes no Tiingo requests.

## Complete Homebrew validation

First, recheck the source archive checksum if the formula changes:

```bash
curl --fail --location --output tiingo-mcp-v2.2.0.tar.gz \
  https://github.com/major7apps/tiingo-mcp/archive/refs/tags/v2.2.0.tar.gz
shasum -a 256 tiingo-mcp-v2.2.0.tar.gz
```

Second, review the source archive and the formula. Confirm that the archive contains the expected version and `Cargo.lock`. Recheck the eligibility policy and repository metrics before preparing an upstream submission.

Third, follow Homebrew's [pull request procedure](https://docs.brew.sh/How-To-Open-a-Homebrew-Pull-Request). Fork `Homebrew/homebrew-core`, run `brew tap --force homebrew/core`, and create a branch from `origin/HEAD` in that checkout. Copy the candidate to `Formula/t/tiingo-mcp.rb`, then run:

```bash
HOMEBREW_NO_INSTALL_FROM_API=1 brew install --build-from-source homebrew/core/tiingo-mcp
brew test homebrew/core/tiingo-mcp
brew audit --strict --online homebrew/core/tiingo-mcp
brew audit --new --formula homebrew/core/tiingo-mcp
brew lgtm --online
```

The [formula test guidance](https://docs.brew.sh/Formula-Cookbook#add-a-test-to-the-formula) calls for a functional test. Passing `--version` alone does not verify the MCP exchange. Record actual results and any untested platforms before requesting review.

## Human review and submission

Homebrew's [AI contribution requirements](https://docs.brew.sh/How-To-Open-a-Homebrew-Pull-Request#artificial-intelligencelarge-language-model-aillm-usage) require the submitting person to review all generated code and prose before asking maintainers to review it. Disclose Codex and the model used in the initial pull request. The submitting person must answer maintainer questions and review comments without AI assistance.

Do not name an AI tool as a commit author, co-author, committer, or signatory, or add AI attribution trailers such as `Assisted-by` or `Co-developed-by`.

After the eligibility and validation checks pass and human review is complete, check for an existing formula or duplicate pull request. Commit the formula as `tiingo-mcp 2.2.0 (new formula)`, push the branch to the fork, and open a pull request against `Homebrew/homebrew-core`. Creating the candidate does not submit it or establish Homebrew acceptance.

## Move existing installations to core

Once Homebrew accepts the formula, coordinate the change using its [tap migration procedure](https://docs.brew.sh/Migrating-A-Formula-To-A-Tap). Retire this repository's binary formula and its publication job, and add a root `tap_migrations.json` entry:

```json
{
  "tiingo-mcp": "homebrew/core"
}
```

Verify migration from an existing tap installation before changing the public install command to `brew install tiingo-mcp`. Keep the current tap working until the core formula is available. Submit later source-version updates through Homebrew's `brew bump-formula-pr` process, and verify users can receive them with `brew update` and `brew upgrade tiingo-mcp`.
