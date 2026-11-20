Fixed:

- SSH listens on IPv4 only while `net.ipv6` is off (the default), as the web remote, SCPI and VXI-11 do. It listened on
  IPv6 too, where only the firewall kept connections out ([docs/network.md](docs/network.md#8-ipv6)).
