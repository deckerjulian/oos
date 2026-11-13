# oos-ui: the OOS scope screen

Rust, no dependencies besides `libc`. Software renderer into a RAM canvas, shown through DRM/KMS dumb buffers with page
flips (no GPU, no compositor). Runs on the DHO900 under OOS (Linux 6.12, initramfs) as one static binary (~1 MB).

Runs on the unit at 30 fps. Data sources: the acquisition hardware (`--hw --afe`), the built-in demo generator, or the
replay of a captured raw frame. Notes dated below describe the state on that day.

## Screen (1024 x 600)

Dark theme, the colour-blind safe channel colours yellow / sky blue / pink / teal, orange as the accent, Roboto.

| area | content |
|---|---|
| top bar (40 px) | state pill (TRIG'D / AUTO / WAIT / STOP, tap = run/stop), acquisition "rate · depth · mode" (e.g. 12.5 MSa/s · 100 kpts · Normal; the hardware's numbers while `--hw` runs), tap on it = Acquire page, buttons for what is used most and the menus without a front-panel key: Measure, Cursor, Display, Storage, System (with About and the Quick key setting), network box ("Ethernet · DHCP" + IPv4 address, or "not connected"). Buttons are drawn 30 px high and tapped over the full 40 px |
| plot 970 x 448 (10 x 8 div; 970 x 464 without strip rows) | from y = 52 to the cards, 24 px gutters: channel markers left of it, trigger level marker right of it (its value inside the plot), trigger position above it. Drawn inside the plot: volt labels of the active channel along the left edge (a label that would overlap another label, the status line, the time labels, the mask pill or the decode band is left out; 5 px gap to the channel markers), time labels along the bottom edge (0 s in orange), a status line at the top (left: Touch locked, History n/m, Search count; right: source · wfm/s · fps), messages as an amber pill at the top centre (beside or under an open dialog, never over its controls), the mask pill, the math label, cursors (X1/X2, Y1/Y2 tags), the decode band above the time labels |
| strip | one row per channel with measurements (label grey, value in the channel colour), one row for the cursor read-outs. Measure > Dock puts it under the plot (the default; each row makes the plot 18 px shorter), above the trace, or in a column on the left or the right |
| results overlay | with Measure > Statistics on: a translucent card at the top right of the plot (left of the strip where a drag moves the trigger level) with value, mean, ±σ per item and the number of acquisitions; tap its header to fold it to one line. It takes no room from the plot |
| cards (66 px) | **Menu** button (start menu, see below), CH1-CH4 (wide with V/div, coupling, probe, bandwidth, offset when on; narrow with a ring when off; tap = channel menu), TIME (time/div, delay or zoom time/div; tap = horizontal menu), trigger (type, slope / polarity / video standard, level; tap = trigger menu; Source and Sweep buttons over the card height cycle CH1-4 and Auto/Normal/Single) |

Layout before 2026-11-03: top bar 40 + info row 18, toolbar 38 px over the cards, 74 px label gutter, results as a
fixed 178 px column; the plot was 880 x 384 (700 x 384 with statistics). The toolbar is gone, LIVE and ? as well
(state pill = run/stop, About in System), the info row and the labels moved into the plot.

Offline mode (System > Offline, or `oos-ui --offline`) hides the CH1–CH4 cards. A session can start in that mode
with those channels off and no demo on them. Import slot and SIM Play then keep analog off, set TIME from the
record's own interval and length, and put the reference name (R1–R10 and its label) where the trigger card is.
The DVM and the counter read that reference. With the mode off, the live screen is unchanged.

**No touch buttons for functions with a front-panel key.** They are on their keys (see Input) and, so that the scope
works by touch alone, in the **start menu** (Menu button at the bottom left, or Multi 1 with no menu open): a grid in
groups, each entry 140 x 40 px:

