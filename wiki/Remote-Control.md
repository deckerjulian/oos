# Remote control

> **Apart from SSH, OOS has no authentication.** Everything else here is open to anyone who can reach the scope.
> Keep it on a trusted bench network and never connect it directly to the internet.

| Interface | Port | Purpose |
|---|---|---|
| Web remote | 8080 | live screen, keys, knobs, touch in the browser; saves screenshots and CSV to your computer |
| SCPI over TCP | 5555 | [[SCPI and VXI-11]]: the DHO800/DHO900 subset and OOS extensions; RAW reads retained native CH memory, and references retain their own native timing |
| VXI-11 | 111, 9009 | on the scope, where port 111 is free: VISA `TCPIP::<ip>::INSTR` |
| HiSLIP | 4880 | on a computer only: the same SCPI interpreter, `TCPIP::<ip>::hislip0::INSTR` |
| LXI identification | 8080 | `/lxi/identification` |
| MCP | 8080 | `POST /mcp`: tools `scpi` and `screenshot` for AI assistants |
| SSH | 22 | root shell with the scope's own password or a trusted key, off by default ([below](#ssh)) |
| mDNS, DNS-SD | 5353 (UDP) | the scope by name, `http://dho924s-123456.local:8080`, and its services for browsing ([below](#ethernet)): the web remote (`_http._tcp`, path `/`), LXI discovery (`_lxi._tcp`, path `/lxi/identification`), the SCPI socket and VXI-11; on a computer (`--serve`) HiSLIP instead of VXI-11 |

None of them slows the scope down: they are read without waiting, and the live picture of the web remote is made on
the slow cores with spare time only. All of them run over Ethernet and over [WLAN](#wlan).

## Web remote

Open `http://<scope>:8080/`. Click on the picture to touch the screen, drag to swipe. It scales with the window, so it
also works on a phone. The buttons at the bottom save a screenshot or the last record to your computer, as CSV or
binary; CSV takes records up to 2 million points, larger ones go as binary, through Storage on the scope or over SCPI.
The Update link leads to the update page ([[Updates and Going Back]]).
Every address of the web remote, for a script or a program of your own:
[docs/http.md](https://github.com/dekajoule/oos/blob/main/docs/http.md).

The live picture is a PNG (about 90 to 115 KB, depending on what the screen shows). **Live picture format** can switch
it to the uncompressed 1.8 MB BMP, or to a JPEG (below). The choice is kept in the browser. The PNG is made off the
scope's frame loop, only of a changed screen, up to 20 a second (about 13 reach a browser over Ethernet); the page gets
each new picture as soon as it is made, and while the screen stands still it gets "not modified" instead. The scope's
own screen does not slow down for it. Pause stops the pictures, and a tab in the background stops them too.

**From 0.1.6** the web remote, SCPI and VXI-11 no longer slow the scope down: on a DHO924S its screen stays at 30 frames a second with two browsers open (0.1.5: 16 with one), and the browser gets about 13 new pictures a second over Ethernet instead of 4 to 5. A script that keeps an SCPI connection open costs no frames either.

**From 0.1.7** **System > Network > Remote services** switches the web remote, SCPI and VXI-11 on or off, each on its
own. **From 0.1.8** the live picture can be a JPEG made by the scope's video encoder (the button JPEG next
to PNG): about 75 KB a picture against 115 KB, at under a third of the processor time of a PNG; text is a little less
sharp, and screenshots stay PNG.

## Ethernet

Tap the network box at the top right: it shows the addresses of Ethernet and WLAN and the address of the web remote.

**From 0.1.12 the scope has a name:** `http://dho924s-123456.local:8080` (model and serial number) reaches it on any computer that speaks mDNS (macOS, Linux with Avahi, Windows 10 and later), also when it only has a link-local address on a cable straight to the computer. Computers that browse find the web remote, SCPI and VXI-11 (DNS-SD). **System > Network > Remote services** switches mDNS off and changes the Name, which is also the host name the scope sends to DHCP, so a router lists it. When another device has the name already, the scope takes `<name>-2`.

The scope takes an address by DHCP. Without a DHCP answer it takes a link-local address, 169.254.x.y (from 0.1.9; before, 192.168.177.64), and without a cable none at all. **System > Network** shows the speed of the link. From 0.1.6, a link that comes up at 10 Mbit/s gets one more negotiation by itself (**10 Mbit retry**, on by default). If your scope still ends at 10 Mbit/s after a cold start, please report it with `/tmp/oos-ui.log`.

## WLAN

Put an RTL8188EU stick or a TP-Link Archer TX10UB Nano (RTL8851BU, 2.4 and 5 GHz) into the front USB port, then **System > Network > Find WLAN networks**, tap a network and enter
the password on the on-screen keyboard ([[First Steps]] has how to correct inside it). It joins by itself from then on:
open, WPA and WPA2 personal networks. Everything open on Ethernet is then open on that network too. Details:
[docs/wlan.md](https://github.com/dekajoule/oos/blob/main/docs/wlan.md). On the DHO924S of the author the scope has
joined a home network with a password and worked over it for days.

## SSH

SSH is the only shell on the network. **From 0.1.14** there is no telnet any more: nothing listens on port 23, and
the telnet switch is gone from Remote services (an old setting of it is ignored). The serial console keeps its shell.

SSH is off until you switch it on, and it listens only to the addresses you allow. The first login:

1. **System > Network > Remote services > Allowed**: pick the network the scope is on, or Other for your computer's
   prefix (`192.168.1.0/24`).
2. **SSH** on.
3. **SSH settings > Show password** shows `ssh root@<address>`, the password and the host key's fingerprint. Run that
   command on your computer, check that it shows the same fingerprint, answer `yes` and type the password.
4. For a login without the password: `ssh-copy-id root@<address>`, or import a key from a USB stick on the SSH settings
   page. Then Password login can go off.

The password, the host key and the keys stay over updates and power cycles. `scp` and `sftp` copy files. Details:
[docs/ssh.md](https://github.com/dekajoule/oos/blob/main/docs/ssh.md) and
[docs/network.md](https://github.com/dekajoule/oos/blob/main/docs/network.md) 7.

## SCPI

[[SCPI and VXI-11]] describes the ways in, the syntax and every command family, with Python examples. From 0.1.9 also the counter: `:COUNter:ENABle|SOURce|MODE|NDIGits`, `:COUNter:CURRent?`, `:COUNter:TOTalize:CLEar`.
