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
| `:MEASure:ITEM? <item>[,<source>]` | Standard scalar; default source is the current Measure selection. Valid items are listed below. |
| `:MEASure:ITEM? <pair>,<A>,<B>` | Pair scalar, with native physical-time alignment; omitted roles use the Measure selections. |
| `:MEASure:ITEM <item>[,<A>[,<B>]]` | Add a standard/pair readout. Duplicates preserve statistics and Single. |
| `:MEASure:SOURce[?] <A>[,<B>]` | Default Measure source(s); query returns A. Sources are `CHANn|Rn`. |
| `:MEASure:CLEar` | Clear standard, pair and catalog selections through the shared statistics reset. |
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
| `:DVM:{ENABle|SOURce|MODE}[?]` | ON/OFF, acquired CHANn, and `DC|ACRMS|ACDC`; `:DVM:CURRent?` reads volts. |
| `:COUNter:{ENABle|SOURce|MODE|NDIGits}[?]` | ON/OFF, acquired CHANn, `FREQuency|PERiod|TOTalize`, digits 3..7. |
| `:COUNter:CURRent?` / `:COUNter:TOTalize:CLEar` | Current result / reset accumulated counter statistics. |

Cursor changes preserve capture statistics and an armed Single. Track values use the selected native/reference waveform and physical origin; positions remain viewport fractions. DVM/counter sources are acquired channels, not references; physical meter/counter parity is not established.

## Reference storage, SIM and decoding

| Command | Meaning |
| --- | --- |
| `:REFerence:SLOT[?] R1..R10` | Choose the destination/working slot. |
| `:REFerence:LOAD "path"` | Load into that slot. Absolute paths refer to the server PC/scope; relative paths are beside setups. |
| `:REFerence:IMPort ["path"]` | Load named file, or the selected `refN` file beside setups. |
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

Read `:SYSTem:ERRor?` until `0,"No error"`. Each read removes one queued error. The queue holds at most 20 bounded messages; overflow is -350. Errors latch their event bit once; draining old errors does not relatch it. Unsupported or failed queries reply with specific error text as well as queuing it, even when full. Legitimate pending `*OPC?` keeps its deferred response. Typical errors include -113 undefined header, -224 invalid parameter, -230 unavailable analysis, -241 missing hardware, -256 missing file, and -350 queue overflow. Action queries such as `:RUN?`, `:STOP?`, `:SING?`, `*CLS?` and `*RST?` are unsupported and do not execute the action.

A Rigol script commonly needs adjustments when it assumes stock `*IDN?`, big-endian WORD assumptions, vendor-specific setup files, a full status/SRQ system, arbitrary trigger-specific subtrees, or references as physical ADC inputs. Check errors and actual readbacks after configuration. TekScope's `REMOTE:RUNStop`, `REMOTE:ACQMethod`, DATA/WFMOutpre/CURVe dialect is not implemented by these Rigol-style headers.

The canonical implementation is [scpi.rs](../ui/src/scpi.rs), with transport details in [hislip.rs](../ui/src/hislip.rs) and [vxi11.rs](../ui/src/vxi11.rs). This manual records the supported subset rather than treating a matching vendor header as proof of equivalent behavior.

## Scalar identifiers

These canonical long spellings are accepted case-insensitively. Some additionally have aliases in the source.

### Standard measurement items

`VPP`, `VMAX`, `VMIN`, `VAVG`, `VRMS`, `VAMP`, `FREQUENCY`, `PERIOD`, `RTIME`, `FTIME`, `PWIDTH`, `PDUTY`, `NDUTY`, `NWIDTH`, `VTOP`, `VBASE`, `OVERSHOOT`, `PRESHOOT`, `HTIME`, `LTIME`, `BWIDTH`, `TOUTSIDE`, `NPERIODS`, `CAMP`, `CTOP`, `CBASE`, `CPP`, `CMAX`, `CMIN`, `VUPPER`, `VMID`, `VLOWER`, `CAVG`, `CRMS`, `ACRMS`, `SDEVIATION`, `AREA`, `CAREA`, `PPULSES`, `NPULSES`, `PEDGES`, `NEDGES`, `TVMAX`, `TVMIN`, `PSLEW`, `NSLEW`, `ESTUI`, `MEDIAN`, `MODE`, `LSLO`, `LINT`, `PAM0`, `PAM1`, `PAM2`, `PAM3`, `PSEP`, `BITS`, `PLLUI`, `UIWTD`, `FOLD`.

### Pair items

`RRDELAY`, `FFDELAY`, `RFDELAY`, `FRDELAY`, `RRPHASE`, `FFPHASE`, `RFPHASE`, `FRPHASE`, `RRSKEW`, `FFSKEW`, `RFSKEW`, `FRSKEW`, `SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD`, `DSETUP`, `DHOLD`, `DSETHOLD`.

Setup and hold take source A as the clock and B as the data. `SETUP`/`HOLD` latch on rising clock edges, `FSETUP`/`FHOLD` on falling ones, and the `D` items on both, as a DDR strobe (DQS) does. A clock edge counts only when the data changed inside its own cycle: after the previous latching edge and within one typical edge spacing. Data that holds steady between strobe bursts adds no setup or hold. `SETHOLD`/`DSETHOLD` count the edges that break `:MEAS:TSETup`/`:MEAS:THOLd`.

### Eye figures

`ONE`, `ZERO`, `AMPLITUDE`, `CROSSING`, `RISE`, `FALL`, `HEIGHT`, `WIDTH`, `Q`, `HBER`, `RECORDS`, `EDGES`, `UI`, `SAMPLING`, `TPP`, `TRMS`, `DCD`, `DDJ`, `PJRMS`, `PRMS`, `C2C`, `F2`, `F4`, `F8`, `SRJ`, `PN`, `TPRE`, `TPOST`, `PPRE`, `PPOST`, `DCCM`, `ACCM`, `XOVER`, `TNT`, `SSCPPM`, `SSCRATE`, `TJ`, `RJ`, `DJ`, `OPENING`, `SEEN`, `TJBER`, `EWBER`, `J2`, `J9`, `NPJ`.

With an Eye clock source (a strobe such as DQS, setup key `eye_more`), the data is folded on each strobe edge itself, so bursts that start at unrelated phases line up as the receiver latches them; idle time more than 1.5 UI from a strobe edge is left out. The jitter figures (TIE, TJ, ...) then describe the strobe against an ideal clock fitted to each burst separately, not one clock across the whole record. Example for DQ in R2 and DQS in R1, over the whole record:

```text
:SYST:SET eye = R2 0 1
:SYST:SET eye_more = 0.000000000001 1000 1000000 0 - 0 R1 1
:VIEW EYE
:EYE:ITEM? WIDTH
```

A full-record Eye of a long reference runs in the background. Until it finishes, the page and `:EYE:ITEM?` still show the previous result, so check that `EDGES` and `UI` belong to the new source before reading the figures.



## Additional controller commands (frozen PR41 revision3374f6b)

