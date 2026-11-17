# Troubleshooting

Your scope can always be recovered as long as you have the image from the [[Installation]]: the worst case is writing it to a card again.

| What you see | What to do |
|---|---|
| The stock system starts instead of OOS | One-shot mode, or the loader refused OOS. Check with `boot/switch.py show`, then select OOS again |
| Start-up screen stops at a step | Read the step on the screen. Logs are in `/tmp/oos-boot.status` and `/tmp/oos-cold.log` over telnet |
| "Start-up stopped" after a quick restart | OOS needs a real power cycle: off at the mains, wait 30 seconds |
| A new build misbehaves | **System > Roll back update** |
| Analyser view will not go away | The three dots at the top right, then Exit |
| Rigol logo stays on every start, or a black screen | See [If something goes wrong](https://github.com/dekajoule/oos#if-something-goes-wrong) |
| Ethernet at 10 Mbit/s after a cold start, 100 after replugging | From 0.1.6 OOS negotiates again by itself; else replug. Please report it with `/tmp/oos-ui.log` |
| Files dated 2013 | The scope has no clock battery: **System > Date and time** (from 0.1.6) |
| The web remote makes the scope's screen slow | Fixed in 0.1.6 |
| An update from 0.1.6 says the image brings another device tree | Install 0.1.7 first, then the latest ([[Updates and Going Back]]) |
| Nothing helps | Write your card image to the second card again |

A start with an overclock that does not finish sets the setting aside by itself.

When you report a problem, please add the build from **System > About OOS** and `/tmp/oos-boot.status`.
