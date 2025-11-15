#!/usr/bin/env bash
set -euo pipefail

# Default server address (can be overridden with env var SERVER)
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
DEFAULT_SERVER="${SERVER:-127.0.0.1:7000}"

# If user already passed a --server flag, do not prepend.
contains_server=false
for a in "$@"; do
  case "$a" in
    --server|--server=* ) contains_server=true; break ;;
  esac
done

if [ "$contains_server" = true ]; then
  args=( "$@" )
else
  args=( --server "$DEFAULT_SERVER" "$@" )
fi

# Run the client crate with the composed args
cd "$ROOT_DIR/client"
cargo run -- "${args[@]}"