# OOS SCPI reference

OOS implements a **subset of Rigol DHO800/DHO900 SCPI**, including commands used by DHO924S scripts, plus OOS analysis extensions. A script works when its commands, parameters and transfer assumptions are in this reference. OOS does not implement the complete stock firmware command set, impersonate Rigol, or speak TekScope's command dialect. `*IDN?` identifies OOS.

This reference describes the repository implementation. Primary Rigol syntax was checked against the [DHO800/DHO900 Programming Guide](https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf), including trigger MODE and waveform preamble definitions. Host tests against the interpreter establish the implemented behavior; they do not establish physical instrument or VISA-driver certification.

## Connection and syntax

| Interface | Default | Implementation |
| --- | --- | --- |
| Raw TCP SCPI | TCP 5555 | Recommended for scripts. Newline-terminated program messages; newline-terminated replies. |
| HiSLIP | TCP 4880 | Same interpreter, with HiSLIP framing. Basic sessions; full locking/overlap semantics are not implemented. A reply is split into messages of the client's maximum message size, but of at least 4 KiB of data. |
| VXI-11 | Linux scope service | Same interpreter, partial RPC transport. Pending operation reads have the limitation described below. Not offered on a computer (`--serve`). |
| HTTP/MCP adapter | HTTP web port | `POST /mcp`, JSON-RPC `tools/call` with tool name `scpi`. This is not a VISA interface. `/cmd` uses a separate front-panel command language. |

On a computer, `oos-ui --serve` offers the same interpreter without a scope, and can use other ports. For example:

```sh
(cd ui && cargo run --release -- --serve --offline --web-port 18080 --scpi-port 5555 --hislip-port 14880)
```

For that, use `TCPIP0::127.0.0.1::5555::SOCKET`, or HiSLIP resource `TCPIP0::127.0.0.1::hislip0,14880::INSTR`. On another computer, substitute the PC's LAN address. These services are unauthenticated bench interfaces. A host build does not require or start an SSH server; [scope-image SSH](ssh.md) is separate.

Keywords are case-insensitive. The capitalized part in a mixed-case spelling is the short form: `:TRIG:EDGE:SOUR?` and `:TRIGger:EDGE:SOURce?` are equivalent. Use either the complete long form or its documented short form. Intermediate abbreviations are not generally accepted.

Use an absolute header for every semicolon-separated command. The parser splits literal semicolons; SCPI relative-header inheritance and quoted strings containing semicolons are unsupported. Prefer one query per message when replies themselves contain semicolon-separated rows.

Numerical setters normally accept finite decimal/scientific notation in base units: volts, seconds and hertz. Use `2e-1`, not `200mV`. Memory depth additionally accepts K/M suffixes. Boolean parameters use `ON|OFF|1|0`. A table's `[?]` means set and query; `?` alone means query only.

Waveform names are `CHAN1` through `CHAN4` as available on the model, and `R1` through `R10` (`REF1` through `REF10` are aliases). Use `CHAN1`, rather than the UI label `CH1`, in scripts. Reference sources are an OOS extension. An absent reference is not a physical input.

## Common commands and acquisition

| Command | Meaning |
| --- | --- |
| `*IDN?` | `OOS,<model>,<serial>,oos-ui <version>`. On a computer the serial can be `0`. |
| `*RST` | Restore the app's default configuration. This changes the acquisition workspace. |
| `*CLS` | Clear the SCPI error/status state and pending operation-complete event registrations. Does not clear waveform memory. |
| `*OPC?` | Wait for the defined finite operations below, then return `1`. |
| `*OPC` | Register completion of those finite operations in standard-event bit 0. |
| `*ESR?` | Read and clear implemented event bits: OPC 0, query error 2, device error 3, execution error 4, command error 5. Starts clear; no power-on bit or complete status/SRQ subsystem. |
| `:RUN` / `:STOP` | Idempotent acquisition assignments, also used for stored replay and SIM. |
| `:SINGle` | Arm once; wait for a qualified capture and then stop. |
| `:TFORce` or `:TRIGger:FORCe` | Explicitly accept a capture without requiring a trigger match. |
| `:AUToscale` / `:AUToscale?` | Start ADC Autoset / query whether it is running. Autoset does not fit a reference. |
| `:CLEar` | Clear display persistence and measurement statistics through the normal Clear action. |

There is one acquisition context. A loaded reference alongside live channels is a passive overlay. Selecting it as the primary trigger source enters stored-record acquisition. Changing a primary source preserves Run/Stop, preserves an armed Single, discards the previous source's trigger result, and re-arms a running acquisition in WAIT. Changing focus or a secondary source does not take over acquisition.

Stored Run searches successive qualifying windows through the immutable record and wraps at its end. It does not create new ADC samples. SIM produces fresh candidates; a candidate replaces accepted data only when its trigger qualifies, Auto permits fallback, or Force is requested. Normal/Single misses remain WAIT and retain the accepted picture. Stopped panning updates Screen analysis without adding captures or statistics observations. Full-record values on an immutable reference can legitimately remain constant during replay.

### Operation completion

`*OPC?` fences the finite operation current at the barrier: Autoset, a submitted SIM generation, and an armed Single. Continuous Run and background measurement/Math/decode/Eye calculations are not drained by OPC. Use their readiness/value interfaces separately. Operation completion is not a statement that a result is valid; inspect the error queue and trigger status.

Raw TCP and HiSLIP retain a bounded per-client continuation while waiting, allowing UI progress and other clients. Commands after the barrier execute after it completes; earlier writes are not repeated. A Single with no matching event can wait indefinitely, so choose a client timeout appropriate to the test. Cancellation/rearming invalidates the old operation rather than silently waiting on a newer arm. See the interpreter's cancellation errors in `:SYST:ERR?`.

Cancellation or rearming interrupts a waiting query with error -410, emits no `1`, and discards its remaining commands. A cancelled `*OPC` event registration does not set the completion bit.

The synchronous command/HTTP adapter cannot retain a pending `*OPC?` and reports error -221 instead of returning a false `1` or `ok`. Commands preceding the rejected barrier have already executed; its remaining commands are discarded. VXI-11's legacy read path can return timeout error 15 immediately while output is pending, without honoring the requested read timeout. A VXI client must retry reads after completion; prefer raw TCP or HiSLIP for a waiting `*OPC?`. Full VISA synchronization equivalence is not claimed.

## Channels, timebase and trigger

