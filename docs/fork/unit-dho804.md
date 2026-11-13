# Unit support: DHO804

Private fork notes for one unit. The mechanisms are generic and documented in the upstream docs; this page lists what
this unit uses, how to build its image, and what is still unverified.

## The unit

- Physically a DHO804: 4 channels, no logic analyser connector, no generator board (an AWG board is planned).
- `vendor.bin` names a DHO924S. OOS takes the identity from it, so it would start with generator and LA.
- Boot card: 64 GB, runs SDR104 under the patched stock kernel.
- LAN 192.0.2.0/24, fixed MAC `02:00:00:00:00:01`.

## Changes

| Area | What | Where |
|---|---|---|
| Capabilities | `/etc/oos/limits.conf` in the image: capability keys that apply from the first start, between vendor.bin and `/oos/limits.conf`. Not experimental; identity unchanged | `hal/src/model.rs` (`UNIT_CONF`, `Platform::build_unit`), docs/models.md 4.2 |
| Generator lines | `afg::Io::open` refuses without a generator; `oos-fpgad gpio` refuses `afg_in1..5` and `FG_M2C_GPIO` without `--force`; `oos-fpgad afg --force` uses `Io::open_forced` | `hal/src/dho/afg.rs` (`LINES`, `line_allowed`), `fpgad/src/main.rs` |
| Boot card | opt-in device tree with `sd-uhs-sdr104`, `sd-uhs-sdr50`, `regulator-settling-time-down-us = <30000>` on `vccio_sd`, no `regulator-init-microvolt` | `kernel/dts/rk3399-rigol-dho900-uhs.dts`, `kernel/build-base.sh` copies every board DTB |
| Firewall | `net.allow` prefixes: nftables ruleset before any service starts; static `nft` in the image | `rootfs/initramfs/bin/oos-firewall`, `rootfs/build-nft.sh`, rcS |
| Remote ACL | the same prefixes in oos-ui: `POST /update`, `/mcp` get 403, SCPI and VXI-11 connections are closed | `ui/src/netacl.rs`, `http.rs`, `scpi.rs`, `vxi11.rs` |
| SSH, no telnet | Dropbear on port 22 replaces telnet: rcS starts `oos-ssh` instead of `telnetd`, the telnet switch and `crate::remote` are gone. Generated root password and host keys in `/oos/ssh`, trusted keys from a USB stick, password login off only with a key. Listens only with `net.allow` and the firewall loaded. Keys `remote.ssh` (default off, unit image on), `ssh.password` (docs/network.md 7) | `rootfs/initramfs/bin/oos-ssh`, `rootfs/build-dropbear.sh`, rcS, `ui/src/ssh.rs`, `ui/src/ui/ssh.rs`, `ui/src/ui/remote.rs`, `tools/ssh-sim.sh` |
| MAC | `eth.mac = cid / vendor / <address>` | `rootfs/initramfs/bin/oos-mac`, `oos-board mac`, rcS |
| Settings lookup | `/etc/oos/system.conf` (image default), then `/oos/system.conf`; last line wins | `rootfs/initramfs/bin/oos-conf`, `netacl::Acl::from_texts` |
| Unit files | `limits.conf` (`afg = no`, `la = no`), `system.conf` (`net.allow = 192.0.2.0/24 169.254.0.0/16`, `remote.ssh = on`, `eth.mac = 02:00:00:00:00:01`), `build.env` (UHS DTB) | `rootfs/unit/dho804/` |
| Fine V/div | upstream's Fine scale already walks the 801 rows of `cal_vertical` (the AFE is set per row, `afe::scale_index`); the fork adds `:CHANnel<n>:VERNier ON\|OFF` (one switch for all channels, as the menu) and `:CHANnel<n>:SCALe` taking any value with vernier on: the first row at or above it | `ui/src/scpi.rs` (`fine_scale`) |
| Image build | the fork's part of `boot/build-dev-image.sh` (unit dir, `build.env`, DTB check, `nft`, unit `*.conf`, `OOS_BOOTARGS_EXTRA`) lives in two sourced hooks; upstream's script carries two lines. The hook stages its container part in `rootfs/out/unit-hook/` (mounted as `/wlan`). A bad `OOS_UNIT` now fails after the Rust builds | `boot/unit-hook.sh`, `boot/unit-hook-image.sh`, docs/fork/sync-checklist.md |

