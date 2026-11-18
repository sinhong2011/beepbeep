#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# End-to-end engine checks against scripts/rangeserver.py (plan §30-31).
# Every scenario must finish with a SHA-256 equal to the served file's.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BEEP="$ROOT/target/debug/beep"
WORK="${1:-$(mktemp -d)}"
PORT=$(python3 -c "import socket; s=socket.socket(); s.bind((\"127.0.0.1\", 0)); print(s.getsockname()[1])")
pass=0; fail=0

# Starts the server and waits until it listens (generating the test file can take a while on a busy machine).
server() {
  python3 "$ROOT/scripts/rangeserver.py" --port $PORT "$@" > "$WORK/server.log" 2>&1 & SRV=$!
  for _ in $(seq 1 100); do grep -q '^ready' "$WORK/server.log" 2>/dev/null && return; sleep 0.1; done
  echo "server did not start"; cat "$WORK/server.log"
}
stop_server() { kill $SRV 2>/dev/null; wait $SRV 2>/dev/null; }
served_sha() { grep -o '[0-9a-f]\{64\}' "$WORK/server.log" | tail -1; }
# Runs beep with a watchdog so a hang fails the scenario instead of blocking.
beep() { "$BEEP" "$@" > "$WORK/beep.log" 2>&1 & local p=$!; ( sleep 90; kill -9 $p 2>/dev/null ) & local w=$!; wait $p; local rc=$?; kill $w 2>/dev/null; return $rc; }
check() {
  local name=$1 want=$2
  local got; got=$(grep -o 'sha256 [0-9a-f]*' "$WORK/beep.log" | tail -1 | cut -d' ' -f2)
  if [[ -n "$got" && "$got" == "$want" ]]; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name (want $want got ${got:-none})"; tail -8 "$WORK/beep.log"; fail=$((fail+1)); fi
}

rm -rf "$WORK/out" "$WORK/state"; mkdir -p "$WORK/out"

server; beep get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" -c 8 --sha256; check "clean 64 MB, 8 connections" "$(served_sha)"; stop_server
server --drop 0.2 --err 0.1; beep get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" -c 8 --sha256; check "20% dropped connections + 10% HTTP 503" "$(served_sha)"; stop_server
server --stall 0.05 --size 32; beep get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" -c 8 --sha256; check "5% stalled connections (half-open sockets)" "$(served_sha)"; stop_server
server --no-range --size 16; beep get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" --sha256; check "server without Range support" "$(served_sha)"; stop_server

# Interrupt mid-download (SIGINT), then run again: must resume, not restart.
server --rate 1500
"$BEEP" get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" -c 4 --state "$WORK/state" > "$WORK/run1.log" 2>&1 & P=$!
sleep 3; kill -INT $P; wait $P
saved=$(grep -o '"received": [0-9]*' "$WORK/state/tasks.json" | awk '{s+=$2} END {print s+0}')
echo "      interrupted with $saved bytes saved"
beep get -o "$WORK/out" -c 4 --state "$WORK/state" --sha256
check "resume after interrupt" "$(served_sha)"
grep -q "Recovered after restart\|Starting" "$WORK/state/tasks.json" && [[ $saved -gt 0 ]] || { echo "FAIL  resume did not keep progress"; fail=$((fail+1)); }
stop_server

# The remote file changes between runs: the stale partial must be discarded.
rm -rf "$WORK/state"
server --rate 1500
"$BEEP" get "http://127.0.0.1:$PORT/file.bin" -o "$WORK/out" -c 4 --state "$WORK/state" > "$WORK/run1.log" 2>&1 & P=$!
sleep 2; kill -INT $P; wait $P; stop_server
server --mutate-after 1
beep get -o "$WORK/out" -c 4 --state "$WORK/state" --sha256
check "remote file changed between runs" "$(served_sha)"
stop_server

echo "$pass passed, $fail failed"
[[ $fail -eq 0 ]]
