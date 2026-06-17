#!/usr/bin/env bash
#
# install.sh — build the `commenter-cat` binary and register it as an MCP server
# for Claude (Claude Code / Desktop), in one command.
#
# Usage:
#   scripts/install.sh                    # install binary + register (user scope)
#   scripts/install.sh --scope project    # register for THIS repo only (./.mcp.json)
#   scripts/install.sh --no-build         # skip `cargo install`; only (re)register
#   scripts/install.sh --help
#
# Scopes:
#   user     ~/.claude.json  — commenter-cat available to Claude in every project (default)
#   project  ./.mcp.json     — committable, this repo only
#
# Registration prefers the `claude` CLI when present; otherwise it merges the
# config in place (existing servers preserved, original backed up to *.bak,
# written atomically). Re-running is idempotent.

set -euo pipefail

SERVER_NAME="commenter-cat"
SCOPE="user"
DO_BUILD=1
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m  ✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m  !\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '3,18p' "${BASH_SOURCE[0]}" | sed 's/^#\{0,1\} \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --scope)    SCOPE="${2:-}"; shift 2 ;;
    --scope=*)  SCOPE="${1#*=}"; shift ;;
    --no-build) DO_BUILD=0; shift ;;
    -h|--help)  usage; exit 0 ;;
    *)          die "unknown argument: $1 (try --help)" ;;
  esac
done
case "$SCOPE" in user|project) ;; *) die "scope must be 'user' or 'project' (got '$SCOPE')" ;; esac

# 1. Prerequisites ------------------------------------------------------------
command -v cargo >/dev/null || die "cargo not found — install Rust from https://rustup.rs first"

# 2. Build + install the binary ----------------------------------------------
if [ "$DO_BUILD" -eq 1 ]; then
  say "Building + installing the commenter-cat binary (release)…"
  cargo install --path "$REPO_ROOT/crates/commenter-cat-cli" --force
fi

# 3. Resolve the installed binary --------------------------------------------
BIN="$(command -v commenter-cat || true)"
[ -n "$BIN" ] || BIN="${CARGO_HOME:-$HOME/.cargo}/bin/commenter-cat"
[ -x "$BIN" ] || die "commenter-cat not found after install (looked at: $BIN)"
ok "binary: $BIN — $("$BIN" --version)"

# Absolute path for user scope (robust under Claude's launcher PATH); bare
# command for the committable project file (portable across machines).
if [ "$SCOPE" = "user" ]; then CMD="$BIN"; else CMD="commenter-cat"; fi

# 4. Register the MCP server --------------------------------------------------
if command -v claude >/dev/null 2>&1; then
  say "Registering '$SERVER_NAME' via the claude CLI (scope: $SCOPE)…"
  claude mcp remove "$SERVER_NAME" --scope "$SCOPE" >/dev/null 2>&1 || true
  claude mcp add "$SERVER_NAME" --scope "$SCOPE" -- "$CMD" mcp
elif [ "$SCOPE" = "user" ]; then
  CFG="$HOME/.claude.json"
  say "Registering '$SERVER_NAME' in $CFG (user scope)…"
  python3 - "$SERVER_NAME" "$CMD" "$CFG" <<'PY'
import json, os, shutil, sys
name, command, path = sys.argv[1], sys.argv[2], sys.argv[3]
data = {}
if os.path.exists(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)               # fail loudly on a malformed config, before writing
    shutil.copy2(path, path + ".bak")     # back up the original first
servers = data.setdefault("mcpServers", {})
print("  preserving existing servers:", [k for k in servers if k != name] or "(none)")
servers[name] = {"command": command, "args": ["mcp"]}
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2); f.write("\n")
os.replace(tmp, path)                     # atomic swap
PY
else
  CFG="$REPO_ROOT/.mcp.json"
  say "Registering '$SERVER_NAME' in $CFG (project scope)…"
  python3 - "$SERVER_NAME" "$CMD" "$CFG" <<'PY'
import json, os, sys
name, command, path = sys.argv[1], sys.argv[2], sys.argv[3]
data = {}
if os.path.exists(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
servers = data.setdefault("mcpServers", {})
servers[name] = {"command": command, "args": ["mcp"]}
tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2); f.write("\n")
os.replace(tmp, path)
PY
fi
ok "MCP server '$SERVER_NAME' registered ($SCOPE scope)"

# 5. Smoke-test the MCP handshake + tool listing -----------------------------
# A successful handshake is necessary but NOT sufficient: a server can connect
# yet return a tools/list that strict clients (Claude Code, the Anthropic API)
# reject — e.g. tool inputSchemas lacking a JSON-Schema object "type", which
# silently drops the entire tool list. Exercise both so the installer's "OK"
# means the agent can actually see the tools.
say "Verifying the server responds to initialize + tools/list…"
REQ_INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"install","version":"0"}}}'
REQ_INITD='{"jsonrpc":"2.0","method":"notifications/initialized"}'
REQ_LIST='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
RESP="$(printf '%s\n%s\n%s\n' "$REQ_INIT" "$REQ_INITD" "$REQ_LIST" | "$BIN" mcp 2>/dev/null || true)"
if ! printf '%s' "$RESP" | grep -q '"serverInfo"'; then
  warn "could not verify the handshake (the server may still be fine)"
elif printf '%s' "$RESP" | grep -q '"title":"AnyValue"'; then
  warn "tools/list returned input schemas with no object \"type\" — strict clients (Claude Code) reject these and the agent sees no tools. Rebuild from a version that types the MCP tool parameters."
elif printf '%s' "$RESP" | grep -q '"inputSchema"'; then
  ok "MCP handshake + tools/list OK"
else
  warn "handshake OK but tools/list returned no tools"
fi

# 6. Next steps ---------------------------------------------------------------
say "Done — next steps:"
echo "  1. Reload / restart Claude Code so it re-reads the MCP config."
echo "  2. Run  /mcp  in Claude to confirm '$SERVER_NAME' is connected."
echo "  3. The agent can then drive: query · context · check · candidates · apply-edit · remove"
