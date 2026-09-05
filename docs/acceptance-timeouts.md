# Acceptance deadlines

Native acceptance commands and browser subprocesses use separate wall-clock
limits: `TITAN_RUNTIME_TIMEOUT_SECONDS` defaults to 60 seconds and
`TITAN_BUILD_TIMEOUT_SECONDS` defaults to 1200 seconds. Set positive finite
seconds in the environment to accommodate cold builds or to diagnose a hang.
Build limits cover Cargo build/test/check/clippy/metadata, tool installation,
and packaging; runtime limits cover CLI requests, replay verification and
browser test subprocesses. These are hang bounds, not performance thresholds.
Existing game assertions and exact reference checksums remain authoritative.

The Python acceptance process helper starts owned processes in POSIX process
groups. On timeout or final cleanup it requests termination, escalates to a kill,
and bounds output draining even when descendants retain pipes after the direct
child exits. It removes only registrations matching the owned process and
project/instance identity. Temporary projects still clean themselves up normally.
The graceful-shutdown acceptance assertions run before forced registration cleanup,
so they continue to verify the runtime removes its own registration. Nested
helpers inherit an absolute deadline with cleanup headroom; owned children are
also stopped on harness exit or termination signals.

This support targets the existing Linux/macOS acceptance hosts; it does not
claim cancellation of engine work merely because a CLI transport request timed
out. Harness cleanup explicitly stops its owned host after a failed request.

CI jobs have a 55-minute limit, below the merge queue's 60-minute check deadline.
Every shell step shares a 45-minute command budget measured from the first run
step, including subsequent action/cache time. Once exhausted, later commands
fail immediately. This reserves ten minutes within the job limit for failure
artifact actions and cleanup (evidence collection is owned by issue #35).
Setup actions before the first shell step still count toward GitHub's job limit;
runner scheduling and service outages cannot be bounded by a repository script.
Required checks and semantic coverage are not skipped to meet the budget.

Verify deliberately hanging processes, descendant cleanup, output-pipe hangs and
registration isolation with `python3 scripts/test-acceptance-process.py`; verify
browser subprocess cleanup with `node --test scripts/acceptance_process.test.mjs`.
The swarm measurement runner retains its explicit `--timeout-seconds` control
and direct-child RSS measurement, with the same owned-group final cleanup.
