# Changes

## Not released yet

New:

- Bode with an external generator on the USB-A port of the scope (USBTMC, no PC): Analysers > Bode > Connection = USB
  takes the first USBTMC instrument, shows its `*IDN?` answer or why there is none; SCPI `:BODE:GENerator "usb"`. A
  new preset for the UNI-T UTG900 (UTG932, UTG962); Rigol DG and Siglent SDG work over USB as well. Tested on the
  host with a simulated device only.

Fixed:

- Measurements froze in Run (since 0.1.7): the measurements are made beside the frame loop, and a result was only
  taken while its record was still the one shown. In Run a new record comes nearly every frame, so every result was
  dropped; values, statistics, the trend and the counter from the record only changed after STOP. On a DHO924S the
  device test went from 10 failures to none in the tests that read measurements.
- NORMAL without a trigger: a changed trigger took up to 2 s to act (the wait for a trigger ran to its end before the
  new settings were read). The wait now ends as soon as the settings change.
- Web remote: a PNG screenshot asked for while the live view ran as JPEG could come back as JPEG bytes (11 of 20 on a
  DHO924S); each request now gets its own format. One error of the video encoder no longer switches JPEG off until a
  restart; it is tried again after a minute.
- Display: a late page flip event no longer switches the panel to SETCRTC for the rest of the session. With
  Graphics = GPU and a monitor whose buffers the GPU could not take, the monitor's picture is fresh every frame instead
  of twice a second.

Tests: the script commands `menu`, `set` and `do` reach menus and items by name; the mask script tests the mask again
(it tapped a top-bar button that no longer exists).

## 0.1.10

Install from 0.1.7, 0.1.8 or 0.1.9 as always: the web page, System > Update, or the USB stick; then switch the scope
off at the mains, wait 30 seconds and switch it on. From 0.1.6 or older: install 0.1.7 first (see 0.1.9).

Fixed:

- Web remote with Display > Graphics = GPU (the default): the live picture went back and forth between the current
  screen and an older one, most visibly when a menu opened or closed (and the WLAN box seemed to drop its address).
  The frame loop swapped the shown screen out to the web page, which only works when the next screen is drawn whole on
  the CPU; with the GPU the screen comes back from it only now and then. Each picture that comes back now goes to the
  web page as a copy. On a DHO924S, 120 pictures in a row while menus open and close: 86 older screens before, none
  after, about 9.5 pictures a second over WLAN.

## 0.1.9

Install from 0.1.7 or 0.1.8 as always: the web page, System > Update, or the USB stick; then switch the scope off at
the mains, wait 30 seconds and switch it on. From 0.1.6 or older: install 0.1.7 first (the device tree changes from
0.1.8 on, which only 0.1.7 and later can try before they install it), or use the USB stick. Most of this version comes
from the reports and wishes in the forum thread.

New:

- The frequency counter (Measure > DVM / counter, also in Menu > Instruments) counts in the FPGA on the unit: a
  reciprocal counter, so a 1 s gate gives 6 digits and more at any frequency (12345.600 Hz from the built-in generator,
  where the counter from the record wandered by 0.2 Hz). The gate follows the digits (0.25 s for 3, 0.5 s for 4, 1 s
  above). Frequency, period and totalize, as in the menu; signals below about 625 Hz from one whole period. SCPI:
  `:COUNter:ENABle|SOURce|MODE|NDIGits`, `:COUNter:CURRent?`, `:COUNter:TOTalize:CLEar`. docs/protocols/counter.md.
- Display > Backlight: the brightness of the panel, 10 to 100 % in steps of 5, even to the eye (the PWM goes with the
  square of the percent). A setting of the scope, `backlight` in `/oos/system.conf`.
- A channel key held for more than half a second, or a finger held as long on the channel's card, switches the channel
  between AC and DC coupling. Channel keys now act when they are let go.
- LAN without DHCP: a link-local address (169.254.x.y, from the MAC, checked on the cable) instead of the fixed
  192.168.177.64, and no address at all without a cable. A computer connected straight to the scope reaches it without
  any set-up. The top bar says `LAN · local`.
