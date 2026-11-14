# OOS Plus SCPI reference

OOS Plus implements a **subset of Rigol DHO800/DHO900 SCPI**, including commands used by DHO924S scripts, plus OOS analysis extensions. A script works when its commands, parameters and transfer assumptions are in this reference. OOS does not implement the complete stock firmware command set, impersonate Rigol, or speak TekScope's command dialect. `*IDN?` identifies OOS.

This reference describes the repository implementation. Primary Rigol syntax was checked against the [DHO800/DHO900 Programming Guide](https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf), including trigger MODE and waveform preamble definitions. Local backend tests establish the implemented behavior; they do not establish physical instrument or VISA-driver certification. See [local validation](local-validation.md).

## Connection and syntax

| Interface | Default | Implementation |
| --- | --- | --- |
| Raw TCP SCPI | TCP 5555 | Recommended for scripts. Newline-terminated program messages; newline-terminated replies. |
| HiSLIP | TCP 4880 | Same interpreter, with HiSLIP framing. Basic sessions; full locking/overlap semantics are not implemented. |
| VXI-11 | Linux scope service | Same interpreter, partial RPC transport. Pending operation reads have the limitation described below. Not advertised by the Windows app. |
| HTTP/MCP adapter | HTTP web port | `POST /mcp`, JSON-RPC `tools/call` with tool name `scpi`. This is not a VISA interface. `/cmd` uses a separate front-panel command language. |

The source launcher can use different ports. For example:

```powershell
.\run-windows.ps1 -Watch -Offline -WebPort 18080 -ScpiPort 5555 -HislipPort 14880
```

For that launcher, use `TCPIP0::127.0.0.1::5555::SOCKET`, or HiSLIP resource `TCPIP0::127.0.0.1::hislip0,14880::INSTR`. On another computer, substitute the PC's LAN address. These services are unauthenticated bench interfaces. The desktop app does not require or start an SSH server; [scope-image SSH](ssh.md) is separate.

Keywords are case-insensitive. The capitalized part in a mixed-case spelling is the short form: `:TRIG:EDGE:SOUR?` and `:TRIGger:EDGE:SOURce?` are equivalent. Use either the complete long form or its documented short form. Intermediate abbreviations are not generally accepted.

Use an absolute header for every semicolon-separated command. The parser splits literal semicolons; SCPI relative-header inheritance and quoted strings containing semicolons are unsupported. Prefer one query per message when replies themselves contain semicolon-separated rows.

Numerical setters normally accept finite decimal/scientific notation in base units: volts, seconds and hertz. Use `2e-1`, not `200mV`. Memory depth additionally accepts K/M suffixes. Boolean parameters use `ON|OFF|1|0`. A table's `[?]` means set and query; `?` alone means query only.

Waveform names are `CHAN1` through `CHAN4` as available on the model, and `R1` through `R10` (`REF1` through `REF10` are aliases). Use `CHAN1`, rather than the UI label `CH1`, in scripts. Reference sources are an OOS extension. An absent reference is not a physical input.

## Common commands and acquisition