These commands are implemented by the same model and acquisition router above. They extend the documented subset; their presence does not imply support for every sibling command in the Rigol guide. Reference tokens in Math, decoder roles and Track cursors are OOS extensions. The primary Rigol guide covers the corresponding command families in sections3.4,3.6,3.8,3.13,3.15–3.17 and3.26. Explicit unsupported states return an error or the existing invalid numeric sentinel rather than a fabricated measurement.

### Channels, acquisition and display

| Command | Parameters / behavior |
| --- | --- |
| `:CHANnel<n>:POSition[?]` | Vertical position in divisions, converted through that channel's current scale. |
| `:CHANnel<n>:UNITs[?]` | Supported channel unit. Use the configured unit when interpreting cursor/readout values. |
| `:CHANnel<n>:VERNier[?]` | Boolean fine vertical control. |
| `:CHANnel<n>:TCALibrate[?]` | Channel deskew in seconds; native transfer metadata remains unchanged. |
| `:CHANnel<n>:BIAS[?]` | Software channel zero-bias correction. This is not a calibration procedure. |
| `:CHANnel<n>:EXPand[?]` | Vertical expansion reference supported by the model. |
| `:CHANnel<n>:LABel:SHOW[?]`, `:LABel:CONTent[?]` | Label visibility and text. |
| `:ACQuire:TYPE[?]` | `NORMal|PEAK|AVERages|HRESolution`. |
| `:ACQuire:AVERages[?]` | Requested average count, mapped to supported powers of two. Read back the selected count. |
| `:ACQuire:ULTRa:MAXFrame[?]` | Supported segmented frame count. This does not claim the stock UltraAcquire capture rate. |
| `:TIMebase:DELay:ENABle[?]`, `:SCALe[?]`, `:OFFSet[?]` | Zoom enable, seconds/div and physical center time. |
| `:TIMebase:VERNier[?]` | Boolean fine horizontal control. |
| `:TIMebase:HREFerence:MODE[?]` | Horizontal expansion reference. |
| `:TIMebase:MODE[?]` | Main/zoom/XY mode supported by the model. |
| `:TIMebase:ROLL[?]` | Model roll setting; acquisition source restrictions still apply. |
| `:TIMebase:XY:ENABle[?]`, `:X[?]`, `:Y[?]`, `:Z[?]` | XY display and channel roles; `NONE` disables the optional Z input. |
| `:DISPlay:TYPE[?]` | `DOTS|VECTors`. |
| `:DISPlay:GRADing:TIME[?]` | `MIN`, `INFinite`, or a duration mapped to the supported persistence ladder. |
| `:DISPlay:WBRightness[?]`, `:GBRightness[?]` | Waveform/grid brightness. |
| `:DISPlay:GRID[?]` | Supported full/half/no-grid display choice. |
| `:DISPlay:RULers[?]`, `:COLor[?]` | Rulers and color grading. |
| `:REFerence:COLor[?]`, `:RESet` | Selected/explicit slot color and reference display-scale reset. Recorded samples and native timing are retained. |

Display configuration is not proof of hardware acquisition capability. Force aliases route through the active context: stored/SIM acquisition receives a software force; live acquisition requests one hardware pulse. Local mock tests verify routing/counting, not a physical FPGA outcome.

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
| `:MATH<n>:FFT:SOURce[?]`, `:WINDow[?]`, `:SCALe[?]` | FFT input, supported window and dB display scale. |
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

### Track cursors, mask and histogram

Track cursor B now has its own CH/ref source: `:CURSor:TRACk:SOURce1[?]` and `:SOURce2[?]`. An unset B follows A. Sampling uses each source's native dt/t0 plus deskew and the current detail view. A/B readback is display-only and does not arm acquisition or enable an ADC. `YDELta` is unavailable when the two source units differ. Manual cursors still have one common source; independent manual B is not a second acquisition channel. Setting both time and voltage positions promotes the UI to XY cursors instead of discarding the other axis.

Mask commands are `:MASK:ENABle[?]`, `:SOURce[?]`, `:X[?]`, `:Y[?]`, `:CREate`, `:RESet`, `:OPERate[?]`, `:FAILed?`, `:PASSed?`, `:TOTal?`. Mask sources are physical CH roles, not reference or Math inputs. A mask needs accepted channel data; enabling a page or querying a counter does not prove a valid mask or physical pass/fail result.

Histogram commands are `:HISTogram:ENABle[?]`, `:TYPE[?]`, `:SOURce[?]`, `:STATistics:RESult?`. Supported sources are CH roles; histogram results describe the rendered display-density distribution and current view/zoom, not an exact full-native-record population. Empty results carry invalid sentinels where no figure exists. This is the same histogram shown in the UI.


## Menu coverage extensions

These headers make the menu options that had no SCPI command reachable. Each one changes the same `Model` field, or runs the same `Ui` function, as the menu item named in its row, so the screen follows and the item shows the new value. They are plain top-level subtrees like `:SIM:` and `:BODE:` (no `:OOS:` prefix). Where Rigol has a header of the same job its name is used; the names marked "verify" were taken from memory of the DHO800/900 Programming Guide and are not checked against it.

Conventions of this section: `[?]` is set and query, `?` is query only, no mark is an action. A source is `CHANn`, `Rn` or (where the menu allows) `MATHn`. A choice item takes the long or short keyword of its option text (`CHANnel2` for CH2; case, spaces and dashes of the option text do not matter) and answers the option text in capitals, spaces as `_`. On/Off items answer `1` and `0`. Errors: -224 illegal parameter value, -221 settings conflict (nothing to do it with, or the command needs `CONFIRM`), -113 undefined header.

**Destructive commands** (power off, restart, update, roll back, boot mode, overclock, back to stock, and switching the SCPI service off) take an explicit `CONFIRM` argument and are refused with -221 without it. A command that would not change anything (the boot mode it already has) needs no confirmation.

### Trigger types

The typed subtrees set the fields the menu pages edit (the same fields as `:SYSTem:SET trig_*`); they do not change the trigger type, `:TRIGger:MODE` does. Times are seconds, 1 ns to 10 s. The pulse-width conditions, the slope and delay times, the window time, the runt width and the duration times share two fields: "lower" (`WIDTh`, `LWIDth`, `TIME`, `TLOWer`, `WLOWer`) is the menu's Time, "upper" (`UWIDth`, `TUPPer`, `WUPPer`) its Upper limit. Source and level of every type are `:TRIGger:EDGE:SOURce`, `:SOURce2` and `:LEVel` (also `:TRIGger:<type>:SOURce`), plus the aliases below.

