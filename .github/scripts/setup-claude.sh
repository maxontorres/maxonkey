#!/usr/bin/env bash
# Prepare a Claude job: pinned cargo-deny, the bubblewrap sandbox that removes secrets from
# Claude's commands, and the pinned Claude Code CLI. Needs CLAUDE_CODE_VERSION.
set -euo pipefail

CARGO_DENY_VERSION="0.20.2"
CARGO_DENY_SHA256="9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
archive="cargo-deny-${CARGO_DENY_VERSION}-x86_64-unknown-linux-musl"
curl -sSfL -o "$RUNNER_TEMP/cargo-deny.tgz" \
  "https://github.com/EmbarkStudios/cargo-deny/releases/download/${CARGO_DENY_VERSION}/${archive}.tar.gz"
echo "${CARGO_DENY_SHA256}  $RUNNER_TEMP/cargo-deny.tgz" | sha256sum --check
tar -xzf "$RUNNER_TEMP/cargo-deny.tgz" -C "$RUNNER_TEMP"
echo "$RUNNER_TEMP/${archive}" >> "$GITHUB_PATH"

# CLAUDE_CODE_SUBPROCESS_ENV_SCRUB needs bubblewrap (same commands as claude-code-action).
sudo apt-get update -qq && sudo apt-get install -y --no-install-recommends bubblewrap socat
if [ -f /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]; then
  sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
fi
# The sandbox hides .mcp.json files above the workspace and fails every command when it
# cannot create one in a root-owned folder (anthropics/claude-code-action#1547).
for dir in / /home; do
  [ -e "$dir/.mcp.json" ] || echo '{"mcpServers": {}}' | sudo tee "$dir/.mcp.json" >/dev/null
done
bwrap --ro-bind / / --dev /dev true

# The CLI version claude-code-action v1.0.253 installs, from the same installer.
curl -fsSL https://claude.ai/install.sh | bash -s -- "$CLAUDE_CODE_VERSION"
echo "$HOME/.local/bin" >> "$GITHUB_PATH"