| Command | Meaning |
| --- | --- |
| `*IDN?` | `OOS,<model>,<serial>,oos-ui <version>`. Desktop serial can be `0`. |
| `*RST` | Restore the app's default configuration. This changes the acquisition workspace. |
| `*CLS` | Clear the SCPI error/status state and pending operation-complete event registrations. Does not clear waveform memory. |
| `*OPC?` | Wait for the defined finite operations below, then return `1`. |
| `*OPC` | Register completion of those finite operations in standard-event bit 0. |
| `*ESR?` | Read and clear OPC bit 0. Other standard-event bits and a complete IEEE status/SRQ subsystem are not implemented. |
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
| `:CHANnel<n>:DISPlay[?]` | Boolean. |
| `:CHANnel<n>:SCALe[?]` | Volts/div; setter chooses the nearest supported scale. Probe factor applies. |
| `:CHANnel<n>:OFFSet[?]` | Volts. |
| `:CHANnel<n>:COUPling[?]` | `DC|AC|GND`; `DC50` only on a board supporting it. |
| `:CHANnel<n>:BWLimit[?]` | `OFF|20M|AUTO` or a supported filter name. OOS filtering extends stock choices. |
| `:CHANnel<n>:PROBe[?]` | Positive attenuation factor. |
| `:CHANnel<n>:INVert[?]` | Boolean. |
| `:TIMebase[:MAIN]:SCALe[?]` | Seconds/div; nearest supported timebase. |
| `:TIMebase[:MAIN]:OFFSet[?]` | Horizontal delay in seconds; `POSition` is an alias. |
| `:ACQuire:SRATe?` | ADC/model sample rate, not a loaded reference's native rate. |
| `:ACQuire:MDEPth[?]` | Set `AUTO` or a supported menu depth, e.g. `1000000`, `1M`. Query returns the numeric configured/effective acquisition depth. |
| `:TRIGger:MODE[?]` | Actual trigger type. Same OOS handler as `:TRIGger:TYPe[?]`. |
| `:TRIGger:SWEep[?]` | `AUTO|NORMal|SINGle`; independent of trigger type. |
| `:TRIGger:STATus?` | `STOP|TD|AUTO|WAIT`. A completed Single reports STOP. |
| `:TRIGger:EDGE:SOURce[?]` | `CHANn|Rn`; available LA inputs can use `D0` through `D15`. |
| `:TRIGger:EDGE:LEVel[?]` | Volts. |
| `:TRIGger:EDGE:SLOPe[?]` | `POS|NEG|RFAL`. |
| `:TRIGger:HOLDoff[?]` | Seconds, 0 through 10. |
| `:TRIGger:NREJect[?]` | Boolean. |
| `:TRIGger:RUNT:UPPer[?]` / `:LOWer[?]` | Absolute high/low thresholds; aliases `ALEVel` / `BLEVel`. |
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

## Waveform transfer

| Command | Meaning |
| --- | --- |
| `:WAVeform:SOURce[?]` | Select `CHANn|Rn` for transfer; does not select the acquisition trigger. |
| `:WAVeform:MODE[?]` | `NORMal|RAW`. `MAX` is unsupported and rejected, rather than silently changing to NORM. |
| `:WAVeform:FORMat[?]` | `BYTE|WORD|ASCii`. Invalid formats are rejected without changing the selection. |
| `:WAVeform:POINts?` | Actual points available for this source and mode. |
| `:WAVeform:PREamble?` | Ten comma-separated fields listed below. |
| `:WAVeform:DATA?` | IEEE definite-length binary block; ASCII format also uses a block. |
| `:DISPlay:DATA?` | IEEE block containing a 24-bit BMP screenshot, not a waveform. |

For CH inputs, NORM reads the interpolated display record; RAW uses retained native acquisition memory when available, including points outside the screen. If no native record exists, it falls back to the available processed record. References transfer their full native arrays in either mode. Do not assume `:ACQ:SRAT?` describes a reference: read its preamble. RAW is permitted while running in OOS; stock Rigol stop-only transfer assumptions differ. Multiple queries while running can observe different accepted captures; Stop first when metadata and data must match.

Preamble fields:

```text
format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference
```

`format` is 0 BYTE, 1 WORD, 2 ASCII. `type` is 0 NORM or 2 RAW. `count` is 1. Native times are seconds. Native transfer origins preserve the recorded metadata; analysis can additionally apply deskew corrections. Integer data converts as:

```text
time[k] = (k - xreference) * xincrement + xorigin
volts[k] = (code[k] - yorigin - yreference) * yincrement
```

BYTE uses unsigned 8-bit samples. WORD uses unsigned **little-endian** 16-bit samples. These are quantized to the reported scale and can clip if the source lies outside that scale. ASCII contains comma-separated volt values in seven-significant-digit scientific notation; do not apply integer scaling again. Neither transfer is a promise of bit-identical original vendor file bytes.

A block is `#<N><N decimal length digits><payload>`, followed by the response newline. Read the announced byte length; never use newline splitting inside a binary payload. `:WAV:STARt`/`:WAV:STOP` chunk selection is not implemented. Long transfers encode the whole response on the main-loop path; slow receivers can stall it within existing send timeouts. WORD generally uses less memory than ASCII. The interpreter's aggregate response guard is not a hard bound on one large waveform result.

