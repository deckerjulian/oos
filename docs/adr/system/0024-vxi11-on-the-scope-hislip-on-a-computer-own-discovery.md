# ADR 0024: VXI-11 on the scope, HiSLIP on a computer; found by mDNS / DNS-SD and LXI identification

- Status: accepted
- Date: 2026-11-15
- Category: system (touches code: ADR 0022)

## Context

VISA finds and opens an instrument in two ways over the LAN: VXI-11 (`TCPIP::<host>::INSTR`, ONC RPC behind a
portmapper on port 111) and HiSLIP (`TCPIP::<host>::hislip0::INSTR`, TCP 4880). Programs that do not use VISA open the
raw socket on 5555. A computer finds instruments by DNS-SD (`_vxi-11._tcp`, `_scpi-raw._tcp`, `_lxi._tcp`,
`_hislip._tcp`) and reads the LXI identification XML.

On the scope nothing else uses port 111. On a computer it is privileged (Linux, macOS) or taken by the system's own
RPC services, and a second portmapper would clash with them. The image has no rpcbind and no avahi, and ADR 0013
keeps further dependencies out of what runs on the scope.

## Decision

- The scope serves VXI-11 with its own portmapper (TCP and UDP 111, GETPORT only) and core channel (TCP 9009). It does
  not serve HiSLIP.
- The desktop build serves HiSLIP (TCP 4880, or `--hislip-port`) and leaves VXI-11 closed.
- Both serve the raw socket (5555) and the web remote (8080) with `GET /lxi/identification`.
- Discovery is written in the tree: on the scope an mDNS responder with DNS-SD (`ui/src/mdns.rs`, RFC 6762 / 6763) in
  a thread of its own, under a name from model and serial (`dho924s-204527.local`); on a computer the announcer
  (`ui/src/announce.rs`). Only a service that is listening is announced.

## Alternatives

- **VXI-11 everywhere:** on a computer it needs root or clashes with the system's portmapper.
- **HiSLIP on the scope as well:** possible, but VXI-11 is what older VISA installations and LXI tools expect; two
  transports on the scope double what has to be checked on the unit. Open for a later record.
- **rpcbind and avahi in the image:** two daemons and their libraries for a few hundred lines of protocol; against
  ADR 0013.

## Consequences

- VISA reaches the scope at `TCPIP::<name>.local::INSTR`, a computer at its `hislip0` resource; a script that needs one
  resource string for both uses the raw socket.
- The mDNS responder announces A records only: the services listen on IPv4 unless `net.ipv6 = on`.
- How it is known (ADR 0015): the raw socket and VXI-11 servers ran on a DHO924S over Ethernet (CHANGELOG 0.1.6);
  a VISA session over VXI-11 on the unit is not recorded in the tree. The mDNS responder is verified on the host only
  (macOS, `dns-sd`), not yet on hardware. HiSLIP runs on a computer only.
- Worth a second look if HiSLIP becomes the transport VISA prefers, or if port 111 is needed for something else on the
  scope.

## References

`ui/src/vxi11.rs`, `ui/src/hislip.rs`, `ui/src/mdns.rs`, `ui/src/announce.rs`, `ui/src/main.rs` (`lan_servers`,
`mdns_services`), [network.md](../../network.md) 4 and 4.1, [scpi.md](../../scpi.md), ADR 0013, ADR 0022.
