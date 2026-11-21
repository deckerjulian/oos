# Measurements and analysers

## Measurements

A measurement is a number the scope reads from a waveform: the peak-to-peak voltage, the frequency, the rise time, the
delay from one channel to another. The pictures on this page come from a host run of OOS with its demo signals
(CH1 a 1 kHz square wave, CH2 a sine), not from a scope; the scope runs the same program.

### The strip

The values stand in the strip under the plot, one line per source, with the source's name in its colour. A default
set-up shows six measurements of CH1: Pk-pk, Freq, Mean, RMS, Rise and +Duty. They are measured again on every record.

![Measurement strip](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-strip.png)

A value with an SI unit takes its prefix (`2.00 mV`). A count reads as a whole number (`12`), a ratio without unit as a
plain decimal (`0.498`), and `ppm`, `rpm` and `rpm/s` take no prefix (`1500 rpm`). Over SCPI (`:MEAS:STRIP?`)
the values come in exponent form.

### Adding a measurement

1. Tap **Measure** in the top bar (or Menu > Measure; on a USB keyboard the key M). The measurement page opens.
2. On the left pick the category: **Basic**, **Two sources**, **Serial & PAM**, **Jitter & eye**, **Power**,
   **Motor drive**, **Power rails** or **Double pulse**.
3. At the top pick the **Source**: a channel or a reference. Categories that compare signals have two to four sources
   (Source A and B, Voltage and Current source, Phase A to C, ...).
4. Tap a group's title (AMPLITUDE, TIMING, EDGES & COUNTS, ...) to open it, then tap a tile. The measurement is in the
   strip at once, the tile turns white and a message says so (`Fall Time CH1 added`).

A second tap on a white tile removes the measurement again. The box at the bottom shows the tile tapped last: a drawing
of what it measures, one sentence about it, and whether it is on. **Selected (n)** at the bottom left counts the
measurements that are on.

![Measurement page](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-page.png)

The same measurement on another source is another line: pick CH2 as the source and tap the tile again. **Copy meas.
to** in the Measure settings (below) copies every measurement of Source A to another channel in one go.

**Two sources** measures from Source A to Source B: delay, phase and skew between the edges of the two (rising to
rising, falling to falling, and the mixed ones), and setup and hold time, with A the data and B the clock. Choose
both sources first, then the tile. In the strip such a measurement names both, `Delay rise-rise 1-2`.

![Two sources](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-two-sources.png)

### Settings of one measurement

A white tile has a **Set** button. It opens the settings of that one measurement: its **Source**, and its
**Reference** levels (10/90 or 20/80 % of the amplitude, for the edges and the times between them). Some measurements
have more: **Idle** for burst width, **Outside** for the time outside a level, **N periods** and **Edges** for the
duration of N periods. Rise and fall time show under **Edges used** how many of the edges in the record qualified
(`12 / 12 qualified`). What is set here holds for this measurement only; the others keep the values of the Measure
settings.

![Settings of one measurement](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-set.png)

### Removing measurements

**Selected (n)** opens the list of the measurements that are on, with their sources. A tap on a row removes it, **Set**
beside it opens its settings as above, **Clear all** removes every measurement. **Back** returns to the tiles.

![Selected measurements](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-selected.png)

### Measure settings

**Settings** at the top of the measurement page holds what applies to all measurements:

![Measure settings](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-settings.png)

| Setting | What it does |
|---|---|
| **Source A**, **Source B** | The sources a new measurement takes; B for the measurements with two sources. |
| **Copy meas. to** | Copies every measurement of Source A to the channel tapped. |
| **Gate** | **Screen** measures what the screen shows, so zoom and delay choose the part; **Full record** the whole record, also what is outside the screen. Also at the top of the measurement page. |
| **Refs** | The reference levels for edges: 10/90 or 20/80 % of the amplitude. |
| **Statistics** | The statistics card over the plot (below). |
| **Labels on trace** | Draws on the trace what each measurement measures (below). |
| **Dock** | Where the strip stands: **Under** the plot, **Left**, **Right** or **Above** it. |
| **Idle** | For Burst Width: the gap that ends a burst; `period` takes it from the signal. |
| **Tsetup**, **Thold** | For Setup+Hold: the setup and hold time the clock edges are checked against. |
| **Outside** | For Time Outside Level: above the high level, below the low level, or both. |
| **N periods**, **Edges** | For Duration N-Periods: how many periods, counted on clock or on data edges. |
| **Channel pair** | Delay, phase, skew, setup and hold between two channels, as a list of buttons. |
| **DVM / counter** | The voltmeter and the frequency counter (below). |
| **Clear all** | Removes every measurement. |