Minimal standard-library Python reader for the running Windows app:

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
| `:MEASure:ITEM? <item>[,<source>]` | Standard scalar; default source CHAN1. Valid items are listed below. |
| `:MEASure:ITEM? <pair>,<A>,<B>` | Pair scalar, with native physical-time alignment. |
| `:MEASure:GATing[?]` | `SCREEN` follows the viewport; `NONE` means Full record. This gate is an OOS extension. |
| `:MEASure:REFS[?]` | `10/90|20/80` edge thresholds. |
| `:MEASure:TSETup[?]` / `:THOLd[?]` | Nonnegative seconds used by applicable setup/hold results. |
| `:MEASure:STRip?` | Configured standard/pair/catalog rows, then enabled DVM/counter rows. |
| `:MEASure:READout?` | Strip rows plus Eye figures when that analysis page is active. |
| `:EYE:ITEM? <figure>` | Current configured Eye result; does not itself run one-click Auto Eye. |
| `:VIEW[?]` | `SCOPE|SPECTRUM|EYE|POWER|BODE|MAGNETIC|INVERTER|DPT`. |

Values use base units, phase in degrees and duty/overshoot in percent. Unavailable or pending numeric values use `9.9E37`, not a real measurement. Poll pending analysis without counting it as another capture. `*OPC?` does not wait for these calculations. Long standard/reference analysis uses bounded workers; some one-off advanced fact queries remain synchronous and should not be polled at high frequency.

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

## Reference storage, SIM and decoding

| Command | Meaning |
| --- | --- |
| `:REFerence:SLOT[?] R1..R10` | Choose the destination/working slot. |
| `:REFerence:LOAD "path"` | Load into that slot. Absolute paths refer to the server PC/scope; relative paths are beside setups. |
| `:REFerence:IMPort ["path"]` | Load named file, or the selected `refN` file beside setups. |
| `:REFerence:SAVE` | Copy the selected acquired channel record into the selected reference slot. Does not write a waveform file. |
| `:REFerence:SHOW[?] ON|OFF` | Visibility of the selected slot. |
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

A file load is not an ADC acquisition. Multi-trace imports validate capacity before replacing slots. Protocol roles may mix references and acquired records; their native intervals/origins must overlap. Decoder events, Search and Bus qualification share the decoded timing.

```text
:REF:SLOT R1
:REF:LOAD "D:/Gen7_65GB_CP_Q6_n_Ch2.wfm"
:WAV:SOUR R1
:WAV:PRE?
```

The exact path is an example from this Windows checkout; it is not valid on a Linux scope unless that file exists there. Load paths are not file uploads. For remote files use the UI/browser's explicit-slot upload workflow.

## Generator, logic and diagnostic extensions

Built-in generator commands use `:SOURce[<n>]:...` for a generator available on the model: `OUTPut[:STATe]`, `FUNCtion`, `FREQuency`, `VOLTage[:AMPLitude]`, `VOLTage:OFFSet`, `FUNCtion:SQUare:DUTY`, `FUNCtion:RAMP:SYMMetry`, `ARB:DATA`, `ARB:CAPTure`, `MOD[:STATe]`, `MOD:TYPe`, `MOD:AM:DEPTh`, `MOD:FM:DEViation`, `MOD:PM:DEViation`, and `MOD:<type>:INTernal:FREQuency|FUNCtion`. These configure the actual generator when hardware operation is enabled. Unsupported models report hardware missing.

Logic commands are `:LA:STATe[?]`, `:LA:DIGital<n>:DISPlay[?]`, `:LA:POD<n>:THReshold[?]`, `:LA:POINts?`, `:LA:PREamble?`, `:LA:DATA?`. DATA is an IEEE block of little-endian u16 words, bit n representing Dn. LA availability is model-dependent.

`:IDENtify?` returns the app's signal identification. Calibration commands `:CALibrate:STARt 0..4[,ZERO]`, `:CALibrate:STATus?`, `:CALibrate:CANCel` operate the self-calibration workflow; start requires the appropriate disconnected-input setup. They are not passive diagnostic reads.