| group | entries |
|---|---|
| Acquisition | Run/Stop (orange while running), Single, Autoset, Clear, Default, Acquire > |
| Trigger | Trigger >, Slope (steps like the key, the menu stays open), Force |
| Analysis | Measure >, Cursor > (turns the cursors on like the key), Math >, Reference >, Decode >, Mask > |
| Display | Display >, Horizontal >, History (Navigate key; "Live" while a stored record is shown, then Older / Newer appear) |
| System | Storage >, Screenshot, Quick · <action> (runs the Quick key action), Touch lock, System >, Power (the power dialog) |

One-shot entries close the menu; the menus opened from it have Back to the start menu. LA and the generator have no
OOS function yet and are left out. While touch is locked the Menu button shows "Locked" in amber and is the only
touch target left: a tap on it unlocks.

Menus open as dialogs: below the top bar for its buttons, above the cards for the cards, the start menu and the menus
of the front-panel keys. Orange frame, rows of 40 px touch targets: segmented buttons for choices (selected =
orange), `- value +` for values, full-width buttons for actions, a Back button in sub menus, × closes. Tapping the
plot outside a dialog closes it.

**Pickers**: a choice whose options do not fit as segmented buttons shows `- value ▾ +`; tapping the value (or the
row label) opens a grid of all options (like Measure > Add item), the current one in orange; tapping one sets it and
returns to the menu, Back returns unchanged. `-`/`+`, Multi 2 and the ± steps keep stepping the value; a push on
Multi 1 opens the picker. Items with pickers: Trigger > Type (12), Math > Operation (14), Math > Window (FFT, 5),
Acquire > Memory (8), Acquire > Averages (10), Display > Persistence (7), Decode > Baud (10), Utility > Quick key
(8), Trigger > When for Runt (5; 4 for the other types fit as buttons). Video sync, decoder protocol, CS, parity etc.
fit as segmented buttons and keep them.

Traces are drawn as a digital phosphor: each segment between two samples adds to a per-channel hit map, so brightness
follows dwell time; persistence (Min .. Infinite) decays the map; brightness is graded relative to the brightest pixel.
Before the 16-bit counts saturate (long / infinite persistence) the maps are halved, which keeps the density ratios.

## Functions

- Channels: on/off, V/div 500 µV..20 V (x probe 1/10/100), offset, DC/AC/GND, 20 MHz BW limit, invert.
- Horizontal: time/div 5 ns..1000 s, delay, **zoom** (H-scale push: overview with the window marked above, zoomed
  view below; then H-scale/H-position act on the zoom window).
- Trigger (software trigger engine `../dsp`): Edge, Pulse width, Slope (transition time), Video (PAL/NTSC: all lines,
  line n, odd/even/all fields), Pattern (H/L/X/rise/fall per channel), Duration, Timeout, Runt, Window (enter/exit/
  inside), Delay (A -> B), Setup/Hold, Nth edge after idle; conditions >, <, inside, outside; Auto/Normal/Single, level
  (knob, drag, 50 %), force. The type is chosen from a picker. Each type shows only its parameters; switching the type sets starting values that trigger
  on the demo signals (the demo adds runts / missing pulses on CH1 for Runt and Timeout, composite video on CH2 for
  Video). Two-level types show a second level marker.
- Acquire: Normal/Peak/Average (2..1024)/High res, memory depth Auto..50M (shown; the demo source ignores it).
- Measure (`../dsp` measure): 35 items (voltage, time, edges, overshoot/preshoot, area, slew rate, pulse/edge counts,
  ...), up to 6 at a time, **statistics** (mean, ±σ, n) in the results panel.
- Cursor: time (dT, 1/dT) or voltage (dV), drag the lines.
- **Math** (`../dsp` math): A+B, A-B, AxB, A/B, integral, derivative, abs, sqrt, lg, exp, low-/high-pass (corner
  frequency), **FFT** (dBV rms, Rect/Hann/Hamming/Blackman/Flattop, dB/div, offset, span 0..fs/2), auto scale.
