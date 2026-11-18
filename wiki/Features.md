# Features

Everything OOS does, by area. Which of it has run on a scope: the [status](https://github.com/dekajoule/oos#status)
in the README. How to use it: the guides listed on [[Home]].

**Acquisition**
- Normal, peak detect, average and high-resolution modes (high resolution in software, it needs more samples than screen
  points); Auto or fixed memory depth up to 125 M points
- Time base from 1 ns/div; roll mode from 200 ms/div, scan mode at slow time bases
- Zoom window, pinch and drag on the touch screen
- Auto set-up, then a bar with four views of the signal: a few periods, many periods, the rising or the falling edge
- Channel bandwidth Auto (follows the sample rate, the default), Full, 20 MHz or a limit in 5 MHz steps

**Trigger**
- Hardware trigger: edge, pulse, slope, video, pattern, duration, timeout, runt, window, delay, setup/hold, Nth edge
- Protocol trigger (bus events found by the decoders)
- Zone trigger: up to two boxes or polygons a waveform must or must not cross
- Noise reject (wider comparator hysteresis); a slow, noisy edge is drawn on the edge you set, not on the other one

**Display**
- Dots or vectors, colour grade, waveform histogram, dotted graticule
- Waveform windows: the plot split into two windows side by side, two stacked, or four, each channel, math channel and
  the LA in a window of your choice, all on one time base
- Full screen: one tap at the top right of the plot hides the top bar and the cards, one more brings them back
- Text size Normal, Large or Larger for the bars, cards, measurement rows, menus and dialogs; backlight brightness
- HDMI output as a mirror, as a separate waveform workspace, or laid out again at the monitor's size
- the picture drawn on the GPU (Mali-T860, Panfrost; System > Graphics, CPU as before is the other choice): a third less
  load on the frame loop, a monitor scaled without CPU work
  ([gpu/README.md](https://github.com/dekajoule/oos/blob/main/gpu/README.md)); the web picture as JPEG from the video
  encoder (VPU)

**Measurement and maths**
- 35 automatic measurements plus delay and phase between channels, 14 at once, with statistics
- Time, voltage, tracking and XY cursors; DVM; frequency counter in the FPGA (reciprocal, up to 6 digits; frequency,
  period, totalize)
- Second pane under the plot, as high as its content: spectrum with a peak list, results table with statistics, cursors
  and peaks, decoder events, or the trends of two measurements
- Measurements labelled on the trace: level lines and arrows with their values
- Up to four views to switch between (Display > Views): each keeps its windows, second pane, measurements, cursors and
  zoom; a swipe from the edge of the plot or the numbers in the top bar switch
- Data logger: measurements as CSV at an interval from 1 s to 1 h, to the stick or the card
- 8 math channels, also on each other, with free formulas (`sqrt(CH1^2+CH2^2)`): add, subtract, multiply, divide,
  integrate, differentiate, abs, sqrt, log, ln, exp, Ax+B, low/high/band-pass and band-stop filters, FFT
- Reference waveforms with scale, offset, colour, name and CSV export and import, mask test

**Protocol decoders (29)**
- Four decoders at once, with an event table you can export and that jumps to an event; DCC lists one command per row
  ("Loco 5  Fwd 50/128")
- Serial: UART, UART 9-bit, I2C, SPI, Quad-SPI, I3C, 1-Wire, DS2482, SWD, JTAG
- Automotive and aerospace: CAN, CAN-FD, LIN, FlexRay, SENT, ARINC 429, MIL-STD-1553
- Audio, lighting, industrial: I2S, S/PDIF, DMX512, Modbus RTU, Modbus ASCII
- Other: USB low/full speed, USB PD, parallel, C-PHY, Manchester, MLT-3
- Model railways: NMRA DCC (contributed by Arjen Halma), with a view of the layout in Display > Second pane > Bus view:
  locomotives, turnouts, signals, and the track status, green while live and red after an emergency stop
- New decoders come as protocol modules: one file, one registration
  ([docs/protocol-modules.md](https://github.com/dekajoule/oos/blob/main/docs/protocol-modules.md))
- New measurements come as measurement modules the same way
  ([docs/measurement-modules.md](https://github.com/dekajoule/oos/blob/main/docs/measurement-modules.md))

**Analysers**
- Spectrum analyser: resolution from the whole record, span that follows the signal in the pane, fundamental and
  harmonics found as numbered peaks, RBW, trace modes, waterfall, THD, SNR, SFDR, limit line from a trace
- Eye diagram with voltage and UI axes, density colours, TIE trend, histogram and spectrum, bathtub curve. Jitter split
  into TJ, RJ and DJ (dual-Dirac model) and measured DCD, DDJ and PJ, pooled over records
- Power analysis: quality, harmonics, switching, ripple
- Bode plot with the built-in generator or an external one on the LAN or the USB-A port (APPL or BSWV command sets, a
  UTG900 preset, generic SCPI), or from one record of a generator that sweeps on its own and marks the sweep start on a
  sync output
- Identify: names a signal and sets up a trigger and decoder for it
- Bandwidth test: rise time, overshoot and magnitude response from a fast edge
- Magnetic: inductance of a winding, B, H and relative permeability once the core geometry is set, cycle loss, and
  the loop of current over flux
- Inverter: the DQ0 transform of three phases, and speed, direction and angle from a shaft source
- Double pulse: one turn-off and one turn-on of a switch, with peak voltage and current, switching times and energies
  and the reverse recovery charge

**Logic analyser** (models with an LA port, PLA2216 probe)
- D0..D15, hardware trigger on the digital lines, also in roll and scan mode. Thresholds are set as under stock, but on
  the unit tested so far neither system changes the level the lines switch at

**Generator** (S models)
- Sine, square, ramp, pulse, noise, DC, arbitrary wave (also captured from a channel), AM, FM, PM

**Remote control**
- Web remote, SCPI over TCP, VXI-11, HiSLIP (on a computer), LXI identification, MCP for AI
  assistants, SSH, mDNS and DNS-SD: every interface with its port is on [[Remote Control]], the commands on
  [[SCPI and VXI-11]]
- None of them slows the scope down; all of them run over Ethernet and over WLAN with an RTL8188EU stick or a TP-Link Archer TX10UB Nano

**System**
- Own cold boot of all hardware: the stock system never has to run first
- Self-calibration into own files (`/oos/cal`); factory files are never changed
- System > Overclock (with `adc.overclock = on`): the sample clock in steps from 1.25 to 2 GHz, for tests only, from the
  next power-on
- Saves setups, CSV, binary waveforms and PNG, also to a USB stick; System info page with model, temperatures and limits
- System > Date and time: the clock from an NTP server (off by default), time zones with daylight saving time, or set by
  hand; once the clock is set, file names carry the local date and time
- System > Network > 10 Mbit retry: an Ethernet link that comes up at 10 Mbit/s gets one more autonegotiation
- On-screen keyboard for names, formulas and WLAN passwords, with arrow keys to correct inside a long entry
- The start-up screen shows the version and the build
- Signed update packages with trial start and automatic rollback