- **Experimental, please test:** Analysers > Bode, Generator = Free-running sweep. A Bode plot with a generator that
  sweeps on its own (start and stop frequency, sweep time, linear or logarithmic) and marks the start of each sweep on
  a sync output, wired to a channel (CH4 by default). OOS sets the time base and memory for one sweep, triggers once on
  the sync edge, and reads the gain over frequency from that record: a short window per frequency (about 4 periods), a
  fit of a sine that follows the sweep (`oos_dsp::bode::sweep_response`). Phase with an input channel, gain against a
  level in dBV without one. Sweeps above about 0.9 s run in scan mode (min/max data, gain only). Tested on the host
  with synthetic log and linear sweeps through a first-order low-pass (within 0.1 dB), not yet with a real generator:
  reports with a sweep generator that has a sync output are very welcome (forum thread or GitHub issue).

Fixed:

- Text size Larger: the sample rate and memory depth in the top bar ran under the Measure button with long values (625
  MSa/s · 62.5 Mpts); it now takes the smaller font or a shorter form, and the acquisition mode only shows when it fits.

## 0.1.8

Install from 0.1.7 as always: the web page, System > Update, or the USB stick. This update brings a new device tree;
0.1.7 starts it by kexec with that tree first (a trial start, as for every update), and installs it only when it
runs. From 0.1.6 or older: install 0.1.7 first, or use the USB stick (Storage > Upgrade in the stock system), which
writes it directly, without a trial start. Then switch the scope off at the mains, wait 30 seconds and switch it on.

New:

- The GPU (Mali-T860) and the video encoder are switched on in the device tree: Display > Graphics = GPU, the default,
  now draws on the GPU, and the web page's live picture can be a JPEG from the video encoder (see 0.1.7). Installed
  on a DHO924S and started cold.

## 0.1.7

Install from 0.1.6 as always: the web page, System > Update, or the USB stick; then switch the scope off at the
mains, wait 30 seconds and switch it on. The kernel changes, the device tree does not. The GPU drawing and the JPEG
live picture below are in this build, but the GPU and the video encoder are switched on by the device tree of 0.1.8:
until then the CPU draws (also with Display > Graphics = GPU) and the live picture stays a PNG.

New:

- Updates that bring another device tree get the trial start by kexec too: `oos-update` starts the new kernel with
  the new tree through kexec-tools (`kexec_load`, which takes a tree; `kexec_file_load` keeps the running one), with
  the changes the stock loader makes to the tree at every power-on (reserved memory, command line, the boot logo's
  display mode) carried over from the running tree (`hal/src/update/fdt.rs`). Before, such an update was refused
  through the web page and System > Update and needed `--new-dt` without a safety net. 0.1.8 is the first to use it.
- USB stick (Storage > Upgrade) on a card that holds OOS: a newer build on the stick is written (the builds are read
  from the images); before, the card's build was kept unless `oos-reinstall.txt` was on the stick. The same or an
  older build is kept and only selected, as before.

- The screen is laid out from its size instead of fixed positions on the 1024 x 600 panel (`ui/src/layout.rs`): top
  bar, plot, measurement rows, bottom bar, menus, dialogs, the start-up screen and the on-screen keyboard. On the panel
  everything stays where it was, pixel for pixel (`ui/tools/layout-check.sh`). On a larger screen the TIME and trigger
  cards keep their largest useful width and stand at the right edge.
- Display > HDMI > Layout: the monitor gets the whole screen laid out again at its own size (up to 1920 x 1080, a
  larger monitor scales that up), sharp, with the traces stretched from the panel's maps. It draws a second picture
  every monitor frame, about 5 to 10 ms on the unit, so the panel may get fewer frames while it is on. Mirror and
  Waveform are as before.
- System > Network > Remote services: a switch for the web remote, SCPI, VXI-11 and telnet, each next to its address
  or port (they moved there from the Network page). All on by default; a service switched off closes its port at
  once. `remote.*` in `/oos/system.conf`; rcS reads `remote.telnet` at power-on (docs/network.md, section 4).
- Display > Graphics: GPU (the new default) or CPU (as before). With GPU the Mali-T860 draws the whole picture: the CPU
  only records what is to be drawn (rectangles, text, lines, corners, images) and leaves the traces as phosphor maps;
  the GPU draws both into the display's buffer and scales the picture for a monitor (filtered, without CPU work for
  Mirror and Waveform). The frame loop takes about a third less of its core than with CPU (35 % against 53 %, four
  channels running; gpu/README.md). The image has the Panfrost driver and Mesa for it (about 8 MB more, compressed).
