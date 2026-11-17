# ADR 0011: OOS lives in `recovery`; the stock system stays on the card and bootable

- Status: accepted
- Date: 2026-11-07 (first public version; in use before)
- Category: system

## Context

The scope boots from an SD card with the stock loader, which OOS does not replace (ADR 0009). The loader starts the
image in `boot`; when that image fails its check it tries `recovery`, then `backup`. The owner must be able to get back
to the stock system at any time, also when an OOS build does not start.

## Decision

OOS is one boot image (kernel, device tree, initramfs) in the `recovery` partition. The card keeps its stock layout:
the stock kernel and system stay where they are. The switch is the magic at the start of `boot`: `KRNL` starts stock,
`OOSK` makes the loader fall through to `recovery`.

By default OOS starts one-shot: the first program of its start-up writes `KRNL` back, so the next power-on is stock
again unless OOS is chosen once more. System > Boot mode > Persist keeps OOS selected.

## Consequences

- A build that hangs is followed by the stock system at the next power-on, with nothing to repair.
- OOS has no loader of its own, so nothing counts the starts before its start-up script runs; the update path works
  around that with a trial start (ADR 0012).
- The image has to fit `recovery` and has to pass the stock loader's checks (its image format, the ramdisk length a
  multiple of 4).
- `backup` holds a copy of the stock boot image as the fallback, the rollback slot and the `/oos` data.

## References

[boot.md](../../boot.md), [install-usb.md](../../install-usb.md), [architecture.md](../../architecture.md).
