// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Network services belong to the same UI session as the desktop window.

use crate::{announce, cmd, gfx::Canvas, hislip, http, scpi, ui::Ui, vxi11};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

pub struct Services {
    web: http::Http,
    scpi: scpi::Scpi,
    vxi: vxi11::Server,
    hislip: hislip::Server,
    announce: announce::Announcer,
    next_status: Instant,
}

impl Services {
    pub fn open(args: &[String]) -> Self {
        let (web, scpi, vxi, hislip, announce) = crate::lan(false, args);
        Self { web, scpi, vxi, hislip, announce, next_status: Instant::now() }
    }

    pub fn poll(&mut self, ui: &mut Ui, cv: &Canvas) {
        self.scpi.poll(ui, cv);
        self.vxi.poll_async(&mut |program| self.scpi.poll_program(ui, cv, program));
        self.hislip.poll_async(&mut |program| self.scpi.poll_program(ui, cv, program));
        for command in self.web.poll(ui, cv, &mut self.scpi) {
            cmd::run(ui, &command);
        }
        self.announce.poll();
        if Instant::now() < self.next_status { return; }
        self.next_status = Instant::now() + Duration::from_secs(1);
        let addresses = self.announce.local_addresses();
        // A UDP connect selects the default route without transmitting any packet.
        let routed = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok().and_then(|socket| {
            socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
            match socket.local_addr().ok()?.ip() { std::net::IpAddr::V4(ip) => Some(ip.octets()), _ => None }
        }).filter(|ip| addresses.contains(ip));
        let primary = routed.or_else(|| addresses.first().copied());
        ui.net = primary.map(|a| Ipv4Addr::from(a).to_string()).unwrap_or_default();
        ui.net_if = "eth0".into();
        ui.eth_link = primary.is_some();
        ui.net_ifs = primary.into_iter().map(|ip| ("eth0".into(), Ipv4Addr::from(ip).to_string()))
            .chain(addresses.iter().copied().filter(|a| Some(*a) != primary).enumerate().map(|(i, ip)| (format!("LAN{}", i + 2), Ipv4Addr::from(ip).to_string()))).collect();
        ui.lan_web_port = self.web.port();
        let bound = |port: Option<u16>| port.map_or_else(|| "unavailable".into(), |p| p.to_string());
        ui.lan_status = format!("SCPI {} / HiSLIP {} / mDNS {}", bound(self.scpi.port()), bound(self.hislip.port()), if self.announce.available() { "on" } else { "unavailable" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;

    fn message(stream: &mut TcpStream) -> hislip::Message {
        let mut bytes = Vec::new();
        let mut chunk = [0; 512];
        loop {
            if let Some(message) = hislip::take_message(&mut bytes).unwrap() { return message; }
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
    }

    #[test]
    fn desktop_network_services_share_the_window_session() {
        let args: Vec<String> = ["--web-port", "0", "--scpi-port", "0", "--hislip-port", "0"].into_iter().map(str::to_string).collect();
        let mut services = Services::open(&args);
        let raw_port = services.scpi.port().unwrap();
        let hislip_port = services.hislip.port().unwrap();
        let web_port = services.web.port().unwrap();
        let client = std::thread::spawn(move || {
            let mut raw = TcpStream::connect(("127.0.0.1", raw_port)).unwrap();
            raw.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            raw.write_all(b":TIM:SCAL 0.00005\n:TIM:SCAL?\n").unwrap();
            let mut response = String::new();
            BufReader::new(raw).read_line(&mut response).unwrap();
            assert!((response.trim().parse::<f64>().unwrap() - 50e-6).abs() < 1e-10);

            let mut h = TcpStream::connect(("127.0.0.1", hislip_port)).unwrap();
            h.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            h.write_all(&hislip::encode(0, 0, 0x0100_0000, b"hislip0")).unwrap();
            assert_eq!(message(&mut h).typ, 1);
            h.write_all(&hislip::encode(7, 0, 1, b":TIM:SCAL?\n")).unwrap();
            let response = message(&mut h);
            assert_eq!(response.typ, 7);
            assert!((String::from_utf8(response.data).unwrap().trim().parse::<f64>().unwrap() - 50e-6).abs() < 1e-10);

            let mut web = TcpStream::connect(("127.0.0.1", web_port)).unwrap();
            web.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            web.write_all(b"GET /lxi/identification HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut response = String::new();
            web.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
            assert!(response.contains("hislip0"));
        });
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let deadline = Instant::now() + Duration::from_secs(6);
        while !client.is_finished() && Instant::now() < deadline {
            services.poll(&mut ui, &cv);
            std::thread::sleep(Duration::from_millis(2));
        }
        client.join().unwrap();
        assert!((ui.m.time_per_div() as f64 - 50e-6).abs() < 1e-10);
        assert_eq!(ui.lan_web_port, Some(web_port));
        assert!(ui.lan_status.contains(&hislip_port.to_string()));
    }
}
