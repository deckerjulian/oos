# Changes

## Unreleased

Fixes from a review of 0.1.13, and the forum's report on it. All of it is tested on the host. Marked "(unit)" where
it also ran on a DHO924S (2026-11-16).

Fixed:

- Eye and jitter (host-tested on SIM signals and built signals of known jitter, not yet verified on hardware):
  - Eye Width read short of what the jitter allows: about 5 % of a unit interval on a jitter-free signal in the
    measurement's 96-column picture. The picture's own spread at the crossing is taken out; with 20 ps of random
    jitter on a 1 ns interval it reads 873 ps for 880 ps.
  - SSC Freq Dev took the span of every single interval's rate, so the timing error of two edges counted as
    frequency spread (7100 ppm for 5000 ppm); it is taken of the averaged series when a modulation line is found.
    SSC Modulation Rate was a line of the spectrum, which is a fifth off when the record holds one modulation
    period, as a SIM record does; with fewer than four periods the rate is fitted (278 kHz for 300 kHz).
  - T/nT Ratio took the bit before an edge as the transition bit about as often as the bit after it, so 3.5 dB of
    de-emphasis read 0.9 dB. It takes the bit that starts at the edge and reads the transition level over the steady
    one, as its name says: +3.5 dB.
  - Phase Noise leaves a periodic tone out, but the bins beside a tone each took a share of it: a clean clock with a
    20 ns tone read 8.5 ns of noise. The tone's whole line is left out of the averages (62 ps now).
  - a value in dB is written without a prefix ("0.83 dB", not "831 mdB").
- Double pulse (host-tested against a built double-pulse record, not yet verified on hardware): Ipeak and the diode
  recovery took the largest current of the whole second pulse, which is the end of the inductor's ramp, so Ipeak and
  Irrm read too high and Trr, Qrr and the diode slope had no value; they use the first peak after the turn-on. The
  descriptions of Td(off), Tf and Toff said "voltage rise"; they are measured on the current fall, and say so.
- A ratio and the units that are not SI are written without a prefix: a power factor of 0.498 read "498 m", a speed
  "3.00 krpm", a frequency deviation "217 nppm".
- Measurements (host-tested against a pulse train of known values, not yet verified on hardware): Positive and Negative
  Duty Cycle and the six Cycle rows (amplitude, top, base, peak-to-peak, maximum, minimum) had no value when the view
  held one cycle between two falling edges, which is the default view of the 1 kHz probe signal; a count (edges,
  pulses, ones) is written as a whole number, not "1.00"; a skew, setup or hold row without an edge pair inside its
  window says "none in window".

Added:

- Remote commands (host-tested, not yet verified on hardware): the zone triggers (`:TRIGger:ZONE<n>:...`), the delay,
  setup/hold, duration, slope and video trigger rows that had no command, history frames (`:RECord:CURRent`,
  `:NAVigate:FRAMe:...`), `:SEARch:VALue?`, `:MATH<n>:RESet`, `:CHANnel<n>:ZERO`, `:MEASure:STRip:DOCK`,
  `:MEASure:COUNter:...` and the fixed answers of `:MEASure:AMP`, `:MEASure:THReshold`, `:MEASure:AREA`,
  `:MEASure:SETup:MID` and `:CURSor:TRACk:MODE`. Where 0.1.13 already had a command of the same name, that one stays.
- Minimal and Studio (host-tested, not yet verified on hardware):
  - a tap on a measurement badge draws what it measures on its lane (Area and Cycle area shaded, levels as lines,
    spans as arrows); the next tap opens its menu;
  - plot menu > **Fit all lanes** sets V/div and position of every shown channel, reference and math channel;
  - an analyser view whose source is off shows a **Turn on CHn** button in its middle, and an empty Bode plot a
    **Start sweep** button; the spectrum beside the waveform keeps its width for the plot (the peak list is behind
    its menu button); Minimal greys Overlay and Stacked out while an analyser has the plot;
  - a long information line of a sheet (Network > Other access) breaks in two instead of losing its first letters; a
    lane no longer repeats the trigger level as a scale label beside the trigger tag;
  - a SIM waveform added to a reference while the scope runs live says how long it is when it is far shorter than the
    screen, and its message carries the tick of a done action instead of the warning triangle;
  - a channel whose record reaches the end of the ADC range is named in red in the plot and gets a red pill border;
    `:CHANnel<n>:CLIPping?` answers 1 or 0;
  - Studio: a channel that is off has a small pill, a tap switches it on; the badge of a channel pill switches it
    off; the SIM panel has **Add** beside Play, so several simulated signals play together;
  - System > About and System info show the commit the binary was built from.
