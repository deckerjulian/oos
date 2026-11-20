# ADR 0056: Add-ons run outside the core and use a stable extension interface

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: code

Touches the system (the image, the network services, ADR 0025) and the hardware (what OOS never writes, ADR 0009).

## Context

The maintainer's goal for milestone M6 (issue #206): add-ons work without being merged into the core. They get a
stable extension interface, as GUIs and integrations get the remote interface (milestone M5, ADR 0055).

What there is, read on 2026-11-18 (main at dd2eadc6):

- **An extension is compiled in.** A decoder, a measurement and an analyser page are modules behind a registry in
  `oos-ui` (ADR 0002), a trigger type, a setting and a remote command have their one place too, and every kind has a
  recipe and a check (ADR 0046). The registries make an addition cheap for the tree; they do not help someone whose
  addition is not wanted in the tree, or not yet: it waits for a review and a release, or lives in a fork.
- **Compiled plugins are no way out.** Rust has no stable ABI: a module built against one `oos-ui` does not fit the
  next. The programs for the scope are static binaries for `aarch64-unknown-linux-musl` (ADR 0013), and a static
  musl program cannot load a shared object at all. A C ABI on top would still run the plugin inside `oos-ui`: as
  root (`rootfs/initramfs/etc/rcS` starts it; `/etc/passwd` has no other user), in the process that owns the frame
  loop and the state without locks (ADR 0022), the board and its register window. A panic or a stray pointer there
  stops the screen, and a wrong write can reach the hardware that ADR 0009 and ADR 0010 protect.
- **A process of its own works on the scope.** The GPU helper `oos-gpu` (ADR 0003) is a separate program with a
  socket; any failure ends the helper and `oos-ui` goes on drawing on the CPU. The web remote's pictures are made on
  the slow cores with spare time only, so a client never costs the screen a frame (`ui/src/http.rs`).
- **What the kernel offers for limits** (`kernel/config`): control groups with memory, pids and cpusets
  (`CONFIG_MEMCG`, `CONFIG_CGROUP_PIDS`, `CONFIG_CPUSETS`), namespaces (`CONFIG_USER_NS`, `CONFIG_PID_NS`,
  `CONFIG_NET_NS`) and `CONFIG_SECCOMP`; no security module (`CONFIG_SECURITY` is not set, so no Landlock).
  Whether the image mounts the control groups, and who may open the device files of the FPGA, is not yet checked on
  a unit.
- **There is no interpreter on the scope** beyond the shell of the initramfs: an add-on is a program for aarch64
  Linux or a shell script.

## Decision

- **An add-on is a program of its own,** outside the tree's crates, with a manifest. The manifest names the add-on,
  its version, the interface version and the extension endpoints it uses, what it contributes, the grants it asks
  for (below), and its limits.
- **Three places on the data partition, two of them the core's** (paths proposed):
  - `/oos/addons/<id>/`: the program and the manifest, as installed. Owned by root, read only for the add-on. The
    manifest the supervisor reads is this one, so an add-on cannot give itself grants or limits.
  - `/oos/addon-settings/<id>`: the add-on's settings, which `oos-ui` keeps. Owned by root; the add-on cannot write
    it and reads its settings over the interface.
  - `/oos/addon-data/<id>/`: the one place the add-on writes, owned by its user. `oos-ui` never writes into it, so a
    link the add-on puts there cannot lead a write of `oos-ui` onto `/oos/system.conf` or anything else.
- **One user per add-on**, made when it is installed: no root, no capabilities, no other add-on's files. The user is
  the add-on's identity.
- **OOS starts and stops it.** Each add-on has a switch, off after installing; switching it on shows the grants of
  its manifest. A switched-on add-on is started after `oos-ui` and stopped with it or with its switch. When it ends
  on its own it is started again a few times, then left off with a message on the screen. Its end never ends
  `oos-ui`.
- **Each add-on has a socket of its own.** The supervisor makes a Unix socket per add-on in a folder of the core
  (for example `/tmp/oos-addons/<id>`), which only that add-on's user may open, and hands its path to the add-on.
  `oos-ui` also reads the peer's user from the socket (`SO_PEERCRED`) and drops a connection whose user is not the
  add-on of that socket. An add-on never names itself in a message, so it cannot speak for another.
