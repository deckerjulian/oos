# ADR 0061: A service of the screen has one shape, and the frame loop polls the parts from one list

- Status: proposed
- Date: 2026-11-18
- Category: code

## Context

The screen never waits (ADR 0022): what takes longer than a frame runs on a thread of its own. There are two kinds.

- **A job**: one computation from inputs to a result, such as a decode of a long reference or a SIM generation.
  `Worker<J, R>` in `ui/src/ui/worker.rs` is the one shape for it, used by nine modules.
- **A service**: a thread that lives as long as a page needs it, keeps a connection or a state of its own and reports
  more than once: the WLAN control (`wlan`), Bluetooth (`bt`) and the clock (`ntp`). Each of the three modules has its
  own copy of the same pattern: a job channel, a report channel, a thread started on first use, a `busy` text for the
  toasts and a `poll` that takes the reports. The copies differ in small ways that are not decisions: when the thread
  starts, whether it ends when the page closes, what happens when it cannot start.

Two jobs do not use `Worker<J, R>` either: the firewall run of the remote access list (`net-allow`, a thread and a
channel of its own) and the update (a child process). Every part that has something to take on each frame has a
`*_poll` method, and `Ui::step` in `acquire.rs` calls 17 of them by name, in an order that nothing explains; others
are called from `sim.rs`, `eyepage.rs`, `hwmask.rs` and further down in `acquire.rs`. A new part adds its call by
hand; when the call is forgotten, the part silently never updates.

## Decision

- **A service is a `Service<J, R>`** in `ui/src/ui/service.rs`, beside `Worker<J, R>`: `start` with a thread name and
  a worker function (once; a thread that cannot be started is tried again on the next `start`), a job channel
  (`send`), a report channel (`take`, which never blocks), and an end when it is dropped. No thread is started in
  `cargo test`, so a test never reaches a daemon of the computer it runs on. What differs between services is their
  worker function and when their part calls `start`; what a part shows while a job runs stays the part's.
- **The parts are polled from one list.** `POLLS` in `ui/src/ui/acquire.rs`, beside `Ui::step`, holds the polls of
  the services and the hardware probes, in one order; `step` runs the list and names none of them. The ticks of the
  screen's own timers (`math_fit_tick`, `meas_hold_tick`, `workspace_hold_tick`, `log_tick`) are no parts with a
  thread or a probe; they stay calls in `step`, where their place in the frame is part of what they do.
- **The check.** `every_poll_is_in_polls` fails on a `fn *_poll(&mut self)` of the screen that is neither in `POLLS`
  nor in `POLLED_ELSEWHERE`, the polls called from another place on purpose, each with that place; that list may
  only get shorter. The recipe "a service" goes into AGENTS.md ("Adding something") with this test.
- **A job is a `Worker<J, R>`**, the firewall run included; the update stays a child process, which is what
  `oos-update` is, and its poll is a line of `POLLS` like every other.
- **The move.** The three services in one pull request, then the firewall run, then `POLLS`, each a refactoring that
  keeps every pixel (ADR 0014); the modules' own channels go with them.

## Alternatives

- **An async runtime** (tokio, as a fork of OOS chose for its network layer). It is a dependency the scope's programs
  do not have (ADR 0013, ADR 0021), and the services here are a handful of threads that wait on a socket or a file;
  a thread each is what they need.
- **Making `Worker<J, R>` serve both kinds.** A job has one result and is resubmitted with the newest inputs; a
  service has many reports and a state of its own. One type for both would need modes, which is what the copies are
  today.
- **Leaving the polls as calls.** The order would stay unexplained and a new part could still be forgotten.

## Consequences

- A new service is its worker function and its line in `POLLS`; it cannot be forgotten in `step`.
- The four copies of the channel code go.
- How a service starts, ends and behaves in the tests is one decision for all of them, in `service.rs`.
- The order of the polls is written down; a change of it is visible in a diff of one list.
- Worth a second look if a service needs to wait on several sources at once in a way a thread per service handles
  badly.

## References

`ui/src/ui/worker.rs`, `ui/src/ui/acquire.rs` (`Ui::step`), `ui/src/ui/bt.rs`, `ui/src/ui/wlan.rs`,
`ui/src/ui/time.rs`, `ui/src/ui/remote.rs`, `ui/src/ui/service.rs` (#231), ADR 0013, ADR 0021, ADR 0022, ADR 0046.
