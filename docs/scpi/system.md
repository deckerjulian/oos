# System

Everything under the System menu: identification text, the key = value setup schema, the whole-setup block, date, time and NTP, Ethernet and WLAN, the remote services switches, the scope name, touch lock, the quick key and session, and the commands that power off, restart, update, roll back or change the boot mode. Destructive commands take a literal `CONFIRM` argument and are refused with -221 without it; a command that would change nothing (the boot mode it already has) needs none. `:SYSTem:SETup` (long form) is the setup block; the short form `:SYST:SET` stays the older key = value command. The key list behind `:SYSTem:SET` is in [setup keys](setup-keys.md).

Menu location: System (About, System info, Date and time, Network, Remote services, Update, Power), Run control > Power off / restart, Display and layout > Graphics.

Part of the [SCPI reference](README.md).

## Commands

### Diagnostics, schema and setup block

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:SETup` | Set / Query | Query: the whole setup (the file Save setup writes) as an IEEE block. Set: apply a setup block (or the plain text); the acquisition state stays as it is. The short form `:SYST:SET` is the key = value command. | - | Storage > Recall setup | OOS extension |
| `:SYSTem:LOCK` | Set / Query | Touch lock on/off | `0` | - | name not verified against the DHO guide |
| `:SESSion` | Set / Query | `LIVE`, `SIM` or `REF`; setting `LIVE` leaves SIM and offline sessions. | `LIVE` | System > Live | OOS extension |
| `:SYSTem:INFO` | Query | The System info page as text: `label,value` lines, `# heading` lines | - | System > System info | OOS extension |
| `:SYSTem:GRAPhics` | Set / Query | `CPU` or `GPU` drawing; the query answers what is in use (whether the GPU really draws is the `gpu` part of `:SYSTem:STATe?`). | `GPU` | System > Graphics | OOS extension |
| `:SYSTem:QUICk` | Set / Query | The quick key action, by its option text | `SCREENSHOT` | System > Quick key | OOS extension |
| `:IDENtify` | Query | The app's signal identification text. | - | Run control > Identify signal | OOS extension |
| `:SYSTem:STATe` | Query | Diagnostic state text of the app (includes the `gpu` part). | - | - | OOS extension |
| `:SYSTem:GET` | Query | `<key>` Value of a key of the setup schema, empty when the key is unknown. See [setup keys](setup-keys.md). | - | - | OOS extension |
| `:SYSTem:SET` | Set | `<key> = <value>` Write a key of the setup file on the live settings. Unknown keys fail with -224 after the fact. See [setup keys](setup-keys.md). | - | - | OOS extension. `:SYSTem:SETup` (long form) is a different command. |
| `:SYSTem:ERRor` | Query | Remove and return the oldest error as `code,"message"`; `0,"No error"` when empty. | - | - | Rigol-compatible |

### Name, date, time and NTP

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:DATE` | Set / Query | `yyyy,mm,dd` in the time zone of the settings; setting one part keeps the other part of the clock. | `2026,11,16` | System > Date and time > Set the clock | Rigol-compatible |
| `:SYSTem:TIME` | Set / Query | `hh,mm,ss` in the time zone of the settings. | `16,58,59` | System > Date and time > Set the clock | Rigol-compatible |
| `:SYSTem:TZONe` | Set / Query | The time zone, a POSIX-style string (`CET-1CEST,M3.5.0,M10.5.0/3`) | `UTC0` | Date and time > Time zone | OOS extension |
| `:SYSTem:NTP:ENABle` | Set / Query | NTP on/off | `0` | Date and time > Time from NTP | OOS extension |
| `:SYSTem:NTP:SERVer` | Set / Query | Server name or address, or `DHCP` | `dhcp` | Date and time > NTP server | OOS extension |
| `:SYSTem:NTP:STATus` | Query | The NTP state text | `Off` | System > Date and time > NTP state | OOS extension |
| `:SYSTem:NTP:SYNC` | Action | Sync now | - | System > Date and time > Sync now | OOS extension |
| `:SYSTem:NAME` | Set / Query | The scope's name on the network (letters, digits and `-`, up to 63) | `dho924s` | System > Name | OOS extension |

### Network

Joining a WLAN network needs a password and stays on the screen.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:COMMunicate:LAN:IPADdress` | Query | Address, MAC address or link text (query only). | empty | System > Network > Ethernet | Rigol-compatible |
| `:SYSTem:COMMunicate:LAN:MAC` | Query | Address, MAC address or link text (query only). | `00:15:5d:6d:dc:93` | System > Network > Ethernet | Rigol-compatible |
| `:SYSTem:COMMunicate:LAN:STATus` | Query | Address, MAC address or link text (query only). | `link up, no address yet` | System > Network > Ethernet | Rigol-compatible |
| `:SYSTem:COMMunicate:LAN:RETRy10` | Set / Query | 10 Mbit retry on/off | `1` | Network > 10 Mbit retry | Rigol-compatible |
| `:SYSTem:WLAN:STATus` | Query | WLAN state text. | `no stick` | System > Network > WLAN | OOS extension |
| `:SYSTem:WLAN:SCAN` | Action | Start a scan. | - | System > Network > WLAN | OOS extension |
| `:SYSTem:WLAN:NETWorks` | Query | The scanned networks, one per line. | `Other network ...` | System > Network > WLAN | OOS extension |
| `:SYSTem:WLAN:SAVed` | Query | The saved networks, one per line. | empty | System > Network > WLAN | OOS extension |
| `:SYSTem:WLAN:FORGet` | Action | `<n>` Forget saved network `<n>`. Joining a network needs a password and stays on the screen. | - | System > Network > WLAN | OOS extension |

