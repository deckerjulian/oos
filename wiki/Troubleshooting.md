# Troubleshooting

Your scope can always be recovered as long as you have the image of your original card ([[Installation]], step 1):
the worst case is writing it to a card again. The fixes with `boot/switch.py` and `boot/card-install.py` need the card
in a reader, so you have to open the scope again (`diskutil unmountDisk` first).

| What you see | What to do |
|---|---|
| The stock system starts instead of OOS | One-shot mode ([[Installation]], How OOS starts), or the loader refused the OOS image and used the fallback. Check with `boot/switch.py show`, then select OOS again. If that repeats, rebuild or download the image again |
| Rigol logo stays, nothing else, on every start | The kernel hangs before OOS runs: `sudo python3 boot/switch.py stock /dev/rdiskM --execute`, then install a working image |
| Black screen; a Rockchip USB device (2207:330c) appears | The loader refused `recovery` and has no fallback: `switch.py stock`. If needed, `sudo python3 boot/card-install.py restore /dev/rdiskM --backup backups/recovery-<date>.bin --execute` |
| Start-up screen stops at a step | A hardware step failed. Read the step on the screen. Logs are in `/tmp/oos-boot.status` and `/tmp/oos-cold.log` over telnet or SSH |
| "Start-up stopped" after a quick restart | OOS needs a real power cycle: off at the mains, wait 30 seconds |
| A new build misbehaves | **System > Roll back update**, or `oos-update rollback --execute` |
| Analyser view will not go away | The three dots at the top right, then Exit |
| Ethernet at 10 Mbit/s after a cold start, 100 after replugging | From 0.1.6 OOS negotiates again by itself; else replug. Please report it with `/tmp/oos-ui.log` |
| Files dated 2013 | The scope has no clock battery: **System > Date and time** (from 0.1.6) |
| The web remote makes the scope's screen slow | Fixed in 0.1.6 |
| An update from 0.1.6 says the image brings another device tree | Install 0.1.7 first, then the latest ([[Updates and Going Back]]) |
| Nothing helps | Write your card image to the second card again ([[Installation]], step 2) |

A start with an overclock that does not finish sets the setting aside by itself.

Serial console: UART2, 115200 8N1. The stock loader also reports there why it refuses an image.

When you report a problem, please add the build from **System > About OOS** and `/tmp/oos-boot.status`.
