# Security

**OOS has no authentication, by design for now.** Telnet gives a root shell without a password. SCPI, VXI-11, HiSLIP, the web remote,
the LAN announcement, MCP and the update page are open to everyone who can reach the scope, over Ethernet and over WLAN. The community signing key
is public, so anyone on that network can also install an update. Keep the scope on a trusted bench network and never
expose it to the internet. None of this is a vulnerability to report: it is listed in the
[disclaimer](README.md#disclaimer).

Signed updates protect against damaged packages, not against an attacker: see [docs/update.md](docs/update.md).

Web pages on other sites are kept out: the web remote refuses a request whose `Host` is a foreign name, whose `Origin` is
not the scope, or that the browser marks as cross-site. That needs a browser that sends `Sec-Fetch-Site` (all current
ones). It does nothing against someone on your network, which is the case above.

USB sticks count as foreign data: they are mounted `nodev,nosuid,noexec`, and a save refuses links and anything that is not
a regular file.

Please do report, through GitHub's private vulnerability reporting:

- ways to damage a scope or lose its calibration through normal use or a damaged file;
- anything that writes the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and `parameter` partitions;
- a crash of `oos-ui` from network input or a file.