`:SYSTem:STATe?` returns app diagnostic state. `:SYSTem:GET? <key>` and `:SYSTem:SET <key> = <value>` access the explicit setup schema, including Math configuration that has no dedicated stock-compatible command subtree here. Key names and internal indices are implementation-specific: see [setup serialization](../ui/src/store.rs), rather than assuming a Rigol syntax. `:SYSTem:LOG ON|OFF` controls the measurement logger.

## Errors and compatibility limits

Read `:SYSTem:ERRor?` until `0,"No error"`. Each read removes one queued error. The queue is bounded. Typical errors include -113 undefined header, -224 invalid parameter, -230 unavailable analysis, -241 missing hardware, -256 missing file, and -350 queue overflow. Action queries such as `:RUN?`, `:STOP?`, `:SING?`, `*CLS?` and `*RST?` are unsupported and do not execute the action.

A Rigol script commonly needs adjustments when it assumes stock `*IDN?`, `WAV:MODE MAX`, START/STOP transfer chunks, vendor-specific setup files, a full status/SRQ system, arbitrary trigger-specific subtrees, or references as physical ADC inputs. Check errors and actual readbacks after configuration. TekScope's `REMOTE:RUNStop`, `REMOTE:ACQMethod`, DATA/WFMOutpre/CURVe dialect is not implemented by these Rigol-style headers.

The canonical implementation is [scpi.rs](../ui/src/scpi.rs), with transport details in [hislip.rs](../ui/src/hislip.rs) and [vxi11.rs](../ui/src/vxi11.rs). This manual records the supported subset rather than treating a matching vendor header as proof of equivalent behavior.

## Scalar identifiers

These canonical long spellings are accepted case-insensitively. Some additionally have aliases in the source.

### Standard measurement items

`VPP`, `VMAX`, `VMIN`, `VAVG`, `VRMS`, `VAMP`, `FREQUENCY`, `PERIOD`, `RTIME`, `FTIME`, `PWIDTH`, `PDUTY`, `NDUTY`, `NWIDTH`, `VTOP`, `VBASE`, `OVERSHOOT`, `PRESHOOT`, `HTIME`, `LTIME`, `BWIDTH`, `TOUTSIDE`, `NPERIODS`, `CAMP`, `CTOP`, `CBASE`, `CPP`, `CMAX`, `CMIN`, `VUPPER`, `VMID`, `VLOWER`, `CAVG`, `CRMS`, `ACRMS`, `SDEVIATION`, `AREA`, `CAREA`, `PPULSES`, `NPULSES`, `PEDGES`, `NEDGES`, `TVMAX`, `TVMIN`, `PSLEW`, `NSLEW`, `ESTUI`, `MEDIAN`, `MODE`, `LSLO`, `LINT`, `PAM0`, `PAM1`, `PAM2`, `PAM3`, `PSEP`, `BITS`, `PLLUI`, `UIWTD`, `FOLD`.

### Pair items

`RRDELAY`, `FFDELAY`, `RFDELAY`, `FRDELAY`, `RRPHASE`, `FFPHASE`, `RFPHASE`, `FRPHASE`, `RRSKEW`, `FFSKEW`, `RFSKEW`, `FRSKEW`, `SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD`.

### Eye figures

`ONE`, `ZERO`, `AMPLITUDE`, `CROSSING`, `RISE`, `FALL`, `HEIGHT`, `WIDTH`, `Q`, `HBER`, `RECORDS`, `EDGES`, `UI`, `SAMPLING`, `TPP`, `TRMS`, `DCD`, `DDJ`, `PJRMS`, `PRMS`, `C2C`, `F2`, `F4`, `F8`, `SRJ`, `PN`, `TPRE`, `TPOST`, `PPRE`, `PPOST`, `DCCM`, `ACCM`, `XOVER`, `TNT`, `SSCPPM`, `SSCRATE`, `TJ`, `RJ`, `DJ`, `OPENING`, `SEEN`, `TJBER`, `EWBER`, `J2`, `J9`, `NPJ`.

