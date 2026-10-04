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

## Set up Homebrew publication

Homebrew publication is optional and requires setup outside this repository. Generating a formula does not make the Homebrew install command available.

Release notes include the Homebrew command only after the tap publication succeeds or confirms the formula is already current. Skipped or failed publication leaves those instructions out.

1. Create and initialize `major7apps/homebrew-tap` as a public GitHub repository.
2. Add `HOMEBREW_TAP_TOKEN` as an Actions secret in `major7apps/tiingo-mcp`. Use a token with permission to write contents only in the tap repository.
3. Set the Actions variable `HOMEBREW_TAP_ENABLED` to `true` after the repository and token are ready.
4. After an authorized stable release, verify that the formula was published and install it on a Mac with `brew install major7apps/tap/tiingo-mcp`.

Run `tiingo-mcp --version` and verify MCP initialization from the installed executable. Check the formula's version, download URLs, and checksums against the matching release. Confirm that `brew upgrade tiingo-mcp` and `brew uninstall tiingo-mcp` work before recommending the tap to users.

Keep the install script available for Linux. The release binaries cover x86_64 and ARM64 Linux with static musl linking; Linux Homebrew installation needs its own verification before it is advertised.

## Verify the Windows installer

The MSI installs the executable and offers PATH setup. Use a new terminal to check that `tiingo-mcp --version` works after installation. Also test MCP initialization, installation over an older MSI, and removal through Windows Settings.

The existing PowerShell script and ZIP archive remain available for users who want a different installation directory or do not want an MSI installation.

## Release documentation

Keep the version synchronized in `Cargo.toml`, `Cargo.lock`, and `mcpb/manifest.json`. Follow [AGENTS.md](../AGENTS.md) for version selection and release authorization.

The README labels Homebrew and MSI as pending until they are published. Remove the matching pending wording only after verifying each public installation method. Keep download links on `releases/latest/` and `releases/latest/download/`.

Publishing a crate, tag, GitHub release, desktop bundle, or Homebrew formula requires release authorization. A successful local build is preparation for publication.
