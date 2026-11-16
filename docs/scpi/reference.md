# Reference waveforms

Ten reference slots `R1` to `R10` (`REF1` to `REF10` are aliases). Loading a file is not an ADC acquisition, and a reference keeps its own native timing. Absolute paths refer to the server PC or scope; relative paths are beside the setups. For remote files use the UI explicit-slot upload workflow.

Menu location: Reference (Slot select, Import, Save to slot, Show / hide, Scale / offset, Name, Color, Export, Clear, Reference look) and, in Studio, the per-reference horizontal scale.

Part of the [SCPI reference](README.md).

## Commands

### Slots and files

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:REFerence:SLOT` | Set / Query | `R1..R10` Choose the destination/working slot. | `R1` | Reference > Slot | OOS extension |
| `:REFerence:LOAD` | Action | `"path"` Load into that slot. Absolute paths refer to the server PC/scope; relative paths are beside setups. | - | Reference > Import (load file) | OOS extension |
| `:REFerence:IMPort` | Action | `["path"]` Load named file, or the selected `refN` file beside setups. | - | Reference > Import (load file) | OOS extension |
| `:REFerence:SAVE` | Action | Copy the selected acquired channel record into the selected reference slot. Does not write a waveform file. | - | Reference > Save to slot | OOS extension |
| `:REFerence:CURRent` | Set / Query | `1..10` Working slot, returned as a number; SLOT returns `Rn`. | `1` | Reference > Slot select | OOS extension |
| `:REFerence:SOURce` | Set / Query | `[<slot>,]CHANn` Acquired-channel source used when saving a reference. | `CHAN1` | Reference > Save from (source) | OOS extension |
| `:REFerence:CLEar` | Action | Clear all loaded reference slots. | - | Reference > Clear all | OOS extension |
| `:REFerence:EXPort` | Action | `["path"]` Write the selected slot as CSV (`Time (s),Volts`); without a path to `ref<n>.csv` beside the setups | - | Reference > Export | OOS extension |

### Display

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:REFerence:SHOW` | Set / Query | `[<slot>,]ON\|OFF` Visibility of one slot; DISPlay aliases SHOW. Query accepts a slot number. | `0` | Reference > Show / hide per slot | OOS extension |
| `:REFerence:VSCale` | Set / Query | `[<slot>,]<V/div>` Display scale of a loaded reference; stored samples are unchanged. | - | Reference > Slot scale | OOS extension |
| `:REFerence:VOFFset` | Set / Query | `[<slot>,]<V>` Display offset of a loaded reference. | - | Reference > Slot offset | OOS extension |
| `:REFerence:LABel:CONTent` | Set / Query | `[<slot>,]<text>` Reference display label. | - | Reference > Name | OOS extension |
| `:REFerence:COLor` | Set / Query | Color of the selected or explicit slot: `GRAY`, `WHITe`, `RED`, `GREen`, `BLUE`, `ORANge`, `VIOLet`. | - | Reference > Slot color | OOS extension |
| `:REFerence:RESet` | Action | Reset the reference display scale; recorded samples and native timing are retained. | - | Reference > Scale / offset | OOS extension |

### Look and horizontal scale (Studio)

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:REFerence:LOOK` | Set / Query | `LIVE`, `TAKEover`, `OFFLine` (offline waveforms) | `LIVE` | Reference > Offline / reference look | OOS extension |
| `:REFerence:HORizontal:MODE` | Set / Query | `FOLLow` (main time base) or `OWN` for the selected slot (Studio) | `FOLL` | Reference > Follow / Own | OOS extension |
| `:REFerence:HORizontal:SCALe` | Set / Query | Own seconds per division of the selected slot. | `2.000000E-04` | Reference > time/div | OOS extension |
| `:REFerence:HORizontal:OFFSet` | Set / Query | Own position of the selected slot. | `0.000000E+00` | Reference > position | OOS extension |
| `:REFerence:HORizontal:FIT` | Action | The whole record fills the lane; -221 outside Studio's stacked lanes | - | Reference > Fit | OOS extension |

## Loading a file

A file load is not an ADC acquisition. Multi-trace imports validate capacity before replacing slots. Protocol roles may mix references and acquired records, but their native intervals and origins must overlap. Decoder events, Search and Bus qualification share the decoded timing. Load paths are not file uploads: the path is read on the server PC or scope, and it is not valid on a Linux scope unless that file exists there.

## Examples

Save a channel into a slot and show it.

```text
:STOP
:REFerence:SLOT R2
:REFerence:SOURce CHAN1
:REFerence:SAVE
:REFerence:SHOW 2,ON
:REFerence:SLOT?                       -> R2
```

Own horizontal scale in Studio.

```text
:REFerence:HORizontal:MODE?            -> FOLL
:REFerence:LOOK?                       -> LIVE
```

Export a slot as CSV and clear it.

```text
:REFerence:SLOT R2
:REFerence:EXPort
:REFerence:CLEar R2
```