Without `OOS_UNIT` the image behaves like upstream's apart from carrying `nft` (about 1 MB) and Dropbear (450 KB), and
having no telnet. SSH stays off there: no `remote.ssh` and no `net.allow`.

## Building the image

Needs Docker, rustup with `aarch64-unknown-linux-musl`, and the kernel base archive in `kernel/src/` (from an OOS
release, `kernel/README.md`).

```sh
kernel/build-base.sh                                   # kernel + rk3399-rigol-dho900{,-uhs}.dtb into kernel/out/base
OOS_UNIT=dho804 boot/build-dev-image.sh     # boot/out/oos-dev.img, .oosupd, GEL
```

Check the result before it goes anywhere near the scope:

- `boot/out/initramfs.cpio.gz` contains `etc/oos/limits.conf`, `etc/oos/system.conf`, `bin/nft`, `bin/oos-firewall`,
  `bin/oos-conf`, `bin/oos-mac`, `bin/oos-ssh`, `bin/dropbearmulti` with its `dropbear`, `dropbearkey`, `scp` links
  (`gzip -dc ... | cpio -t`).
- The DTB in the image is the UHS one: `fdtget -p boot/out/oos-dho900.dtb /mmc@fe320000` lists `sd-uhs-sdr104`.

The GEL install accepts the model vendor.bin names (DHO924S), so the unit passes the model check. Install on the second SD card only
(private RE notes `docs/oos-review.md`, "If we try it").

When the AWG board is fitted: `afg = yes` in `/oos/limits.conf` (shows as experimental), or remove the line from
`rootfs/unit/dho804/limits.conf` and rebuild. `cal_afg.hex` will be missing until the stock software
calibrates the new board.

## Verified on the host

- `cargo test` in `hal`, `dsp`, `ui`, `fpgad`: all pass (new tests: `model::unit_defaults_tests`,
  `afg::line_tests`, `netacl::tests`).
- `cargo check` of `hal`, `ui`, `fpgad` on linux/arm64 musl (container), so the `target_os = "linux"` paths compile.
- Both device trees preprocess and compile with dtc 1.8.1 against `rk3399.dtsi` and the headers of Rockchip
  `develop-6.12` at 470f9dcc (fetched file by file; not the release archive with the 6.12.115 merges). The
  decompiled difference is exactly the three properties.
- In the same Rockchip tree: RK808 LDO_REG4 has no ramp delay and no `set_voltage_time_sel`, so
  `_regulator_set_voltage_time` returns `settling_time_down` on a falling switch. The 30 ms apply.
- The firewall ruleset loads with the static `nft` into a Linux kernel (arm64 container with `NET_ADMIN`), including
  mixed IPv4/IPv6 lists, host bits in a prefix and an all-invalid list. Packet filtering itself was not exercised.
- `oos-conf`, `oos-firewall --print` and `oos-mac` under busybox `sh`: precedence, empty values, bad prefixes, bad and
  multicast MACs, missing CID; the CID path gives the same MAC as the old rcS code.
- `tools/ssh-sim.sh`: `oos-ssh` with the image's busybox, `nft` and Dropbear against an OpenSSH client (docs/network.md
  7, last paragraph).

## Needs verification on hardware

1. First start: `oos-fpgad model` shows `DHO924S`, caps `250 MHz rated, 4 ch`, `unit afg = no`, `unit la = no`;
   System > System info shows "Unit defaults"; `/sys/kernel/debug/gpio` shows no consumer on `afg_in1..5` and `FG_M2C_GPIO`; no
   `cal_afg.hex` warning in `/tmp/oos-cold.log`.
