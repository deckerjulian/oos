// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The protocol of the responder without sockets: the configuration and interfaces it is given, probing, announcing,
//! answering questions, conflicts and goodbyes (`Responder`). It takes packets and times and returns packets to send,
//! so the tests in `mdns.rs` drive it directly; a new kind of answer goes into `Responder::answer` and `additional`.

use super::*;

// ------------------------------------------------------------------------------------------------ the responder

/// A service of the scope: `_http._tcp` and its port.
#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    pub kind: String,
    pub port: u16,
    pub txt: Vec<String>,
}

/// What the responder is to announce.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub on: bool,
    /// the host label, `dho924s-123456` (`name_ok`)
    pub name: String,
    /// the instance label of the services, `Rigol DHO924S OOS (123456)`
    pub instance: String,
    pub services: Vec<Service>,
    /// the services listen on IPv6 too (`net6::enabled`): AAAA records for the interface's IPv6 addresses
    pub v6: bool,
}

/// An interface with its addresses (only those that are up, running and take multicast).
#[derive(Clone, Debug, PartialEq)]
pub struct Iface {
    pub index: u32,
    pub name: String,
    /// address and netmask
    pub v4: Option<(Ipv4Addr, Ipv4Addr)>,
    pub v6: Vec<Ipv6Addr>,
}

/// Where a packet goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Dest {
    /// 224.0.0.251:5353 on the interface
    Group4,
    /// [ff02::fb]:5353 on the interface
    Group6,
    To(SocketAddr),
}

/// A packet to send on an interface.
#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub iface: u32,
    pub dest: Dest,
    pub pkt: Vec<u8>,
}

/// What the responder does, for the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Off,
    /// on, but no interface with an address
    NoNetwork,
    Probing,
    Running,
    /// the socket could not be opened (port 5353 taken, no multicast)
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Phase {
    Off,
    /// on without an interface
    Idle,
    Probe {
        sent: u32,
        at: f64,
    },
    Announce {
        sent: u32,
        at: f64,
    },
    Run,
}

/// Which of the scope's unique names a record is about.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Host,
    Instance,
}

pub struct Responder {
    pub(super) cfg: Config,
    ifaces: Vec<Iface>,
    /// 1: the name as set, 2: `<name>-2` ... (after conflicts)
    host_n: u32,
    inst_n: u32,
    pub(super) phase: Phase,
    /// times of the last conflicts (RFC 6762 §8.1: 15 in 10 s and it waits 5 s)
    conflicts: Vec<f64>,
    seed: u64,
    /// last multicast of a record per interface: (interface, name in lower case, type), time
    sent: Vec<((u32, String, u16), f64)>,
    /// fingerprints of the packets sent in the last seconds, and when (`note`)
    mine: Vec<(u64, f64)>,
    /// lines for the log
    pub log: Vec<String>,
}

impl Responder {
    pub fn new(seed: u64) -> Responder {
        Responder {
            cfg: Config::default(),
            ifaces: Vec::new(),
            host_n: 1,
            inst_n: 1,
            phase: Phase::Off,
            conflicts: Vec::new(),
            seed: seed | 1,
            sent: Vec::new(),
            mine: Vec::new(),
            log: Vec::new(),
        }
    }