### Statistics

With **Statistics On** a card over the plot shows each measurement with its current value in the colour of its source
and under it the mean (`μ`) and the standard deviation (`σ`) of all values since the statistics began. `n` at the top
counts the values taken into the statistics: quick measurements can take one per drawn frame, those that need the whole
record one per record, so the count can differ from one measurement to the next (the Count column of the Results pane
shows each); `Screen` or `Full record` is the gate. With more measurements than fit, **Prev** and **Next** page
through them (`1-8 / 9`). A tap on the title folds the card to one line and opens it again.

![Statistics](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/meas-stats.png)

The statistics begin again when a measurement is added or removed, the gate changes, a setting of the scope changes
(time base, scale, trigger, most settings in a menu, Statistics itself; **Dock** not), or the display is cleared with
**Clear**. So set up first, then let the statistics run.

Minimum, maximum and count stand in the **Results** pane, a table with one row per measurement:

![Results pane](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/pane-results.png)

Open it with **Display > Windows**: **Density** Split, **Lower** Results. **Lower** Trend draws two measurements over
time instead (more under [Panes](#panes)). Over SCPI, `:MEASure:STATistic:ITEM? AVERages,VPP,CHAN1` reads the mean
(also `MAXimum`, `MINimum`, `CURRent`, `DEViation`, `CNT`), `:MEASure:STATistic:RESet` starts the statistics again;
see [[SCPI and VXI-11]].

### Labels on trace

**Labels on trace** in the Measure settings draws what each measurement means, with level lines and arrows:

![Labels](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/labels.png)

## Frequency counter

**Measure > DVM / counter** (also Menu > Instruments): **Counter** on, **Count source** a channel that is on, **Count mode** Frequency, Period or Totalize, **Digits** 3 to 6. From 0.1.9 the scope counts in hardware: a reciprocal counter in the FPGA, so 6 digits hold at any frequency, from below 1 Hz to tens of MHz. More digits take longer: 0.25 s for 3, 0.5 s for 4, 1 s for 5 and 6. Over SCPI: `:COUNter:CURRent?`.

## Panes

**Display > Windows** with **Density** Split puts a second view under the waveform; **Lower** chooses which. Tap the pane's title for its settings. The x closes it.

**Spectrum:** the spectrum with numbered peaks and a peak list. The span follows the signal. Tap the title for its settings, the button above the list to sort by frequency or level.

![Spectrum pane](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/pane-spectrum.png)

**Results:** every measurement with mean, min, max, sigma and count, with cursors and peaks beside it (picture under
[Statistics](#statistics)).

**Trend:** two measurements over time. Tap a chart to change its measurement.

![Trend pane](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/pane-trend.png)

**Events:** what the decoders found, with time and decoder. From 0.1.6 the DCC decoder lists one command per row ("Loco 5  Fwd 50/128", "Turnout 3  Left").

**Bus view** (from 0.1.6): a protocol's own picture of what it decoded. The DCC decoder for model railways shows the layout it controls, collected over the records until **Clear**: locomotives with direction, speed and functions, turnouts, signals, and the track status, green while the track is live and red after an emergency stop. On a track, measure the two rails with two probes as a pair (**Decode > B line (A-B)**) with a threshold of 0 V.

## Analysers

**Menu > Analysers** holds Spectrum, Eye / jitter, Power analysis, Bode, the bandwidth test, Magnetic, Inverter and
Double pulse. They replace the waveform. **Eye / jitter** opens as a full analysis page with optional side or lower
panes; its source and rate settings and **Close** are in its pane header. The other analyser views have three dots
at the top right, which lead to **Exit**, **Settings** and what fits the view.

If the source of an analyser is switched off, choose another one in its Settings. From 0.1.1 on it takes the first channel that is on by itself.

### Bode with an external generator

In **Analysers > Bode** pick the Generator preset by the commands the generator takes: **APPL** (Rigol DG, `:APPL`),
**BSWV** (Siglent SDG), **CHANnel** (UNI-T UTG900: UTG932, UTG962), or **SCPI** for one that takes `FREQ` and `VOLT`.
Then **Connection**:

- **LAN**, with **Gen address**: the IP, port 5555 if none is given; the knob moves the last number.
- **USB** (from 0.1.11, please test and report), for a generator in the scope's USB-A port instead of the network. The
  scope takes the first USBTMC instrument it finds (no PC, no driver), asks it for `*IDN?` and shows the answer;
  without one the sweep stops with the reason (no USB device, a device that is not USBTMC, no permission). Rigol DG
  and Siglent SDG generators are USBTMC instruments as well. So far this is tested with a simulated device only.

The scope sets channel 1 of the generator to a sine of **Amplitude** (Vpp) without offset, switches its output on and
steps through the frequencies. Over SCPI, `:BODE:GENerator "usb"` selects USB and
`:BODE:GENerator "192.168.1.20:5025"` the LAN again.

The **CHANnel** preset sends `:CHANnel1:MODe CONTINUE`, `:CHANnel1:BASE:WAVe SINe`, `:CHANnel1:AMPLitude:UNIT VPP`,
`:CHANnel1:BASE:AMPLitude`, `:CHANnel1:BASE:OFFSet 0`, `:CHANnel1:BASE:FREQuency` and `:CHANnel1:OUTPut ON`, from the
UNI-T UTG900 series programming manual (REV 00, 2022-07-15; the English UTG900E manual has the same commands). It has
not been tried with a real UTG900 yet.

### Bode with a free-running generator

**Experimental, from 0.1.9, please test and report.** For a generator that the scope does not control, but that
sweeps on its own and gives a sync (marker) pulse at the start of each sweep (for example a UTG932). Wire the generator
to the input of the device under test and to the input channel (CH1), the output of the device to the output channel
(CH2), and the sync output to CH4.

In **Analysers > Bode** set Generator = **Free-running sweep**, then the same **Start freq**, **Stop freq**, **Sweep
time** and **Sweep** (Lin or Log) as on the generator, the **Sync channel**, its edge and level (the sync pulse must
cross the level; set the channel's scale so the pulse is on the screen). **Input (ref) Off** compares the output with
**Ref level** in dBV (0 dBV = 1 V RMS) instead of an input channel. **Points** is the number of frequencies read from
the sweep. **Start / stop sweep** sets the time base and the memory for one sweep, triggers once (SINGLE) on the sync
edge and waits; start the sweep on the generator. The plot shows the gain over frequency, the settings come back
afterwards.

- Each frequency is read in a short window of the record, about 4 periods long and never wider than 25 % in
  frequency, by a fit of a sine that follows the sweep. Where the sweep passes a frequency in less than one period,
  that frequency is left out (the message says so). Over three decades, 20 Hz needs a sweep time of a few seconds; a
  linear sweep needs much more time for its low end than a logarithmic one.
- Phase comes from the fit of both channels and is shown only with an input channel, where both fits are clean.
- Sweeps up to about 0.9 s run as normal records (100 ms/div at most), the trigger one division from the left edge.
  Longer sweeps run in scan mode (Auto Roll off during the sweep, up to 200 s/div, 500 s and more): the trigger sits
  at the centre, the sweep in the right half, and the record holds min/max pairs. There the gain is half the
  peak-to-peak value in each window, and there is no phase. The start of the sweep is taken from the sync edge in the
  record itself. The scan path for sweeps above about 0.9 s has not been tried on a unit yet.
- On a 2-channel model use CH1 for the output, CH2 for the sync and Input (ref) Off.
- `:BODE:RUN ON` starts it over SCPI, `:BODE:DATA?` returns the points (phase NaN where there is none).

It has only been tested with simulated sweeps so far: results from a real generator are very welcome in the
[forum thread](https://www.eevblog.com/forum/testgear/oos-an-open-operating-system-for-the-rigol-scopes/) or as a
GitHub issue.

### Spectrum

![Spectrum](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/spectrum.png)

Harmonics in the Spectrum menu sets how far the view follows the signal, All shows the whole band. For exact amplitudes choose the Flattop window. The default Blackman window resolves more.

### Eye and jitter

![Eye](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/eye.png)

White figures are measured, amber ones come from a model. Where data is missing the page says what it needs. A bit
rate that the record holds under two samples a bit for (one kept from a faster time base) is found again from the
record when it came from one; a rate that was set stays, and the page asks for a faster time base.

## High resolution

**Acquire > High res** averages the samples behind each screen point. It lowers the noise only where the record holds more samples than the screen has points, so use a deeper memory.

## Overclocking and experimental limits

**System > Overclock**, shown with `adc.overclock = on` in `/oos/system.conf`, sets the sample clock from the next
power-on, in steps from 1.25 to 2 GHz. Above 1.25 GHz every step boots, but records at the full rate lose samples and
frequencies read too high: it is for tests only, and 1.25 GHz stays the default. Measured values are in
[docs/models.md](https://github.com/dekajoule/oos/blob/main/docs/models.md).

`/oos/limits.conf` overrides limits for your own tests. Every applied value is shown at start and in the System info.
Values outside the defaults are outside any rating: read the
[disclaimer](https://github.com/dekajoule/oos#disclaimer) first.
