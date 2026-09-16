#!/usr/bin/env bash
# Real-hardware soak test for niri-session-manager, run against the LIVE niri
# compositor on the daily driver. Proves what the in-repo fake server cannot:
# real niri-event timing, the up-front state-sync burst, terminal-state
# capture against real process trees, and idempotent restore with real
# windows.
#
# DISRUPTION NOTICE: this script changes YOUR desktop while it runs —
# focus toggles, workspace round-trips, several self-closing
# `ghostty -e sleep N` windows, and one self-closing kitty carrier window
# (~4 minutes total). Original focus and workspace are restored; every
# spawned window closes itself.
#
# Usage:
#   scripts/soak-test.sh [phase] [path-to-binary]
#   phase: all (default) | a (reactive saves) | b (session vs live) | c (restore proof)
#
# The binary defaults to target/release/niri-session-manager. All manager
# state is isolated under a scratch XDG dir — the deployed service's files
# are never touched.
#
# Exit code = number of failed assertions (0 = green).
set -uo pipefail

PHASE="${1:-all}"
BIN="${2:-$(dirname "$0")/../target/release/niri-session-manager}"
NMSG="${NMSG:-niri}"
SCRATCH="${SOAK_SCRATCH:-/tmp/nsm-soak}"

mkdir -p "$SCRATCH"
export XDG_DATA_HOME="$SCRATCH/data"
export XDG_CONFIG_HOME="$SCRATCH/config"
LOG="$SCRATCH/soak.log"
TIMELINE="$SCRATCH/timeline.log"
SESSION="$SCRATCH/data/niri-session-manager/session.json"
MARKER="$SCRATCH/data/niri-session-manager/restore-marker"

if [ -z "${NIRI_SOCKET:-}" ]; then
	# The compositor itself does not export NIRI_SOCKET; discover the socket
	# from its canonical runtime location instead.
	NIRI_SOCKET="$(ls /run/user/$(id -u)/niri.wayland-*.sock 2>/dev/null | head -1 || true)"
fi
if [ -z "${NIRI_SOCKET:-}" ]; then
	echo "ERROR: NIRI_SOCKET not set and no running niri found" >&2
	exit 1
fi
export NIRI_SOCKET
echo "niri socket: $NIRI_SOCKET"
echo "manager:     $BIN ($("$BIN" --version 2>/dev/null || echo '?'))"