    fn rand(&mut self) -> f64 {
        // xorshift64: enough to spread the first probe of several units over 250 ms
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The host label in use: the name, after conflicts with `-2`, `-3` ...
    pub fn host_label(&self) -> String {
        if self.host_n <= 1 {
            return self.cfg.name.clone();
        }
        let suffix = format!("-{}", self.host_n);
        let mut base = self.cfg.name.clone();
        base.truncate(NAME_MAX - suffix.len());
        format!("{}{}", base.trim_end_matches('-'), suffix)
    }

    pub(super) fn inst_label(&self) -> String {
        if self.inst_n <= 1 {
            return self.cfg.instance.clone();
        }
        let suffix = format!(" ({})", self.inst_n);
        let mut base = self.cfg.instance.clone();
        while base.len() + suffix.len() > NAME_MAX {
            base.pop();
        }
        format!("{}{}", base, suffix)
    }

    fn host(&self) -> Name {
        vec![self.host_label(), "local".into()]
    }

    fn service_type(kind: &str) -> Name {
        let mut n: Name = kind.split('.').map(str::to_string).collect();
        n.push("local".into());
        n
    }

    fn instance(&self, kind: &str) -> Name {
        let mut n = vec![self.inst_label()];
        n.extend(Self::service_type(kind));
        n
    }

    /// The scope's names that only it may have.
    fn unique_names(&self) -> Vec<(Name, Owner)> {
        let mut v = vec![(self.host(), Owner::Host)];
        for s in &self.cfg.services {
            v.push((self.instance(&s.kind), Owner::Instance));
        }
        v
    }

    /// Every record of the scope on an interface.
    pub fn records(&self, ifc: &Iface) -> Vec<Rec> {
        let host = self.host();
        let mut v = Vec::new();
        if let Some((a, _)) = ifc.v4 {
            v.push(Rec::new(host.clone(), Data::A(a), TTL_HOST, true));
            v.push(Rec::new(reverse4(a), Data::Ptr(host.clone()), TTL_HOST, true));
        }
        // AAAA only when the services listen on IPv6 (`Config::v6`): a computer that gets an IPv6 address tries it
        // first (curl took seconds on a Mac before it fell back to IPv4); without, questions over IPv6 get the A
        // record and an NSEC
        if self.cfg.v6 {
            v.extend(ifc.v6.iter().map(|&a| Rec::new(host.clone(), Data::Aaaa(a), TTL_HOST, true)));
        }
        for s in &self.cfg.services {
            let ty = Self::service_type(&s.kind);
            let inst = self.instance(&s.kind);
            v.push(Rec::new(
                dname(&["_services", "_dns-sd", "_udp", "local"]),
                Data::Ptr(ty.clone()),
                TTL_OTHER,
                false,
            ));
            v.push(Rec::new(ty, Data::Ptr(inst.clone()), TTL_OTHER, false));
            v.push(Rec::new(inst.clone(), Data::Srv { port: s.port, target: host.clone() }, TTL_HOST, true));
            v.push(Rec::new(inst, Data::Txt(s.txt.clone()), TTL_OTHER, true));
        }
        v
    }

    pub fn state(&self) -> State {
        match self.phase {
            Phase::Off => State::Off,
            Phase::Idle => State::NoNetwork,
            Phase::Probe { .. } => State::Probing,
            Phase::Announce { .. } | Phase::Run => State::Running,
        }
    }

    fn published(&self) -> bool {
        matches!(self.phase, Phase::Announce { .. } | Phase::Run)
    }

    /// Probing from the start: after a change, a conflict, or a lost tie.
    fn restart(&mut self, now: f64, delay: f64) {
        if !self.cfg.on {
            self.phase = Phase::Off;
            return;
        }
        if self.ifaces.is_empty() {
            self.phase = Phase::Idle;
            return;
        }
        self.conflicts.retain(|t| now - t < 10.0);
        let wait = if self.conflicts.len() >= 15 { 5.0 } else { delay + self.rand() * PROBE_GAP };
        self.phase = Phase::Probe { sent: 0, at: now + wait };
    }

    /// Goodbyes for the records of `old` (records per interface before a change) that are not there any more.
    fn goodbyes(&self, old: &[(u32, Vec<Rec>)]) -> Vec<Out> {
        let mut out = Vec::new();
        for (idx, recs) in old {
            let Some(ifc) = self.ifaces.iter().find(|i| i.index == *idx) else { continue };
            let new = if self.cfg.on { self.records(ifc) } else { Vec::new() };
            let gone: Vec<Rec> = recs.iter().filter(|r| !new.iter().any(|n| n.same_rec(r))).cloned().collect();
            if gone.is_empty() {
                continue;
            }
            for pkt in responses(0, &[], &gone, &[], Style::Goodbye) {
                for dest in Self::groups(ifc) {
                    out.push(Out { iface: ifc.index, dest, pkt: pkt.clone() });
                }
            }
        }
        out
    }

    fn snapshot(&self) -> Vec<(u32, Vec<Rec>)> {
        if !self.published() {
            return Vec::new();
        }
        self.ifaces.iter().map(|i| (i.index, self.records(i))).collect()
    }

    /// A new configuration: goodbyes for what goes away, then probing again (nothing when it is the same).
    pub fn set_config(&mut self, now: f64, cfg: Config) -> Vec<Out> {
        if cfg == self.cfg {
            return Vec::new();
        }
        let old = self.snapshot();
        if cfg.name != self.cfg.name {
            self.host_n = 1;
        }
        if cfg.instance != self.cfg.instance {
            self.inst_n = 1;
        }
        self.cfg = cfg;
        let out = self.goodbyes(&old);
        self.restart(now, 0.0);
        self.note(now, out)
    }

    /// The interfaces as they are now (sorted by index): probing again on any change. An address that went away from
    /// an interface still there gets its goodbye.
    pub fn set_ifaces(&mut self, now: f64, mut ifaces: Vec<Iface>) -> Vec<Out> {
        ifaces.sort_by_key(|i| i.index);
        if ifaces == self.ifaces {
            return Vec::new();
        }
        let old = self.snapshot();
        self.ifaces = ifaces;
        let out = self.goodbyes(&old);
        self.restart(now, 0.0);
        self.note(now, out)
    }

    fn groups(ifc: &Iface) -> Vec<Dest> {
        let mut v = Vec::new();
        if ifc.v4.is_some() {
            v.push(Dest::Group4);
        }
        if !ifc.v6.is_empty() {
            v.push(Dest::Group6);
        }
        v
    }

    /// When `tick` has something to do next.
    pub fn next_wake(&self) -> Option<f64> {
        match self.phase {
            Phase::Probe { at, .. } | Phase::Announce { at, .. } => Some(at),
            _ => None,
        }
    }

    /// Probes and announcements that are due.
    pub fn tick(&mut self, now: f64) -> Vec<Out> {
        let out = self.due(now);
        self.note(now, out)
    }

    fn due(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        if let Phase::Probe { sent, at } = self.phase {
            if now < at {
                return out;
            }
            if sent < PROBES {
                for ifc in &self.ifaces {
                    let pkt = self.probe(ifc);
                    for dest in Self::groups(ifc) {
                        out.push(Out { iface: ifc.index, dest, pkt: pkt.clone() });
                    }
                }
                self.phase = Phase::Probe { sent: sent + 1, at: now + PROBE_GAP };
                return out;
            }
            self.phase = Phase::Announce { sent: 0, at: now };
        }
        if let Phase::Announce { sent, at } = self.phase {
            if now < at {
                return out;
            }
            for ifc in &self.ifaces {
                for pkt in responses(0, &[], &self.records(ifc), &[], Style::Normal) {
                    for dest in Self::groups(ifc) {
                        out.push(Out { iface: ifc.index, dest, pkt: pkt.clone() });
                    }
                }
            }
            if sent == 0 {
                let ifs: Vec<String> = self.ifaces.iter().map(|i| i.name.clone()).collect();
                let services: Vec<&str> = self.cfg.services.iter().map(|s| s.kind.as_str()).collect();
                self.log.push(format!(
                    "mDNS: {}.local on {}, services {}",
                    self.host_label(),
                    ifs.join(" "),
                    if services.is_empty() { "none".into() } else { services.join(" ") }
                ));
            }
            self.phase = if sent + 1 >= ANNOUNCES {
                Phase::Run
            } else {
                Phase::Announce { sent: sent + 1, at: now + ANNOUNCE_GAP }
            };
        }
        out
    }

    /// A probe (RFC 6762 §8.1): QU questions of type ANY for the unique names, the records in the authority section.
    pub(super) fn probe(&self, ifc: &Iface) -> Vec<u8> {
        let mut w = Wr::new(0, 0);
        let names = self.unique_names();
        for (n, _) in &names {
            w.question(n, T_ANY, CLASS_IN | TOP);
        }
        let recs: Vec<Rec> =
            self.records(ifc).into_iter().filter(|r| r.unique && names.iter().any(|(n, _)| same(n, &r.name))).collect();
        for r in &recs {
            w.rec(r, r.ttl, false);
        }
        w.counts([names.len(), 0, recs.len(), 0]);
        w.b
    }

    /// Keeps a fingerprint of each packet sent for a few seconds: the scope's own packets come back (multicast loop, a
    /// second interface on the same network) and are neither questions nor conflicts. By the bytes, not the sender: a
    /// host's own responder (macOS) sends from the same address and port and is answered.
    fn note(&mut self, now: f64, out: Vec<Out>) -> Vec<Out> {
        self.mine.retain(|(_, t)| now - t < 5.0);
        for o in &out {
            self.mine.push((fnv(&o.pkt), now));
        }
        out
    }

    /// A unicast sender on the link of the interface (RFC 6762 §11: answers only there).
    fn on_link(ifc: &Iface, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(a) => {
                let ll = a.octets()[0] == 169 && a.octets()[1] == 254;
                ll || ifc.v4.is_some_and(|(x, m)| u32::from(x) & u32::from(m) == u32::from(a) & u32::from(m))
            }
            IpAddr::V6(a) => {
                (a.segments()[0] & 0xffc0) == 0xfe80 || ifc.v6.iter().any(|x| x.segments()[..4] == a.segments()[..4])
            }
        }
    }