- System > Network > Remote services > **Allowed** sets `net.allow` from the scope's screen (unit). The picker offers
  every network the scope is on, and Other opens the keyboard. A wrong entry is refused with a message and not
  stored. A change takes effect at once, without a restart:
  - the firewall is reloaded;
  - SCPI, VXI-11 and HiSLIP close the connections the new list leaves out, and the web remote checks every request;
  - SSH starts or stops with the list.
  The Firewall row shows the state in force (host-tested; the earlier build of the row ran on a unit).

Changed:

- Minimal opens the settings sheets of Studio for every menu; the stock layout keeps the stock dialogs. Every
  remaining dialog of Studio is a sheet too (host-tested).
- The top bars of Minimal and Studio no longer repeat front-panel keys: Measure, Cursor, Autoset, Clear and Force
  are on their keys and in the Menu.
- Studio lanes: a solid line between lanes, a bar in the source's colour at the left, and each lane's scale at its
  right edge (top and bottom value always). Overlay traces works in Studio and is kept with the setup.
- A measurement is pinned from the moment it is added. Measurement cards are three lines and as many columns as
  fit; the measurement tables size their columns to what they hold.
- Every top bar shows the rate and length of the record on screen (the demo's real record, not the nominal depth).
- SCPI setup and hold (`SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD` in `:MEASure:ITEM?`) take the data first and the
  clock second, as the tiles say (A = data, B = clock). Before, they took the clock first: a script that sends the
  clock first gets swapped results now. These are OOS items; the stock programming guide has none.
- A change of trigger type keeps the user's pulse width, pattern, levels and limits on the hardware, Live and Replay.
  The demo's starting values are set on the demo only. Slope, Runt and Window keep the lower level at or below the
  upper one; `:TRIGger:RUNT:UPPer/LOWer` refuses a threshold past the other with -224.
- Choosing a decoder protocol on Live or a Replay keeps the sources, as on the hardware. Before, they moved to the
  demo's channels.
- Touch lock has one place on the screen: the Touch lock action in the start menu, next to the other front-panel
  keys (and the TOUCH key itself). Utility > Touch, the same switch a second time, is gone.

Fixed:

- Two fingers never open the long-press menu: a pinch that rested for half a second opened it mid-gesture.
- Stacked lanes: the logic analyser keeps its own strip instead of drawing over the first lane.
- The measurement band has no row of its own for page controls, and its names no longer run into the source.
- A vertical drag in the lane of a math channel moves it.
- The expand button in the plot's corner enlarges the panel also while HDMI is on Waveform or Layout. Before, it
  only changed the density there and the panel stayed as it was (forum report). Full chosen under Density still
  keeps the panel's bars while the external display has the waveform.
- A typo in `net.allow` (`192.168.1.0/`, `2001:db8:::1`) made nft refuse the whole firewall, and the scope came up
  without one while telnet listened. Such entries are left out and logged; a ruleset nft refuses anyway closes the
  scope to everything but loopback.
- STOP (unit): the stopped record is no longer counted twice in the statistics, trend and catalog, and a change of view while
  stopped adds nothing. Clear while stopped keeps the hardware stopped (it went on acquiring and overwrote the stored
  record). RUN after STOP starts with a frame from after the RUN press.
- Deep records read as min/max pairs: Mean, RMS, Std dev and AC RMS statistics no longer count pair values while the
  reading is empty; with the LA on or three channels the measurements no longer stand still waiting for a raw read
  the hardware never makes; after a Clear a held deep measurement no longer shows the cleared record.
- The acquisition thread no longer holds the shared state while it decodes a raw window (up to ~24 MB), and the
  newest frame of a batch gets the ADC-core correction like every other record.
- Leaving a reference, Take over, Offline or SIM session brings back the live set-up from before it, whichever way it
  is left (Live, Record = With live, a channel key, a channel as trigger source, Sources). A set-up recalled during a
  session ends it and stays. Leaving the Bus trigger after a recall keeps the recalled decoder on. In Live, switching
  a channel on from Sources leaves the trigger and Run/Stop alone.
- Issue #5, stray traces (unit): frame 0 of a batch is left out in Batch draw mode too. Frame 1 is the first
  trigger after the arm, and now and then it holds a piece of older memory. With an edge trigger it is left out
  when its edge is not at the trigger point. SINGLE returned no frame from before the arm in 40 tries.
- `:REF:LOAD "/proc/kmsg"` froze the screen for good (unit). Files under /proc, /sys and /dev are refused now. So is
  any file larger than the 128 MiB import limit, before it is read.
- Waveform import (unit for the first two):
  - A CSV with more columns than reference slots is refused while it is read. It used to take gigabytes first.
  - An upload over the web page is parsed beside the frame loop: 32 MB took 6 s, and SCPI answered within 43 ms
    meanwhile.
  - A frame file (`frame-NNN.bin`) whose counts do not fit its size no longer crashes oos-ui.
- Web remote: `/update` and `/mcp` from outside `net.allow` are refused at the request head, before the body is read.
  Only one large body is read at a time.
- HiSLIP (desktop): a tiny maximum message size no longer splits a reply into one message per byte.
- The zoom window no longer overflows at extreme delays.
- The console banner and the Network page said SSH takes a provisioned key only. It takes the password or a key.
- With hardware segments recording, the DVM, the levels and the ADC core trim wait for the recording to end.
- STOP on a deep record (e.g. 10 M at 1 ms/div): the views read again as min/max pairs are drawn, while Mean, RMS,
  Std dev and the other readouts stay measured on the raw record of the STOP. Before, they turned to "----".
- Reference > Clear also removes the readouts of the cleared slots. Before, an import's Vpp, Freq, RMS and Rise rows
  stayed empty (unit).
- A `system.conf` that leaves out every decoder no longer crashes Decode > Protocol.
- Measurements: line and switching ripple without the rail's DC level on records of partial cycles; double pulse
  I_L, td_on, tr, td_off, tf, di/dt, Ipeak - I_L and Qoss with a ramping load current; high time, low time and time
  outside without noise from slow edges; Ones counts bits, not samples; setup/hold violations judged per clock edge,
  and the Setup + Hold tile says it counts violations; Eye Width@BER in seconds; Hall shaft speed and angle
  mechanical (divided by the pole pairs) and no longer stuck after a fault code at the record start.
- Decoders: MLT-3 decodes every change between its three levels (half of the ones were lost).
- Phase noise and switching dV/dt, dI/dt no longer take quadratic time on long records; a power-rail clock longer
  than the rail no longer crashes.
- Tests no longer depend on the speed of the computer (wall-time limits replaced), and the SSH key tests run in a
  checkout without `local/`.

## 0.1.13

Merged from the forks on GitHub (PR #7 by ichnograph, DHO804; PR #8 by W4R/wasimlhr, OOS Plus), with main since
0.1.12. Their work is credited in the commits. PR #9 to #13 (W4R/wasimlhr, topics cut from PR #8) are merged as
well: their functions were in the tree with PR #8, new from them are the Cursor B source row and the power
measurement tests. From PR #8's second round (2026-11-15) the fixes are in, its Studio layout is not.

New:

- One measurement page for everything: the categories down the left (Basic, Two sources, Serial & PAM, Jitter & eye,
  Power, Motor drive, Power rails, Double pulse; under them Selected), right of them the sources and the groups, one
  open at a time, at the bottom the last tile tapped with what it measures. Basic now holds all 60 standard
  measurements with full names (Peak-to-Peak, Upper Level, Time of Maximum ...), Two sources all 17 pair variants;
  the separate All 60 / Pairs page is gone. One tap on a tile adds it, a second removes it; a tile that is on is
  filled. Selected lists what is on with Remove, Set and Clear all. Analysis > Measure in the start menu opens this
  page (with Back), as the Measure button and key do. The settings are in named groups: Sources, Measure over,
  Display, Parameters, More.
- Lists scroll by touch: the measurement page, the Results pane and the event / search / segment tables have a scroll
  bar (drag the thumb, or press the track to jump there) and follow a finger drawn over them; a press that does not
  move is still a tap. The Up / Down buttons are gone. Fixed: folding a group of the measurement page after scrolling
  left the list past its end with no way back; a list's position is now always kept in range.
- Graphics = GPU: buttons and dialog frames no longer cover their area twice (the border is drawn only where the
  fill leaves it, `Canvas::framed`), pixel for pixel the same picture. With the measurement page open and HDMI
  Layout the monitor's GPU time fell from 38.5 to 12.3 ms a frame, the frame rate rose from 25.3 to 27.4 (DHO924S,
  four channels, 1920 x 1080).
- Measurements are modules (`ui/src/measures`, docs/measurement-modules.md): a module gives its measurements' tiles
  (name, description, mark, unit, sources, category and group) and computes their values on the worker thread; one
  line registers it. The 83 analysis measurements moved into nine modules (NRZ rate, jitter & eye, power, rails,
  magnetics, motor, double pulse ...), the standard and pair measurements are placed by two more. Their values are
  bit for bit the same; every module is checked by the tests (numbers, names, places, a square wave through
  `compute`).
- Every decoder is a module (`ui/src/protocols`, docs/protocol-modules.md), the core ones too: UART, I2C, SPI, CAN,
  LIN, Parallel, C-PHY, I2S, USB, ARINC 429, MIL-STD-1553, Manchester, MLT-3, 9-bit UART, Modbus RTU/ASCII, 1-Wire,
  DS2482, SWD, JTAG, QSPI, I3C, CAN FD, FlexRay and SENT moved out of the hand-wired code. The Decode menu, the
  protocol trigger, the bus timing and the demo traffic come from the module alone; values several protocols use (baud
  rate, bits, parity, SPI mode, CAN rate ...) are shared rows. Saved setups load as before. Choosing a protocol no
  longer changes the sources of roles it doesn't have.
- Decoders can be left out of a build: every optional decoder is a Cargo feature (`dec-<file>`, all on by default),
  `OOS_DECODERS="usb dcc"` or `OOS_DECODERS_OFF="avionics"` with ui/build.sh and boot/build-dev-image.sh. The seven
  core decoders are always in. Without the optional ones the scope binary is 260 KB smaller. Suggested by rdtsc.
- Analyser pages register the same way (`ui/src/ui/analysers.rs`, docs/analyser-modules.md): the start menu, System
  > Features, the view bar and its menu, SCPI `:VIEW`, the `view` of setups, which record a page needs, computing and
  drawing ask one registry instead of eight lists. Every page is checked the same way by the tests; the pages look
  and compute as before, pixel for pixel.
- Architecture decision records in `docs/adr`, by category (code, ui-ux, hardware, system, process), with a template
  and the first fifteen records of decisions already taken.
- The screens' symbols ψ, —, ≠, θ, α, β and μ have glyphs (they showed as '?', e.g. μ before the mean in the
  statistics card); the existing glyphs of the atlases are unchanged.
