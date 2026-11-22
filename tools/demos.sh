#!/usr/bin/env bash
# Cycle through WAFFLE_DEMO scenes with the arrow keys: right/down next, left/up previous, q quits.
#   tools/demos.sh voice-status soundboard call-chat
# Builds once, then runs one scene at a time from target/debug/waffle.
set -u
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || { echo "usage: tools/demos.sh <scene> [scene...]"; exit 1; }
scenes=("$@")
cargo build -q 2>/dev/null || cargo build || exit 1

pid=""
stop() { [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null; pid=""; }
trap 'stop; exit' INT TERM EXIT

i=0
while true; do
  stop
  printf '\r\033[K[%d/%d] %s   (left/right to cycle, q to quit)' $((i + 1)) ${#scenes[@]} "${scenes[$i]}"
  WAFFLE_DEMO="${scenes[$i]}" ./target/debug/waffle >/dev/null 2>&1 &
  pid=$!
  while true; do
    IFS= read -rsn1 key
    case "$key" in
      q) echo; exit 0 ;;
      $'\e')
        read -rsn2 -t 0.05 rest
        case "$rest" in
          '[C'|'[B') i=$(( (i + 1) % ${#scenes[@]} )); break ;;
          '[D'|'[A') i=$(( (i - 1 + ${#scenes[@]}) % ${#scenes[@]} )); break ;;
        esac ;;
    esac
  done
done