- **The socket carries an allowlisted surface, not the whole remote interface.** What is on it:
  - the **extension endpoints**, part of the interface of ADR 0055 with its version and marks (they start
    experimental), the protocol left to the issues of the milestone: subscribing to the records (or the frames) as
    the scope accepts them, with their scale and time base; contributing the packets of a decoder for the bus lane
    and the decode table, the values of a measurement for its tiles, a page or an overlay that `oos-ui` draws from
    what the add-on sends (text, lines, traces, areas, in the named colours of ADR 0008: data, not pixels); its own
    settings, declared in the manifest, which `oos-ui` shows in a menu of the add-on;
  - SCPI queries, by the grant `read`, which every add-on has;
  - SCPI setters of the scope's settings, by the grant `control`, which the manifest has to ask for.

  The interpreter knows the class of each command; the socket runs only the classes its add-on was granted. Never on
  it, whatever the manifest says: `/cmd` and every other HTTP route, `POST /update` and MCP, the development
  commands, `:SYSTem:SET`, and the commands that change the system, the network, the calibration or files (a class
  of their own). The socket is no network transport and leads to nothing the network may not reach: `net.allow`
  (ADR 0025) is not gone around, because the add-on offers no network service of its own and nothing on its socket
  is beyond what the bench LAN may do.
- **`oos-ui` never waits for an add-on.** A request is answered in the frame loop without waiting, as every
  transport is (ADR 0022); what an add-on sends is shown on a later frame; an add-on that falls behind loses records
  instead of holding the scope back.
- **What an add-on may not do**, by its socket and by its user:
  - touch the hardware: no register window, sample stream, front-end bus, GPIO or panel UART. Hardware stays with
    `oos-hal`, the board of `oos-ui` and `oos-fpgad` (ADR 0001); an add-on that needs a register sequence brings it
    into the tree. So no trigger type in hardware as an add-on.
  - write the protected areas (ADR 0009), any block device of the card, `/oos/system.conf`, its own program,
    manifest or settings, or another add-on's places; it writes `/oos/addon-data/<id>/` only.
  - offer a network service, or pass requests from the network on to its socket.
- **The sandbox is what the scope can hold:** the user of its own, resource limits (memory, processes, open files)
  through `setrlimit` and the control groups, and a place on the slow cores at low priority. It keeps a faulty
  add-on from the screen, the hardware and the card. It does not make a hostile add-on harmless: one granted
  `control` can change what any client on the bench LAN can (ADR 0025). Installing an add-on is the owner's choice,
  as running a script on the bench computer is. A seccomp filter or namespaces can tighten it later without changing
  the interface.
- **Versioning is ADR 0055's, endpoint by endpoint.** On connecting, `oos-ui` tells the add-on the interface version
  and the endpoints it serves with their marks. The manifest names the endpoints the add-on uses. For a stable one
  the major version it was written for is enough, and a later minor of that major is fine. For an experimental one
  the manifest names the exact interface version it was written against, and `oos-ui` starts the add-on only on that
  version, since an experimental entry may change with any minor (ADR 0055). An add-on that cannot be started says
  so on the screen, with the endpoint and the versions.
- **The registries of ADR 0002 stay** for what ships in the tree, with their recipes and checks (ADR 0046). The
  modules in the tree are not rewritten as add-ons. An add-on appears in the same places through one entry per kind
  in the registry that forwards to the add-ons, compiled in and checked like the others. Stored identifiers of
  add-ons are their own (`<add-on id>:<key>`) and never collide with those of the tree.

## Alternatives

- **Plugins loaded into `oos-ui`** (shared objects with a C ABI). Not possible in a static musl program without
  giving up the static build of ADR 0013, and they would run as root inside the frame loop: a crash or a wrong write
  of a plugin is a crash or a wrong write of the scope (ADR 0009, ADR 0022).
- **Rust modules built against `oos-ui`.** No stable ABI: every add-on rebuilt for every release, which is a fork by
  another name.