FAILURES=0
check() { # check <label> <actual> <expected>
	if [ "$2" = "$3" ]; then
		echo "PASS: $1 ($2)"
	else
		echo "FAIL: $1 (got '$2', want '$3')"
		FAILURES=$((FAILURES + 1))
	fi
}
check_ge() {
	if [ "$2" -ge "$3" ]; then echo "PASS: $1 ($2 >= $3)"; else
		echo "FAIL: $1 (got '$2', want >= $3)"
		FAILURES=$((FAILURES + 1))
	fi
}
check_le() {
	if [ "$2" -le "$3" ]; then echo "PASS: $1 ($2 <= $3)"; else
		echo "FAIL: $1 (got '$2', want <= $3)"
		FAILURES=$((FAILURES + 1))
	fi
}
mark() { printf '%s %s\n' "$(date +%s.%N)" "$1" >>"$TIMELINE"; }
focus() { $NMSG msg action focus-window --id "$1"; }
nwindows() { $NMSG msg -j windows 2>/dev/null | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'; }
nwindows_of_app() { $NMSG msg -j windows 2>/dev/null | python3 -c "import json,sys; print(sum(1 for w in json.load(sys.stdin) if w['app_id'] == '$1'))"; }
saves_so_far() { grep -c "Session saved to" "$LOG" 2>/dev/null || true; }

phase_a() {
	echo "=== Phase A: reactive saves against live niri ==="
	: >"$LOG"
	: >"$TIMELINE"
	rm -f "$SESSION" "$MARKER"

	mark "start-manager"
	"$BIN" --save-only --save-interval 15 >>"$LOG" 2>&1 &
	local MGR=$!
	sleep 4
	# niri pushes the full state-sync burst right after accepting the
	# subscription; it is layout-relevant, so the first save must land ~2s in.
	check "state-sync burst produced the first save (file exists)" "$(test -e "$SESSION" && echo exists || echo absent)" "exists"
	check_ge "burst save logged within 4s" "$(saves_so_far)" "1"
	check_le "at most burst+drift saves by 4s" "$(saves_so_far)" "2"

	local S1
	S1=$(saves_so_far)
	mark "focus-3"
	focus 3
	sleep 0.5
	mark "focus-2"
	focus 2
	sleep 5
	check_le "focus round-trip collapsed to <=1 save" "$(($(saves_so_far) - S1))" "1"

	local S2
	S2=$(saves_so_far)
	mark "burst-start"
	local id
	for id in 3 2 3 2 3; do
		focus "$id"
		sleep 0.3
	done
	mark "burst-end"
	sleep 5
	check_le "toggle burst collapsed to <=1 save" "$(($(saves_so_far) - S2))" "1"

	mark "focus-2-restore"
	focus 2
	sleep 5

	local S3
	S3=$(saves_so_far)
	mark "spawn-8s-1"
	$NMSG msg action spawn -- ghostty -e sleep 8
	sleep 6
	mark "after-open"
	local S3_OPEN
	S3_OPEN=$(saves_so_far)
	sleep 8
	mark "after-close"
	local S3_CLOSE
	S3_CLOSE=$(saves_so_far)
	check_ge "window open produced a save" "$((S3_OPEN - S3))" "1"
	check_ge "window close produced a save" "$((S3_CLOSE - S3_OPEN))" "1"

	local S4
	S4=$(saves_so_far)
	mark "ws-roundtrip"
	$NMSG msg action focus-workspace 3
	sleep 1
	$NMSG msg action focus-workspace 4
	sleep 5
	check_le "workspace round-trip <=1 save" "$(($(saves_so_far) - S4))" "1"

	local S_IDLE
	S_IDLE=$(saves_so_far)
	local MTIME_START
	MTIME_START=$(stat -c %Y "$SESSION" 2>/dev/null || echo none)
	mark "idle-start"
	sleep 50
	mark "idle-end"
	check "idle: no saves" "$(saves_so_far)" "$S_IDLE"
	check "idle: mtime unchanged" "$(stat -c %Y "$SESSION" 2>/dev/null || echo none)" "$MTIME_START"

	local S5
	S5=$(saves_so_far)
	mark "spawn-8s-2"
	$NMSG msg action spawn -- ghostty -e sleep 8
	sleep 14
	check_ge "second open/close cycle saved" "$(($(saves_so_far) - S5))" "1"

	mark "focus-3b"
	focus 3
	sleep 4
	mark "focus-2b"
	focus 2
	sleep 4

	# Carrier: alive at the final save, dead shortly after — Phase C's deficit.
	mark "spawn-45s-carrier"
	$NMSG msg action spawn -- ghostty -e sleep 45
	sleep 6

	mark "sigterm"
	kill -TERM "$MGR"
	local WAIT_RC=0
	wait "$MGR" || WAIT_RC=$?
	mark "manager-exited"
	check "manager exit code on SIGTERM" "$WAIT_RC" "0"
	check "final-save log present" "$(grep -c "Final session saved" "$LOG")" "1"
	check "graceful shutdown log" "$(grep -c "Shutdown complete" "$LOG")" "1"

	check "zero stream deaths (IPC drift regression)" "$(grep -c "event stream ended" "$LOG")" "0"
	check "zero unparsable-line warnings" "$(grep -c "unparsable" "$LOG")" "0"

	check "final session holds carrier window" \
		"$(python3 -c 'import json;print(len(json.load(open("'"$SESSION"'"))["windows"]) > 0)' 2>/dev/null || echo none)" "True"
	check "carrier terminal state captured (sleep 45)" \
		"$(python3 -c 'import json;d=json.load(open("'"$SESSION"'"));print(any(w.get("terminal_state") and w["terminal_state"].get("child_command")==["sleep","45"] for w in d["windows"]))')" "True"

	echo "--- Phase A summary: total saves $(saves_so_far); log WARN/ERROR lines:"
	grep -E "WARN|ERROR" "$LOG" || echo "  (none)"
}

phase_b() {
	echo "=== Phase B: saved session vs live niri state ==="
	python3 - "$SESSION" "$NMSG" <<'EOF'
import json, subprocess, sys, os

session, nmsg = sys.argv[1], sys.argv[2]
env = dict(os.environ)
live_wins = json.loads(subprocess.run([nmsg, "msg", "-j", "windows"], env=env, capture_output=True, text=True).stdout)
live_ws = json.loads(subprocess.run([nmsg, "msg", "-j", "workspaces"], env=env, capture_output=True, text=True).stdout)
ws_by_id = {w["id"]: w for w in live_ws}
saved = json.load(open(session))
fails = 0

def check(label, ok, detail=""):
    global fails
    print(("PASS" if ok else "FAIL"), label, detail)
    if not ok:
        fails += 1

matched = 0
for sw in saved["windows"]:
    lw = next((w for w in live_wins if w["id"] == sw["id"]), None)
    if lw is None:
        continue
    matched += 1
    ok = (sw["app_id"] == lw["app_id"]
          and sw["idx"] == ws_by_id.get(lw["workspace_id"], {}).get("idx")
          and sw["name"] == ws_by_id.get(lw["workspace_id"], {}).get("name")
          and sw["is_focused"] == lw["is_focused"]
          and isinstance(sw.get("layout"), dict) and "tile_width" in sw["layout"])
    check(f"id {sw['id']} app/workspace/focus/layout match", ok, sw["app_id"])
check("every live window matched a saved entry", matched == len(live_wins), f"matched={matched} live={len(live_wins)}")
print(f"Phase B failures: {fails}")
sys.exit(1 if fails else 0)
EOF
	FAILURES=$((FAILURES + $?))
}

phase_c() {
	echo "=== Phase C: idempotent restore proof (spawns one real carrier) ==="
	local RC=0
	: >"$SCRATCH/restore.log"

	# Close kitty windows left over from a previous phase-C run: the restored
	# carrier intentionally keeps running after its command exits (the restore
	# composition ends with `; exec $SHELL`, so the terminal stays usable), so
	# it lingers as an idle shell and would fill the next run's per-app
	# deficit. This assumes the user has no kitty windows of their own — kitty
	# is chosen as the carrier precisely because this daily driver runs ghostty.
	$NMSG msg -j windows 2>/dev/null | python3 -c 'import json,sys; [print(w["id"]) for w in json.load(sys.stdin) if w["app_id"] == "kitty"]' | while read -r kid; do
		$NMSG msg action close-window --id "$kid" 2>/dev/null || true
	done

	# Self-sufficient proof: take a FRESH capture containing a freshly spawned
	# carrier, so the restore deficit reflects the desktop as it is right now
	# (a live desktop drifts — reusing an older session invites races with the
	# user's own windows). The carrier is a KITTY window: the user's daily
	# terminals are ghostty, so the kitty per-app deficit belongs exclusively
	# to the carrier, and the kitty restore profile gets real-binary coverage
	# for free. (A custom `--app-id` carrier does NOT work: spawn confirmation
	# matches app_id, and profile-launched terminals can't reproduce it.)
	rm -f "$SESSION" "$MARKER" "$MARKER.removed"
	$NMSG msg action spawn -- kitty sleep 45
	sleep 3
	"$BIN" --save-only --save-interval 15 >>"$SCRATCH/restore-save.log" 2>&1 &
	local MGR=$!
	sleep 4
	kill -TERM "$MGR"
	local WAIT_RC=0
	wait "$MGR" || WAIT_RC=$?
	check "phase C capture manager exited cleanly" "$WAIT_RC" "0"

	# The carrier self-closes after `sleep 45`; wait until NO kitty window is
	# live before restoring, so the session's kitty entry is a genuine deficit.
	# The wait must be app-specific, not a window-count drop: OTHER windows can
	# die first (a Phase-A leftover carrier) and would otherwise end the wait
	# while the phase-C carrier is still alive, making the deficit 0. (This and
	# the cleanup above were each a real flake on the live desktop.)
	local DEADLINE=$((SECONDS + 75))
	while [ "$(nwindows_of_app kitty)" -gt 0 ] && [ "$SECONDS" -lt "$DEADLINE" ]; do
		sleep 2
	done
	check "carrier window died before restore proof" "$(nwindows_of_app kitty)" "0"

	local OUT
	OUT=$("$BIN" --dry-run 2>&1)
	RC=$?
	echo "$OUT" >>"$SCRATCH/restore.log"
	check "dry-run exit code" "$RC" "0"
	check "dry-run: no marker written" "$(test -e "$MARKER" && echo yes || echo no)" "no"
	local PLANNED
	PLANNED=$(echo "$OUT" | sed -n 's/.*DRY RUN: would restore \([0-9]*\) window(s).*/\1/p' | head -1)
	check "dry-run planned a positive count" "$([ -n "${PLANNED:-0}" ] && [ "$PLANNED" -gt 0 ] && echo yes || echo no)" "yes"

	local BEFORE
	BEFORE=$(nwindows)
	OUT=$("$BIN" --restore 2>&1)
	RC=$?
	echo "$OUT" >>"$SCRATCH/restore.log"
	check "restore exit code" "$RC" "0"
	check "restore marker written" "$(test -e "$MARKER" && echo yes || echo no)" "yes"
	check "restore spawned exactly the deficit (1)" "$(echo "$OUT" | grep -c 'Restored 1 window(s)')" "1"
	check "live window count +1" "$(($(nwindows) - BEFORE))" "1"

	mv -f "$MARKER" "$MARKER.removed" 2>/dev/null || true
	OUT=$("$BIN" --restore 2>&1)
	RC=$?
	echo "$OUT" >>"$SCRATCH/restore.log"
	check "re-restore exit code" "$RC" "0"
	check "re-restore spawned 0 (idempotent)" "$(echo "$OUT" | grep -c 'Restored 0 window(s)')" "1"

	OUT=$("$BIN" --restore 2>&1)
	RC=$?
	echo "$OUT" >>"$SCRATCH/restore.log"
	check "boot gate skipped third restore" "$(echo "$OUT" | grep -c 'already restored for this boot; skipping')" "1"

	echo "--- restore log WARN/ERROR (stateless-terminal drops are by design):"
	grep -E "WARN|ERROR" "$SCRATCH/restore.log" | grep -v "without captured state" || echo "  (none)"
}

case "$PHASE" in
a | A | all) phase_a ;;
esac
case "$PHASE" in
b | B | all) phase_b ;;
esac
case "$PHASE" in
c | C | all) phase_c ;;
esac

echo "=== soak result: $FAILURES failure(s) ==="
exit "$FAILURES"
