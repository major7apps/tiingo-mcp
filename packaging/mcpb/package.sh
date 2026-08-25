#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <target-triple> <binary-directory>" >&2
  exit 64
fi

target="$1"
binary_directory="$2"
case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin)
    platform="darwin"; executable="tiingo-mcp" ;;
  aarch64-unknown-linux-musl|x86_64-unknown-linux-musl)
    platform="linux"; executable="tiingo-mcp" ;;
  x86_64-pc-windows-msvc)
    platform="win32"; executable="tiingo-mcp.exe" ;;
  *) echo "unsupported target: $target" >&2; exit 65 ;;
esac
binary="$binary_directory/$executable"

if [[ ! -f "$binary" ]]; then
  echo "binary not found: $binary" >&2
  exit 66
fi

stage="target/mcpb/$target"
bundle="target/distrib/tiingo-mcp-$target.mcpb"
rm -rf -- "$stage"
mkdir -p "$stage/server" target/distrib
cp packaging/mcpb/manifest.json "$stage/manifest.json"
cp "$binary" "$stage/server/$executable"
chmod +x "$stage/server/$executable" 2>/dev/null || true
jq --arg platform "$platform" --arg executable "$executable" \
  '.compatibility.platforms = [$platform]
   | .server.entry_point = "server/" + $executable' \
  "$stage/manifest.json" > "$stage/manifest.tmp.json"
mv "$stage/manifest.tmp.json" "$stage/manifest.json"
npx --yes @anthropic-ai/mcpb@2.1.2 validate "$stage/manifest.json"
npx --yes @anthropic-ai/mcpb@2.1.2 pack "$stage" "$bundle"
npx --yes @anthropic-ai/mcpb@2.1.2 info "$bundle"