    fn conflict(&mut self, now: f64, owner: Owner, why: &str) {
        self.conflicts.push(now);
        if matches!(self.phase, Phase::Probe { .. }) {
            let old = if owner == Owner::Host {
                format!("{}.local", self.host_label())
            } else {
                format!("\"{}\"", self.inst_label())
            };
            match owner {
                Owner::Host => self.host_n += 1,
                Owner::Instance => self.inst_n += 1,
            }
            let new = if owner == Owner::Host {
                format!("{}.local", self.host_label())
            } else {
                format!("\"{}\"", self.inst_label())
            };
            self.log.push(format!("mDNS: {} is taken ({}), now {}", old, why, new));
            self.restart(now, 0.0);
        } else {
            self.log.push(format!("mDNS: another host answers for a name of the scope ({}), probing again", why));
            self.restart(now, 0.0);
        }
    }

    /// A packet from `src` on interface `iface`; `to_group`: it was sent to the multicast group (not to the scope's
    /// own address). Gives the answers to send.
    pub fn receive(&mut self, now: f64, pkt: &[u8], src: SocketAddr, iface: u32, to_group: bool) -> Vec<Out> {
        let Some(m) = parse(pkt) else { return Vec::new() };
        // a packet of the host itself can come in on loopback (macOS): then the interface of its address
        let by_addr = |i: &&Iface| match src.ip() {
            IpAddr::V4(a) => i.v4.is_some_and(|(x, m)| u32::from(x) & u32::from(m) == u32::from(a) & u32::from(m)),
            IpAddr::V6(a) => i.v6.contains(&a),
        };
        let Some(ifc) =
            self.ifaces.iter().find(|i| i.index == iface).or_else(|| self.ifaces.iter().find(by_addr)).cloned()
        else {
            return Vec::new();
        };
        if !self.cfg.on || !m.standard() {
            return Vec::new();
        }
        if self.mine.iter().any(|(h, _)| *h == fnv(pkt)) {
            return Vec::new();
        }
        if m.is_response() {
            // answers come from port 5353 (RFC 6762 §11)
            if src.port() == PORT {
                self.check_answers(now, &m);
            }
            return Vec::new();
        }
        if !m.ns.is_empty() && matches!(self.phase, Phase::Probe { .. }) {
            self.tie_break(now, &m, &ifc);
        }
        if !self.published() || (!to_group && !Self::on_link(&ifc, src.ip())) {
            return Vec::new();
        }
        let out = self.answer(now, &m, src, &ifc, to_group);
        self.note(now, out)
    }

