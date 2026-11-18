# Troubleshooting

Your scope can always be recovered as long as you have the image of your original card
([On a second card](https://github.com/dekajoule/oos/wiki/Installation#on-a-second-card), step 1): the worst case is writing it to a card again. After an install
from a USB stick, the folder `OOS-rescue` takes its place for the parts that changed. The fixes with `boot/switch.py`
and `boot/card-install.py` need the card in a reader, so you have to open the scope again. The commands are for
macOS, the card as `/dev/rdiskM` after `diskutil unmountDisk /dev/diskM`; on Linux it is `/dev/sdX`.

| What you see | What to do |
|---|---|
| The stock system starts instead of OOS | One-shot mode ([How OOS starts](https://github.com/dekajoule/oos/wiki/Installation#how-oos-starts)), or the loader refused the OOS image and used the fallback. Check with `sudo python3 boot/switch.py show /dev/rdiskM`, then select OOS again: `sudo python3 boot/switch.py oos /dev/rdiskM --execute`. If that repeats, rebuild or download the image again |
| Rigol logo stays, nothing else, on every start | The kernel hangs before OOS runs: `sudo python3 boot/switch.py stock /dev/rdiskM --execute`, then install a working image |
| Black screen; a Rockchip USB device (2207:330c) appears | The loader refused `recovery` and has no fallback: `sudo python3 boot/switch.py stock /dev/rdiskM --execute`. If needed, put a saved `recovery` back: `sudo python3 boot/card-install.py restore /dev/rdiskM --backup FILE --execute`, with `FILE` the `backups/recovery-<date>.bin` of a card install, or `OOS-rescue/recovery.bin` of a USB install |
| Start-up screen stops at a step | A hardware step failed. Read the step on the screen. Logs are in `/tmp/oos-boot.status` and `/tmp/oos-cold.log` over SSH ([[Remote Control]]) |
| "Start-up stopped" after a quick restart | OOS needs a real power cycle: off at the mains, wait 30 seconds |
| A new build misbehaves | **System > Roll back update**, or `oos-update rollback --execute` |
| Analyser view will not go away | The three dots at the top right, then Exit |
| Ethernet at 10 Mbit/s after a cold start, 100 after replugging | From 0.1.6 OOS negotiates again by itself; else replug. Please report it with `/tmp/oos-ui.log` |
| Files dated 2013 | The scope has no clock battery: **System > Date and time** (from 0.1.6) |
| The web remote makes the scope's screen slow | Fixed in 0.1.6 |
| An update from 0.1.6 says the image brings another device tree | Install 0.1.7 first, then the latest ([[Updates and Going Back]]) |
| Nothing helps | Write your card image to the second card again ([On a second card](https://github.com/dekajoule/oos/wiki/Installation#on-a-second-card), step 2) |

A start with an overclock that does not finish sets the setting aside by itself.

Serial console: UART2, 115200 8N1, verified on the unit (the console of OOS's kernel). The stock loader also reports
there why it refuses an image ([docs/boot.md](https://github.com/dekajoule/oos/blob/main/docs/boot.md)).

When you report a problem, please add the build from **System > About OOS** and `/tmp/oos-boot.status`.