- **Reference** R1-R4: save a channel, show/hide, kept at its saved scale.
- **Decode** (`../dsp` decode): UART (baud, 5-9 data bits, parity, ASCII/hex), I2C (SDA/SCL: start, repeated start,
  address with R/W, data, NACK in red, stop), SPI (MOSI/CLK/CS, modes 0-3, word length); bus row with frame boxes
  (ANALYSE key or menu). Demo: UART "OOS 0.1 " on CH3; choosing I2C or SPI puts bus traffic on CH3/CH4 (CS on CH2)
  and sets a trigger that holds it still (I2C start = Nth edge after idle, SPI = CS falling).
- **Search** (Trigger menu, Mark events): marks every event of the current trigger condition in the record (amber
  ticks) and counts them; works for all 12 trigger types.
- **History**: the last 200 records; Navigation back/forward step through them (stops the acquisition), play/stop or
  Navigate returns to live; measurements are recomputed for the record shown.
- Display: persistence Min..Infinite, **colour grade**, intensity, grid full/light/off, **XY** (CH1 vs CH2), clear.
- **Clear** (CLEAR key, Display > Clear, Quick key set to Clear, SCPI `:CLEar`): empties the screen like a scope:
  waveforms, persistence / colour-grade maps, averaging (restarts from the next record), measurements and their
  statistics, mask pass/fail counts and the search marks (the last record is dropped, so math, decode, CSV and Ref
  save have nothing until the next one); toast "Display cleared". In STOP the screen stays empty until the next
  acquisition (RUN, SINGLE, history navigation); in RUN the next record draws again at once. The history is kept.
- **Quick key** (System > Quick key; also the start menu entry Quick): Screenshot (default, PPM in /tmp as
  before), Save PNG (storage directory), Save setup (current slot), Save CSV, Autoset, Single, Run/Stop, Clear.
  Saved in the setup text as `quick = <name>` (`Screenshot`, `SavePng`, `SaveSetup`, `SaveCsv`, `Autoset`, `Single`,
  `RunStop`, `Clear`; missing = Screenshot) and therefore also across restarts of oos-ui (`/tmp/oos-ui.setup`);
  Default set-up keeps it.
- **Colour grade** (Display > Colour grade On): the hit maps of all enabled channels are added up and every pixel is
  coloured by its density relative to the densest pixel on a temperature scale blue -> cyan -> green -> yellow -> red
  (rare -> frequent; intensity shifts the scale). Merged rather than per channel, so overlapping traces count together;
  with infinite persistence it shows jitter and rare events. Cost per frame: one 1024-entry LUT from a
  256-colour palette, then per pixel one add per channel and a table look-up (about 0.6 ms per frame on a host with
  4 channels, so a few ms on the A72).
- **Storage** (toolbar Storage, or Menu > Storage): save setup to `setup<slot>.txt` (slot 1-9), recall setup from it,
  save the waveform of the record shown as CSV (`wave-NNN.csv`: `Time (s)` relative to the trigger + one `CHn (V)`
  column per enabled channel, empty fields where there is no data), save a screenshot as PNG (`shot-NNN.png`, taken
  without the dialog). NNN is the first free number; a toast shows the file name. Directory: `/tmp/oos` on the unit
  (created when missing), `--store-dir DIR` or the command `storedir DIR`; host script runs use the script's
  directory (`target/test-shots`). The setup file has one `key = value` line per Model field (enums by name, ladder
  positions as indices, floats exact; e.g. `ch2.vscale = 9`, `trig_type = Edge`, `meas = Freq:1 Vpp:1`); recall starts
  from the default set-up, ignores unknown keys, rejects bad values with the line number and keeps run/stop.
  The PNG encoder is our own (`src/store.rs`: zlib stored blocks, CRC32, Adler32).