- Web page, Live picture format JPEG: made by the scope's video encoder (the VPU, `ui/src/vpu.rs`), about 75 KB a
  picture against 115 KB as PNG. The encoder takes the screen as it is and converts the colours itself (a kernel
  patch, `kernel/patches/0012-…`): the web thread takes 8.6 % of a slow core at 9.4 pictures a second, against 30 %
  for PNG. PNG stays the default and the format of screenshots.
- The measurements are made beside the frame loop, each channel on its own core (`Ui::measure_in_background`): four
  channels of 100 kpts took 27 ms of every frame with a new record, and the screen ran at 27 instead of 30 frames a
  second. The frame loop and the acquisition keep the two fast cores.
- Host test runs: `--size WIDTHxHEIGHT`, the script command `textsize`, and `OOS_FIXED_TIME` to stop the clock.

Fixed:

- Downloads of the web page are named by the scope's clock in its time zone (`oos-20261111-143005.png`, as files on
  the scope), not by the computer's; while the scope's clock is not set, the computer's time stays.
- Touch on the web page's picture follows the size of the picture instead of a fixed 1024 x 600.

## 0.1.6

From 0.1.1 to 0.1.5: install the package through the web page or `oos-update install`, the scope restarts by itself
(kexec); then switch it off at the mains, wait 30 seconds and switch it on. The kernel changes too (a patch for the
Ethernet PHY).

New:

- Protocol modules: a new decoder is one file in `ui/src/protocols` with a type that implements `Protocol`, and one
  line that registers it. The Decode menu (source rows named after the roles, option rows), saved setups, the protocol
  trigger, the labels on the trace and in the event table, the demo traffic of the host build and a view of its own
  all come from it. `docs/protocol-modules.md` walks through it; `ui/src/protocols/example.rs` uses every hook.
  DMX512, S/PDIF and USB PD moved into modules; their saved setups load as before.
- Display > Second pane > Bus view: a protocol module's own picture of what it decoded (a status line and columns of
  rows, for example the devices on a bus and their last state). A module may collect over the records until Clear;
  the pane opens by itself when such a protocol is chosen and no other pane is open.