| Header | Values | Menu item |
| --- | --- | --- |
| `:TRIGger:PULSe:POLarity[?]` | `POSitive`, `NEGative` | Polarity |
| `:TRIGger:PULSe:WHEN[?]` | `GREater`, `LESS`, `RANGe`, `OUTside`; or `PGReater`, `PLESs`, `PGLess`, `NGReater`, `NLESs`, `NGLess` (polarity and condition together) | When |
| `:TRIGger:PULSe:WIDTh[?]`, `:TRIGger:PULSe:LWIDth[?]`, `:TRIGger:PULSe:UWIDth[?]` | seconds | Time, Upper limit |
| `:TRIGger:PULSe:LEVel[?]` | volts | Level |
| `:TRIGger:SLOPe:SLOPe[?]` | `POSitive`, `NEGative`, `RFALl` | Slope |
| `:TRIGger:SLOPe:WHEN[?]` | `GREater`, `LESS`, `RANGe`, `OUTside` | When |
| `:TRIGger:SLOPe:TIME[?]`, `:TRIGger:SLOPe:TLOWer[?]`, `:TRIGger:SLOPe:TUPPer[?]` | seconds | Time, Upper limit |
| `:TRIGger:SLOPe:ALEVel[?]`, `:TRIGger:SLOPe:BLEVel[?]` | volts | Level, Level 2 |
| `:TRIGger:RUNT:POLarity[?]` | `POSitive`, `NEGative` | Polarity |
| `:TRIGger:RUNT:WHEN[?]` | `NONE`, `GREater`, `LESS`, `RANGe`, `OUTside` | When |
| `:TRIGger:RUNT:WLOWer[?]`, `:TRIGger:RUNT:WUPPer[?]` | seconds | Time, Upper limit |
| `:TRIGger:VIDeo:STANdard[?]` | `PAL`, `NTSC` | Standard |
| `:TRIGger:VIDeo:MODE[?]` | `ALINes`, `LINE`, `ODDField`, `EVENfield`, `AFIelds` | Sync |
| `:TRIGger:VIDeo:LINE[?]` | line number, 1 to (lines / 2 - 10) of the standard | Line |
| `:TRIGger:VIDeo:LEVel[?]` | volts | Level |
| `:TRIGger:PATTern:PATTern[?]` | `"HLXRF..."`: one character per channel (H, L, X, R rising, F falling), optionally followed by the 16 logic lines D0..D15 (H, L, X) | CH1..CH4, LA pattern |
| `:TRIGger:PATTern:LEVel[?]` | `<ch>,<volts>`; the query takes `<ch>` (`1` or `CHAN1`) | Level (per channel) |
| `:TRIGger:DURation:PATTern[?]`, `:TRIGger:DURation:LEVel[?]` | as the pattern trigger (R and F count as X) | the same fields |
| `:TRIGger:DURation:WHEN[?]`, `:TRIGger:DURation:TLOWer[?]`, `:TRIGger:DURation:TUPPer[?]` | as the pulse trigger | When, Time, Upper limit |
| `:TRIGger:WINDow:SLOPe[?]` | `POSitive`, `NEGative`, `RFALl` | Slope |
| `:TRIGger:WINDow:POSition[?]` | `ENTer`, `EXIT`, `TIMe` (also `INSide`) | Trigger on |
| `:TRIGger:WINDow:ALEVel[?]`, `:TRIGger:WINDow:BLEVel[?]` | volts | Level, Level 2 |
| `:TRIGger:WINDow:WHEN[?]`, `:TRIGger:WINDow:TIME[?]` | as the pulse trigger, for `TIMe` | When, Time |
| `:TRIGger:NEDGe:SLOPe[?]` | `POSitive`, `NEGative`, `RFALl` | Slope |
| `:TRIGger:NEDGe:IDLE[?]` | seconds | Idle time |
| `:TRIGger:NEDGe:EDGE[?]` | 1 to 65535 | Edge no. |
| `:TRIGger:NEDGe:LEVel[?]` | volts | Level |
| `:TRIGger:SHOLd:STIMe[?]`, `:TRIGger:SHOLd:HTIMe[?]` | seconds | Setup, Hold |
| `:TRIGger:SHOLd:DSOurce[?]`, `:TRIGger:SHOLd:CSOurce[?]` | data and clock source | Source, Source B (names verify) |
| `:TRIGger:SHOLd:SLOPe[?]` | clock edge | Clock edge (Slope B) |
| `:TRIGger:TIMeout:SLOPe[?]`, `:TRIGger:TIMeout:TIME[?]`, `:TRIGger:TIMeout:LEVel[?]` | slope, seconds, volts | Slope, Timeout, Level |
| `:TRIGger:DELay:SLOPA[?]`, `:TRIGger:DELay:SLOPB[?]` | slopes of A and B | Slope A, Slope B |
| `:TRIGger:DELay:WHEN[?]`, `:TRIGger:DELay:TLOWer[?]`, `:TRIGger:DELay:TUPPer[?]` | as the pulse trigger | When, Time, Upper limit |
| `:TRIGger:DELay:ALEVel[?]`, `:TRIGger:DELay:BLEVel[?]` | volts | Level A, Level B |
| `:TRIGger:BUS:WHEN[?]` | `FRAMe`, `ID`, `DATA`, `ERRor` | Match |
| `:TRIGger:BUS:VALue[?]` | value, decimal or `0x` hexadecimal; the query answers `0x..` | Value |
| `:TRIGger:LEVel[?]`, `:TRIGger:LEVel2[?]` | volts; the first and the second level whatever the type | Level, Level 2 |
| `:TRIGger:LEVel:AUTO` | action: level to the middle of the source (the menu's Level 50 %, `Item::Trig50`) | Level 50 % |

### Acquisition history and search

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:ACQuire:HISTory:ENABle[?]` | `ON` shows the newest stored record (stops the acquisition), `OFF` goes back to live (Run). -221 when nothing is stored. | History / Live |
| `:ACQuire:HISTory:FRAMe[?]` | record `1`..count; `0` when not in history | History, Back, Forward |
| `:ACQuire:HISTory:COUNt?` | number of stored records | |
| `:ACQuire:HISTory:NEXT`, `:ACQuire:HISTory:PREVious` | one record on / back, like the Display menu's Forward and Back | Forward, Back |
| `:SEARch:STATe[?]` | mark the events of the trigger condition | Mark events |
| `:SEARch:MODE[?]` | `EDGE` (the only kind) | |
| `:SEARch:EDGE:SOURce[?]` | search source; without a search of its own the trigger's. The first set of a source, level or slope adds Search 1 like the Add rail does | Search source |
| `:SEARch:EDGE:SLOPe[?]` | `POSitive`, `NEGative`, `RFALl` | Search slope |
| `:SEARch:EDGE:LEVel[?]` | volts | Search level |
| `:SEARch:EDGE:LEVel:AUTO` | action: level to the middle of the source | Level 50 % |
| `:SEARch:COPY:TO TRIGger`, `:SEARch:COPY:FROM TRIGger` | copy the search to the trigger / the trigger to the search | To trigger, From trigger |
| `:SEARch:COUNt?` | number of events in the record shown | |
| `:SEARch:EVENt[?]` | go to event `1`..count (the view moves there); the query answers the current one | Event |
| `:SEARch:DATA?` | rows `time,source #n,type,ok`, one per line, like `:DECode:EVENts?` | Event list |
| `:SEARch:SAVE` | save the events as CSV (`events-*.csv` in the save folder) | Save events |
| `:SEARch:REMove` | remove Search 1 | Remove |

### Measurements, channels, math, deskew

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:MEASure:COPY <source>` | copy the measurements of Source A to another source | Copy to |
| `:MEASure:ITEM:REMove <n>` or `<item>[,<src>[,<srcB>]]` | remove one row, by its number in `:MEASure:STRip?` order, or by item and source | Remove |
| `:CHANnel<n>:BIAS:ZERO` | action: the mean of the last record becomes part of the zero bias | Zero this input |
| `:CALibrate:DESKew:AUTO` | action: line up the shown traces near the view | Auto deskew |
| `:CALibrate:DESKew:CLEar` | action: clear the analysis deskew | Clear deskew |
| `:CALibrate:DESKew[?] <wave>,<seconds>` | analysis deskew of a channel or reference (the query takes `<wave>`); this is not the probe deskew `:CHANnel<n>:TCALibrate` | schema keys `deskew_ch<n>`, `deskew_ref<n>` |
| `:CALibrate:STARt <0..4>[,ZERO][,ADC]` | (extended) `ADC` also calibrates the ADC cores | Self-calibration ADC cores |
| `:MATH<n>:AUTO` | action: fit scale and offset to the result | Auto scale |
| `:MATH<n>:LINear:A[?]`, `:MATH<n>:LINear:B[?]` | linear `A x + B` of the channel | Linear A, B |
| `:EYE:AUTO`, `:EYE:CLEar` | Auto Eye/Jitter (measure the bit rate and open the eye) / clear the eye | Auto Eye/Jitter, Clear eye |
| `:SPECtrum:PEAK:CENTer` | action: centre the spectrum on its strongest line | Centre on peak |
| `:SPECtrum:LIMit:SET`, `:SPECtrum:LIMit:CLEar` | limit line from the trace / clear | Limit line |
| `:SPECtrum:LIMit:MARGin[?]` | dB, 0 to 60 | Limit margin |

### Analyser setup

The analyser pages were only reachable through packed `:SYSTem:SET` keys. Sources take `CHANn`/`Rn`; the "optional" sources also take `OFF`. A change restarts the averaging and the waterfall like the menu does.

| Header | Values | Menu item |
| --- | --- | --- |
| `:SPECtrum:SOURce[?]` | source | Source |
| `:SPECtrum:CENTer[?]`, `:SPECtrum:SPAN[?]`, `:SPECtrum:RBW[?]` | Hz; span 0 is full / automatic | Centre, Span, RBW |
| `:SPECtrum:WINDow[?]`, `:SPECtrum:TRACe[?]` | option text of the Window and Trace rows | Window, Trace |
| `:SPECtrum:RLEVel[?]` | dBV | Ref level |
| `:SPECtrum:WATerfall[?]`, `:SPECtrum:DISTortion[?]` | on/off | Waterfall, THD / SNR |
| `:SPECtrum:PEAK:HARMonics[?]` | option text of the Harmonics row | Peak list |
| `:SPECtrum:ACPR:WIDTh[?]`, `:SPECtrum:ACPR:SPACing[?]` | Hz | Ch width, Ch space |
| `:EYE:SOURce[?]`, `:EYE:CLOCk[?]`, `:EYE:RAIL[?]` | source; clock and rail also `OFF` | Source, Clock, Rail |
| `:EYE:RATE[?]` | bit rate, Hz (switches "from record" off) | Bit rate |
| `:EYE:BER[?]` | target bit error ratio | BER |
| `:EYE:PN:LOW[?]`, `:EYE:PN:HIGH[?]`, `:EYE:NOTCh[?]`, `:EYE:PLL:BWIDth[?]` | Hz | PN band, notch, PLL bandwidth |
| `:EYE:NPJ[?]`, `:EYE:HPASs[?]` | on/off | NPJ, High-pass |
| `:EYE:PLACe[?]` | `FULL`, `SIDE`, `UNDER` | Place |
| `:POWer:VOLTage:SOURce[?]`, `:POWer:CURRent:SOURce[?]`, `:POWer:VIN:SOURce[?]`, `:POWer:CLOCk:SOURce[?]` | source; VIN and clock also `OFF` | Voltage, Current, Input, Clock |
| `:POWer:PAGE[?]` | option text of the Page row | Page |
| `:POWer:FLINe[?]`, `:POWer:FSWitch[?]`, `:POWer:REFerence[?]` | Hz, Hz, volts (0 = none) | Line f, Switch f, Reference |
| `:MAGnetic:VOLTage:SOURce[?]`, `:MAGnetic:CURRent:SOURce[?]` | source | Voltage, Current |
| `:MAGnetic:TURNs[?]`, `:MAGnetic:AREA[?]`, `:MAGnetic:LENGth[?]`, `:MAGnetic:RCU[?]` | turns; m^2; m; ohms | Turns, Ae, Le, Rcu |
| `:MAGnetic:PLOT[?]`, `:MAGnetic:CURSor[?]` | `I-PSI` or `B-H`; loop point | Plot, Cursor |
| `:INVerter:PAGE[?]` | `DQ0`, `MECHANICAL` | Page |
| `:INVerter:PHASe1:SOURce[?]`, `:INVerter:PHASe2:SOURce[?]`, `:INVerter:PHASe3:SOURce[?]` | source | Phase A, B, C |
| `:INVerter:ANGLe:SOURce[?]` | `OFF`, `PHASE`, `ANALOG`, `QUAD`, `HALL` | Angle source |
| `:INVerter:POLEs[?]` | 1 to 32 | Poles |
| `:INVerter:ANALog:SOURce[?]`, `:INVerter:ANALog:SCALe[?]`, `:INVerter:ANALog:DEGRees[?]` | source; scale; degrees per volt | Analog |
| `:INVerter:ANGLe:OFFSet[?]` | degrees, +-3600 | Offset |
| `:INVerter:QUANtity[?]` | `VOLTAGE`, `CURRENT` | Quantity |
| `:INVerter:TORQue:SOURce[?]`, `:INVerter:BUS:VOLTage:SOURce[?]`, `:INVerter:BUS:CURRent:SOURce[?]` | source or `OFF` | Torque, Bus V, Bus I |
| `:INVerter:ELECtrical[?]` | `MECH`, `ELEC` | Mech / Elec |
| `:INVerter:ENCoder:CPR[?]`, `:INVerter:ENCoder:SWAP[?]`, `:INVerter:ENCoder:A[?]`, `:INVerter:ENCoder:B[?]` | counts per revolution; on/off; LA line 0..15 | Encoder |
| `:INVerter:HALL1:LINE[?]`, `:INVerter:HALL2:LINE[?]`, `:INVerter:HALL3:LINE[?]` | LA line 0..15 | Hall lines |
| `:DPT:VDS:SOURce[?]`, `:DPT:ID:SOURce[?]`, `:DPT:VGS:SOURce[?]` | source | Vds, Id, Vgs |
| `:DPT:FOURth[?]`, `:DPT:FOURth:SOURce[?]` | `OFF`, `GATE`, `DIODE_V`, `DIODE_I`; source | Fourth channel |
| `:DPT:GATE[?]`, `:DPT:ZOOM[?]` | percent 1 to 49; `BOTH`, `TURN-ON`, `TURN-OFF` | Gate, Zoom |
| `:BODE:STARt[?]`, `:BODE:STOP[?]`, `:BODE:POINts[?]`, `:BODE:AMPLitude[?]` | Hz, Hz, 5 to 200, volts | Start, Stop, Points, Amplitude |
| `:BODE:INPut[?]`, `:BODE:OUTPut[?]` | source (the input is `OFF`able on the free sweep) | Input, Output |
| `:BODE:SWEep:TIME[?]`, `:BODE:SWEep:LOG[?]` | seconds; `LIN`, `LOG` | Free sweep: time, log |
| `:BODE:SYNC:SOURce[?]`, `:BODE:SYNC:EDGE[?]`, `:BODE:SYNC:LEVel[?]` | `CH1`..; `RISING`, `FALLING`; volts | Sync |
| `:BODE:REFLevel[?]` | dBV | Ref level |
| `:BODE:RUN[?]` | start / stop the sweep; the query answers whether it runs | Start / Stop |
| `:BANDwidth:STATe[?]` | the bandwidth test view (also `:VIEW BWTest`) | View |
| `:BANDwidth:SOURce[?]`, `:BANDwidth:FREQuency[?]`, `:BANDwidth:BINS[?]` | channel; Hz (0 = auto; the nearest hint of the menu); bins per period (nearest step) | Source, Signal freq, Bins / period |

### Reference, mask, SIM

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:REFerence:EXPort ["path"]` | write the selected slot as CSV (`Time (s),Volts`); without a path to `ref<n>.csv` beside the setups | Export |
| `:REFerence:CLEar [slot]` | without a slot all references, with `Rn` (or `n`) one | Clear all, Clear slot |
| `:REFerence:LOOK[?]` | `LIVE`, `TAKEover`, `OFFLine` (offline waveforms) | Offline / reference look |
| `:REFerence:HORizontal:MODE[?]` | `FOLLow` (main time base) or `OWN` for the selected slot (Studio) | Follow / Own |
| `:REFerence:HORizontal:SCALe[?]`, `:REFerence:HORizontal:OFFSet[?]` | own seconds per division, position | time/div, position |
| `:REFerence:HORizontal:FIT` | action: the whole record fills the lane; -221 outside Studio's stacked lanes | Fit |
| `:MASK:CREate:REF <Rn>` | mask from a reference | Create from ref |
| `:MASK:CLEar` | clear the mask | Clear mask |
| `:MASK:INVert[?]`, `:MASK:OUTPut[?]` | on/off; `STOP`, `CONTinue` (on fail) | Invert, On fail |
| `:MASK:SAVE ["path"]`, `:MASK:LOAD ["path"]` | mask file; without a path the slot's `mask<n>.txt` and the save folder | Save mask, Load mask |
| `:SIM:DEFault` | the SIM knobs back to the preset | Defaults |
| `:SIM:ADDRef` | add the generated signal to the SIM slot's reference | Add to R |
| `:SIM:LOAD "path"` | load a waveform file into the SIM slot | Load file |
| `:SIM:LIVE` | leave SIM mode | Live |
| `:SIM:RATE[?]`, `:SIM:LEVel:HIGH[?]`, `:SIM:LEVel:LOW[?]`, `:SIM:RISE[?]`, `:SIM:BWLimit[?]`, `:SIM:DEEMphasis[?]`, `:SIM:NOISe[?]` | b/s (1e8 to 6.4e10); volts +-2; 20-80 % rise (5 ps to 2 ns); Hz; dB 0 to 12; volts RMS | Rate, High, Low, Rise, Bandwidth, De-emph, Noise |
| `:SIM:JITTer:RANDom[?]`, `:SIM:JITTer:PERiodic:AMPLitude[?]`, `:SIM:JITTer:PERiodic:FREQuency[?]` | seconds RMS; seconds peak; Hz | RJ RMS, PJ peak, PJ freq |

Like the older `:SIM:` settings these apply on the next `:SIM:PLAY ON`.

### Storage and the whole setup

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:SAVE:DESTination[?]` | `AUTO`, `USB`, `DESKtop` | Save destination |
| `:SAVE:SLOT[?]` | setup slot 1 to 9 | Setup slot |
| `*SAV <n>`, `*RCL <n>` | save / recall setup slot `n` (the file `setup<n>.txt`) | Save setup, Recall setup |
| `:SAVE:SETup ["path"]`, `:LOAD:SETup ["path"]` | the current slot, or a file | Save setup, Recall setup |
| `:SAVE:WAVeform ["path"[,CSV or BIN]]` | newest record to a file (default CSV, in the save folder) | Save waveform |
| `:SAVE:IMAGe ["path"]` | screenshot as PNG, written after the next frame | Save screenshot |
| `:SAVE:FFRame`, `:LOAD:FFRame` | save / load the fast-frame history (`frame-NNN.bin` in the save folder) | Save fast-frame, Load fast-frame |
| `:SYSTem:SETup?` | the whole setup (the file Save setup writes) as an IEEE block | |
| `:SYSTem:SETup <block>` | apply a setup block (or the plain text); the acquisition state stays as it is | Recall setup |

`:SYSTem:SETup` is spelled in full: the short form `:SYST:SET` stays the older key = value command. A relative path of a save is taken in the save folder. The text of a setup contains newlines and semicolons: the interpreter does not split a definite-length block `#<n><length><data>` that follows a blank, and TCP framing waits for the whole block, so one message can carry it. The setup text is also what the key table below describes.

### Studio zoom

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:TIMebase:DELay:SOURce[?]` | the lane that zooms: `ALL`, `CHANn`, `Rn`, `MATHn`; turns the zoom on | Zoom source |
| `:TIMebase:DELay:VZOom[?]` | vertical zoom factor 1 to 1024, the nearest power of two | Y zoom +/- |
| `:TIMebase:DELay:VZOom:RESet` | vertical zoom back to 1 | Reset Y |
| `:TIMebase:DELay:FIT` | action: the zoom window back to the whole screen | Fit record |
| `:TIMebase:DELay:CENTer` | action: the trigger in the middle of the zoom window | Center trigger |

### Data logger

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:SYSTem:LOG[?]` | log on/off (query added; the older set form is the same) | Logging |
| `:SYSTem:LOG:INTerval[?]` | seconds, the nearest of 1, 2, 5, 10, 30, 60, 300, 600, 1800, 3600 | Interval |
| `:SYSTem:LOG:FILE?` | path of the CSV file while logging, else empty | Log file |
| `:SYSTem:LOG:INFO?` | the info row, for example `log-001.csv: 12 rows` | Log info |

### System

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:SYSTem:DATE[?]`, `:SYSTem:TIME[?]` | `yyyy,mm,dd` / `hh,mm,ss` in the time zone of the settings; setting one keeps the other part of the clock | Set the clock |
| `:SYSTem:TZONe[?]` | the time zone, a POSIX-style string (`CET-1CEST,M3.5.0,M10.5.0/3`) | Time zone |
| `:SYSTem:NTP:ENABle[?]` | NTP on/off | Time from NTP |
| `:SYSTem:NTP:SERVer[?]` | server name or address, or `DHCP` | NTP server |
| `:SYSTem:NTP:STATus?` | the NTP state text | NTP state |
| `:SYSTem:NTP:SYNC` | action: sync now | Sync now |
| `:SYSTem:COMMunicate:LAN:IPADdress?`, `:SYSTem:COMMunicate:LAN:MAC?`, `:SYSTem:COMMunicate:LAN:STATus?` | address, MAC, link text | Ethernet |
| `:SYSTem:COMMunicate:LAN:RETRy10[?]` | 10 Mbit retry on/off | 10 Mbit retry |
| `:SYSTem:WLAN:STATus?`, `:SYSTem:WLAN:SCAN`, `:SYSTem:WLAN:NETWorks?`, `:SYSTem:WLAN:SAVed?`, `:SYSTem:WLAN:FORGet <n>` | WLAN state; scan; the scanned networks and the saved ones (one per line); forget saved network `n`. Joining a network needs a password and stays on the screen | WLAN |
| `:SYSTem:REMote:SERVice:SCPI[?]`, `:SYSTem:REMote:SERVice:VXI11[?]`, `:SYSTem:REMote:SERVice:TELNet[?]`, `:SYSTem:REMote:SERVice:WEB[?]`, `:SYSTem:REMote:SERVice:MDNS[?]` | the switches of the Remote services page. `SCPI OFF` needs `,CONFIRM` | Remote services |
| `:SYSTem:NAME[?]` | the scope's name on the network (letters, digits and `-`, up to 63) | Name |
| `:DISPlay:BACKlight[?]` | percent, 10 to 100 | Backlight |
| `:DISPlay:HDMI[?]`, `:DISPlay:TEXTsize[?]`, `:DISPlay:LAYout[?]` | option text of the rows | HDMI, Text size, Layout |
| `:SYSTem:LOCK[?]` | touch lock on/off | Touch |
| `:SESSion[?]`, `:SESSion LIVE` | `LIVE`, `SIM` or `REF`; `LIVE` leaves SIM and offline sessions | Live |
| `:SYSTem:INFO?` | the System info page as text: `label,value` lines, `# heading` lines | System info |
| `:SYSTem:POWer:OFF CONFIRM`, `:SYSTem:POWer:RESTart CONFIRM` | power off / restart (refused while an update runs) | Power dialog |
| `:SYSTem:UPDate:STATus?` | the Update page as text | Update |
| `:SYSTem:UPDate:ROLLback CONFIRM` | roll back to the kept image | Roll back update |
| `:SYSTem:UPDate:INSTall "path",CONFIRM` | install a signed package file (same checks as the web page) | Update |
| `:SYSTem:BOOT[?]` | `ONEShot`, `PERSist`; a change needs `,CONFIRM` | Boot mode |
| `:SYSTem:OVERclock[?]` | ADC clock in Hz, one of the board's steps; a change needs `,CONFIRM` | Overclock |
| `:SYSTem:STOCk CONFIRM` | what Back to stock does: says whether the next power-on is stock | Back to stock |

### Headers that were implemented but not documented

| Header | Parameters / behavior |
| --- | --- |
| `:MEASure:LEVel:TYPE[?]` | Reference levels as `PERCent` of the waveform or `VOLTs`. Query answers `PERC` or `VOLT`. |
| `:MEASure:LEVel:PRESet[?]` | `10/90`, `20/80` or `CUSTom` (the Reference preset row). |
| `:MEASure:LEVel:LOW[?]`, `:MEASure:LEVel:MID[?]`, `:MEASure:LEVel:HIGH[?]` | Low, mid and high reference level, percent or volts (by `TYPE`). |
| `:MEASure:LEVel:HYSTeresis[?]` | Hysteresis of the edge detection, percent or volts. |
| `:MEASure:LEVel:BASEtop[?]` | Base/top method: `AUTO`, `MINMax`, `MEAN`, `MODE`. |
| `:MEASure:LEVel:FALL:SEParate[?]` | `ON` gives the falling edge its own levels. |
| `:MEASure:LEVel:FALL:LOW[?]`, `:MEASure:LEVel:FALL:MID[?]`, `:MEASure:LEVel:FALL:HIGH[?]` | Falling-edge levels. |
| `:MEASure:STATistic:POPulation[?]` | `CURRent` (the statistics of the shown acquisition run) or `ACCumulated`. A change resets the statistics. |
| `:MEASure:STATistic:SHOW[?]` | Statistics columns: any of `SDEViation`, `MINimum`, `MAXimum` comma-separated, or `NONE`. |
| `:TRIGger:COUPling[?]` | Coupling of the edge trigger on a channel: `DC`, `AC`, `LFR` (low-frequency reject), `HFR`. |
| `:DISPlay:VIEW[?]` | The view shown, `1` to the count (Display > Views). |
| `:DISPlay:VIEW:COUNt[?]` | The number of views, 1 to 4. |
| `:SYSTem:GRAPhics[?]` | `CPU` or `GPU` drawing; the query answers what is in use (whether the GPU really draws is the `gpu` part of `:SYSTem:STATe?`). |

## The `:SYSTem:SET` key list

`:SYSTem:SET <key> = <value>` writes a key of the setup file on the live settings; `:SYSTem:GET? <key>` reads it. It is the setup schema, an escape hatch for tests and for the few settings that have no header; where a header exists above or in the sections before, use that. The file written by Save setup and returned by `:SYSTem:SETup?` is the same list as `key = value` lines (`#` starts a comment). Values: booleans `true`/`false`, numbers in base units, enumerations as the Rust variant name (`Dc`, `Rise`, `Auto`), lists separated by blanks. A key that is not in the list is rejected with -224 after the fact; some keys are only written when they differ from their default (listed as "optional"). The keys that carry a number are written with it (`ch2.vdiv`); here that number is `<n>`.

### Channels, timebase, trigger, acquisition

| Key | Value |
| --- | --- |
| `ch<n>.on`, `ch<n>.vdiv`, `ch<n>.offset`, `ch<n>.coupling`, `ch<n>.bw`, `ch<n>.probe`, `ch<n>.invert` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert |
| `ch<n>.vfine`, `ch<n>.unit`, `ch<n>.bias`, `ch<n>.skew`, `ch<n>.label` | optional: fine scale, unit, zero bias, probe deskew, label |
| `active` | the selected channel, 0-based |
| `tdiv`, `delay`, `fine_t`, `tfine`, `fine_v`, `v_expand_gnd`, `h_expand`, `auto_roll` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll |
| `zoom`, `zoom_tdiv`, `zdelay` | zoom on/off, zoom time/div step, zoom delay |
| `trig_src`, `trig_ref`, `trig_la`, `trig_src2`, `trig_ref2` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` |
| `trig_level`, `trig_level2`, `slope`, `trig_slope2`, `trig_mode`, `holdoff`, `trig_type`, `trig_nrej`, `trig_coup` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling |
| `trig_pol`, `trig_when`, `trig_t1`, `trig_t2` | `Pos`/`Neg`; `Off`/`Gt`/`Lt`/`In`/`Out`; time and upper limit |
| `video_std`, `video_sync`, `video_line` | `Pal`/`Ntsc`; `AllLines`/`Line`/`Odd`/`Even`/`AllFields`; line |
| `pat`, `pat_levels`, `pat_la` | four bits `H L X R F`; four levels; the 16 logic-line bits as one word of `H L X` |
| `win_mode`, `nth_idle`, `nth_n`, `setup`, `hold`, `trig_bus`, `trig_bus_val` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value |
| `acq`, `avg_count`, `segments`, `mdepth` | `Normal`/`Peak`/`Average`/`HiRes`; averages; segments (0 = off); memory depth index |
| `touch_lock`, `quick`, `text_size`, `hdmi` | touch lock, quick key action, text size, HDMI mode |
| `deskew_ch<n>`, `deskew_ref<n>` | optional: analysis deskew seconds |

### Display, windows, views

| Key | Value |
| --- | --- |
| `persist`, `intensity`, `grid`, `grid_bright`, `grid_frame`, `color_grade`, `dots` | display settings |
| `windows`, `density`, `split_pane`, `ch_window`, `math_window`, `la_window`, `ref_window`, `pane`, `trend_sel`, `strip`, `meas_labels` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) |
| `layout`, `studio_view`, `studio_dock`, `studio_ov`, `studio_meas_view`, `studio_badges` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) |
| `view`, `views`, `view<n>` | the analyser view (`Scope`, `Spectrum`, `Eye`, ...); `views` is `<count> <current, 1-based>`; `view<n>` is the text of view slot n (optional) |

### Measurements, cursors, DVM, counter, logger

| Key | Value |
| --- | --- |
| `meas` | rows as `<Item>:<source>`, blank separated, for example `Vpp:1` or `Vpp:R2` (the source token is the channel number, 1 = CH1, or `R<n>`) |
| `meas2` | pair rows as `<pair number>:<source A>:<source B>` |
| `cat` | optional: catalogue rows `<id>:<A>:<B>:<C>:<D>` |
| `row_cfg` | optional: per-row settings, `<row key>\|<settings text>`, blank separated; implementation specific |
| `meas_cfg` | `<idle> <Above/Below/Both> <N periods> <data edges 0/1> <low pct> <high pct> <Tsetup> <Thold>` |
| `meas_levels` | `<absolute 0/1> <custom 0/1> <mid> <hysteresis> <low V> <mid V> <high V> <hysteresis V> <separate fall 0/1> <fall low> <fall mid> <fall high> <fall low V> <fall mid V> <fall high V> <base/top method>` |
| `meas_stat` | optional: `<current 0/1> <sigma 0/1> <min 0/1> <max 0/1>` |
| `meas_full_record` | gate: whole record or the screen |
| `cursor`, `cur_a`, `cur_b`, `cur_c`, `cur_d`, `cur_b_source` | cursor mode, positions (fractions of the screen), second cursor source (optional) |
| `dvm`, `counter` | `<on> <source> <mode>`; `<on> <source> <mode> <digits>` |
| `log_ivl` | logger interval, index into 1, 2, 5, 10, 30, 60, 300, 600, 1800, 3600 s |

### Math

| Key | Value |
| --- | --- |
| `math`, `math_a`, `math_b`, `math_scale`, `math_fc`, `math_lin`, `math_offset`, `fft_win`, `math_bw`, `math_sel` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth |
| `m<n>.op`, `m<n>.a`, `m<n>.b`, `m<n>.scale`, `m<n>.fc`, `m<n>.bw`, `m<n>.offset`, `m<n>.win`, `m<n>.lin` | the same for math channels 2 to 8 (the selected one is written as `math*`) |
| `math_expr<n>` | optional: the formula text of channel n |
| `spec_margin` | spectrum limit margin, dB |

### Decoders and bus

The keys of the selected decoder are plain (`dec_*`); decoders 2 and up repeat them with a prefix, `d<n>.on`, `d<n>.src`, and so on.

| Key | Value |
| --- | --- |
| `dec_sel`, `dec_on`, `dec_proto`, `dec_src`, `dec_src2`, `dec_cs`, `dec_src4`, `dec_diff`, `dec_thr`, `dec_ascii` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex |
| `dec_baud`, `dec_canrate`, `dec_canh`, `dec_fdrate`, `dec_arate`, `dec_tick`, `dec_nibbles`, `dec_uspeed`, `dec_cphy_rate`, `dec_mode`, `dec_bits`, `dec_parity`, `dec_opt` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol |
| `dec_la` | `<input 0/1> <16 logic line numbers or ->` |
| `bus_on`, `bus_label`, `bus_pos`, `bus_kind`, `bus_clk`, `bus_data`, `bus_clk_thr`, `bus_data_thr`, `bus_rw` | the plot-bus (I2C lane) |
| `la` | the logic analyser: `<on> <line mask hex> <...> <threshold pod 1> <threshold pod 2> <...>` as `la = false ffff 0 0 1.65 1.65 1` |

### Analysers and tools: the packed keys

The analyser pages keep several fields in one key, positional, blank separated. A source in a setup key is its token: the channel number (`1` is CH1) or `R1`; `-` or `off` is none. The view key takes `Scope`, `Spectrum`, `Eye`, `Power`, `Bode`, `Step` (the bandwidth test), `Magnetic`, `Inverter`, `Dpt`.

| Key | Fields, in order |
| --- | --- |
| `spec` | source token, centre Hz, span Hz (0 = full), RBW Hz, window name, trace mode index, ref level dBV, waterfall 0/1, THD/SNR 0/1 (the last two as `true`/`false`) |
| `spec_peaks` | peak sort index, harmonics index |
| `spec_acpr` | channel width Hz, channel spacing Hz |
| `eye` | source, bit rate Hz, from-record 0/1 |
| `eye_more` | BER, PN low Hz, PN high Hz, NPJ 0/1, rail source or `-`, notch Hz, clock (strobe) source or `-`, high-pass 0/1 |
| `eye_pll`, `eye_place` | optional PLL bandwidth Hz (only when above 0); `Full`/`Side`/`Under` |
| `power` | voltage source index, current source index, page name |
| `power_more` | line f Hz, switch f Hz, input source or `-`, reference volts or `-`, clock source or `-` |
| `magnetic` | voltage source, current source, turns, Ae, Le, Rcu, plot (`IPsi`/`BH`), loop cursor |
| `inverter` | page (`Dq0`/`Mech`), phase A, B, C source, angle source name, poles, analog source, volts-mode 0/1, torque source or `off`, bus voltage source or `off`, bus current source or `off`, analog scale, degrees per volt, offset, electrical 0/1, encoder line A, encoder line B, index line or `off`, counts per revolution, swap 0/1, Hall line A, B, C |
| `dpt` | Vds, Id, Vgs source tokens, fourth channel mode (`Off`/`Gate`/`DiodeV`/`DiodeI`), fourth source, gate percent, zoom (`Both`/`TurnOn`/`TurnOff`) |
| `bode` | start Hz, stop Hz, points, amplitude V, input source, output source, generator index, generator address `ip:port` |
| `bode_link`, `bode_mode` | `lan`/`usb`; `Loop`/`Psrr`/`Impedance` |
| `bode_sweep` | free sweep: time s, log true/false, sync channel (1-based), sync edge (`Rise`/`Fall`), sync level V, input channel (0 = none), reference dBV |
| `sim` | preset index, slot, rate b/s, PRBS order, high V, low V, rise s, noise V, bandwidth Hz, de-emphasis dB, spread 0/1, random jitter s, periodic jitter s, periodic jitter Hz, reserved 0 |
| `sim_extra` | spread centre true/false, spread deviation, spread frequency Hz, DCD s, correlated RJ s, correlated corner Hz |
| `zone<n>` | `<on> <source, 1-based> <inside true/false> <t:v points, blank separated>` |
| `z_src`, `hist` | Z input channel or `off`; histogram type |
| `mask_on`, `mask_src`, `mask_x`, `mask_y`, `mask_stop`, `mask_inv` | mask test settings (see also `:MASK:...`) |

### Decoder parameters, plot bus, logic analyser, zones and small rows

Decoder parameters take the option text of the menu row (`:BUS1:CAN:BAUD?` answers it in capitals). `BUS<n>` is decoder n; the decoder the menu edits stays the same afterwards. The protocol-specific rows exist only for their protocol (an option the protocol does not have is -224).

| Header | Values | Menu item |
| --- | --- | --- |
| `:BUS<n>:SPI:MODE[?]` | `0` to `3` (clock polarity and phase) | Mode |
| `:BUS<n>:CAN:BAUD[?]`, `:BUS<n>:CAN:SIGNal[?]` | bit rate; `RX/TX` or `CAN_H` | Bit rate, Signal |
| `:BUS<n>:CAN:FD:BAUD[?]`, `:BUS<n>:CAN:FD:ABAUd[?]` | FD data rate; arbitration rate | FD data rate, Arbitration rate |
| `:BUS<n>:SENT:TICK[?]`, `:BUS<n>:SENT:NIBBles[?]` | tick; nibbles 1 to 6 | Tick, Nibbles |
| `:BUS<n>:USB:SPEed[?]` | speed | Speed |
| `:BUS<n>:OPTion1[?]`, `:BUS<n>:OPTion2[?]`, `:BUS<n>:OPTion3[?]`, `:BUS<n>:OPTion4[?]` | the options of the protocol module, in the order of the menu | Option |
| `:BUS:LANE:STATe[?]`, `:BUS:LANE:LABel[?]`, `:BUS:LANE:POSition[?]` | the plot bus (the I2C lane): on/off; label (12 characters); position in divisions, +-3.6 | Plot bus |
| `:BUS:LANE:CLOCk:SOURce[?]`, `:BUS:LANE:DATA:SOURce[?]` | source | Clock, Data |
| `:BUS:LANE:CLOCk:THReshold[?]`, `:BUS:LANE:DATA:THReshold[?]` | volts | thresholds |
| `:BUS:LANE:RW[?]` | `LEFT_OUT`, `INCLUDED` (the read/write bit in the address) | R/W bit |
| `:DECode:SAVE` | save the decoded events as CSV | Export events |
| `:LA:DIGital<n>:PATTern[?]` | `H`, `L`, `X` for line n (0 to 15), as the trigger pattern | LA pattern |
| `:LA:POD<n>:PRESet[?]` | threshold preset of pod 1 or 2 by its name (`TTL`, `CMOS`, ..., `USER`) | Thr D0-7, Thr D8-15 |
| `:LA:SIZE[?]` | lane height by its name | Lane height |
| `:ZONE<n>:STATe[?]`, `:ZONE<n>:SOURce[?]`, `:ZONE<n>:CONDition[?]` | zone n (1 or 2): on/off; `CHANn`; `INTersect` or `NOTintersect` | Zone on, source, condition |
| `:ZONE<n>:BOX[?]` | `<t1>,<v1>,<t2>,<v2>`: seconds from the trigger and volts of two corners (the query only for a box) | Draw zone box |
| `:ZONE<n>:POINts[?]` | `<t>,<v>,...`: a polygon of three or more points | Draw zone shape |
| `:ZONE<n>:CLEar` | delete the outline, keep the source | Delete zone |
| `:SOURce:ARB:LOAD ["path"]`, `:SOURce<n>:ARB:LOAD ["path"]` | arbitrary wave from a file (one number per line or a CSV); default `arb.csv` in the storage folder | Load arb.csv |
| `:CHANnel:ACTive[?]` | the selected channel, 1 to the number of channels | active channel |
| `:SYSTem:QUICk[?]` | the quick key action, by its option text | Quick key |
| `:DISPlay:LOWer[?]`, `:DISPlay:TRENd1[?]`, `:DISPlay:TRENd2[?]` | the lower pane; the content of trend chart 1 and 2, by option text | Lower, Trend chart |

### Menu options with no SCPI path

These rows stay without a header: pure chrome (window layout, density, strip position, dock arrangement and the Studio panels), keypad and picker navigation, the candidate pick of Identify, joining a WLAN (it needs a password: do it on the screen), measurement catalogue rows (adding by catalogue id and reading `:MEASure:ITEM? CAT<id>`, source C and D, and the per-row settings card): these remain on `:SYSTem:SET cat` and `row_cfg`, and `:MEASure:READout?` exports them as `CAT<id>`.

### Measurement catalogue and the inverter index line

| Header | Meaning | Menu item |
| --- | --- | --- |
| `:MEASure:CATalog:ITEM <id>[,A[,B[,C[,D]]]]` | add the catalogue row `id` on the given sources (defaults: the current Source A, B, C, D); a row already there is left. The query `:MEASure:CATalog:ITEM? <id>[,A,...]` answers its value (`9.9E37` when none) | Add (catalogue) |
| `:MEASure:CATalog:SOURceC[?]`, `:MEASure:CATalog:SOURceD[?]` | Source C and D of the Add page | Source C, Source D |
| `:INVerter:ENCoder:INDex[?]` | encoder index line 0 to 15 or `OFF` | Index line |