- **WebAssembly in `oos-ui`** (a runtime such as wasmtime or wasmi). A real sandbox and add-ons that run on every
  host. It is a large dependency in the scope's program, against ADR 0013, and its work would run beside the frame
  loop in the same process. Not now; worth a second look if a small runtime written in the tree or a dependency
  accepted for it appears. It can also come later as an add-on itself: a process that hosts WebAssembly add-ons
  over the same interface.
- **An embedded script language** (Lua and the like). The same dependency, or an interpreter of our own to write and
  keep; and the same process.
- **Merging every addition into the tree** (today). It keeps the tree whole and makes every addition wait for its
  review; additions for one bench or one product never get in.
- **Add-ons only on a computer, over the network.** Possible with ADR 0055 alone, and still allowed. It cannot work
  on a scope standing alone, and a contribution to the screen would have to cross the network and `net.allow`.

## Consequences

- Someone can write a decoder, a measurement or a page in any language that builds for aarch64 Linux, install it on
  a scope and update it on their own schedule, without a pull request to the core.
- A crash of an add-on costs the add-on, not the screen; an add-on that is slow costs its own records.
- An add-on is slower than a module: records cross a socket, and the scope's memory and slow cores are shared. A
  decoder that must keep up with every frame may still belong in the tree.
- The interface grows by the extension endpoints, which have to be kept stable like every other stable entry
  (ADR 0055); its first versions are experimental on purpose.
- The interpreter gets a class per command (read, control, system and the like), declared in the table that the
  commands come from and kept complete by a check (ADR 0046), so a new command cannot reach an add-on by default.
- The image grows by the supervision of add-ons and a user per add-on, which is a change of `rootfs/` and is tried on
  a unit (ADR 0052).
- The milestone needs one example add-on outside the tree's crates that uses the interface, documented and tested
  like the remote interface (issue #206).
- Worth a second look when the kernel gets a security module, when a WebAssembly runtime fits ADR 0013, or when an
  add-on needs something the out-of-process interface cannot carry at the scope's speed.

Open questions for the maintainer, to be answered in the issues of the milestone:

1. Which kinds come first: data only (decoder packets, measurement values), or pages and overlays from the start?
2. May the extension endpoints also be reached from a computer (under `net.allow`, with an identity other than a
   user on the scope), so an add-on can run there? This record allows only the socket on the scope.
3. The form of the extension messages: SCPI commands under a subsystem of their own with binary blocks for data,
   or JSON lines (`oos-ui` has its own JSON code), or both?
4. Who supervises the add-ons: `oos-ui` as their parent, or a small supervisor started by `rcS`?
5. Installing: copying over SSH or from a USB stick, an upload on the web page, or both? Are add-on packages
   signed (the keys of ADR 0012, or keys of their own), or unsigned with a warning?
6. Does the image mount the control groups for the limits, and which limits does an add-on get when its manifest
   names none?
7. Which grants come after `read` and `control`: writing to a USB stick, files of the scope's storage, a network
   service of its own (which would need its switch and the `net.allow` check, ADR 0025)?
8. What a setup does that names an add-on that is not installed or is switched off.
9. Where the example add-on lives: a folder of this repository outside the crates, or a repository of its own?

## References

Issue #206, issue #205; ADR 0001, ADR 0002, ADR 0003, ADR 0008, ADR 0009, ADR 0010, ADR 0012, ADR 0013, ADR 0022,
ADR 0025, ADR 0046, ADR 0052, ADR 0055; [architecture.md](../../architecture.md);
[protocol-modules.md](../../protocol-modules.md), [measurement-modules.md](../../measurement-modules.md),
[analyser-modules.md](../../analyser-modules.md); `ui/src/protocols/mod.rs`, `ui/src/measures/mod.rs`,
`ui/src/ui/analysers.rs`, `ui/src/gpu.rs`, `ui/src/http.rs`, `ui/src/netacl.rs`, `kernel/config`,
`rootfs/initramfs/etc/rcS`, `rootfs/initramfs/etc/passwd`.