2. Boot card under the OOS kernel with the UHS DTB: `/sys/kernel/debug/mmc0/ios` with debugfs mounted (timing SDR104,
   1.80 V, 150 MHz), read rate, `dmesg | grep mmc0` without errors, over several cold starts. A failure here can hang the
   kernel before rcS, which leaves `OOSK` set: recovery is card out, `switch.py stock` on a host. Use cold power
   cycles only; a warm restart from stock with the card at 1.8 V is not tested.
3. Firewall on the OOS kernel: `nft list ruleset`, then from 192.0.2.x the web page, SCPI 5555, VXI-11 work; from
   another subnet (or a link-local 169.254.x.y peer, upstream's fallback since 0.1.9) they do not; DHCP renewals and NTP still work.
4. oos-ui ACL: `POST /update` and `/mcp` from an allowed host work. For the refusal path the firewall has to let an
   outside host through: on the scope `nft delete table inet oos_fw`, then `POST /mcp` from another subnet gives 403
   and SCPI 5555 closes at once. Power-cycle afterwards to restore the firewall.
5. MAC: `ip link show eth0` gives `02:00:00:00:00:01`, the DHCP server hands out the expected address.
6. SSH: port 23 closed; port 22 open from 192.0.2.x only; `/tmp/oos-ssh.state` says `on: password and keys` after the
   first start; Show password gives a password that logs in; the host key fingerprint on the page matches the client's;
   the password and the host key are unchanged after a power cycle and after an update; a key imported from a stick
   and one added with `ssh-copy-id` both log in; Password login > Off refused with no key, then keys only works; SSH >
   Off closes port 22 and ends sessions.
7. Fallback path (risk 2 in `docs/oos-review.md`): unchanged by this work, still open.

## Transfer efficiency (spec 10): verification on hardware

What changed and why: `docs/fork/specs/10-transfer-efficiency.md` section 7. Buffer reuse and the fused decode are always
on; the arm ahead, interrupt completion and MaxPayload 256 are off until the steps below pass. Do one switch at a time,
with the baseline of spec 10 section 5 step 1 first.

1. Buffer reuse and fused decode (default build): 10 MHz sine at 100 ns/div, 1 and 4 channels, 10 minutes. No two
   traces, no broken edges, no `c2h` errors in `/tmp/oos-ui.log`, oos-ui does not restart (`/tmp/oos-ui.log` has one
   start). Repeat at 200 us/div with 1 M depth. Compare CPU of `oos-ui` in `top -d 1` with the baseline.
2. Arm ahead: `acq.prearm = on` in /oos/system.conf and restart oos-ui, or `touch /tmp/oos-ui.off`, stop oos-ui,
   run `OOS_PREARM=1 /bin/oos-ui $(cat /tmp/oos-ui.args)`.
   - Waveform rate at 100 ns/div and 1 us/div, 1 and 4 channels, against step 1.
   - Change time base, channel set, trigger level and V/div while running: the picture follows within one batch, no
     `disarm` or `acquire` errors, no 30 ms stall per change (`configure` takes as long as without `OOS_PREARM`).
   - Read 0x4034 right after a disarm (add a Peek if needed): bit 13 set or not decides whether `disarm` can drop its
     10 ms wait.
   - SINGLE in Normal mode with a slow trigger: no frame from before the press.
   - Roll (200 ms/div) and back, self-cal, generator on/off (when fitted) while running.
   - The LA path is not testable on this unit (no LA).
3. Interrupt completion: in `/oos/system.conf` `xdma.params = poll_mode=0`, cold power cycle.
   - `/tmp/oos-cold.log` shows `insmod xdma.ko poll_mode=0`.
   - `dmesg | grep -i -e xdma -e msi`: with `interrupt_mode` unset the driver tries MSI first (the endpoint offers one
     vector). Under the 6.12 kernel MSI goes through the GIC ITS (`msi-map` of the RK3399 PCIe node); stock 4.4 used
     legacy INTx (IRQ 241).
   - MSI enable bit: `od -An -tx2 -j 74 -N2 /sys/bus/pci/devices/0000:01:00.0/config` (MSI capability at 0x48 on the
     stock dump; message control at 0x4a, bit 0 = enabled).
   - `grep xdma /proc/interrupts` twice, 1 s apart, while acquiring: the count rises.
   - The `cmpl_status_th` kernel thread is gone from `top` (it only exists with `poll_mode=1`).
   - No C2H timeout over 10 minutes at 100 ns/div and at 200 us/div. A missing interrupt shows as a read that blocks
     10 s and fails.
   - If MSI fails: `xdma.params = poll_mode=0 interrupt_mode=2` (legacy INTx, as stock), same checks.
   - To go back: remove the line (or `xdma.params = poll_mode=1`) and power-cycle.
4. MaxPayload 256: uncomment `OOS_BOOTARGS_EXTRA="pci=pcie_bus_perf"` in `rootfs/unit/dho804/build.env`,
   rebuild, install on the second card only.
   - `/proc/cmdline` contains `pci=pcie_bus_perf`.
   - Device control, bits 7:5 = 1 (256 B) on both ends: endpoint `od -An -tx2 -j 104 -N2
     /sys/bus/pci/devices/0000:01:00.0/config` (capability at 0x60), root port the same at `-j 200` on `0000:00:00.0`
     (capability at 0xc0). Offsets from the stock `lspci` dump; confirm them on the OOS kernel by walking the list from
     0x34 if the values look wrong.
   - A long run at 1 M depth and at 100 ns/div with data checks (step 1), and the read throughput from the
     `acquire_batch` timings against 128 B.

## Hardware segments (spec 03): verification on hardware

What changed and why: `docs/fork/specs/03-segmented-timestamps.md` section 7. Off by default: Acquire > Segments, then
HW segments On (or `:ACQ:SEGM 100;:ACQ:SEGM:HARD ON`). Pulse source on CH1, Normal trigger, 100 ns/div (1408 points)
unless stated. The log lines below are in `/tmp/oos-ui.log`.

1. First-trigger stamp: record N = 10. The log shows `first-trigger stamp Some(..)`, non-zero (0x40b8 bit1 set at the end
   of the recording). If it is `None`, read 0x40b8 by hand right after the first trigger: bit 1 is spec 1.1 open.
2. Tick length (open question 3): 1 kHz pulses, N = 100. `:ACQ:SEGM:TIM?` steps should be 1 ms. Derive the tick from the
   raw latch values in the log (`segment k latch ..`): expect 4,000,000 per ms (250 ps). If the steps are 16 x too short
   or long, the counter is 4 ns with 4 fraction bits: change `seg::LATCH_TICK_FS`.
3. Latch reference (open question 1): the latch value of segment k must differ from segment k - 1 by one period. If all
   latches are equal, the latch holds "now" or the last recorded frame, not the last played one: then the stamps must
   come from the headers (`Timeline` uses them when the latch is None).
4. Header words 5..7 (open question 2): compare `header` with `latch` per segment in the log. Same steps (scaled by
   1.6 ns / 250 ps) means the header is the trigger stamp; then the per-segment stamp plays can be dropped and the stamps
   taken from the batch reads.
5. Wrap: with the header as the source and a 250 ps tick, 48 bits wrap after 70 s. 10 Hz pulses, N = 1000 (100 s): the
   times must rise monotonically through the whole table.
6. Dead time: pulse pairs 2 us apart, N = 100 at 100 ns/div. Both pulses of a pair in consecutive segments, delta 2 us.
7. Interval (0x4068): Seg. interval 1 ms with a 10 kHz source: segments 1 ms apart (within one source period).
   Open question 4: whether 0x4068 also acts in live batches is not touched (the live arm writes 0).
8. Early stop: N = 1000 with a slow source, STOP after about 100. The status line said "Recording k/1000"; the table
   holds k rows; the history keys play segments 1..k and nothing beyond.
9. Largest N at 1 k points: 61,440 at 100 ns/div with 1408 points (256 bursts). Frame 0 before and after the recording
   of all 61,440 must be the same frame (no overwrite): browse to segment 1 at the end and compare its time stamp 0.
10. Read-back from frame `first` > 0 (the live path only ever played from 0): the frames of the second and later
    batches show the right signal, and each played segment shows the waveform of its stamp (use a source whose shape
    changes over time, e.g. a sweep).
11. Stamp play: the 272-byte read completes for 1, 2 and 4 channels, no `c2h` timeout, and 0x4034 bit13 is set after it.
12. Memory size (open question 5): only the stock 16,777,216 bursts are used. A larger ring needs the deep-record
    measurement first.
13. Time per stamp play: the log timestamps of the read-back of N = 10,000; expect about 0.5 ms per segment.

## Hardware averaging (spec 01): verification on hardware

What changed and why: `docs/fork/specs/01-hw-averaging.md` section 7. Off by default: Acquire > Mode Average, Averages
16, HW average On (or `:ACQ:TYPE AVER;:ACQ:AVER 16;:ACQ:AVER:HARD ON`). The top bar says "Average 16 HW" while the FPGA
averages. Log lines in `/tmp/oos-ui.log`: `average Running batch k of n triggers, t in, show .., 0x10b0 = .., words ..`
for the first 32 batches after every clear. Calibrator square on CH1 unless stated.

1. Normal path unchanged: `tools/plan-matrix.sh` with the old and the new `oos-fpgad` on the unit, `diff` empty.
   `oos-fpgad plan --average 16 --tdiv 1e-6 --all` shows the clear pulse, 0x1004 bit3, 0x10b0 = 15, 0x404c = 10, the
   doubled 0x10e0 and 0x1110.
2. Noise (spec 5.1): 200 us/div, Auto depth, RMS on the flat top (Measure Vrms on a cursor-gated part, or `:WAV:DATA?`
   twice and the standard deviation on the host). Normal, then software Average 16, then HW Average 16: both averages
   about 4 x below Normal. No drop with HW on means the frames are raw (option B of spec 3.2): leave HW average off and
   note it in spec 01.
3. Running or hold (open question 1, decides `avg::Kind`): pulse or square source at 20 Hz, Normal trigger, 1 us/div,
   HW Average 16, default build (Running). Wait 5 s, then change the source amplitude by half.
   - The trace follows within about 16 triggers (moving average: all the way after 16; exponential: 63 % after 16, the
     rest over the next 50): the FPGA runs. Note which of the two from the settling.
   - The trace stays at the old amplitude until CLEAR, and the log shows `0x10b0` or the words frozen: the FPGA holds.
     Then `touch /tmp/oos-ui.off`, stop oos-ui, run `OOS_HWAVG=hold /bin/oos-ui $(cat /tmp/oos-ui.args)` and repeat:
     the picture now changes every 16 triggers and follows the amplitude.
   - Make the result the default in `ui/src/board/dho/hw.rs` `avg_kind` and record it in spec 01 section 6.
4. Start of an average: CLEAR, then the first log lines. Words that start near 0x8000 and grow over the first batches
   mean the accumulator starts at 0 (exponential from zero): then the first frames after a clear need hiding or
   scaling (count from the log, `t in`).
5. Progress register (open question 2): the `0x10b0 =` values over the first 32 batches. Always 15: configured value
   only. Rising to 15: progress; then the status line can show it.
6. Fraction bits (open question 4): DC input from the generator or a battery, 10 mV/div, HW Average 256. The averaged
   words in the log, or `:WAV:MODE RAW;:WAV:DATA?`, take values between the ADC steps (about 14.6 codes apart in
   Normal): the output carries bits below the ADC LSB.
7. Restart on change (spec 5.5): while running, change V/div, time base, channel set and count. The old trace must not
   blend into the new one (the configuration clears). Then RUN/STOP twice and SINGLE: each starts a new average (log
   restarts at batch 1).
8. Trigger position (spec 5.6, 0x404c = 10): 5 ns/div and 100 ns/div, fast edge at the trigger point. The edge stays at
   the trigger point with HW average as in Normal. An offset of a few samples means 0x404c is wrong for this image.
9. 2 and 4 channels (spec 5.7): CH1+CH2, then all four, HW Average 16 at 1 us/div. All channels average, none swapped
   or shifted against Normal.
10. Window word 0x1110 (spec 5.9; oos-ui always reads from point 0, so 0x1110 is 0 there): stop oos-ui as in step 3,
    then `oos-fpgad run --average 2 --tdiv 1e-6 --all --out /tmp/a.bin --execute` and `oos-fpgad run --average 2 --tdiv
    1e-6 --n 4000 --at -1997 --out /tmp/w.bin --execute`. The window must equal the full record from point pre - 1997
    (within the noise of two different triggers on the calibrator). Repeat with `--mask 3` and `--mask 15` (m = 2, 4;
    the start is not a multiple of 5).
11. Deep records (spec 5.8): HW Average 16 at 1 M, 10 M and 25 M depth (the top bar shows "HW"; at 21 M to 25 M raw
    points it falls back to software). The trace is complete and averaged, no wrap or garbage at the end.
12. Fall-backs: roll (200 ms/div), scan (Auto Roll off), hardware segments on: the top bar drops "HW" and the software
    average runs. The LA is not testable on this unit.

## On-unit session: next-round table 1 (2026-11-13)

Build 20261113-170512 (dho804 d1f76c6, default DTB). Full numbers, per-run tables and method: private RE notes
`docs/logs/oos_unitlab_20261113.md`. Source: the 1 kHz probe-comp square, Auto, rising edge, so at most 1000
triggers/s; rates are the fraction of triggers captured. Install: kexec install, then a software `reboot` was the trial
start and confirmed itself; a warm reboot counts.

| Switch | Verdict | Numbers (frames/s, 1 ch / 2 ch, Auto depth) |
|---|---|---|
| `acq.pipeline = batch` | default on | 100 ns/div 774 -> 785 / 767 -> 788; 10 us/div 385 -> 421 / 287 -> 313; 100 us/div 240 -> 267 / 177 -> 199. UI 30 fps at 10 us/div where frames mode falls to 2-6 fps. Screenshots equal |
| `xdma.params = poll_mode=0` | default on (unit `system.conf`) | MSI on (0x4a = 0x0081), `ITS-PCI-MSI` IRQ counts, no `cmpl_status_th`; rates equal to poll within 2 %, one A53 5-20 points less busy; 20 min soak clean |
| `perf.place = on` | default on | +1 to +8 % over batch; main thread cpu4, hw thread cpu5, workers cpu0-3. `oos-irq-place` was broken on the unit (busybox awk without math): fixed on this branch |
| `acq.prearm = on` | default on | +1 to +8 % over place (1 ch 100 ns/div 805, 100 us/div 294; 2 ch 100 us/div 228). Settings changes, STOP, SINGLE in Normal behave as without it. Drops at 2 ch 10 us/div 20/s (A53 workers at ~90 %) |
| MPS256 | not run | needs a newer image; not measurable with a 1 kHz trigger |
| `trig.hw_status` | default on (already) | status correct in every run |
| `acq.interp = fast` | keep opt-in | no rate or CPU difference; the probe-comp edge cannot show the interpolation error |
| B1 HW average | broken | all-zero frames at >= 625 MSa/s (1 us/div, any 1 M depth, 10 us/div Auto); works at 62.5 MSa/s but levels off: noise 1.1 mV Normal, 0.75 / 0.63 / 0.53 / 0.46 mV at N = 2 / 4 / 16 / 256, SW 16 0.31-0.41 mV. 0x10b0 = N-1 always (no progress) |
| B3 HW segments | broken | every recording ends at 10 segments (N = 10..1000). Stamps 0.3125 ms apart for 1 ms: the tick is 0.8 ns, `seg::LATCH_TICK_FS` must be 800,000. Header of segment k = latch of segment k-1 |

Soak of batch + place + prearm + MSI, 20 min: 2 ch 100 ns/div 821/s, 1 ch 200 us/div 1 M 56/s, 2 ch 10 us/div 354/s;
no restart, no xdma time-out, no C2H error.

Found on the way:

- Killing oos-ui during a read leaves a C2H transfer that times out 10 s later in the kernel. Every time-out of the
  session came from such a restart.
- In STOP the hw thread keeps acquiring (~550 frames/s counted, not used; oos-ui 38 % CPU). With or without prearm.
- `acquire: no acquisition (0x4080 bit30 not set)` every ~2 s in Normal without a trigger reads like an error.
- 10 us/div, delay 200 us, Auto depth (160 us record) draws 10 us of trace and leaves the rest of the screen empty.
- Auto depth depends on history: 100 kpts at 100 ns/div from the user's setup, 1408 points after `:ACQ:MDEPth AUTO`.
- The oos-ui keys `acq.pipeline`, `perf.place`, `acq.prearm` are read from `/oos/system.conf` only (`conf_value`), so
  the unit's image `system.conf` cannot default them. Flipping them takes a code default change.

### Round 2 on dho804 47223f9 (build 20261113-181907)

Details: private RE notes `docs/logs/oos_unitlab_20261113.md`, "Round 2".

| Item | Verdict | Evidence |
|---|---|---|
| Live compression | keep opt-in | `compress-check` FAILs for every k, but every read matches a min/max pair per 2k samples **in time order** (min first only when the minimum comes first): k 3..128, 1 and 2 channels, 100 %. k 256 at 1 M: C2H read of 7824 bytes times out (twice). 2 ch 10 us/div 341 -> 409 frames/s, drops 18 -> 0; A53 load ~92 % -> ~20 %; 1 ch 1 us and 100 us/div 7-9 % fewer frames. STOP record raw, measurements equal |
| DVM (`dvm.hw`) | keep opt-in, decode wrong | `hw` reads -0.67 V for 0.15 V. Words: N = 2^28, sum = `(sum_hi >> 16) << 32 \| sum_lo`, squares = `sq_hi << 32 \| sq_lo`, 12-bit codes centred at 2048, 428 codes/div at 1 V and 500 mV/div, ~200 at 100 mV/div (pre-gain dependent). Independent of time base and mode. CH2 alone uses lane 2. `cfg` read-back = `sq_hi` with bits 7:0 replaced |
| Hardware mask (`mask.hw`) | broken | slot 0 / ctrl 0x00100000 and frames = acquired (171/s) are right, but every frame fails on a clean signal; it only passes at X 1 div and Y 3 div. Vertical scale and column mapping both off |
| C2H chunks | pass | 4 ch, 1 M points, live, 5 min: no C2H error, no time-out, no restart |
| B3 segments | count fixed | 100 of 100 and 1000 of 1000 recorded; tick still 0.8 ns (0.3125 ms per 1 ms) |
| B1 average | unchanged | all-zero frames at >= 625 MSa/s |

Not done, needs a cable moved: hwmask step 7 (AUX OUT to CH2), the AUX OUT check, the slot map with separate signals
on CH1 and CH2, DVM with a DC reference or a signal on CH3/CH4.

### Round 3 on dho804 69b3d43 (build 20261113-184600)

Details: private RE notes `docs/logs/oos_unitlab_20261113.md`, "Round 3". `oos-irq-place` fix confirmed (`irq 78 ->
cpus 0xf`); rates equal round 1.

| Item | Verdict | Evidence |
|---|---|---|
| STOP exact | pass | STOP freezes `acquired` and the picture; oos-ui 12 % CPU stopped (38 % before); settings in STOP acquire nothing; Normal without signal stops in 0.34 s. With `meas.deep_raw` at 10 M, Vavg 2.3 s after STOP equals the host mean of the RAW window to 1e-6 V (4 of 4); at 0.8 s still NaN or an older value. One SINGLE adds 248 to `acquired` at 1 us/div |
| Deep measurements (`meas.deep_raw`) | broken live | 1 ch 10 M at 1 ms/div: Vavg/Vrms stay NaN for 20 s (only after STOP correct); acquired/s 11.2 -> 9.0. At 2 and 5 ms/div one carried value; 500 us and 20 ms/div NaN. Suspect `deep_raw_settings` returns None when the screen window is set |
| Hardware zone (`zone.hw`) | keep opt-in | qualification right in all cases of steps 1-4 and 6 (box on/above the trace, intersect/not, narrow box on the edge, last division, two zones): blocked cases stop acquisitions at the trigger. Debug line dead (`armed=0`, all counters 0). Pass rate 29 frames/s against 187 with the software filter |
| C2H chunks | pass | 4 ch 1 M live, no error |
| MPS256 | already in effect | 6.12 kernel: DevCtl 0x2830 (MPS 256 B) on endpoint (cap 0x60, DevCap 512 B) and root port (cap 0xc0, DevCap 256 B) without `pci=pcie_bus_perf` |

## Upstream sync 0.1.11 (2026-11-13)

What changed for the fork with the merge of upstream 0.1.7 to 0.1.11:

- Telnet: the fork's `telnet` key is dropped for upstream's `remote.telnet` (System > Network > Remote services). oos-ui
  now layers the image default `/etc/oos/system.conf` under `/oos/system.conf` (known keys only; `net.allow` and
  `eth.mac` are not copied into the settings file). Without that layering oos-ui would start telnetd at its start.
- Network: upstream's link-local fallback (169.254.x.y/16, `arping -D`) replaces 192.168.177.64. It runs after the
  firewall and the fixed MAC. With `net.allow = 192.0.2.0/24` a computer on a direct cable is dropped; add
  `169.254.0.0/16` for that case. Upstream's remote switches close web, SCPI and VXI-11 ports; the ACL still applies to
  what stays open. The fork's network section is now docs/network.md 6.
- Acquisition: upstream's early end of a trigger wait (`seq::NEW_SETTINGS`) runs through the fork's generic
  `execute_bus_until`, so prearm, DmaBuf reuse and the host model keep working. Upstream's deep-memory screen window
  (`batch_plan` sets `window`, `decode` counts the trigger point from it) is part of the prearm key (`Settings` equality).
- Counter: upstream's FPGA counter (`hal/src/dho/counter.rs`, `:COUNter`) replaces fork item B8's counter half.
- Not verified on hardware: all of the above. The counter is polled in every pass of the acquisition loop, also during
  a hardware segment recording or a hardware average; check that it does not disturb them.


## Upstream sync 0.1.12 (2026-11-13)

- mDNS and DNS-SD (`ui/src/mdns.rs`): taken. The services announce `_ssh._tcp` (port 22) while `ssh::listens()`,
  never telnet. LAN questions pass the firewall by `net.allow` like every other service; IPv6 questions from fe80::
  are dropped. The unit is at `http://dho924s-<serial tail>.local:8080`; the web rebinding guard takes `.local`.
- Host name: rcS sets it from `net.name` through `oos-conf` (so an image default applies too); upstream's
  `remote::hostname` is `hostname::set` in the fork, since `remote.rs` stays deleted.
- NTP server from DHCP option 42 (`ntp.server = dhcp`, the new default): `udhcpc -O ntpsrv`, the answers return
  through the firewall's established rule. The unit's `system.conf` sets no NTP server, so it follows the lease.
- GPU traces in two steps and own textures per screen: the Rust interface is unchanged. `gpu/test-host.sh` shows 92
  pictures equal to the CPU picture within rounding, so drawpipe, the sparse path and persistence draw the same.
- `stat` and `:SYSTem:STATe?`: the fork's pipeline counters now stand before upstream's graphics and HDMI fields,
  which upstream's test expects at the end.
- Not verified on hardware: the responder on the unit, the host name in the DHCP request, option 42 from the DHCP server.