    /// A response from another host with a record of one of the scope's unique names that the scope does not have:
    /// a conflict (RFC 6762 §8.1 while probing, §9 after).
    fn check_answers(&mut self, now: f64, m: &Msg) {
        if matches!(self.phase, Phase::Off | Phase::Idle) {
            return;
        }
        let ours: Vec<Rec> = self.ifaces.iter().flat_map(|i| self.records(i)).collect();
        for r in m.an.iter().chain(&m.ar) {
            if r.ttl == 0 || r.class != CLASS_IN {
                continue;
            }
            let Some((_, owner)) = self.unique_names().into_iter().find(|(n, _)| same(n, &r.name)) else { continue };
            if ours.iter().any(|o| o.matches(r)) {
                continue;
            }
            let why = format!("{} type {}", r.name.join("."), r.rtype);
            return self.conflict(now, owner, &why);
        }
    }

    /// Two hosts probing for one name at once (RFC 6762 §8.2): the records of the authority sections compared, class,
    /// type and data in that order; the lower set loses and probes again after a second.
    fn tie_break(&mut self, now: f64, m: &Msg, ifc: &Iface) {
        let recs = self.records(ifc);
        for (name, _) in self.unique_names() {
            let mut theirs: Vec<(u16, u16, Vec<u8>)> =
                m.ns.iter().filter(|r| same(&r.name, &name)).map(|r| (r.class, r.rtype, r.canon.clone())).collect();
            if theirs.is_empty() {
                continue;
            }
            let mut ours: Vec<(u16, u16, Vec<u8>)> = recs
                .iter()
                .filter(|r| r.unique && same(&r.name, &name))
                .map(|r| (CLASS_IN, r.data.rtype(), r.data.canon()))
                .collect();
            theirs.sort();
            ours.sort();
            if ours < theirs {
                self.log.push(format!(
                    "mDNS: another host probes for {} too and wins the tie, probing again in 1 s",
                    name.join(".")
                ));
                self.conflicts.push(now);
                self.phase = Phase::Probe { sent: 0, at: now + 1.0 };
                return;
            }
        }
    }