- Host layout scripts (ui/tests) open menus and set values by name (`menu`, `set`, `do`, and new: `close`, `do <tile>`
  on the measurement page) instead of tapping coordinates that moved; the storage script no longer shows a path that
  differs between runs.
- Top bar: Measure, Cursor, Display and Storage only. System is the first entry of its group in the start menu, SIM is
  under Analysis there, and Load file is a row of the Reference menu (the file comes from storage on the scope, from
  the file dialog in the desktop window).
- Cursor > Cursor B source (Track mode): cursor B reads another channel or reference than cursor A (PR #10; SCPI
  had it already).
- Short time bases draw every frame they can: the frames of a screen are placed on the little cores and added as
  differences per band, the hardware thread decodes only the frames the screen takes, a time budget keeps 27 to 30
  frames a second (4 channels at 100 ns/div: 29.5 instead of 3.5). Max, Min, Pk-pk, Mean, RMS and Std dev count every
  frame drawn.
- STOP keeps the whole record in the scope's memory and reads it again for every view: zoom out to see all of it, zoom
  in or move the delay anywhere for every sample of that part.
- Acquire > ADC cores: Matched (default) takes the offset pattern of the eight interleaved ADC cores off every frame
  (spur at 156.25 MHz: -52 to -77..-80 dBc with one channel); Raw shows the words as they come. `:ACQuire:CORes`.
  Acquire says "Aliasing: above <half the rate>, Peak shows it" while the rate is below the ADC's.
