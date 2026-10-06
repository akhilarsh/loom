# Daemon Fork After Threads

> Fork after threads, early ready byte

## The Daemon Was Forked After Threads Existed, and the Ready Byte Went Out Before the Crash (2026-10-06)

**What happened:** on macOS `loom run` printed its banner and the daemon never came up, yet `loom run`
exited 0 (issue #21).

**Why:** the daemon was a double `fork()` of a process that had already run threads (`run_bounded` reader
threads). The grandchild touched CoreFoundation (reqwest's system-proxy lookup in the quota poller) and
the Objective-C runtime aborted it. The success byte was written before the grandchild could die, so the
parent reported a daemon that was already gone. Only the child of a fork keeps the forking thread, so any
lock another thread held at the fork stays held forever.

**Prevention:** never fork after threads in Rust; re-execute the binary into a clean process with an
allowlisted environment. Write the readiness byte after the socket is bound and require the child to stay
alive through a grace period. A `SAFETY` comment that claims a fork is safe needs a reproduction, and
silencing a runtime's abort does not fix its cause.

**Stopgap that was replaced:** an outside PR re-executed every macOS `loom run` with
`OBJC_DISABLE_INITIALIZE_FORK_SAFETY=YES` (the `objc_fork_safety` module of `commands/run`). It silenced objc's abort in
the forked grandchild; the fork after threads and the early ready byte stayed, and the author had not
reproduced the fix end to end.

**Fix:** the fork path and the `objc_fork_safety` module are deleted; `daemon/server/launch.rs` re-executes
`loom run --daemon-child <root>` and `await_ready` waits for `0x01` plus a grace
([Launching the Daemon](../architecture/daemon-launch.md)).
