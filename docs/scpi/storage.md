# Save and load

Save and load setups, waveforms, screenshots and the fast-frame history. A relative path of a save is taken in the save folder. `*SAV` and `*RCL` (see [common](common.md)) and the setup block `:SYSTem:SETup` (see [system](system.md)) are the same setup files.

Menu location: Storage (Save destination, Setup slot, Save setup, Recall setup, Save waveform, Save screenshot, Fast-frame save / load).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SAVE:DESTination` | Set / Query | `AUTO`, `USB`, `DESKtop` (the local storage directory) | `AUTO` | Storage > Save to | OOS extension |
| `:SAVE:SLOT` | Set / Query | Setup slot 1 to 9 | `1` | Storage > Setup slot | OOS extension |
| `:SAVE:SETup` | Action | `["path"]` Save the current slot, or `"path"`. | - | Storage > Save setup | Rigol-compatible |
| `:LOAD:SETup` | Action | `["path"]` Recall the current slot, or `"path"`. | - | Storage > Recall setup | Rigol-compatible |
| `:SAVE:WAVeform` | Action | `["path"[,CSV or BIN]]` Newest record to a file (default CSV, in the save folder) | - | Storage > Save waveform | Rigol-compatible |
| `:SAVE:IMAGe` | Action | `["path"]` Screenshot as PNG, written after the next frame | - | Storage > Save screenshot | Rigol-compatible |
| `:SAVE:FFRame` | Action | Save the fast-frame history (`frame-NNN.bin` in the save folder). | - | Storage > Save fast-frame | OOS extension |
| `:LOAD:FFRame` | Action | Load the fast-frame history. | - | Storage > Load fast-frame | OOS extension |
| `:SAVE:SESSion` | Action | `["path"]` The setup, the record on the screen and the loaded reference slots as one file (`session-NNN.oss` in the save folder, or `"path"`) | - | Storage > Save session | OOS extension |
| `:LOAD:SESSion` | Action | `["path"]` Load a session (the newest `session-NNN.oss`, or `"path"`): the scope stops and shows it; reference slots the file does not hold are emptied | - | Storage > Load session | OOS extension |

## Examples

Save to a slot and recall.

```text
:SAVE:DESTination AUTO
:SAVE:SLOT 3
:SAVE:SETup
:LOAD:SETup
:SAVE:SLOT?                            -> 3
```

Save the newest waveform as CSV.

```text
:STOP
:SAVE:WAVeform
:SAVE:IMAGe
```