- Trigger level: no mark with AC or LF reject coupling; with DC a dashed line for 1.5 s after every change.
- From PR #7 (opt-in through `/oos/system.conf` unless noted): hardware segments with trigger time stamps (Acquire >
  HW segments), hardware averaging (Acquire > HW average), the trigger status from the SCU (default on), auto set-up
  levels from the FPGA measure unit (`autoset.levels = unit`; it answered with the levels of an earlier signal on a
  DHO924S, so Auto missed a 5 MHz sine of 0.2 Vpp), hardware DVM, hardware mask and zone tests, UART/SPI/I2C hardware
  triggers (Trigger > Bus), System > AUX out (trigger out, pass/fail), live min/max compression in the FPGA, deep-memory
  measurements from raw reads, an ADC core offset trim, `:CHANnel<n>:VERNier`, auto noise reject, batch pipeline,
  core placement, arming ahead, MSI for the DMA driver.
- SSH (Dropbear, PR #7): System > Network > Remote services > SSH and SSH settings (password, trusted keys from a USB
  stick, host key); off by default and only with `net.allow` and the firewall. telnet stays as it was. Keys can also
  be built into an image or written to a card on a computer (PR #8, docs/ssh.md).
- Network (PR #7, opt-in): a LAN-only firewall and source ACL (`net.allow`), the time zone from DHCP option 100, IPv6
  with stable addresses and the services and mDNS on IPv6.
- From PR #8: SCPI answers every query (errors instead of silence) and much more of the stock command tree, `*ESR?`,
  `*OPC?` that waits, `:TFORce`, `:WAVeform:STARt/STOP`, HiSLIP on a computer; references loaded from CSV, OOS BIN and
  WFM files (Load file), an offline mode that leaves the live screen alone; many new measurements and a measurement
  catalog in tabs, copy measurements from one source to another, measurements on references; SIM, a serial-record
  simulator into reference slots; an eye diagram; Measure > Dock; a zoom-area button; Ctrl+wheel zoom; channel
  deskew; a desktop build for Windows and macOS.
- Display > Bottom bar: Cards (default) or Compact, PR #8's bar with Menu, Sources, Overlay/Stacked, time and trigger.
- Display > Monitor view: with HDMI Layout the monitor shows one of the views (Display > Views) instead of the
  panel's, a second screen with its own arrangement (asked for in the forum): the panel keeps the controls, the monitor
  shows e.g. the spectrum, the eye or four windows. Set it up by switching the panel to that view; its analyser page
  is worked out beside the panel's. Saved with a setup (`hdmi_view`), `:DISPlay:VIEW:MONitor <n>` (0: the panel's).
- Acquire > Draw path: Standard (the fast path) or Batch (PR #7's pipeline), to compare them; `:SYSTem:PIPeline`.
- System > Features: leave out what you do not use (asked for in the forum): the analyser pages (Spectrum, Eye,
  Power, Bode, Bandwidth test, Magnetic, Inverter, Double pulse), SIM, Load file, and each decoder (System > Features >
  Decoders). What is left out is gone from the start menu, the top bar and the pickers and runs nothing. A setting of
  the scope (`features.off`, `decoders.off` in `/oos/system.conf`); nothing is left out by default.
- A setup loads what it can: a line this build cannot take (a decoder it does not have, a value out of range) is left
  out and named, the decoder switched off; before, the whole setup failed.

Changed:

- Channel keys with several channels on: the first press on a channel that is not selected only selects it, so V/div
  and position act on it without its menu covering the screen; the next press opens the menu, the one after that
  switches the channel off, as before. A channel menu that is open closes when another channel is selected. A tap on
  a channel's card selects and opens the menu the same way (a tap with the menu open still closes it).
- Mean, RMS, Std dev and Area of deep records (above 1 M points, shown as min/max pairs) are measured again: the
  screen window is read raw once in a while (`meas.deep_raw`, now on by default; `meas.deep_raw = off` turns it
  off). 0.711 Vrms for a 2 Vpp sine at 10 to 125 M on the DHO924S. The reads come less often for a larger window,
  so they cost about 5 % of the records; a window above 24 MB keeps "not available". Max, Min and Pk-pk come from
  the pairs until the first raw read.
- Choosing a decoder protocol on the hardware sets its own parameters only (threshold, baud, bits ...): no channel is
  switched on and the trigger stays. The sources the protocols share stay; a role the old protocol did not have takes
  a channel that is on and no other role uses, or none. The demo keeps the sources and trigger that suit its traffic.
  Same rule as the trigger types (the forum: one setting must not change another).
- Auto keeps a memory depth you fixed (PR #7); with depth Auto it picks a short record, as before.
- Frame 0 of a hardware batch of several frames is left out: the FPGA records it before the arm, sometimes joined
  with older memory, a stray trace (issue #5). `acq.frame0 = keep` keeps it.
- With two channels at different V/div the read-path gain and offset follow the stock lane order (PR #7): the DC level
  was off by about 7 % of the offset.
- The DHCP script flushes only IPv4 addresses (the IPv6 link-local address stayed gone).
- DMA reads are at most 4 MiB each; SINGLE arms one frame per batch; STOP disarms (PR #7).
- `:ACQuire:SRATe?` and `:MDEPth?` answer the hardware's rate and depth, as the top bar shows them.
- `:ACQuire:AVERages` snaps a count the menu does not offer down to the next one (PR #8).
- Storage > Save to: Desktop is now Local, the storage directory also while a USB stick is in (on the unit
  `/tmp/oos`, in RAM). Desktop wrote to `/root/Desktop/OOS` on the unit, a RAM folder nobody looks in, and to
  `Documents\OOS\Desktop` on Windows. The Storage title names where the files go now, also with a stick in.

Fixed:

- The top bar and the Network page showed eth0's link-local address (169.254.x.x, kept when no DHCP server answered)
  instead of an address set later by DHCP or by hand; a link-local address now comes only when there is no other.
- The first record after a change of settings in a deep record (one frame per batch) could show the older memory of
  the settings before, e.g. twice the amplitude after the memory depth was changed: it is left out.
- A deep record measured from its raw window no longer shows "not available" for the time the background
  measurement takes: the last value of the same settings stays until the new one is there.
- SCPI over the raw socket answered nothing when the client closed its side after the command
  (`echo "*IDN?" | nc <scope> 5555`); the lines it sent are answered again.
- From PR #8's second round (W4R): SIM no longer overwrites a loaded reference or switches channels off; math with
  a reference as source fits its scale; setups keep how references look; references with no channel shown get a volt
  axis; the power page says why it has no numbers (no signal, no current probe, no whole cycle) instead of zeros;
  the eye of a held SIM reference is computed instead of waiting for edges; Measure > Dock docks the card again after
  it floated, a floated eye follows the placement, a card docked under the plot shows up to 8 rows a page; the
  trigger pill shows the Pattern/Duration level; Delay and Setup/Hold take their second source from a channel that is
  on; the Sources popup keeps one source on; overlapping labels of the eye, spectrum, Bode, histogram, references and
  time axis, and the floating measurement card kept clear of the corner buttons.
- Choosing a trigger type changed settings the other types share (found by a new test after eTobey's point in the
  forum): Pulse switched CH4 on and made it the source, Video set CH2 to 0.2 V/div, Setup/Hold switched CH3 on, Nth
  edge turned the slope to falling. These presets suit the demo signals only; on the hardware a trigger type now sets
  its own parameters and leaves the channels, the source, the level and the slope as they were.
- Dots mode drew min/max pairs as two dots and lost the edges of a square at slow time bases (PR #7).
- Minimum persistence flickered with the batch sizes; the picture is graded by the frames it holds (PR #7).
- Mean-type measurements are no longer computed from min/max pairs, which made them wrong (PR #7); deep records get
  them from raw reads (see Changed).
- The record path no longer takes up to 48 records in a screen, which split a batch over two screens (issue #6).
- Axis labels: enough decimals for small steps at a large delay.

## 0.1.12

Install from 0.1.7 or newer as always: the web page, System > Update, or the USB stick; then switch the scope off at
the mains, wait 30 seconds and switch it on. From 0.1.6 or older: install 0.1.7 first (see 0.1.9).

New:

- Display > Views: up to four arrangements of the screen to switch between, instead of one busy screen (asked for in
  the forum). Each keeps its windows, second pane, measurements, DVM and counter, cursors, zoom, histogram, analyser
  page, XY and full screen; channels, time base, trigger and acquisition are shared. Switch with the numbers in the
  top bar, a swipe from the left or right edge of the plot inwards, the Quick key (Next view) or
  `:DISPlay:VIEW <n>`; `:DISPlay:VIEW:COUNt`. Saved with a setup.
- System > Date and time > NTP server = DHCP: the scope asks the NTP server its DHCP lease names (option 42), on eth0
  first, else on WLAN, and `pool.ntp.org` when the lease names none. The NTP state line says which server was asked
  and where it came from, e.g. `Synced 14:30:05, 192.168.1.1 (DHCP)` or `pool.ntp.org (no DHCP)`. DHCP is the first
  entry of the picker and the default of a new settings file; a server chosen before stays. Time from NTP stays off
  by default.
- The scope by name: `http://dho924s-123456.local:8080` reaches the web remote, also with only a link-local address
  (a cable straight to a computer). oos-ui answers mDNS (RFC 6762) for `<name>.local` and announces the web remote,
  SCPI and VXI-11 for browsing (DNS-SD, RFC 6763: `_http._tcp`, `_scpi-raw._tcp`, `_vxi-11._tcp`), only the services
  that are switched on. System > Network > Remote services has two new rows: mDNS (on by default) and Name, which
  shows `<name>.local`. The name comes from the model and the serial number and can be changed in the Name picker
  (Other: the keyboard); it is also the host name the scope sends to DHCP, so a router lists it. When another host
  already has the name, the scope takes `<name>-2`. Tested on the host (macOS finds the name and the services), not yet
  on a scope.

Changed:

- Graphics (CPU or GPU) moved from Display to System: it is a setting of the scope, not of the picture.
- CH2 is teal and CH4 sky blue now (the other way round until 0.1.11), as on the colour rings of the scope's inputs
  and probes: CH1 yellow, CH2 cyan, CH3 magenta, CH4 blue. Asked for in the forum.

Fixed:

- Display > Graphics = GPU with a monitor was slower than the CPU (reported in the forum: 16 to 19 frames a second
  with four channels and HDMI Layout, the CPU 22 to 23). The GPU mixed the counts of the stretched traces itself, 16
  reads of integer textures per pixel: 36 to 41 ms for the traces of a 1920x1080 monitor alone. Now the helper first
  works out each map's LUT indices at the map's size and the monitor takes them through the GPU's own filter, one
  read per pixel: 8 to 9 ms. Panel and monitor also keep their own textures in the helper, so a monitor frame no
  longer makes the panel upload its maps whole and draw its layers again. On a DHO924S, four channels, 1920x1080:
  30 frames a second in Mirror, Waveform and Layout (were 29, 25, 25; the CPU gives 30, 30, 26), the frame loop at
  22 to 29 % of its core (CPU: 51 to 70 %). The panel's pixels are the same as before; the monitor's traces differ
  from before only at their edges.
- Tests that would have caught it: the device test measures the frame rate, the frame loop's load and the GPU's time
  per frame with Graphics = CPU and = GPU and, with a monitor, in each HDMI mode (`frame_rate_by_graphics_and_monitor`,
  run against 0.1.11's helper it fails); `gpu/test-host.sh` checks a monitor frame's cost per pixel. New for it:
  `:SYSTem:GRAPhics CPU|GPU`, and `:SYSTem:STATe?` names the graphics, the monitor and the GPU time per screen.

## 0.1.11

Install from 0.1.7 or newer as always: the web page, System > Update, or the USB stick; then switch the scope off at
the mains, wait 30 seconds and switch it on. From 0.1.6 or older: install 0.1.7 first (see 0.1.9).

New:

- Bode with an external generator on the USB-A port of the scope (USBTMC, no PC): Analysers > Bode > Connection = USB
  takes the first USBTMC instrument, shows its `*IDN?` answer or why there is none; SCPI `:BODE:GENerator "usb"`. A
  new preset for the UNI-T UTG900 (UTG932, UTG962); Rigol DG and Siglent SDG work over USB as well. Tested on the
  host with a simulated device only.
- Trigger > Coupling for the edge trigger: DC, AC, LF reject, HF reject (`:TRIGger:COUPling`). AC and LF reject take
  the DC out of the trigger path, so a level of 0 V is the zero crossing of the signal whatever its offset; HF reject
  keeps the DC and suppresses fast signals. The trace itself is not filtered.

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
- Deep memory at short time bases: above 1 M points the screen got the whole record compressed to about 625 k
  min/max points, so at 10 M and 100 ns/div about 80 of them for the screen. A sine showed as a staircase, rise time
  and other measurements were off, and only about 3 records came per second. Such a record is now read for the screen
  as a window at every sample: on a DHO924S a clean sine at 10 M and 50 M, 21 to 27 records per second.
- HDMI with Display > Graphics = GPU: the panel flashed black now and then, mostly with a menu open, and the picture
  could be broken on both screens. The panel's third display buffer and the monitor's first one shared a place in the
  GPU's list, and the monitor's frame overwrote the panel's while the GPU still drew it. On a DHO924S with a
  1920x1080 monitor panel and monitor are now steady in Mirror, Layout and Waveform.

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