    /// The answers to a query (RFC 6762 §6).
    fn answer(&mut self, now: f64, m: &Msg, src: SocketAddr, ifc: &Iface, to_group: bool) -> Vec<Out> {
        let recs = self.records(ifc);
        let uniq = self.unique_names();
        let mut an: Vec<Rec> = Vec::new();
        let mut all_qu = true;
        for q in &m.qs {
            if q.qclass != CLASS_IN && q.qclass != T_ANY {
                continue;
            }
            let hits: Vec<&Rec> = recs
                .iter()
                .filter(|r| same(&r.name, &q.name) && (q.qtype == T_ANY || q.qtype == r.data.rtype()))
                .collect();
            if hits.is_empty() {
                // a name of the scope without that type: say which types it has (§6.1)
                if uniq.iter().any(|(n, _)| same(n, &q.name)) {
                    let n = Self::nsec(&recs, &q.name);
                    if !an.iter().any(|a| a.same_rec(&n)) {
                        an.push(n);
                    }
                } else {
                    continue;
                }
            }
            all_qu &= q.qu;
            for r in hits {
                if !an.iter().any(|a| a.same_rec(r)) {
                    an.push(r.clone());
                }
            }
        }
        // known answers (§7.1): what the asker has with at least half the TTL is left out
        an.retain(|r| !m.an.iter().any(|k| r.matches(k) && k.ttl >= r.ttl / 2));
        if an.is_empty() {
            return Vec::new();
        }
        let ar = self.additional(&recs, &an);
        let legacy = src.port() != PORT;
        if legacy {
            return responses(m.id, &m.qs, &an, &ar, Style::Legacy)
                .into_iter()
                .map(|pkt| Out { iface: ifc.index, dest: Dest::To(src), pkt })
                .collect();
        }
        if all_qu || !to_group {
            return responses(0, &[], &an, &ar, Style::Normal)
                .into_iter()
                .map(|pkt| Out { iface: ifc.index, dest: Dest::To(src), pkt })
                .collect();
        }
        // multicast: a record sent in the last second is not sent again, except to defend against a probe (§6)
        self.sent.retain(|(_, t)| now - t < 1.0);
        if m.ns.is_empty() {
            an.retain(|r| {
                !self.sent.iter().any(|((i, n, t), _)| {
                    *i == ifc.index && *t == r.data.rtype() && *n == r.name.join(".").to_ascii_lowercase()
                })
            });
            if an.is_empty() {
                return Vec::new();
            }
        }
        for r in &an {
            self.sent.push(((ifc.index, r.name.join(".").to_ascii_lowercase(), r.data.rtype()), now));
        }
        let dest = if src.is_ipv4() { Dest::Group4 } else { Dest::Group6 };
        responses(0, &[], &an, &ar, Style::Normal)
            .into_iter()
            .map(|pkt| Out { iface: ifc.index, dest: dest.clone(), pkt })
            .collect()
    }