- **Mask test** (toolbar Mask, or Menu > Mask): Create mask takes the source channel of the record shown and builds an
  envelope with tolerance X (divisions of the 10-div window) and Y (divisions of the channel's V/div); changing the
  tolerances rebuilds it from the same reference. Every live acquisition is then tested (`../dsp` mask): the forbidden
  region is tinted red, a pill shows "Mask CH1 · Passed n · Failed m"; on fail: None or Stop. The mask is kept in
  volts and screen time, so it follows the channel's scale/offset and stays on the screen grid when the time base
  changes; changing the source discards it. Reset counts restarts the counters. It is drawn in the normal (not zoomed)
  view.
- Auto set-up (one band per channel), default set-up, screenshot (QUICK key with the default action, start menu or
  System > Screenshot: PPM in /tmp, taken without the dialog; PNG via Storage or the Quick action Save PNG).

## Input

- **Touch** (edt-ft5x06, `/dev/input/event*` found by name): tap on cards, buttons and dialog controls. Drag in the plot: vertical = offset of the active channel (start at the left edge
  = the nearest channel marker), horizontal = delay, at the right edge = trigger level, on a cursor line = that cursor.
  Options `--touch-swap`, `--touch-invx`, `--touch-invy`, `--touch /dev/input/eventN` if the axes turn out wrong.
- **Front panel** (`/dev/ttyS0`, 1.5 MBd, frames `FA cmd data crc7 AF`, see `docs/protocols/frontpanel.md`):
  CH1-4 (off -> on + menu; menu open -> off; else select), RUN/STOP, SINGLE, AUTO (auto set-up, one band per channel),
  CLEAR (see Clear), DEFAULT, Trigger slope, Force, MEASURE, CURSOR, Trigger MENU, Horizontal MENU (Acquire menu, with
  Horizontal as its sub menu; the TIME card opens Horizontal directly), ANALYSE (Decode), REF, MATH, Navigate (history),
  QUICK (the action of System > Quick key, default screenshot), Touch lock (the Menu button unlocks by touch).
  Knobs: vertical scale (CW = smaller V/div, push = channel menu), vertical position (push = 0), horizontal scale
  (CW = faster), horizontal position (push = 0), trigger level (push = 50 %), Multi 1 = select menu item
  (push = activate; with no menu open it opens the main menu), Multi 2 = change the selected value. Fast turns
  accelerate. MATH, REF and ANALYSE open their menus; LA, GI and Navigate show "not in OOS yet".
- **LEDs**: CH1-4 follow the channels, RUN green / STOP red / SINGLE, slope LEDs follow the trigger slope, TOUCH
  when touch is locked. `Ui::leds` builds the mask in the stock order (0-3 CH1-CH4,
  4 RUN, 5 STOP, 6/7 SINGLE, 8 LA, 9 G1, 10 G2, 11 TOUCH, 12 A up/down, 13 B left/right, 14 SLOPE1, 15 SLOPE2,
  16 A left/right, 17 B up/down, 18 INTENSITY); `Inputs::set_leds` translates it to the MCU's hardware bits with a
  table (see `docs/protocols/frontpanel.md`) and sends frames cmd 1/2/3.
- **Control FIFO** `/tmp/oos-ui.ctl` (for remote tests over telnet): `key ch1`, `knob vscale +2`, `tap x y`,
  `drag x0 y0 x1 y1`, `shot /tmp/x.ppm`, `storedir /tmp/oos`, `stat`, `quit` (see `src/cmd.rs`).

## SCPI (TCP 5555)

A stock-compatible subset, enough for scripts and the openSciLab bridge: `*IDN? *RST *OPC? *CLS`, `:RUN :STOP :SINGle
:AUToscale :CLEar`, `:CHANnel<n>:DISPlay|SCALe|OFFSet|COUPling|BWLimit|PROBe|INVert`, `:TIMebase[:MAIN]:SCALe|OFFSet`,
`:TRIGger:MODE? :TRIGger:EDGE:SOURce|LEVel|SLOPe :TRIGger:SWEep :TRIGger:STATus?`, `:ACQuire:SRATe?|MDEPth?`,
`:MEASure:ITEM? <item>[,CHANn|Rn]` and `:MEASure:ITEM? <pair>,<src>,<src>` (R1–R10 as well as CHANn),
`:MEASure:STRip?` (the rows under the plot, plus the DVM and counter when they are on),
`:MEASure:READout?` (those rows, and the eye figures when the eye page is up),
`:EYE:ITEM? <figure>` (height, width, unit interval, total jitter, and the other figures on that page),
`:WAVeform:SOURce|MODE|FORMat(BYTE/WORD/ASCii)|POINts?|PREamble?|DATA?` (the displayed
record, volts = (code - yorigin - yref) * yinc), `:DISPlay:DATA?` (24-bit BMP of the screen), `:SYSTem:ERRor?`.
Remote changes go through the same model as the screen. Tested from a host against the unit on 2026-11-02.

## Live FPGA source (experimental)

`oos-ui --fpga-live [--fpga-arm] [--fpga-play4] [--n 4000] [--start 1248000] [--pre "<:WAV:PRE? of the hand-over>"]
[--period-ms 30]` starts a thread that reads frames from the FPGA and shows them (source tag FPGA). It writes FPGA
registers, so it runs only on request, after a warm hand-over from the stock system (no OOS FPGA init).

- without `--fpga-arm`: repeats the read-out verified word-exact on 2026-11-02 (28 writes + one C2H read of 2*N bytes).
  Tested 2026-11-03: stable, but it only replays the stored frame (status stays STOP): play-last does not record.
- `--fpga-arm`: before each read, arms one acquisition like the stock system (stop sequence, type-2 run,
  record sequence, FSM start; 29 writes, `docs/protocols/acquisition-start.md` section 4A), waits for 0x4080 bit30
  (acquisition done, auto trigger), then reads. Not run yet.
- `--fpga-play4`: read with the stock type-4 play instead of play-last (section 4C variant 2), in case play-last still
  returns the old frame after an arm.

`oos-ui --fpga-dry` prints all sequences without hardware access; the read-out is checked by `cargo test` against
the recorded write list (`hal/testdata/second-frame-writes.txt`). The log (`/tmp/oos-ui.log`) shows per frame the changed
words against the previous frame, the wait time and the status registers.

## Hardware source (`--hw`, not run on the unit yet)

`oos-ui --hw [--app-mask 1] [--afe] [--cal /rigol/data]` acquires through `../hal` (`oos_hal::seq`): a thread takes
the UI settings (channel set, time base, V/div, offset, 20 MHz limit, trigger source/slope/level, Auto/Normal),
configures the FPGA when they change (ADC mode switch when the channel set differs from `--app-mask`, AFEs with
`--afe`, edge trigger, acquisition configuration, auto trigger), then per frame arms, waits for 0x4080 bit30, reads the whole
record (Auto depth is limited to 100 k points for a fluid display), de-interleaves and converts to volts. The trigger
type goes to the TPU as well (all 12 types, `hal::tpu`); the UI then aligns each record with the software trigger
engine around the hardware trigger point. Without `--afe` the AFEs keep
the stock system's set-up (warm hand-over), so the UI V/div and offset must match the stock ones for correct volts.

## Bottom row

Behind the channels the LA and the generator have cards of their own, on the models that have them. Off they are narrow with
a ring. On they are wide with their values, or narrow with a full dot when four channels need the room. A tap on the LA
card works like one on a channel: an LA that is off comes on. A tap on the generator card only opens its page, the output
stays as it is.

## Web remote (HTTP 8080)

`http://<scope>:8080/` shows the live screen (about 5 frames/s as BMP), a click on it is a touch tap, buttons below
act as front-panel keys and knobs. `GET /cmd?c=<text command>` runs one key, knob or touch command, `GET /screen.bmp` returns the screen, `GET /screen.png`, `/wave.csv` and `/la.csv` are the downloads of the page. Answers above 256 KiB go out in a thread of their own (at most two), so a slow link does not stall the frame loop. `cargo test web_demo -- --ignored --nocapture` runs this page with the demo signals on a host.
Like telnet and SCPI it is unauthenticated: bench network only.

## Build and test

```
./build.sh          # scope binary target/aarch64-unknown-linux-musl/release/oos-ui + host binary
./build.sh test     # renders tests/*.txt off-screen to target/test-shots/*.png
cargo test --release   # unit tests: setup round trip (incl. quick key), CSV, PNG (structure, CRCs, Pillow when installed),
                       # mask, colour grade, layout / touch targets, pickers, every option selectable, Clear, Quick key
target/release/oos-ui --replay FRAME.bin --pre "<:WAV:PRE? string>" --script tests/replay.txt
```

`tests/colorgrade.txt` (a0-a2: menu, CH2 + CH4 graded, PWM with sweeping duty triggered on CH4), `tests/storage.txt`
(b0-b5: menu, setup / CSV / PNG saved, default, recalled; files in `target/test-shots/storage/`) and
`tests/mask.txt` (c0-c3: menu, CH1 passing, CH4 failing, stop on fail) exercise these features.
`tests/picker.txt` (d0-d5: trigger type picker, Runt set, math operation picker, FFT set, Quick key picker, Clear
set), `tests/clear.txt` (e0-e2: stopped, cleared in STOP, SINGLE draws again), `tests/results.txt` (f0-f1: results
overlay, folded), `tests/startmenu.txt` (g0-g3: start menu, history entries, touch locked, unlocked by the Menu
button) and `tests/axes.txt` (h0-h1: volt labels with a non-round offset, in a short zoom view). Tap coordinates in
the scripts are those of the layout of 2026-11-03 (second revision: start menu, network box).

On the scope (OOS shell): `insmod .../edt-ft5x06.ko` once, then `/tmp/oos-ui &`. SIGTERM restores the previous
display content and turns the LEDs off. In the OOS image `etc/rcS` loads the touch driver and starts `/bin/oos-ui`
in a restart loop (`touch /tmp/oos-ui.off` before `killall oos-ui` keeps it stopped).

Frame timing on the unit (4 demo channels, 6 acquisitions per frame): step 25 ms (demo signal generation dominates),
render 4 ms, present 1 ms; so the drawing itself has room for 60 fps and more with a cheaper source.

## Files

`src/gfx.rs` renderer, `src/font.rs` + `assets/font.bin` (generated by `tools/mkfont.py` from Roboto Regular/Medium
in `tools/fonts/`, Apache License 2.0), `src/model.rs` settings and number formatting, `src/wave.rs` sources, phosphor map,
measurements, auto set-up, `src/ui.rs` layout, menus, input, colour grade, mask display, `src/store.rs` setup files, CSV, PNG, `src/drm.rs` KMS output, `src/input.rs` touch, front
panel, FIFO, `src/cmd.rs` text commands, `src/main.rs`.

## Next

Live data from `oos-fpgad` (shared-memory ring), settings persistence across reboots and a storage directory on a
writable partition (Storage writes to `/tmp/oos` for now), HDMI as a second output, GPU (Mali) path for higher waveform rates, LA (D0-D15) display.

## Protocol trigger (2026-11-03)

Trigger type **Bus** uses the protocol and sources of the Decode menu. Match: *Frame* (every frame start), *ID/Addr*
(CAN/LIN identifier, I2C address, MIL-STD-1553 command word equal to the value), *Data* (data byte/word/audio sample
equal to the value) or *Error* (failed CRC, parity, checksum, NACK, missing ACK). The value is entered on a hex
keypad (tap the value). On the hardware the TPU fires on the falling edge of the bus line (SPI/I2S: CS/WS; 1553 and
I2S: either edge) and `trig::find_all` decodes the record and puts the trigger point at the end of the matching field.
