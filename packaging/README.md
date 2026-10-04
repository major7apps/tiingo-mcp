# Packaging and distribution

Tiingo MCP uses cargo-dist 0.32.0 to build release archives, install scripts, a Homebrew formula, and a Windows MSI. The MCPB packaging script adds a desktop bundle for each supported target.

Cargo installation, direct binary downloads, and manual MCP client configuration remain supported. Installers add the executable; they do not change client settings or collect a Tiingo API key. Desktop bundles ask for the key through the host's extension settings.

## Build and verify installers

Run the release checks in [QUALITY.md](../QUALITY.md), then run:

```bash
dist plan
cargo publish --dry-run --locked
```

The distribution plan must include the existing native archives and scripts, `tiingo-mcp.rb`, and `tiingo-mcp-x86_64-pc-windows-msvc.msi`. MSI builds require Windows and WiX v3. Keep the `upgrade-guid` and `path-guid` values in `Cargo.toml` stable so Windows can update an existing installation.

The release workflow contains custom MCPB packaging, upload steps, and restricted job permissions. Review those steps when regenerating cargo-dist files. `allow-dirty = ["ci"]` preserves the customized workflow.

## Publish the Homebrew formula

This repository also serves as the Homebrew tap. Stable releases update `Formula/tiingo-mcp.rb` on the default branch after uploading the release artifacts. The formula uses versioned download URLs and checksums from that release.

The publication job uses the repository's built-in `GITHUB_TOKEN` with `contents: write`. It does not need a personal access token, a separate repository, or Actions variables. Keep publication restricted to stable releases and ordinary pushes. If branch protection later blocks the bot's push, review and merge the generated formula through a pull request.

Release notes include the Homebrew commands only after formula publication succeeds or confirms the formula is already current. Skipped or failed publication leaves those instructions out. Check the publication job before announcing Homebrew support.

After a stable release, verify a public installation:

```bash
brew tap major7apps/tiingo-mcp https://github.com/major7apps/tiingo-mcp
brew install major7apps/tiingo-mcp/tiingo-mcp
tiingo-mcp --version
```

Verify MCP initialization from the installed executable. Check the formula's version, download URLs, and checksums against the matching release. Verify updates with `brew update` and `brew upgrade major7apps/tiingo-mcp/tiingo-mcp`, and removal with `brew uninstall major7apps/tiingo-mcp/tiingo-mcp`.

The explicit URL is needed because the repository is not named `homebrew-tiingo-mcp`. Homebrew remembers the URL after the first `brew tap` command. See [Homebrew's tap documentation](https://docs.brew.sh/Taps).

An official `homebrew/core` formula requires a source build and acceptance by Homebrew's maintainers. The repository tap remains available while the project works toward Homebrew's [acceptance requirements](https://docs.brew.sh/Package-Acceptance-Policy).

Keep the install script available for Linux. The release binaries cover x86_64 and ARM64 Linux with static musl linking; Linux Homebrew installation needs its own verification before it is advertised.

## Verify the Windows installer

The MSI installs the executable and offers PATH setup. Use a new terminal to check that `tiingo-mcp --version` works after installation. Also test MCP initialization, installation over an older MSI, and removal through Windows Settings.

The existing PowerShell script and ZIP archive remain available for users who want a different installation directory or do not want an MSI installation.

## Release documentation

Keep the version synchronized in `Cargo.toml`, `Cargo.lock`, and `mcpb/manifest.json`. Follow [AGENTS.md](../AGENTS.md) for version selection and release authorization.

The README labels Homebrew and MSI as pending until they are published. Remove the matching pending wording only after verifying each public installation method. Keep download links on `releases/latest/` and `releases/latest/download/`.

Publishing a crate, tag, GitHub release, desktop bundle, or Homebrew formula requires release authorization. A successful local build is preparation for publication.
