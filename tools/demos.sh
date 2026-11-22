#!/usr/bin/env bash
# Steps through WAFFLE_DEMO scenes one window at a time: Enter or n next, p previous, r reload, q quit.
# Usage: tools/demos.sh [scene...]   (defaults to the create server, create channel and invite scenes)
set -u
cd "$(dirname "$0")/.."
scenes=("$@")
[ ${#scenes[@]} -eq 0 ] && scenes=(
  create-server create-server-template create-server-customize create-server-join create-server-limit
  create-channel create-channel-private create-channel-community create-channel-stage
  invite invite-sent invite-settings invite-vanity invite-paused
)
bin=target/debug/waffle
[ -x "$bin" ] || cargo build || exit 1

pid=
stop() { [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null; pid=; }
trap 'stop; exit' INT TERM

i=0
while :; do
  stop
  WAFFLE_DEMO="${scenes[$i]}" "$bin" >/dev/null 2>&1 &
  pid=$!
  printf '\r\033[K[%d/%d] %s  (Enter/n next, p prev, r reload, q quit) ' $((i + 1)) ${#scenes[@]} "${scenes[$i]}"
  read -rsn1 key
  case "$key" in
    q) stop; echo; exit ;;
    p) i=$(( (i - 1 + ${#scenes[@]}) % ${#scenes[@]} )) ;;
    r) ;;
    *) i=$(( (i + 1) % ${#scenes[@]} )) ;;
  esac
done