| Command | Parameters / reply |
| --- | --- |
| `:CHANnel<n>:SCALe[?]` | Volts/div; setter chooses the nearest supported scale. Probe factor applies. |
| `:CHANnel<n>:OFFSet[?]` | Volts. |
| `:CHANnel<n>:COUPling[?]` | `DC|AC|GND`; `DC50` only on a board supporting it. |
| `:CHANnel<n>:PROBe[?]` | Positive attenuation factor. |
| `:TIMebase[:MAIN]:SCALe[?]` | Seconds/div; nearest supported timebase. |
| `:TIMebase[:MAIN]:OFFSet[?]` | Horizontal delay in seconds; `POSition` is an alias. |
| `:ACQuire:SRATe?` | ADC/model sample rate, not a loaded reference's native rate. |
| `:ACQuire:MDEPth[?]` | Set `AUTO` or a supported menu depth, e.g. `1000000`, `1M`. Query returns the numeric configured/effective acquisition depth. |
| `:TRIGger:MODE[?]` | Actual trigger type. Same OOS handler as `:TRIGger:TYPe[?]`. |
| `:TRIGger:STATus?` | `STOP|TD|AUTO|WAIT`. A completed Single reports STOP. |
| `:TRIGger:EDGE:SOURce[?]` | `CHANn|Rn`; available LA inputs can use `D0` through `D15`. |
| `:TRIGger:EDGE:LEVel[?]` | Volts. |
| `:TRIGger:RUNT:UPPer[?]` / `:LOWer[?]` | Absolute high/low thresholds; aliases `:TRIGger:RUNT:ALEVel` / `:BLEVel`. A threshold past the other one is refused with `-224`, as `:MEASure:SETup:MAX` / `MIN`. |
| `:TRIGger:SOURce2[?]` | Secondary CH/ref source for applicable OOS trigger types. |

Trigger type names are `EDGE`, `PULSE`, `SLOPE`, `VIDEO`, `PATTERN`, `DURATION`, `TIMEOUT`, `RUNT`, `WINDOW`, `DELAY`, `SETUPHOLD`, `NTHEDGE`, `BUS`. MODE replies use canonical short tokens: `EDGE`, `PULS`, `SLOP`, `VID`, `PATT`, `DUR`, `TIM`, `RUNT`, `WIND`, `DEL`, `SET`, `NEDG`, `BUS`; TYPE retains the UI's descriptive names. This list does not imply that every stock type-specific subtree exists. Additional settings use the UI/setup schema; unsupported headers report an error.

Reference triggering is software search over the native stored samples and is independent of ADC bandwidth. It is not a stock DHO reference-trigger promise. SIM cannot qualify LA/Pattern/Duration inputs and reports an explicit unavailable condition. Bus uses the configured decoder protocol/roles/match settings; auxiliary sources must have real overlapping data.

Example: arm an edge on a loaded reference, without changing its samples:

```text
:STOP
:TRIG:EDGE:SOUR R1
:TRIG:MODE EDGE
:TRIG:EDGE:LEV 0
:TRIG:EDGE:SLOP POS
:SING
:TRIG:STAT?
```

Choose a level inside the actual waveform range. Use `*OPC?` only when willing to wait for this arm. Return to live inputs with a primary `CHANn` trigger selection or the UI's Live action.

### Channel settings

