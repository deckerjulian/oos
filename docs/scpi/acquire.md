# Run control and acquisition

Run, stop, single, autoset and clear; the acquisition type, averages, memory depth and the segmented frame count; and the history of stored acquisitions. There is one acquisition context: a loaded reference beside live channels is a passive overlay, and selecting a reference as the primary trigger source enters stored-record acquisition. `:RUN` and `:STOP` are assignments, so repeating either does not toggle it.

Menu location: Run control (Run / Stop, Single, Autoset, Clear), Acquisition (Mode, Averages, Memory depth, Segments), Run control > History.

Part of the [SCPI reference](README.md).

## Commands

### Run control

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:RUN` | Action | Start continuous acquisition. Idempotent assignment; also starts stored replay and SIM. | - | Run control > Run / Stop | Rigol-compatible |
| `:STOP` | Action | Stop acquisition. Idempotent assignment; also stops stored replay and a SIM generation. | - | Run control > Run / Stop | Rigol-compatible |
| `:SINGle` | Action | Arm once: wait for a qualified capture, then stop. | - | Run control > Single | Rigol-compatible |
| `:TFORce` | Action | Accept a capture without a trigger match (software force on stored/SIM acquisition, one hardware pulse live). | - | Run control > Force trigger | Rigol-compatible. Alias of `:TRIGger:FORCe`. |
| `:AUToscale` | Set / Query | Start ADC Autoset; the query answers `1` while it runs, else `0`. Autoset does not fit a reference. | `0` | Run control > Autoset | Rigol-compatible |
| `:CLEar` | Action | Clear display persistence and measurement statistics (the Clear action). | - | Run control > Clear display / stats | Rigol-compatible |

### Acquisition

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:ACQuire:SRATe` | Query | ADC/model sample rate, not a loaded reference's native rate. | `2.500000E+08` | Acquisition > Memory depth (read-out) | Rigol-compatible |
| `:ACQuire:MDEPth` | Set / Query | Set `AUTO` or a supported menu depth, e.g. `1000000`, `1M`. Query returns the numeric configured/effective acquisition depth. | `5.000000E+05` | Acquire > Memory | Rigol-compatible |
| `:ACQuire:TYPE` | Set / Query | `NORMal\|PEAK\|AVERages\|HRESolution`. | `NORM` | Acquire > Mode | Rigol-style (command family of the DHO guide) |
| `:ACQuire:AVERages` | Set / Query | Requested average count, mapped to supported powers of two. Read back the selected count. | `16` | Acquire > Averages | Rigol-style (command family of the DHO guide) |
| `:ACQuire:ULTRa:MAXFrame` | Set / Query | Supported segmented frame count. This does not claim the stock UltraAcquire capture rate. | `0` | Acquire > Segments | Rigol-style (command family of the DHO guide) |

### History

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:ACQuire:HISTory:ENABle` | Set / Query | `ON` shows the newest stored record (stops the acquisition), `OFF` goes back to live (Run). -221 when nothing is stored. | `0` | Run control > History / Live | name not verified against the DHO guide |
| `:ACQuire:HISTory:FRAMe` | Set / Query | Record `1`..count; `0` when not in history | `0` | Run control > History, Back, Forward | name not verified against the DHO guide |
| `:ACQuire:HISTory:COUNt` | Query | Number of stored records | `200` | - | name not verified against the DHO guide |
| `:ACQuire:HISTory:NEXT` | Action | One record on (the Display menu's Forward). | - | Run control > History: newer | name not verified against the DHO guide |
| `:ACQuire:HISTory:PREVious` | Action | One record back (the Display menu's Back). | - | Run control > History: older | name not verified against the DHO guide |

## Examples

Stop, pick averaging and read it back.

```text
:STOP
:ACQuire:TYPE AVERages
:ACQuire:AVERages 16
:ACQuire:TYPE?                         -> AVER
:ACQuire:AVERages?                     -> 16
```

Memory depth and sample rate.

```text
:ACQuire:MDEPth?                       -> 5.000000E+05
:ACQuire:MDEPth 1M
:ACQuire:MDEPth?                       -> 1.000000E+06
:ACQuire:SRATe?                        -> 2.500000E+08
```

Walk the acquisition history.

```text
:STOP
:ACQuire:HISTory:COUNt?                -> 200
:ACQuire:HISTory:ENABle ON
:ACQuire:HISTory:FRAMe?                -> 200
:ACQuire:HISTory:PREVious
:ACQuire:HISTory:FRAMe?                -> 199
:ACQuire:HISTory:ENABle OFF
```

