# Restore burst benchmark

**Date:** 2026-09-04 · **Command:** `cargo test restore_burst --release -- --ignored --nocapture`

## What it measures

The benchmark (`restore_burst_benchmark` in `src/fake_niri.rs`, `#[ignore]`d
so it never slows the normal suite) restores a 30-window session spread over
10 apps and 3 workspaces against the fake niri IPC server. It isolates
**this daemon's own overhead** — session parsing, idempotent planning,
spawn/request round-trips over a real Unix socket, placement calls — from
real compositing work, which the fake does not simulate.

## Results

```
BENCH: 30 windows in 3.011s (10 windows/s, 100.4 ms/window)
```

## Interpretation

- The 100 ms/window floor is dominated by the **poll quantum**: after each
  spawn, the code waits 500 ms before the first poll for the new window
  (`wait_for_new_window`), and every window is confirmed on the first poll.
- Throughput scales with apps: windows of *different* apps spawn in parallel
  (global cap 5), while windows of the *same* app are intentionally
  serialized (`SpawnLimiter`) to prevent workspace swaps.
- Against a real compositor the latency picture is the same order: niri
  needs some hundreds of milliseconds to map a window, so the 500 ms poll
  quantum is the sensible knob — halving it buys faster placement at 2× the
  IPC request volume.

## Method notes

- `ipc_config()` uses `spawn_timeout = 1` (2 polls); the benchmark's windows
  appear immediately after the spawn reply.
- Re-run after touching `plan_spawns`, `SpawnLimiter`, or the IPC client path
  to catch throughput regressions.
- Suite time is unaffected: the benchmark is `#[ignore]`d.