The settings of the Channel menu, their `:CHANnel<n>` commands and their keys in a setup file. They are declared once,
in `ui/src/settings/channel.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A command sets
channel `<n>`, a key `ch<n>.*` channel n; `fine_v` and `v_expand_gnd` hold for every channel. A value `:UNITs`,
`:VERNier` or `:EXPand` does not take is refused with `-224`, one of the others with `-113`. The scale, offset,
coupling, probe, bias, deskew and label (above and below) are written by hand.

<!-- settings:channel:begin -->
| Command | Takes | Setup key | Channel menu | What it does |
| --- | --- | --- | --- | --- |
| `:CHANnel<n>:DISPlay[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `ch<n>.on` | Display | Shows the channel. The menu switches on only the inputs the model has. |
| `:CHANnel<n>:BWLimit[?]` | `OFF|20M|AUTO` or a supported filter name | `ch<n>.bw` | Bandwidth | The channel's bandwidth limit. OOS filtering extends stock choices. |
| `:CHANnel<n>:INVert[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `ch<n>.invert` | Invert | Inverts the channel. |
| `:CHANnel<n>:VERNier[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `fine_v` | Fine scale | Fine vertical control, one switch for every channel: the scale steps through the rows of the calibration instead of 1-2-5. Off keeps a fine scale until the next coarse step. |
| `:CHANnel<n>:UNITs[?]` | `VOLTage`, `AMPere`, `WATT`, `UNKNown` | `ch<n>.unit` | Unit | Supported channel unit. Use the configured unit when interpreting cursor/readout values. |
| `:CHANnel<n>:EXPand[?]` | `GND`, `CENTer`; the query answers `GND` or `CENT` | `v_expand_gnd` | Expand around | Vertical expansion reference supported by the model, one switch for every channel: a change of scale keeps the ground line or the screen centre in place. |
<!-- settings:channel:end -->

### Trigger settings

The values of the Trigger menu, their `:TRIGger` commands and their keys in a setup file. They are declared once, in
`ui/src/settings/trigger.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value a
command does not take is refused with `-113`.

<!-- settings:trigger:begin -->
| Command | Takes | Setup key | Trigger menu | What it does |
| --- | --- | --- | --- | --- |
| `:TRIGger:EDGE:SLOPe[?]` | `POSitive`, `NEGative`, `RFALl`, or any word that starts with a short form | `slope` | Slope | The edge that triggers; also of the slope, timeout and Nth edge triggers, and A of the delay trigger. |
| `:TRIGger:SWEep[?]` | `AUTO`, `NORMal`, `SINGle`, or any word that starts with a short form | `trig_mode` | Sweep | Auto draws without a trigger after a while, Normal only on a trigger, Single once; the menu's Single also starts the acquisition. |
| `:TRIGger:HOLDoff[?]` | seconds from 0 to 10 | `holdoff` | Holdoff | The time after a trigger in which no other one is taken; 0 is off. The menu steps 1-2-5 from 100 ns. |
| `:TRIGger:NREJect[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `trig_nrej` | Noise reject | A wider hysteresis of the trigger comparator (the values by scale: docs/protocols/trigger.md 2.3). |
| none | a choice of the auto set-up, not of the trigger | `auto_nrej` | Auto noise rej. | The auto set-up sets noise reject from the noise on the flat levels of the trigger signal. |
| `:TRIGger:COUPling[?]` | `DC`, `AC`, `LFReject`, `HFReject` | `trig_coup` | Coupling | A filter in the trigger path of the edge trigger; the trace keeps the channel's coupling (docs/protocols/trigger.md 2.5). |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `trig_slope2` | Slope B | The edge of B (delay trigger) or of the clock (setup/hold trigger). |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `trig_pol` | Polarity | The polarity of the pulse (pulse and runt triggers). |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `trig_t1` | Time | The time the condition compares with, the lower limit of Inside and Outside; the timeout of the timeout trigger. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `trig_t2` | Upper limit | The upper time limit of Inside and Outside. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `video_std` | Standard | The video standard of the video trigger. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `video_sync` | Sync | What the video trigger fires on: every line, line n, odd or even fields, every field. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `win_mode` | Trigger on | When the window trigger fires: the signal enters the window, leaves it, or stays inside. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `nth_idle` | Idle time | The quiet time before the edges the Nth edge trigger counts. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `setup` | Setup | The setup time the setup/hold trigger checks the data against the clock edge. |
| none | OOS has no `:TRIGger` subtree of this trigger type yet | `hold` | Hold | The hold time the setup/hold trigger checks the data against after the clock edge. |
<!-- settings:trigger:end -->

### Acquisition settings

The settings of the Acquire menu, their `:ACQuire` commands and their keys in a setup file. They are declared once,
in `ui/src/settings/acquisition.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value
`:TYPE` or `:AVERages` does not take is refused with `-224`, one of the others with `-113`. The memory depth
(`:ACQuire:MDEPth`, above) and the segment count (`:ACQuire:ULTRa:MAXFrame`, below) are written by hand.

<!-- settings:acquisition:begin -->
| Command | Takes | Setup key | Acquire menu | What it does |
| --- | --- | --- | --- | --- |
| `:ACQuire:TYPE[?]` | `NORMal`, `PEAK`, `AVERages`, `HRESolution` | `acq` | Mode | How a record is made: Normal, Peak, Average (over the count of `avg_count`) or High res (in software, from more samples than screen points). |
| `:ACQuire:CORes[?]` | any word that starts with `MATC` or `RAW`, or `ON`, `OFF`, `1`, `0`; the query answers `MATC` or `RAW` | `cores` | ADC cores | Matched takes the offsets of the interleaved ADC cores off in software (docs/protocols/adc-mode.md 6); Raw leaves the words as they come. |
| `:ACQuire:AVERages[?]` | a whole number from 2, set to the step at or below it: 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024 | `avg_count` | Averages | How many records the Average mode averages. |
| `:ACQuire:AVERages:HARDware[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `avg_hw` | HW average | Average mode: the FPGA averages where the board can, else the software does; experimental, not verified on the unit (docs/fork/upstream-audit.md). The menu offers it in Average mode with a hardware source. |
| `:ACQuire:SEGMents:HARDware[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `seg_hw` | HW segments | Segments recorded by the FPGA instead of the software history; experimental, not verified on the unit (docs/fork/upstream-audit.md). The menu offers it while segments are on. |
<!-- settings:acquisition:end -->

## Waveform transfer

| Command | Meaning |
| --- | --- |
| `:WAVeform:SOURce[?]` | Select `CHANn|Rn` for transfer; does not select the acquisition trigger. |
| `:WAVeform:MODE[?]` | `NORMal|MAXimum|RAW`. MAX uses the CH screen while running and native memory while stopped. |
| `:WAVeform:FORMat[?]` | `BYTE|WORD|ASCii`. Invalid formats are rejected without changing the selection. |
| `:WAVeform:STARt[?]` / `:STOP[?]` | One-based inclusive transfer page; default start 1, stop through the last available point. Setting either keeps start <= stop. |
| `:WAVeform:POINts[?]` | Query actual points in the page; set the requested count starting at STARt. Pages clip at the available data boundary. |
| `:WAVeform:PREamble?` | Ten comma-separated fields listed below. |
| `:WAVeform:DATA?` | IEEE definite-length binary block; ASCII format also uses a block. |
| `:DISPlay:DATA? [BMP|PNG]` | IEEE block containing a screenshot; default 24-bit BMP. |

For CH inputs, NORM reads the interpolated display record; RAW uses retained native acquisition memory when available, including points outside the screen. If no native record exists, it falls back to the available processed record. References use their native arrays in all three modes, restricted by the chosen transfer page. Do not assume `:ACQ:SRAT?` describes a reference: read its preamble. RAW is permitted while running in OOS; stock Rigol stop-only transfer assumptions differ. Multiple queries while running can observe different accepted captures; Stop first when metadata and data must match.

Preamble fields:

```text
format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference
```

`format` is 0 BYTE, 1 WORD, 2 ASCII. `type` is the selected mode: 0 NORM, 1 MAX, 2 RAW. MAX still switches its CH backing record according to Run/Stop. `count` is 1. Native times are seconds. Native transfer origins preserve the recorded metadata; analysis can additionally apply deskew corrections. Integer data converts as:

```text
time[k] = (k - xreference) * xincrement + xorigin
volts[k] = (code[k] - yorigin - yreference) * yincrement
```

BYTE uses unsigned 8-bit samples. WORD uses unsigned **little-endian** 16-bit samples as the explicit OOS wire policy, preserved for existing clients. Rigol section 3.28.3 specifies two bytes per point without specifying their order, so this is not a verified physical Rigol byte-order claim. These are quantized to the reported scale and can clip if the source lies outside that scale. ASCII contains comma-separated volt values in seven-significant-digit scientific notation; do not apply integer scaling again. Neither transfer is a promise of bit-identical original vendor file bytes.

A block is `#<N><N decimal length digits><payload>`, followed by the response newline. Read the announced byte length; never use newline splitting inside a binary payload. Use `:WAV:STARt`/`:WAV:STOP` for smaller pages; preamble origin is the time of the page's first point. Metadata borrows the record without copying samples. Before copying DATA, the server rejects pages whose worst-case encoded size exceeds 64 MiB with -223; ASCII uses a conservative 14 bytes per sample bound. Long pages still encode on the main-loop path and slow receivers can stall it within existing send timeouts. WORD generally uses less memory than ASCII.

Minimal standard-library Python reader for a host build started as above:

```python
import socket
import struct

with socket.create_connection(('127.0.0.1', 5555), timeout=10) as scope:
    scope.settimeout(120)

    def send(command):
        scope.sendall((command + '\n').encode('ascii'))

    def exact(n):
        data = bytearray()
        while len(data) < n:
            part = scope.recv(min(65536, n - len(data)))
            if not part:
                raise EOFError('scope disconnected')
            data.extend(part)
        return data

    def query(command):
        send(command)
        line = bytearray()
        while True:
            ch = exact(1)
            if ch == b'\n':
                return line.decode('ascii').strip()
            line.extend(ch)

    print(query('*IDN?'))
    send(':STOP')
    send(':WAV:SOUR R1')  # Load R1 first, or use CHAN1.
    send(':WAV:MODE RAW')
    send(':WAV:FORM WORD')
    pre = [float(x) for x in query(':WAV:PRE?').split(',')]
    send(':WAV:DATA?')
    assert exact(1) == b'#'
    digits = int(exact(1))
    payload = exact(int(exact(digits)))
    assert exact(1) == b'\n'
    assert len(payload) == 2 * int(pre[2])
    if payload:
        code = struct.unpack_from('<H', payload)[0]
        print('points:', int(pre[2]), 'dt:', pre[4],
              'first volts:', (code - pre[8] - pre[9]) * pre[7])
    print(query(':SYST:ERR?'))
```

The example deliberately stops acquisition and retains that state. It reads one payload rather than constructing millions of Python float objects.

## Measurements and analysis

| Command | Reply / parameters |
| --- | --- |
| `:MEASure:ITEM? <item>[,<source>]` | Standard scalar; default source is the current Measure selection. Valid items are listed below. |
| `:MEASure:ITEM? <pair>,<A>,<B>` | Pair scalar, with native physical-time alignment; omitted roles use the Measure selections. |
| `:MEASure:ITEM <item>[,<A>[,<B>]]` | Add a standard/pair readout. Duplicates preserve statistics and Single. |
| `:MEASure:SOURce[?] <A>[,<B>]` | Default Measure source(s); query returns A. Sources are `CHANn|Rn`. |
| `:MEASure:CLEar` | Clear standard, pair and catalog selections through the shared statistics reset. |
| `:MEASure:GATing[?]` | `SCREEN` follows the viewport; `NONE` means Full record. This gate is an OOS extension. |
| `:MEASure:REFS[?]` | `10/90|20/80` edge thresholds. |
| `:MEASure:TSETup[?]` / `:THOLd[?]` | Nonnegative seconds: `SETHOLD` counts the clock edges whose setup or hold is shorter. |
| `:MEASure:STRip?` | Configured standard/pair/catalog rows, then enabled DVM/counter rows. |
| `:MEASure:READout?` | Strip rows plus Eye figures when that analysis page is active. |
| `:EYE:ITEM? <figure>` | Current configured Eye result; does not itself run one-click Auto Eye. |
| `:VIEW[?]` | `SCOPE|SPECTRUM|EYE|POWER|BODE|MAGNETIC|INVERTER|DPT`. |

Values use base units, phase in degrees and duty/overshoot in percent. Unavailable or pending numeric values use `9.9E37`, not a real measurement. Poll pending analysis without counting it as another capture. `*OPC?` does not wait for these calculations. Long standard/reference analysis uses bounded workers; some one-off advanced fact queries remain synchronous and should not be polled at high frequency.

For the pair items `SETUP`, `HOLD`, `SETHOLD`, `FSETUP` and `FHOLD`, A is the data and B the clock, as on the
measurement page. They are OOS items: the stock DHO800/DHO900 programming guide has no setup or hold measurement under
`:MEASure:ITEM` (its pair items are the delays and phases). The order follows the stock setup/hold trigger, which
names the data source (`:TRIGger:SHOLd:DSRC`) before the clock source (`CSRC`).

Readout rows are separated by semicolons. Standard rows are `ITEM,SOURCE,value`; pairs are `ITEM,A,B,value`. Catalog rows are `CAT<id>,A,B,C,D,value`, including unused stored role columns. DVM/counter rows use `DVM,mode,SOURCE,value` and `COUNTER,mode,SOURCE,value`; Eye rows use `FIGURE,value`. The six-tab catalog's selected rows are exported here; `:MEAS:ITEM? CAT<id>` is not a direct-query command. Select/configure catalog rows through the UI or the explicit setup schema.

Examples:

```text
:MEAS:GAT NONE
:MEAS:ITEM? VPP,R1
:MEAS:ITEM? VRMS,R1
:MEAS:ITEM? RRDELAY,R1,R2
:MEAS:STRIP?
```

Additional OOS families accept item names defined by their analysis page/source implementation:

| Family | Form / examples |
| --- | --- |
| Power | `:POWer:ITEM? <item>[,V,I]`: `VRMS`, `IRMS`, `REAL`, `APP`, `REAC`, `PF`, `PHAS`, `FREQ`, `EON`, `EOFF`, `ECOND`, `RDSON`. Single-source ripple items also exist. |
| Harmonics | `:POWer:HARMonics? [source]`: semicolon-separated harmonic-index/amplitude pairs. |
| Spectrum | `:SPECtrum:ITEM? <item>[,source]`: `F1`, `THD`, `THDPCT`, `THDN`, `SNR`, `SINAD`, `SFDR`, `ENOB`, `ACPR`, `ACPRLO`, `ACPRHI`, `FLOOR`. |
| Magnetic | `:MAGnetic:ITEM? <item>[,V,I]`: `L`, `B`, `H`, `MUR`, `LOSS`. Requires the configured geometry/calibration. |
| Inverter | `:INVerter:ITEM? <item>[,sources...]`: configured inverter result, numeric or text. |
| Double pulse | `:DPT:ITEM? <item>[,V,I[,G]]`: e.g. `VPEAK`, `IPEAK`, `EON`, `EOFF`, `TDON`, `TR`, `TDOFF`, `TF`, `QRR`. |
| Bandwidth | `:BANDwidth:ITEM? <item>[,source]`: `RISE`, `BW`, `OVER`. Estimator limits apply. |
| Bode | `:BODE:MODE[?] LOOP|PSRR|IMPedance`, `:BODE:RUN ON|OFF`, `:BODE:DATA?`, `:BODE:GENerator[?] "ip:port"`. RUN can control the configured generator/acquisition. |

See [reference analysis](reference-analysis.md) for sampling, model-derived jitter, physical alignment and C-PHY limits. These extensions are not stock Rigol/DPOJET compatibility claims.

## Cursors, DVM and counter

| Command | Meaning |
| --- | --- |
| `:CURSor:MODE[?]` | `OFF|MANual|TRACk|XY`; MAN keeps the selected time/amplitude type. |
| `:CURSor:MANual:TYPE[?]` | `TIME|AMPLitude`. |
| `:CURSor:{MANual|TRACk}:SOURce[?]` | Selected `CHANn|Rn`, without changing acquisition. SOURce1 aliases it; a different SOURce2 is unsupported. |
| `:CURSor:{MANual|TRACk}:CAX[?]` / `:CBX[?]` | A/B physical time in seconds; clips to the current visible span. |
| `:CURSor:{MANual|TRACk}:CAY[?]` / `:CBY[?]` | A/B volts in the selected source's displayed lane, including display zoom. |
| `:CURSor:XY:{AX|BX|AY|BY}[?]` | Independent X/Y coordinates on one selected waveform, not independent input sources. |
| `:CURSor:{MANual|TRACk|XY}:{AXValue|BXValue|AYValue|BYValue|XDELta|IXDelta|YDELta}?` | Values/deltas; IXDelta is 1/abs(time delta). Missing is `9.9E37`. |
| `:DVM:SOURce[?]`, `:COUNter:SOURce[?]` | Acquired CHANn. |
| `:DVM:CURRent?` | Reads volts. |
| `:COUNter:CURRent?` / `:COUNter:TOTalize:CLEar` | Current result / reset accumulated counter statistics. |

Cursor changes preserve capture statistics and an armed Single. Track values use the selected native/reference waveform and physical origin; positions remain viewport fractions. DVM/counter sources are acquired channels, not references; physical meter/counter parity is not established.

### DVM and counter settings

The switches and modes of the DVM and the counter, their commands and their place in a setup file. They are declared
once, in `ui/src/settings/meters.rs`; these tables are written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value a
command does not take is refused with `-224`. Each meter keeps its settings in one setup line,
`dvm = <on> <source> <mode>` and `counter = <on> <source> <mode> <digits>` (the modes by their place in the list);
the sources (above) are written by hand.

<!-- settings:dvm:begin -->
| Command | Takes | Setup key | DVM and counter menu | What it does |
| --- | --- | --- | --- | --- |
| `:DVM:ENABle[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `dvm` (on) | DVM | The voltmeter's reading of its source in the readouts; from the DVM block where it is switched on and the board has one (docs/fork/dvm.md), else from the measurements of the records. |
| `:DVM:MODE[?]` | `DC`, `ACRMs`, `ACDCrms` (also `ACDC`); the query answers `DC`, `ACRMS` or `ACDC` | `dvm` (mode) | DVM mode | What the voltmeter reads: the mean (DC), the rms without the mean (AC rms) or the whole rms (AC+DC rms). |
<!-- settings:dvm:end -->

<!-- settings:counter:begin -->
| Command | Takes | Setup key | DVM and counter menu | What it does |
| --- | --- | --- | --- | --- |
| `:COUNter:ENABle[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `counter` (on) | Counter | The counter's reading of its source in the readouts (docs/protocols/counter.md). |
| `:COUNter:MODE[?]` | `FREQuency`, `PERiod`, `TOTalize` | `counter` (mode) | Count mode | What the counter shows: the frequency, the period, or the edges counted since `:COUNter:TOTalize:CLEar`. |
| `:COUNter:NDIGits[?]` | a whole number from 3 to 7 | `counter` (digits) | Digits | The digits of the counter's reading. |
<!-- settings:counter:end -->

## Reference storage, SIM and decoding

| Command | Meaning |
| --- | --- |
| `:REFerence:SLOT[?] R1..R10` | Choose the destination/working slot. |
| `:REFerence:LOAD "path"` | Load into that slot. Absolute paths refer to the server PC/scope; relative paths are beside setups. A file under `/proc`, `/sys` or `/dev` (where the path resolves to) and a file over 128 MiB are refused (-224) before it is read. |
| `:REFerence:IMPort ["path"]` | Load named file (as LOAD), or the selected `refN` file beside setups. |
| `:REFerence:SAVE` | Copy the selected acquired channel record into the selected reference slot. Does not write a waveform file. |
| `:REFerence:SHOW[?] [<slot>,]ON|OFF` | Visibility of one slot; DISPlay aliases SHOW. Query accepts a slot number. |
| `:REFerence:CURRent[?] 1..10` | Working slot, returned as a number; SLOT returns `Rn`. |
| `:REFerence:SOURce[?] [<slot>,]CHANn` | Acquired-channel source used when saving a reference. |
| `:REFerence:VSCale[?] [<slot>,]<V/div>` / `:VOFFset[?] [<slot>,]<V>` | Loaded-reference display scale/offset; stored samples are unchanged. |
| `:REFerence:LABel:CONTent[?] [<slot>,]<text>` | Reference display label. |
| `:REFerence:CLEar` | Clear all loaded reference slots. |
| `:SIM:PRESet[?] <name>` | Select a supported preset by its UI name. |
| `:SIM:SLOT[?] R1..R10` | Generated waveform destination. |
| `:SIM:PATTern[?] PRBS7` | Supported orders: 7, 9, 13, 15, 23, 31. |
| `:SIM:PLAY[?] ON|OFF` | Start/apply the configured simulation or stop/cancel it. |
| `:SIM:BUSY?` | Whether generation is pending. |
| `:SIM:SHOW[?] ON|OFF` | Visibility of the generated reference slot. |
| `:SIM:SSC:MODE[?]` | `OFF|DOWN|CENTER`. |
| `:SIM:SSC:DEViation[?]` | Fraction, 0 through 0.05. |
| `:SIM:SSC:FREQuency[?]` | Hz, 1e3 through 1e6. |
| `:SIM:JITTer:DCD[?]` | Peak-to-peak seconds, 0 through 100e-12. |
| `:SIM:JITTer:CORRelated[?]` | RMS seconds, 0 through 50e-12. |
| `:SIM:JITTer:CORNer[?]` | Hz, 1e3 through 100e6. |
| `:DECode:SELect[?]` | One-based decoder slot. |
| `:DECode:PROTocol[?] <name>` | Protocol UI name; comparison ignores case/punctuation, e.g. I2C, SPI, C-PHY. |
| `:DECode:STATe[?]` | Boolean. |
| `:DECode:INPut[?]` | `WAVEFORM|LA`; `ANALOG` aliases waveform. LA requires available/enabled logic inputs. |
| `:DECode:SOURce[?]` / `:SOURce2[?]` | Main data/clock roles; CHANn or Rn. |
| `:DECode:CS[?]` / `:SOURce4[?]` / `:DIFFerential[?]` | Additional roles; optional roles can use NONE. Roles depend on protocol. |
| `:DECode:THReshold[?]` | Configured threshold. For C-PHY this is minimum adjacent-wire level separation. |
| `:DECode:CPHY:RATE[?]` | `AUTO` or known symbols/second; aliases CPHY:SRATe and DECode:SRATe. |
| `:DECode:BUSY?` | Worker readiness, without changing acquisition. |
| `:DECode:EVENts?` | Physical-time event rows, `BUSY` while pending, or an error if unavailable. |

A file load is not an ADC acquisition. Multi-trace imports validate capacity before replacing slots; a CSV line with more data columns than there are reference slots (10) ends the import while the file is read. Protocol roles may mix references and acquired records; their native intervals/origins must overlap. Decoder events, Search and Bus qualification share the decoded timing.

```text
:REF:SLOT R1
:REF:LOAD "D:/Gen7_65GB_CP_Q6_n_Ch2.wfm"
:WAV:SOUR R1
:WAV:PRE?
```

The exact path is an example; it is not valid on a scope unless that file exists there. Load paths are not file uploads. For remote files use the UI/browser's explicit-slot upload workflow.

## Generator, logic and diagnostic extensions

Built-in generator commands use `:SOURce[<n>]:...` for a generator available on the model: `OUTPut[:STATe]`, `FUNCtion`, `FREQuency`, `VOLTage[:AMPLitude]`, `VOLTage:OFFSet`, `FUNCtion:SQUare:DUTY`, `FUNCtion:RAMP:SYMMetry`, `ARB:DATA`, `ARB:CAPTure`, `MOD[:STATe]`, `MOD:TYPe`, `MOD:AM:DEPTh`, `MOD:FM:DEViation`, `MOD:PM:DEViation`, and `MOD:<type>:INTernal:FREQuency|FUNCtion`. These configure the actual generator when hardware operation is enabled. Unsupported models report hardware missing.

Logic commands are `:LA:STATe[?]`, `:LA:DIGital<n>:DISPlay[?]`, `:LA:POD<n>:THReshold[?]`, `:LA:POINts?`, `:LA:PREamble?`, `:LA:DATA?`. DATA is an IEEE block of little-endian u16 words, bit n representing Dn. LA availability is model-dependent.

`:IDENtify?` returns the app's signal identification. Calibration commands `:CALibrate:STARt 0..4[,ZERO]`, `:CALibrate:STATus?`, `:CALibrate:CANCel` operate the self-calibration workflow; start requires the appropriate disconnected-input setup. They are not passive diagnostic reads.

`:SYSTem:STATe?` returns app diagnostic state. `:SYSTem:GET? <key>` and `:SYSTem:SET <key> = <value>` access the explicit setup schema, including Math configuration that has no dedicated stock-compatible command subtree here. Key names and internal indices are implementation-specific: see [setup serialization](../ui/src/store.rs), rather than assuming a Rigol syntax. `:SYSTem:LOG ON|OFF` controls the measurement logger.

The settings of `:SYSTem` that a setup file keeps, their commands and their keys. They are declared once, in
`ui/src/settings/system.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value a
command does not take is refused with `-224`. The graphics choice and the AUX OUT (below) are written by hand.

<!-- settings:system:begin -->
| Command | Takes | Setup key | Acquire menu | What it does |
| --- | --- | --- | --- | --- |
| `:SYSTem:PIPeline[?]` | `BATCh`, `FRAMes` or `STANdard`, any word that starts with `BATC`, `FRAM` or `STAN`, in any case; the query answers `BATC` or `FRAM` | `draw_path` | Draw path | The draw path of the Acquire menu: Batch, the default ([ADR 0029](adr/code/0029-batch-is-the-default-draw-path.md)), or Frames, the standard path; `STANdard` is an alias of `FRAMes`. Which path runs is the `pipeline` part of `:SYSTem:STATe?`. |
<!-- settings:system:end -->

| Command | Meaning |
| --- | --- |
| `:SYSTem:GRAPhics[?] CPU|GPU` | System > Graphics, the choice; `:SYSTem:STATe?` shows the GPU's times (`gpu layers ...`) only while it really draws. |
| `:SYSTem:AOUTput[?] TOUT|PFAil|OFF` | The rear AUX OUT: trigger out (the default), pass/fail where the board has the mask or the zone unit, or constant low (`OFF`, an OOS choice); all three verified on the unit (DHO924S, 2026-11-16, [auxout.md](fork/auxout.md)). The query answers `TOUT`, `PFA` or `OFF`; the interpreter refuses a choice the board does not offer with `-224`. |

The development commands below read the raw state of the scope's FPGA units next to what the software computes, for
sessions on a unit. Their answers are no stable interface. On a computer, without the units, they answer `off,...`,
`no hardware` or `AUTO` (seen on the host). How each unit's behaviour is known is on its page, and in short below.

| Command | Meaning |
| --- | --- |
| `:DEBug:SLOTs?` | Asks for one raw run of the measure unit and answers the previous request: `<request>`, then for each slot 0..7 `;<min>,<max>,<mean>` (min and max 12-bit, the mean on the 16-bit scale); `<request>;none` when the unit gave nothing, `pending` before the first answer, `no hardware` on a source without the unit. Ask twice, about 0.3 s apart. The slot map is assumed; on a DHO924S (2026-11-15) the unit answered with the levels of an earlier signal ([trigger-lockin.md](fork/trigger-lockin.md)). |
| `:SYSTem:DEBug:{UART|SERial}?` | The words of the trigger unit's UART receiver for the current settings; `off,enabled=0` without it. Its register facts are assumed, not yet verified on hardware ([hwuart.md](fork/hwuart.md)). |
| `:SYSTem:DEBug:ZONE?` | The zone unit's raw status and counters next to the software's zone filter and the frame counters; `off,...` without the unit. What the unit does with a frame is verified on the unit (DHO924S, 2026-11-16), its register facts are assumed ([hwzone.md](fork/hwzone.md)). |
| `:SYSTem:DEBug:MASK?` | The mask unit's raw counters and slot next to the software's mask test and the frame counters; `off,...` without the unit. Slot, frame count and verdicts verified on the unit (DHO924S, 2026-11-16), one time base still open ([hwmask.md](fork/hwmask.md)). |
| `:SYSTem:DEBug:MASK:MAP[?] <key>=<value>` | The comparator mapping of the mask test and the zone trigger, as `key=value` pairs separated by spaces (`domain`, `cpd`, `mid`, `shift`, `offs`, `invert`, `cols`, `range`); the setter changes one pair, the interpreter refuses a bad one with `-224`. The checks on the unit (DHO924S, 2026-11-16) used the default mapping ([hwmask.md](fork/hwmask.md)). |
| `:SYSTem:DEBug:MASK:COLumns? [<a>[,<b>]]` | For each column a..b (default 0..9) of the loaded mask table its limits, and the lowest and highest code of the newest record in that column, as the software computes them, not read from the unit; `off` without the mask unit, `no table` without a table. |
| `:SYSTem:DEBug:DVM?` | The DVM block's latest raw reading next to the software DVM; `off,sw=<V>` without one. Result layout and units verified on the unit (DHO804, 2026-11-13; fields and lanes again on a DHO924S, 2026-11-16), the latch assumed ([dvm.md](fork/dvm.md)). |
| `:SYSTem:DEBug:DVM:LANE[?] 0..3|AUTO` | Makes the DVM block read one lane (a pair of ADC cores) while one channel is acquired; `AUTO`, the default, leaves the choice to it. That every lane carries the channel is verified on the unit (DHO924S, 2026-11-16, [dvm.md](fork/dvm.md)). The interpreter refuses another value with `-224`. |

## Errors and compatibility limits

Read `:SYSTem:ERRor?` until `0,"No error"`. Each read removes one queued error. The queue holds at most 20 bounded messages; overflow is -350. Errors latch their event bit once; draining old errors does not relatch it. Unsupported or failed queries reply with specific error text as well as queuing it, even when full. Legitimate pending `*OPC?` keeps its deferred response. Typical errors include -113 undefined header, -224 invalid parameter, -230 unavailable analysis, -241 missing hardware, -256 missing file, and -350 queue overflow. Action queries such as `:RUN?`, `:STOP?`, `:SING?`, `*CLS?` and `*RST?` are unsupported and do not execute the action.

A Rigol script commonly needs adjustments when it assumes stock `*IDN?`, big-endian WORD assumptions, vendor-specific setup files, a full status/SRQ system, arbitrary trigger-specific subtrees, or references as physical ADC inputs. Check errors and actual readbacks after configuration. TekScope's `REMOTE:RUNStop`, `REMOTE:ACQMethod`, DATA/WFMOutpre/CURVe dialect is not implemented by these Rigol-style headers.

The canonical implementation is [scpi.rs](../ui/src/scpi.rs) and its folder [scpi/](../ui/src/scpi/), one file per subsystem, with transport details in [hislip.rs](../ui/src/hislip.rs) and [vxi11.rs](../ui/src/vxi11.rs). This manual records the supported subset rather than treating a matching vendor header as proof of equivalent behavior.

## Scalar identifiers

These canonical long spellings are accepted case-insensitively. Some additionally have aliases in the source.

### Standard measurement items

`VPP`, `VMAX`, `VMIN`, `VAVG`, `VRMS`, `VAMP`, `FREQUENCY`, `PERIOD`, `RTIME`, `FTIME`, `PWIDTH`, `PDUTY`, `NDUTY`, `NWIDTH`, `VTOP`, `VBASE`, `OVERSHOOT`, `PRESHOOT`, `HTIME`, `LTIME`, `BWIDTH`, `TOUTSIDE`, `NPERIODS`, `CAMP`, `CTOP`, `CBASE`, `CPP`, `CMAX`, `CMIN`, `VUPPER`, `VMID`, `VLOWER`, `CAVG`, `CRMS`, `ACRMS`, `SDEVIATION`, `AREA`, `CAREA`, `PPULSES`, `NPULSES`, `PEDGES`, `NEDGES`, `TVMAX`, `TVMIN`, `PSLEW`, `NSLEW`, `ESTUI`, `MEDIAN`, `MODE`, `LSLO`, `LINT`, `PAM0`, `PAM1`, `PAM2`, `PAM3`, `PSEP`, `BITS`, `PLLUI`, `UIWTD`, `FOLD`.

### Pair items

`RRDELAY`, `FFDELAY`, `RFDELAY`, `FRDELAY`, `RRPHASE`, `FFPHASE`, `RFPHASE`, `FRPHASE`, `RRSKEW`, `FFSKEW`, `RFSKEW`, `FRSKEW`, `SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD`.

### Eye figures

`ONE`, `ZERO`, `AMPLITUDE`, `CROSSING`, `RISE`, `FALL`, `HEIGHT`, `WIDTH`, `Q`, `HBER`, `RECORDS`, `EDGES`, `UI`, `SAMPLING`, `TPP`, `TRMS`, `DCD`, `DDJ`, `PJRMS`, `PRMS`, `C2C`, `F2`, `F4`, `F8`, `SRJ`, `PN`, `TPRE`, `TPOST`, `PPRE`, `PPOST`, `DCCM`, `ACCM`, `XOVER`, `TNT`, `SSCPPM`, `SSCRATE`, `TJ`, `RJ`, `DJ`, `OPENING`, `SEEN`, `TJBER`, `EWBER`, `J2`, `J9`, `NPJ`.



## Additional controller commands

These commands are implemented by the same model and acquisition router above. They extend the documented subset; their presence does not imply support for every sibling command in the Rigol guide. Reference tokens in Math, decoder roles and Track cursors are OOS extensions. The primary Rigol guide covers the corresponding command families in sections 3.4, 3.6, 3.8, 3.13, 3.15–3.17 and 3.26. Explicit unsupported states return an error or the existing invalid numeric sentinel rather than a fabricated measurement.

### Channels, acquisition and display

| Command | Parameters / behavior |
| --- | --- |
| `:CHANnel<n>:POSition[?]` | Vertical position in divisions, converted through that channel's current scale. |
| `:CHANnel<n>:TCALibrate[?]` | Channel deskew in seconds; native transfer metadata remains unchanged. |
| `:CHANnel<n>:BIAS[?]` | Software channel zero-bias correction. This is not a calibration procedure. |
| `:CHANnel<n>:LABel:SHOW[?]`, `:LABel:CONTent[?]` | Label visibility and text. |
| `:ACQuire:ULTRa:MAXFrame[?]` | Supported segmented frame count. This does not claim the stock UltraAcquire capture rate. |
| `:ACQuire:SEGMents[?] <n>|OFF` | Segmented recording (OOS): the segment count, without the menu's steps; the interpreter sets a larger one than 100000 to 100000, `OFF` or `0` switches it off, another value is refused with `-113`. The query answers the count, `0` when off. Where segments are recorded is `:ACQuire:SEGMents:HARDware` (acquisition settings above). |
| `:ACQuire:SEGMents:INTerval[?] <s>` | Hardware segments: the pause after each segment in seconds; the interpreter keeps it within 0 to 10. Not yet verified on hardware ([upstream-audit.md](fork/upstream-audit.md)). |
| `:ACQuire:SEGMents:TIMes?` | Hardware segments: the trigger time of each recorded segment since the first one, comma-separated seconds, `9.9E37` for a segment without a time stamp; empty before segments are recorded. Not yet verified on hardware ([upstream-audit.md](fork/upstream-audit.md)). |
| `:DISPlay:VIEW[?] <n>` | Display > Views (OOS): show view n, 1 through the view count; the query answers the view shown. |
| `:DISPlay:VIEW:COUNt[?] 1..4` | How many views there are. |
| `:DISPlay:VIEW:MONitor[?] <n>` | The view an HDMI monitor shows in the Layout, 0 the panel's own; another than 0 needs at least two views. A value past the view count is refused with `-224`. |
| `:TIMebase:DELay:ENABle[?]`, `:SCALe[?]`, `:OFFSet[?]` | Zoom enable, seconds/div and physical center time. |
| `:TIMebase:VERNier[?]` | Boolean fine horizontal control. |
| `:TIMebase:HREFerence:MODE[?]` | Horizontal expansion reference. |
| `:TIMebase:MODE[?]` | Main/zoom/XY mode supported by the model. |
| `:TIMebase:ROLL[?]` | Model roll setting; acquisition source restrictions still apply. |
| `:TIMebase:XY:ENABle[?]`, `:X[?]`, `:Y[?]`, `:Z[?]` | XY display and channel roles; `NONE` disables the optional Z input. |
| `:REFerence:COLor[?]`, `:RESet` | Selected/explicit slot color and reference display-scale reset. Recorded samples and native timing are retained. |

Display configuration is not proof of hardware acquisition capability. Force aliases route through the active context: stored/SIM acquisition receives a software force; live acquisition requests one hardware pulse. Local mock tests verify routing/counting, not a physical FPGA outcome.

### Display settings

The settings of the Display menu, their `:DISPlay` commands and their keys in a setup file. They are declared once,
in `ui/src/settings/display.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ.

<!-- settings:display:begin -->
| Command | Takes | Setup key | Display menu | What it does |
| --- | --- | --- | --- | --- |
| `:DISPlay:GRADing:TIME[?]` | `MIN`, `INFinite`, or seconds, set to the nearest time of the persistence ladder; the query answers `MIN`, the seconds or `INF` | `persist` | Persistence | How long a drawn waveform stays on the screen. |
| `:DISPlay:WBRightness[?]` | a whole number from 1 to 100, set within 10 to 100 % | `intensity` | Intensity | Brightness of the waveforms; 0 is refused, a value below 10 sets 10. |
| `:DISPlay:GRID[?]` | `FULL`, `HALF`, `NONE` | `grid` | Grid | The graticule: every division line, a lighter one, or none. |
| `:DISPlay:GBRightness[?]` | a whole number from 0 to 100, set within 10 to 100 % | `grid_bright` | Grid bright. | Brightness of the graticule; a value below 10 sets 10. |
| `:DISPlay:RULers[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `grid_frame` | Grid frame | A frame around the graticule with ticks every fifth of a division; none with the grid off. |
| `:DISPlay:COLor[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `color_grade` | Colour grade | Colours the persistence by how often a point was hit. |
| `:DISPlay:TYPE[?]` | `DOTS`, `VECTors`; the query answers `DOTS` or `VECT` | `dots` | Dots | Draws every sample as a dot instead of joining them with lines. |
| none | a choice of the screen's layout, not of the measurement | `bar` | Bottom bar | The bottom bar as cards or as one compact line. |
<!-- settings:display:end -->

### Measurement settings and statistics

| Command | Parameters / behavior |
| --- | --- |
| `:MEASure:STATistic:DISPlay[?]` | Statistics summary visibility. Toggling it retains accumulated observations. |
| `:MEASure:STATistic:ITEM <item>[,source[,source]]` | Add the specified standard/pair readout and show statistics. |
| `:MEASure:STATistic:ITEM? <field>,<item>[,source[,source]]` | `MAXimum|MINimum|CURRent|AVERages|DEViation|CNT` (`COUNt` alias). Current uses the configured row's own settings where applicable. |
| `:MEASure:STATistic:RESet` | Central statistics reset, including catalog/deferred accounting. It does not create an acquisition. |
| `:MEASure:SETup:MIN[?]`, `:MAX[?]` | Global default low/high thresholds in percent. Existing per-row overrides remain a separate UI feature. |
| `:MEASure:SETup:PSA[?]`, `:PSB[?]`, `:DSA[?]`, `:DSB[?]` | Phase/delay source A/B. CH/ref waveforms are supported; stock digital/Math measurement roles are not implied. |
| `:MEASure:BURSt:IDLE[?]` | Burst idle interval in seconds. |
| `:MEASure:OUTSide[?]` | `ABOVe|BELow|BOTH` outside-level measurement selection. |
| `:MEASure:NPERiods[?]` | Configured period count for relevant measurements. |
| `:MEASure:EDGes[?]` | `CLOCk|DATA` edge measurement mode. |
| `:MEASure:INDicator[?]` | Measurement labels; `:CURSor:MEASure:INDicator[?]` is an alias. |

Statistics count accepted observations. A stopped reference can have a valid current value with count0 after reset. Worker completion, polling and panning do not by themselves add observations. The existing SCREEN/NONE gate and per-row settings still determine each result; OPC does not wait for background analysis.

### Math and decoder slots

`<n>` selects an implemented Math/decoder slot; read back the configuration rather than assuming the model has the stock slot count. Math source names are `CHANn`, loaded `Rn`, and an **earlier** `MATHn`. Self/forward dependencies are rejected; an FFT/frequency-domain result cannot be used as a time-domain source. Empty references and unsupported formulas report an unavailable/conflict state.

| Command | Parameters / behavior |
| --- | --- |
| `:MATH<n>:DISPlay[?]`, `:OPERator[?]` | Enable/disable and select an implemented Math operator. |
| `:MATH<n>:SOURce1[?]`, `:SOURce2[?]` | CH/ref/earlier-Math identity, preserving native physical timing. |
| `:MATH<n>:SCALe[?]`, `:OFFSet[?]` | Math display scale and offset. FFT scale uses its dB ladder. |
| `:MATH<n>:EXPRession[?]` | Root formula parser, including loaded reference names such as `R1 - R2`. |
| `:MATH<n>:FFT:SOURce[?]`, `:SCALe[?]` | FFT input and dB display scale; the window is below. |
| `:MATH<n>:FILTer:TYPE[?]`, `:W1[?]`, `:W2[?]` | Supported low/high/band-pass/stop filter and valid corner frequencies. |
| `:BUS<n>:MODE[?]`, `:DISPlay[?]`, `:FORMat[?]` | Decoder protocol, visibility, `HEX|ASCii` formatting. |
| `:BUS<n>:EVENt[?]` | Shared decode event-table visibility. |
| `:BUS<n>:THReshold[?]` | Decoder threshold in source units. |
| `:BUS<n>:DATA?` | Ready event rows in an IEEE block. `BUSY` while required decoding is pending; unavailable sources return an error. |
| `:BUS<n>:RS232:TX[?]`, `:RX[?]` | CH/ref role identities. UART decoding uses the configured primary input. |
| `:BUS<n>:RS232:BAUD[?]`, `:PARity[?]`, `:DBITs[?]` | Supported baud ladder, parity and data-bit count. |
| `:BUS<n>:IIC:SCLK:SOURce[?]`, `:SDA:SOURce[?]` | I2C clock/data CH/ref roles. |
| `:BUS<n>:SPI:SCLK:SOURce[?]`, `:MOSI:SOURce[?]`, `:SS:SOURce[?]` | SPI CH/ref roles; `OFF`/`NONE` disables optional select. |
| `:BUS<n>:SPI:DBITs[?]` | Supported SPI word length. |

Decoder headers preserve the existing `DECode` source schema, including references. Configuring an inactive decoder does not select it as the acquisition trigger. For an active Bus trigger, the decoder's actual primary input owns qualification. Required inputs must overlap in physical time; no synthetic hardware/LA input is invented for a reference. Other protocol-specific Rigol BUS subtrees remain unsupported unless listed.

### Math settings

The settings of the selected math channel that are one value each, their `:MATH<n>` commands and their keys in a
setup file. They are declared once, in `ui/src/settings/math.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A command sets
math channel `<n>`, the setup key the selected one (the others are `m<N>.win`, `m<N>.fc`, `m<N>.bw`). A value a
command does not take is refused with `-224`.

<!-- settings:math:begin -->
| Command | Takes | Setup key | Math menu | What it does |
| --- | --- | --- | --- | --- |
| `:MATH<n>:FFT:WINDow[?]` | `RECTangle`, `HANNing`, `HAMMing`, `BLACkman`, `FLATtop` | `fft_win` | Window | The window the FFT multiplies the record with before the transform. |
| none | set through `:MATH<n>:FILTer:W1` and `:W2`, the corner frequencies (below) | `math_bw` | Bandwidth | The width of the band of the band-pass and band-stop filters. The menu steps 1-2-5 in periods. |
| none | set through `:MATH<n>:FILTer:W1` and `:W2`, the corner frequencies (below) | `math_fc` | Frequency | The cut-off frequency of the low- and high-pass filters, the centre of the band filters. The menu steps 1-2-5 in periods. |
<!-- settings:math:end -->

### Track cursors, mask and histogram

Track cursor B now has its own CH/ref source: `:CURSor:TRACk:SOURce1[?]` and `:SOURce2[?]`. An unset B follows A. Sampling uses each source's native dt/t0 plus deskew and the current detail view. A/B readback is display-only and does not arm acquisition or enable an ADC. `YDELta` is unavailable when the two source units differ. Manual cursors still have one common source; independent manual B is not a second acquisition channel. Setting both time and voltage positions promotes the UI to XY cursors instead of discarding the other axis.

Mask commands are `:MASK:ENABle[?]`, `:SOURce[?]`, `:X[?]`, `:Y[?]`, `:CREate`, `:RESet`, `:OPERate[?]`, `:FAILed?`, `:PASSed?`, `:TOTal?`. Mask sources are physical CH roles, not reference or Math inputs. A mask needs accepted channel data; enabling a page or querying a counter does not prove a valid mask or physical pass/fail result.

The switches of the mask test, their command and their keys in a setup file. They are declared once, in
`ui/src/settings/mask.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value the
command does not take is refused with `-224`. The source and the tolerances are written by hand.

<!-- settings:mask:begin -->
| Command | Takes | Setup key | Mask menu | What it does |
| --- | --- | --- | --- | --- |
| `:MASK:ENABle[?]` | `ON`, `OFF`, `1`, `0`; the query answers `1` or `0` | `mask_on` | Mask test | The pass/fail test of the source against the mask. A mask needs accepted channel data; enabling it does not prove a valid mask or a physical pass/fail result. |
| none | a choice of the Mask menu that no remote command sets | `mask_stop` | On fail | What a failed waveform does: None goes on, Stop stops the acquisition. |
| none | a choice of the Mask menu that no remote command sets | `mask_inv` | Invert | Turns the mask round: the band becomes the forbidden region, and a waveform passes where it stays outside it. |
<!-- settings:mask:end -->

Histogram commands are `:HISTogram:ENABle[?]`, `:TYPE[?]`, `:SOURce[?]`, `:HISTogram:STATistics:RESult?`. Supported sources are CH roles; histogram results describe the rendered display-density distribution and current view/zoom, not an exact full-native-record population. Empty results carry invalid sentinels where no figure exists. This is the same histogram shown in the UI.

The histogram's direction, its command and its key in a setup file. It is declared once, in
`ui/src/settings/histogram.rs`; this table is written from there
(`OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/`), and a test fails when the two differ. A value the
command does not take is refused with `-224`. `:HISTogram:ENABle` and the source are written by hand.

<!-- settings:histogram:begin -->
| Command | Takes | Setup key | Display menu | What it does |
| --- | --- | --- | --- | --- |
| `:HISTogram:TYPE[?]` | `HORizontal`, `VERTical`; the query answers `HOR` or `VERT` (`VERT` while it is off) | `hist` | Histogram | The waveform histogram of the Display menu: off, vertical or horizontal. |
<!-- settings:histogram:end -->