- NMRA DCC decoder for model railways (contributed by Arjen Halma, pull request #3): preamble, address, data and XOR
  check on one line or on the two rails as a pair (B line, threshold 0 V); the Bus view shows the layout it controls:
  locomotives with direction, speed (14/28/128 steps) and functions, turnouts, signal aspects, and the track status,
  green while it is live, red after an emergency stop. Tested with generated packets only, not yet on a track.
- Display > Text size: Normal, Large or Larger. Large and Larger use fonts 2 and 4 pixels bigger in the top bar, the
  cards, the measurement and cursor rows, the statistics card, the menus, the keyboard, the system information and the
  other dialogs; the rows under the plot grow with them and the plot gets a little shorter. Touch targets keep their
  size, menus get wider columns. A text that would not fit (a narrow channel card with four channels on, a long label in
  the start menu, a narrow column of the results table) stays one size smaller; the figure tables of the eye / jitter
  view and the LA lane names keep the normal size. Normal looks exactly as before. Kept across restarts like the other
  preferences, and by Default set-up and Recall.
- System > Date and time: the clock from an NTP server, for a scope without a clock battery that starts in 2013. Off by
  default (then OOS sends nothing to the outside); on, it asks the server (default `pool.ntp.org`, others in the
  picker or typed in) a few seconds after a network link comes up, over Ethernet or WLAN, and then every hour; a
  failure is tried again after 15 s, 30 s, 1 min and so on up to every 15 min. The page shows the local time and the
  last sync or failure; Sync now asks at once. Own SNTP client in oos-ui, the image's busybox has no ntpd.
- Time zone: 22 common zones with their daylight saving rules, or any POSIX `TZ` string
  (`CET-1CEST,M3.5.0,M10.5.0/3`). It applies to the screen and to file names; FAT sticks get their file times in local
  time. Set the clock takes the date and time by hand.
- Once the clock is set (NTP or by hand), new screenshots, CSV, binary waveforms, logs and event tables are named by
  the local date and time (`shot-20261111-143005.png`) instead of a number.
- System > Network shows the speed of the Ethernet link. 10 Mbit retry (on by default): a link that comes up at
  10 Mbit/s gets one more autonegotiation, once per link-up and at most three times, for a report of 10 Mbit/s after
  every cold start and 100 Mbit/s after a replug. The log names the PHY registers before and the speed after. Not yet
  verified on hardware; possible causes in the kernel are in docs/network.md.
- These settings are kept in `/oos/system.conf`, apart from the setups.

Changed:

- Measure > Add item: a second tap on a measurement removes it again (before, only Clear all did).
- Choosing DMX512 or S/PDIF sets the threshold to 1.65 V; before, the threshold of the previous protocol stayed.
- Web remote and remote control no longer slow the scope's screen down. Measured on a DHO924S over Ethernet, with
  one viewer of the web remote: the screen at 30 frames a second instead of 16, and the web page at about 13 new pictures
  a second instead of 4 to 5 (two viewers: 13 each, the screen still at 30).
  - The cause of most of it: the web, SCPI and VXI-11 servers read their connections in the frame loop with a 1 ms
    timeout, which the kernel rounds up to whole timer ticks (4 to 8 ms). Every web request, and every pass of the loop
    with an SCPI connection open, stopped the screen for that long. They now read without waiting. An SCPI script
    that keeps its connection open no longer costs frames either.
  - The live picture is made in its own thread, only of a changed screen, on the slow cores of the SoC and only with
    time nothing else wants (SCHED_IDLE), at most 20 a second and at most half a core. The frame loop hands the shown
    screen over without copying it.
  - The page asks with the ETag of the picture it has, and the scope answers as soon as there is a new one (long
    poll, 304 after 2 s of a still screen): no pause between pictures, no request in vain.
  - The PNG encoder is 3.5 times faster (matches eight bytes at a time, Adler-32 in 16 lanes, CRC-32 eight bytes per
    step; the PNG is the same byte for byte), and the comparison of two screens 4 times (musl's memcmp goes byte by
    byte).
- The USB install checks the hardware, not the model name: a model counts as verified when OOS has run on a model with
  the same hardware (models that differ only in licensed bandwidth; docs/models.md 2.1). Verified now: DHO804, DHO814,
  DHO824 (a DHO804 runs OOS, user report) and DHO914S, DHO924S. A DHO804 that names itself a DHO824 in vendor.bin was
  refused before.
- A refused install says in `oos-install.log` which models are verified, what the risk is, and how to go on anyway: an
  empty file `oos-any-model.txt` on the stick next to the `.GEL` file (`--any-model` for `oos-update gel-install`).
  It used to say only that OOS had never run on the model.

Docs:

- docs/sdcard-backup.md: backing up and cloning the SD card on Windows (Win32 Disk Imager, HDD Raw Copy Tool, WinHex,
  USBImager), macOS and Linux, with what to watch out for: the whole card, never format it, verify the image, keep the
  original card.

## 0.1.5

From 0.1.1 to 0.1.4: install the package through the web page or `oos-update install`, the scope restarts by itself
(kexec); then switch it off at the mains, wait 30 seconds and switch it on.

New:

- Waveform windows: Display > Windows splits the plot into two windows side by side, two stacked, or four. Each
  channel and math channel stands in one window (chosen on the same page; a new split spreads the channels that are
  on), all on the same time base. The LA gets a window too: its lanes stand at the bottom of it (all of it when no
  channel shares it), with that window's time axis. A decoder band goes with its source channel, references stay with
  the first window. Auto set-up shares the height of each window among its channels. Zoom, XY and the analysis views
  use one window. A drag in a window moves a channel of that window. Saved in setups.
- Full screen: the button at the top right of the plot (or Display > Windows > Full screen) gives the plot the whole
  screen without the top bar and the cards; the same button goes back. Not while the touch lock is on.
- After an auto set-up a bar at the top of the waveform window, above the signal, offers four views of the trigger
  channel: about three periods, about ten periods, the rising edge, the falling edge (the edge's 10-90 % time across
  about two divisions, slope set to match). Its cross or a tap elsewhere closes it, otherwise it goes after 30 s.
- Trigger > Noise reject: 0.7 instead of 0.3 div comparator hysteresis, for all trigger types with an analog level;
  in setups and as `:TRIGger:NREJect`.
- The on-screen keyboard has arrow keys either side of Space, and a tap into the entry puts the insertion point
  there: a typo in the middle of a long Wi-Fi password is fixed without deleting everything after it. Typing and
  Delete work at the insertion point.
- The start-up screen shows the OOS version and the build at the bottom.

Changed:

- Channel bandwidth defaults to Auto (as on the stock system): the filter follows the sample rate, so the noise of slow
  time bases stays off the trigger comparator. Full stays selectable; a signal well above the Auto limit then reads
  smaller (10 MHz at 200 us/div about half). Setups saved before keep their bandwidth. Auto set-up searches at full
  bandwidth and then gives each channel its own bandwidth back (it used to leave Full).

Fixes:

- A slow signal is no longer drawn twice, once on each edge (seen after Auto with a 1 kHz sine at 500 us/div, also in
  0.1.4). With full bandwidth the noise of the signal can be larger than the trigger hysteresis, and the comparator
  then also fires on the other edge (docs/protocols/trigger.md 7); the stock system does the same with full
  bandwidth. The display and the waveform data now take the edge of the set direction nearest the hardware trigger
  point, found on the signal smoothed to one screen column with the hysteresis of the hardware.

Diagnostics:

- `:SYSTem:STATe?` ends with counts of where the fast display path found the trigger edge; `OOS_FRAME_HEADER=1` makes
  oos-ui read and log the 16-byte frame header (trigger.md 7).

## 0.1.4

From 0.1.1, 0.1.2 or 0.1.3: install the package through the web page or `oos-update install`, the scope restarts by
itself (kexec); then switch it off at the mains, wait 30 seconds and switch it on.

Fixes:

- The start-up check no longer asks for `cal_afg.hex` on models without a generator (DHO800, DHO900 without "S"). The
  warning "Calibration data" stood there on every start, also after a self-calibration, which never writes that file.
- A self-calibration with `/oos` not mounted fails with that reason. It said "saved" and wrote into the RAM of the
  initramfs, and the result was gone at the next start.
- A cold start no longer stops at "FPGA image" when the BOOT command comes before the FPGA's boot monitor can hear it
  (the factory image needs about 11 s after power-on; seven cold starts in a row on a DHO924S). When register 0x4000 shows the
  wrong image, OOS unloads the PCIe drivers, sends BOOT again and checks again, up to twice, and shows a warning on the
  start-up screen. Run on a DHO924S in four cold starts where the first BOOT was lost: the first repeat loaded the main image each time
  and the start finished (the second repeat is tested on the host only). See docs/protocols/fpga-boot.md.

Build:

- The image builds on Linux, arm64 and x86-64, not only on macOS: the kernel container brings the cross compiler on
  x86-64, `oos-hw` is built with it (it was built by the host's gcc, under a Docker image name that no longer existed),
  the initramfs container is always arm64, and the scripts no longer need `shasum`. `boot/build-dev-image.sh` refuses
  `sudo` (rustup's cargo is not on root's PATH) and says what is missing: rustup, the Rust target, arm64 containers.
- `.github/workflows/image.yml` builds the whole image on an arm64 and an x86-64 Linux runner and compares what the two
  put into it (`tools/image-manifest.py`: kernel, DTB, every file of the initramfs).

Install:

- `tools/sdcard-clone.sh fit` makes a copy of a card image for a smaller second card: the ext4 of `userdata`, the last
  partition, checked and shrunk in Docker. `write` points to it when the card is too small.

Changes:

- Web remote: the live picture is the compressed PNG (about 90 KB) instead of the 1.8 MB BMP, which made it slow on a
  10 Mbit/s link. A "Live picture format" choice (PNG, BMP) on the page switches back; the browser remembers it.

Inside:

- PNG screenshots are compressed (one fixed-Huffman deflate block, matches against the pixel to the left and the one
  above, no new crate). On the 62 stable screens of the UI gallery a picture goes from 1.84 MB to 63 to 173 KB (median
  89 KB), pixel for pixel the same; encoding takes about 3 ms on a PC (not measured on the unit yet). `/screen.png`
  answers go out from the sender thread like `/screen.bmp` (`SEND_INLINE` is 48 KB now).
- `Bar1` unmaps BAR1 when it is dropped. The mapping kept `/dev/xdma0_bypass` in use, and `rmmod xdma` failed while one
  was left.

## 0.1.3

From 0.1.1 or 0.1.2: install the package through the web page or `oos-update install`, the scope restarts by itself (kexec);
then switch it off at the mains, wait 30 seconds and switch it on.

Fixes:

- With deep memory and many edges that seldom reach the 10 % and 90 % levels, the rise and fall time search held the main
  thread for half a minute: no picture, no SCPI, no web. It stops at the neighbouring edges now.

## 0.1.2

From 0.1.1: install the package through the web page or `oos-update install`, the scope restarts by itself (kexec); then
switch it off at the mains, wait 30 seconds and switch it on. From 0.1.0: the same, the install is direct.

Fixes:

- A cold start no longer stops at "ADC data lanes" when the sync bit stays the same over the whole sweep while the ADC
  answers (seen on a DHO924S). It goes on as with any sweep without a falling edge; the checks after the eye scan decide.

Tested:

- The USB stick install ran on a DHO924S: dry run, first install over the stock `recovery`, and the run again.

Inside:

- The board is kept apart, for a port to another one (docs/porting.md): `oos_hal::dho` holds the DHO800/DHO900, the
  GUI reaches the hardware only through `board::Board` (`ui/src/board/`), and a lint test keeps it that way. No change
  in what the scope does: the register sequences are the same as in 0.1.1.
- A description per board (`hal::target`, `rootfs/board/<name>`): card layout, update target, models and limits, what
  `rcS` has to start. The GUI takes channels, generators, memory depths and couplings from the board.

## 0.1.1

From 0.1.0: install the package as before, then power-cycle the scope. From 0.1.1 on, updates start the new build by
kexec first.

Fixes:

- Protocol trigger in Normal: in roll mode the picture stood still with WAIT although data came in. Roll and scan frames
  are no longer held back, and neither are records without the decoder's input.
- Protocol trigger with Single: it waits for a record that holds the event instead of stopping at the first edge.
- System > Overclock no longer wraps from 1.25 GHz to the 2 GHz test clock on one step down, and leaves a limits.conf
  alone that it cannot read.
- The record history keeps the logic analyser data of records up to 1 M words only. With deep memory it could fill the
  scope's RAM.
- Spectrum and eye take the first channel that is on when their source is off.
- The search for the vendor.bin keys skips libraries without the code pattern, remembers a search that found nothing,
  and stays within the file on damaged headers.

- Updates: selecting OOS by hand after a cut install no longer picks the half-written image; a trial is confirmed
  only by the build that runs; images outside the loader's limits are refused.

New:

- Updates start the new build by kexec before anything is written. Only that start, once its kernel found the card,
  installs the package. A kernel that hangs before the start-up no longer needs the card taken out: power-cycle, and
  the previous build starts again. The power-on after the install is the trial start. Not for a new device tree
  (`--new-dt`). `oos-update kexec-check` loads and unloads a package's kernel without a restart.
- The USB stick install, run again, keeps a valid OOS image in `recovery` and only selects OOS: a newer build installed
  over the network stays. `oos-reinstall.txt` on the stick writes the stick's build; a damaged `recovery` gets the
  rollback image back.
- SCPI `:WAVeform:MODE RAW` reads every sample of the record in the screen window.
- OOS drives no hardware on another board: when `vendor.bin` names a model outside the DHO800/DHO900 family,
  `oos-fpgad` refuses and `oos-ui` shows the demo signal.
- `tools/platform-probe.sh` describes another Rigol model for a port, read only ([docs/porting.md](docs/porting.md)).

Measured:

- The generator's AM, FM and PM are accurate within 3 %. The earlier scatter came from the test, which read the screen
  record.
- The logic analyser loses no data at slow time bases. The gaps seen before came from the test equipment.

## 0.1.0

First public version.
