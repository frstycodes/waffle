#!/usr/bin/env bash
# Steps through WAFFLE_DEMO scenes one window at a time: → / l next, ← / h previous, q quits.
# Usage: tools/demos.sh [scene ...]   (no scenes: this branch's navigation and moderation set)
set -u
cd "$(dirname "$0")/.."
scenes=("$@")
[ ${#scenes[@]} -eq 0 ] && scenes=(nav-pins favorites role-popout mod-view-loading mod-view server-settings-members server-settings-members-filter server-settings-members-bulk)
cargo build -q || exit 1
bin="${CARGO_TARGET_DIR:-target}/debug/waffle"

pid=""
stop() { [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null; pid=""; }
trap 'stop; exit' INT TERM EXIT

i=0
while :; do
    stop
    WAFFLE_DEMO="${scenes[$i]}" "$bin" >/dev/null 2>&1 &
    pid=$!
    printf '\r\033[K[%d/%d] %s   ← → to switch, q to quit' $((i + 1)) ${#scenes[@]} "${scenes[$i]}"
    while :; do
        IFS= read -rsn1 key || exit
        if [ "$key" = $'\e' ]; then
            read -rsn2 rest
            key="$key$rest"
        fi
        case "$key" in
            $'\e[C' | l) i=$(((i + 1) % ${#scenes[@]})); break ;;
            $'\e[D' | h) i=$(((i - 1 + ${#scenes[@]}) % ${#scenes[@]})); break ;;
            q) echo; exit ;;
        esac
    done
done
