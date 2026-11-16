# ADR 0027: Automatic restarts are armed only after the boot selection is back on stock

- Status: proposed
- Date: 2026-11-16
- Category: system

## Context

A panic halted the scope until someone pulled the power, and a kernel that hung stayed hung. The scope runs unattended,
so it should restart by itself: a panic restart (`kernel.panic`) and the hardware watchdog (dw_wdt, `oos-watchdog`).

The stock loader starts whatever the boot magic selects. While an OOS image is selected (`OOSK`), a restart starts it
again. `oos-oneshot`, the first program of rcS, puts `KRNL` back; the trial count and the rollback (ADR 0012) run
after it. An image that fails before that line, with a restart armed from the kernel command line, would restart into
itself forever, with nothing counting the starts. Taking the card out would be the only way back.

## Decision

The automatic restarts are armed from rcS, only after `oos-oneshot` has checked any restore write and sync and
read a complete sector with `dd iflag=direct` showing `KRNL` (and matching the intended sector after a restore):
then a restart leads to stock, or to an OOS that a confirmed start armed again. Before that, and when the magic could not be
restored, a panic halts as it always did, and rcS writes a boot warning to the start-up status. The initial
`KRNL` check also uses a complete, checked direct read. `softlockup_panic` is enabled with `kernel.panic` in the
success branch, and kexec drops both panic settings from the old command line.

## Alternatives

- `panic=10` on the kernel command line: covers the earliest panics, but loops on an image that fails before rcS.
- A start counter in the loader: OOS has no loader of its own (ADR 0011).

## Consequences

- A panic in the kernel or the initramfs before rcS still needs a person, as before.
- A kexec start carries a running watchdog into the new kernel (no reset line on the node): the trial start
  (`docs/update.md`, "The watchdog and kexec") has to fit into its period. The previous build is still in `recovery`,
  so a reset there brings it back, never the new image.
- Worth a second look if OOS gets its own start counting before rcS.

## References

`rootfs/initramfs/etc/rcS`, `rootfs/initramfs/bin/oos-oneshot`, `rootfs/initramfs/bin/oos-watchdog`,
`tools/selfheal-sim.sh`, docs/boot.md ("Restart after a panic or a hang").