### Remote services

The switches of the Remote services page. `SCPI OFF` needs `,CONFIRM`.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:REMote:SERVice:SCPI` | Set / Query | Boolean switch of the Remote services page; `SCPI OFF` needs `,CONFIRM`. | `1` | System > Remote services > Remote services | OOS extension |
| `:SYSTem:REMote:SERVice:VXI11` | Set / Query | Boolean switch of the Remote services page; `SCPI OFF` needs `,CONFIRM`. | `1` | Remote services > VXI-11 | OOS extension |
| `:SYSTem:REMote:SERVice:TELNet` | Set / Query | Boolean switch of the Remote services page; `SCPI OFF` needs `,CONFIRM`. | `1` | Remote services > telnet | OOS extension |
| `:SYSTem:REMote:SERVice:WEB` | Set / Query | Boolean switch of the Remote services page; `SCPI OFF` needs `,CONFIRM`. | `1` | Remote services > Web remote | OOS extension |
| `:SYSTem:REMote:SERVice:MDNS` | Set / Query | Boolean switch of the Remote services page; `SCPI OFF` needs `,CONFIRM`. | `1` | Remote services > mDNS | OOS extension |

### Power, update and boot

These take an explicit `CONFIRM` argument and are refused with -221 without it. Power off and restart are refused while an update runs.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:POWer:OFF` | Action | `CONFIRM` `CONFIRM` required. Power off; refused while an update runs. | - | System > Update and power > Power dialog | OOS extension |
| `:SYSTem:POWer:RESTart` | Action | `CONFIRM` `CONFIRM` required. Restart; refused while an update runs. | - | System > Update and power > Power dialog | OOS extension |
| `:SYSTem:UPDate:STATus` | Query | The Update page as text | `Update` | System > Update and power > Update | OOS extension |
| `:SYSTem:UPDate:ROLLback` | Action | `CONFIRM` Roll back to the kept image | - | System > Update and power > Roll back update | OOS extension |
| `:SYSTem:UPDate:INSTall` | Action | `"path",CONFIRM` Install a signed package file (same checks as the web page) | - | System > Update and power > Update | OOS extension |
| `:SYSTem:BOOT` | Set / Query | `ONEShot`, `PERSist`; a change needs `,CONFIRM` | `ONES` | System > Update and power > Boot mode | OOS extension |
| `:SYSTem:OVERclock` | Set / Query | ADC clock in Hz, one of the board's steps; a change needs `,CONFIRM` | `1.250000E+09` | System > Update and power > Overclock | OOS extension |
| `:SYSTem:STOCk` | Action | `CONFIRM` What Back to stock does: says whether the next power-on is stock | - | System > Update and power > Back to stock | OOS extension |

## Examples

Date, time and time zone.

```text
:SYSTem:DATE 2026,11,16
:SYSTem:TIME 12,30,0
:SYSTem:DATE?                          -> 2026,11,16
:SYSTem:TZONe?                         -> UTC0
```

Remote services; the destructive commands need CONFIRM.

```text
:SYSTem:REMote:SERVice:SCPI?           -> 1
:SYSTem:REMote:SERVice:WEB ON
:SYSTem:REMote:SERVice:SCPI OFF
:SYSTem:ERRor?                         -> -221,"Settings conflict" Switching SCPI off needs the argument CONFIRM
:SYSTem:POWer:OFF
:SYSTem:ERRor?                         -> -221,"Settings conflict" Power off needs the argument CONFIRM
```

Whole setup and a key.

```text
:SYSTem:SET tdiv = 5
:SYSTem:GET? tdiv                      -> 5
:SYSTem:NAME "bench-scope"
:SYSTem:NAME?                          -> bench-scope
:SYSTem:BOOT?                          -> ONES
```