    /// The NSEC record of a name: the types the scope has for it.
    fn nsec(recs: &[Rec], name: &[String]) -> Rec {
        let mut types: Vec<u16> = recs.iter().filter(|r| same(&r.name, name)).map(|r| r.data.rtype()).collect();
        types.sort();
        types.dedup();
        Rec::new(name.to_vec(), Data::Nsec { next: name.to_vec(), types }, TTL_HOST, true)
    }

    /// What a resolver will ask next (RFC 6763 §12, RFC 6762 §6.2): for a service PTR its SRV, TXT and the host's
    /// addresses; for an SRV the addresses; for an address the other family's (or an NSEC when there is none).
    fn additional(&self, recs: &[Rec], an: &[Rec]) -> Vec<Rec> {
        let host = self.host();
        let mut want: Vec<Rec> = Vec::new();
        let addrs = |want: &mut Vec<Rec>| {
            want.extend(
                recs.iter().filter(|r| same(&r.name, &host) && matches!(r.data, Data::A(_) | Data::Aaaa(_))).cloned(),
            );
        };
        for a in an {
            match &a.data {
                Data::Ptr(target) => {
                    let inst: Vec<&Rec> = recs
                        .iter()
                        .filter(|r| same(&r.name, target) && matches!(r.data, Data::Srv { .. } | Data::Txt(_)))
                        .collect();
                    if !inst.is_empty() {
                        want.extend(inst.into_iter().cloned());
                        addrs(&mut want);
                    }
                }
                Data::Srv { .. } => addrs(&mut want),
                Data::A(_) | Data::Aaaa(_) => addrs(&mut want),
                _ => {}
            }
        }
        if want.iter().chain(an).any(|r| matches!(r.data, Data::A(_)))
            && !recs.iter().any(|r| matches!(r.data, Data::Aaaa(_)))
        {
            want.push(Self::nsec(recs, &host));
        }
        let mut out: Vec<Rec> = Vec::new();
        for r in want {
            if !an.iter().chain(out.iter()).any(|o| o.same_rec(&r)) {
                out.push(r);
            }
        }
        out
    }
}
