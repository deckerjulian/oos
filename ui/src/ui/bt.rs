// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Bluetooth: finding, pairing, connecting and forgetting mice and keyboards through a USB stick
//! (docs/bluetooth.md, ADR 0057). `/bin/oos-bt` runs the driver and bluetoothd; a worker thread here talks to
//! bluetoothd on the system bus (oos_hal::bluez), so that nothing in the frame loop waits on the radio. A keyboard's
//! passkey comes from the pairing agent and is shown in a dialog until the pairing ends. A paired device's input
//! reaches the UI through evdev like a USB one (crate::hid, ui/pointer.rs).

use super::service::Service;
use super::*;
use oos_hal::bluez::{Adapter, Bluez, Device, Prompt};
use oos_hal::dbus::Message;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// How often bluetoothd's state is read while the page is open.
const POLL: Duration = Duration::from_secs(2);
/// How long "Find devices" listens.
const SCAN: Duration = Duration::from_secs(10);
/// Lines of a device picker.
const LIST_MAX: usize = 18;
/// Characters of a device name in a picker line.
const NAME_MAX: usize = 22;

enum Job {
    Scan,
    Pair(String),
    Connect(String),
    Disconnect(String),
    Forget(String),
}

pub(super) enum Report {
    State(Link),
    Scanned,
    Prompt(Prompt),
    Done(Result<String, String>),
}

/// What the worker last saw.
#[derive(Clone, Debug, Default)]
pub(super) struct Link {
    /// a Bluetooth controller is there (`hci0`)
    pub stick: bool,
    /// bluetoothd answers on the bus
    pub daemon: bool,
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
}

#[derive(Default)]
pub(super) struct BtUi {
    pub link: Link,
    /// a scan or pairing is running (the text is for the toasts)
    pub busy: Option<&'static str>,
    /// the passkey or PIN shown during a pairing
    pub prompt: Option<Prompt>,
    /// the lines of the dialog that shows it: the end of the pairing closes this dialog, not another one
    prompt_lines: Option<Vec<String>>,
    /// what the last Find devices found, in the order of the picker (`Report::Scanned`)
    found: Vec<Device>,
    service: Service<Job, Report>,
}

fn short(name: &str) -> String {
    if name.chars().count() <= NAME_MAX {
        return name.to_string();
    }
    format!("{}\u{2026}", name.chars().take(NAME_MAX - 1).collect::<String>())
}

pub(super) fn stick_there() -> bool {
    std::path::Path::new("/sys/class/bluetooth/hci0").exists()
}

/// How many Bluetooth devices are connected as inputs: of the input devices (`/sys/class/input/inputN/id/bustype`
/// and `uniq`, which the kernel writes), those on bus 0005 with distinct addresses (one device can bring several
/// inputs, such as a keyboard with a touchpad). For the top bar: it needs no bluetoothd and no worker, which runs only
/// once the page was opened.
pub(super) fn inputs_connected(inputs: &[(String, String)]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for (bus, uniq) in inputs {
        if bus.trim() == "0005" && !seen.contains(&uniq.trim()) {
            seen.push(uniq.trim());
        }
    }
    seen.len()
}

/// The bus and address of every input device in sysfs, for `inputs_connected`.
pub(super) fn inputs() -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return Vec::new() };
    let read = |e: &std::fs::DirEntry, f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default();
    let is_input =
        |n: &str| n.strip_prefix("input").is_some_and(|k| !k.is_empty() && k.bytes().all(|c| c.is_ascii_digit()));
    dir.flatten()
        .filter(|e| is_input(&e.file_name().to_string_lossy()))
        .map(|e| (read(&e, "id/bustype"), read(&e, "uniq")))
        .collect()
}

/// A device's kind for a picker line.
fn kind(d: &Device) -> &'static str {
    match d.icon.as_str() {
        "input-mouse" => "mouse",
        "input-keyboard" => "keyboard",
        "input-tablet" => "tablet",
        "input-gaming" => "game controller",
        _ => "",
    }
}

/// The worker: one connection to bluetoothd, the state every `POLL`, the jobs of the page; agent calls are answered in
/// every wait and their prompts go to the page. The agent says yes only to the device of the Pair job that runs.
fn worker(jobs: Receiver<Job>, out: Sender<Report>) {
    let mut bz: Option<Bluez> = None;
    let tell_out = out.clone();
    let pairing: RefCell<Option<String>> = RefCell::new(None);
    let being_paired = &pairing;
    let mut h = move |m: &Message| {
        oos_hal::bluez::agent(m, being_paired.borrow().as_deref(), &mut |p| drop(tell_out.send(Report::Prompt(p))))
    };
    loop {
        let mut l = Link { stick: stick_there(), ..Link::default() };
        if l.stick && bz.is_none() {
            bz = Bluez::open().ok();
        }
        if !l.stick {
            bz = None;
        }
        if let Some(b) = bz.as_mut() {
            match b.state(&mut h) {
                Ok((a, d)) => {
                    l.daemon = true;
                    l.adapter = a;
                    l.devices = d;
                }
                // bluetoothd went (the stick was pulled, or it restarts): a new connection next time
                Err(_) => bz = None,
            }
        }
        let adapter = l.adapter.as_ref().map(|a| a.path.clone());
        if out.send(Report::State(l)).is_err() {
            return;
        }
        let job = match jobs.recv_timeout(POLL) {
            Ok(j) => j,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(b) = bz.as_mut() {
                    let _ = b.serve(Duration::from_millis(1), &mut h);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let (Some(b), Some(a)) = (bz.as_mut(), adapter) else {
            let _ = out.send(Report::Done(Err("Bluetooth is not ready yet".into())));
            continue;
        };
        let r = match job {
            Job::Scan => {
                // the devices are read while discovery still runs: bluetoothd clears their RSSI when it stops, and the
                // page takes the list from this state (`Report::Scanned` follows it)
                let seen =
                    b.discovery(&a, true, &mut h).and_then(|_| b.serve(SCAN, &mut h)).and_then(|_| b.state(&mut h));
                let stop = b.discovery(&a, false, &mut h);
                match seen.and_then(|s| stop.map(|_| s)) {
                    Ok((adapter, devices)) => {
                        let _ = out.send(Report::State(Link { stick: true, daemon: true, adapter, devices }));
                        let _ = out.send(Report::Scanned);
                        continue;
                    }
                    Err(e) => Err(e),
                }
            }
            Job::Pair(d) => {
                // a scan running beside the pairing slows it down
                let _ = b.discovery(&a, false, &mut h);
                *pairing.borrow_mut() = Some(d.clone());
                let r = b.pair(&d, &mut h).map(|_| "Paired and connected".to_string());
                *pairing.borrow_mut() = None;
                let _ = out.send(Report::Prompt(Prompt::Done));
                r
            }
            Job::Connect(d) => b.connect(&d, &mut h).map(|_| "Connected".to_string()),
            Job::Disconnect(d) => b.disconnect(&d, &mut h).map(|_| "Disconnected".to_string()),
            Job::Forget(d) => b.remove(&a, &d, &mut h).map(|_| "Device forgotten".to_string()),
        };
        let _ =
            out.send(Report::Done(r.map_err(|e| format!("Bluetooth: {}", e.rsplit("Error.").next().unwrap_or(&e)))));
    }
}

impl Ui {
    pub(super) fn bt_items(&self) -> Vec<Item> {
        vec![Item::BtState, Item::BtScan, Item::BtPaired, Item::BtForget, Item::BtInfo]
    }

    pub(super) fn bt_kind(it: Item) -> Option<Kind> {
        match it {
            Item::BtScan | Item::BtPaired | Item::BtForget | Item::BtInfo => Some(Kind::Action),
            _ => None,
        }
    }

    pub(super) fn bt_text(it: Item) -> bool {
        it == Item::BtState
    }

    pub(super) fn bt_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Bluetooth) => "Bluetooth",
            Item::BtState => "Bluetooth",
            Item::BtScan => "Find devices",
            Item::BtPaired => "Paired devices",
            Item::BtForget => "Forget device",
            Item::BtInfo => "Bluetooth details",
            Item::BtFound => "Devices found",
            Item::BtPairedPick => "Paired",
            Item::BtForgetPick => "Forget",
            _ => return None,
        })
    }

    pub(super) fn bt_value(&self, it: Item) -> Option<String> {
        let l = &self.bt.link;
        Some(match it {
            Item::BtState if !l.stick && !stick_there() => "no stick in the front USB port".into(),
            Item::BtState if !l.daemon => "starting".into(),
            Item::BtState => match &l.adapter {
                Some(a) if a.powered => {
                    let n = l.devices.iter().filter(|d| d.connected).count();
                    format!("on \u{b7} {} \u{b7} {} connected", a.name, n)
                }
                Some(_) => "off".into(),
                None => "no adapter".into(),
            },
            _ => return None,
        })
    }

    /// From `step`: the worker's reports; the worker starts with the first look at the page and stays (it answers the
    /// agent's calls, and refuses those of a device that is not being paired).
    pub(super) fn bt_poll(&mut self) {
        let in_menu = self.menu_open
            && (self.menu == MenuId::Bluetooth || self.pick.is_some_and(|p| p.menu == MenuId::Bluetooth));
        if in_menu {
            self.bt.service.start("bt", worker);
        }
        while let Some(r) = self.bt.service.take() {
            self.bt_report(r);
        }
    }

    pub(super) fn bt_report(&mut self, r: Report) {
        match r {
            Report::State(l) => self.bt.link = l,
            Report::Scanned => {
                self.bt.busy = None;
                self.bt.found = Self::bt_seen(&self.bt.link.devices);
                if self.bt_found().is_empty() {
                    self.toast("No mouse or keyboard found: is it in pairing mode?");
                } else if self.menu_open && self.menu == MenuId::Bluetooth {
                    self.open_picker(Item::BtFound);
                }
            }
            Report::Prompt(Prompt::Done) => {
                self.bt.prompt = None;
                if let Some(lines) = self.bt.prompt_lines.take() {
                    if self.dialog.as_ref() == Some(&lines) {
                        self.dialog = None;
                    }
                }
            }
            Report::Prompt(p) => {
                let lines = self.bt_prompt_lines(&p);
                self.dialog = Some(lines.clone());
                self.bt.prompt_lines = Some(lines);
                self.bt.prompt = Some(p);
            }
            Report::Done(r) => {
                self.bt.busy = None;
                self.toast(r.as_ref().unwrap_or_else(|e| e));
            }
        }
    }

    fn bt_prompt_lines(&self, p: &Prompt) -> Vec<String> {
        let name = |path: &str| {
            self.bt.link.devices.iter().find(|d| d.path == path).map_or("the keyboard".to_string(), |d| d.name.clone())
        };
        match p {
            Prompt::Passkey { device, passkey, .. } => {
                vec![format!("Pairing {}", name(device)), format!("Type {:06} on it, then Enter", passkey)]
            }
            Prompt::Pin { device, pin } => {
                vec![format!("Pairing {}", name(device)), format!("Type {} on it, then Enter", pin)]
            }
            Prompt::Done => Vec::new(),
        }
    }

    fn bt_send(&mut self, j: Job, what: &'static str) {
        if let Some(m) = self.bt_blocked() {
            return self.toast(m);
        }
        if self.bt.service.send(j) {
            self.bt.busy = Some(what);
            self.toast(&format!("{} ...", what));
        } else {
            self.toast("Bluetooth is not ready yet");
        }
    }

    /// Why nothing can be done yet, for a toast.
    fn bt_blocked(&self) -> Option<&'static str> {
        if self.bt.busy.is_some() {
            Some("Bluetooth is busy")
        } else if !self.bt.link.stick && !stick_there() {
            Some("No Bluetooth stick in the front USB port")
        } else if !self.bt.link.daemon {
            Some("Bluetooth is not ready yet")
        } else {
            None
        }
    }

    /// The devices a scan found, from the state read while it ran: those with an RSSI (bluetoothd has one only for a
    /// device heard in this discovery) that are not paired, inputs and devices with a name, inputs first, then the
    /// strongest.
    fn bt_seen(devices: &[Device]) -> Vec<Device> {
        let heard = |d: &&Device| !d.paired && d.rssi.is_some() && (d.is_input() || d.named);
        let mut v: Vec<Device> = devices.iter().filter(heard).cloned().collect();
        v.sort_by_key(|d| (!d.is_input(), -(d.rssi.unwrap_or(-127) as i32)));
        v.truncate(LIST_MAX);
        v
    }

    /// The picker of Find devices: what the last scan found (it keeps its order and signal strength after the scan),
    /// less what bluetoothd no longer knows or what is paired since.
    fn bt_found(&self) -> Vec<&Device> {
        let now = |d: &Device| self.bt.link.devices.iter().find(|n| n.path == d.path).is_some_and(|n| !n.paired);
        self.bt.found.iter().filter(|d| now(d)).collect()
    }

    /// Every paired device, for the details. Its pickers show the first `LIST_MAX` (a picker does not scroll).
    fn bt_paired(&self) -> Vec<&Device> {
        self.bt.link.devices.iter().filter(|d| d.paired).collect()
    }

    fn bt_line(d: &Device) -> String {
        let k = kind(d);
        let mut s = short(&d.name);
        if !k.is_empty() {
            s += &format!(" \u{b7} {}", k);
        }
        if d.connected {
            s += " \u{b7} connected";
        } else if let Some(r) = d.rssi.filter(|_| !d.paired) {
            s += &format!("  {} dBm", r);
        }
        s
    }

    pub(super) fn bt_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::BtFound => self.bt_found().into_iter().map(Self::bt_line).collect(),
            Item::BtPairedPick | Item::BtForgetPick => {
                self.bt_paired().into_iter().take(LIST_MAX).map(Self::bt_line).collect()
            }
            _ => return None,
        })
    }

    pub(super) fn bt_activate(&mut self, it: Item) -> bool {
        match it {
            Item::BtScan => self.bt_send(Job::Scan, "Looking for mice and keyboards"),
            Item::BtPaired | Item::BtForget => {
                if let Some(m) = self.bt_blocked() {
                    self.toast(m);
                } else if self.bt_paired().is_empty() {
                    self.toast("No paired devices");
                } else {
                    self.open_picker(if it == Item::BtPaired { Item::BtPairedPick } else { Item::BtForgetPick });
                }
            }
            Item::BtInfo => self.dialog = Some(self.bt_lines()),
            _ => return false,
        }
        true
    }

    pub(super) fn bt_is_pick(it: Item) -> bool {
        matches!(it, Item::BtFound | Item::BtPairedPick | Item::BtForgetPick)
    }

    /// A line of one of the device pickers was picked.
    pub(super) fn bt_pick(&mut self, it: Item, k: usize) {
        let path = |v: Vec<&Device>| v.get(k).map(|d| (d.path.clone(), d.connected));
        match it {
            Item::BtFound => {
                if let Some((p, _)) = path(self.bt_found()) {
                    self.bt_send(Job::Pair(p), "Pairing");
                }
            }
            Item::BtPairedPick => match path(self.bt_paired()) {
                Some((p, true)) => self.bt_send(Job::Disconnect(p), "Disconnecting"),
                Some((p, false)) => self.bt_send(Job::Connect(p), "Connecting"),
                None => {}
            },
            Item::BtForgetPick => {
                if let Some((p, _)) = path(self.bt_paired()) {
                    self.bt_send(Job::Forget(p), "Forgetting");
                }
            }
            _ => {}
        }
    }

    /// The details dialog.
    pub(super) fn bt_lines(&self) -> Vec<String> {
        let l = &self.bt.link;
        let mut v = vec!["Bluetooth".to_string()];
        match &l.adapter {
            None if !l.stick => {
                v.push("No stick in the front USB port (docs/bluetooth.md lists the ones that work)".into())
            }
            None => v.push("The stick is there; bluetoothd is starting".into()),
            Some(a) => v.push(format!("Adapter {} \u{b7} {}", a.name, if a.powered { "on" } else { "off" })),
        }
        let paired = self.bt_paired();
        if paired.is_empty() {
            v.push("No paired devices: Find devices, with the mouse or keyboard in pairing mode".into());
        }
        for d in paired {
            v.push(format!(
                "{} \u{b7} {}",
                Self::bt_line(d),
                if d.trusted { "reconnects by itself" } else { "not trusted" }
            ));
        }
        v
    }

    /// For the layout lint: a stick with more paired and heard devices than a picker has lines, all with long names.
    #[cfg(test)]
    pub(super) fn bt_crowded_for_tests(&mut self) {
        let dev = |k: usize, paired: bool| Device {
            path: format!("/org/bluez/hci0/dev_{}_{}", paired, k),
            address: String::new(),
            name: format!("A Rather Long Wireless Keyboard Name {}", k),
            icon: "input-keyboard".into(),
            paired,
            trusted: paired,
            connected: paired && k.is_multiple_of(2),
            rssi: if paired { None } else { Some(-100) },
            named: true,
        };
        let mut devices: Vec<Device> = (0..LIST_MAX + 2).map(|k| dev(k, true)).collect();
        devices.extend((0..LIST_MAX + 2).map(|k| dev(k, false)));
        self.bt.link = Link {
            stick: true,
            daemon: true,
            adapter: Some(Adapter {
                path: "/org/bluez/hci0".into(),
                name: "dho924s-000000000000".into(),
                powered: true,
                discovering: false,
            }),
            devices,
        };
        self.bt_stick = true;
        self.bt_inputs = 2;
        self.bt_report(Report::Scanned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, icon: &str, paired: bool, connected: bool, rssi: Option<i16>) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{}", name),
            address: String::new(),
            name: name.into(),
            icon: icon.into(),
            paired,
            trusted: paired,
            connected,
            rssi,
            named: true,
        }
    }

    fn with_devices() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.bt.link = Link {
            stick: true,
            daemon: true,
            adapter: Some(Adapter {
                path: "/org/bluez/hci0".into(),
                name: "dho924s-000000".into(),
                powered: true,
                discovering: false,
            }),
            devices: vec![
                dev("Phone", "phone", false, false, Some(-40)),
                dev("Mouse", "input-mouse", false, false, Some(-70)),
                dev("Keys", "input-keyboard", true, true, None),
                dev("Gone", "", false, false, None),
            ],
        };
        u
    }

    #[test]
    fn inputs_come_first_in_the_found_list_and_paired_ones_are_left_out() {
        let mut u = with_devices();
        u.bt_report(Report::Scanned);
        let o = u.bt_options(Item::BtFound).unwrap();
        assert_eq!(o, vec!["Mouse \u{b7} mouse  -70 dBm".to_string(), "Phone  -40 dBm".to_string()]);
        assert_eq!(
            u.bt_options(Item::BtPairedPick).unwrap(),
            vec!["Keys \u{b7} keyboard \u{b7} connected".to_string()]
        );
        assert_eq!(u.bt_value(Item::BtState).unwrap(), "on \u{b7} dho924s-000000 \u{b7} 1 connected");
    }

    /// As the worker reports a scan: the state read while discovery ran, then Scanned, then the states after it, in
    /// which bluetoothd has cleared the RSSI (BlueZ 5.82 does at StopDiscovery).
    #[test]
    fn the_found_list_stays_after_the_scan_stops() {
        let mut u = with_devices();
        u.menu_open = true;
        u.menu = MenuId::Bluetooth;
        let during = u.bt.link.clone();
        u.bt_report(Report::State(during.clone()));
        u.bt_report(Report::Scanned);
        assert_eq!(u.pick.map(|p| p.it), Some(Item::BtFound), "the picker opens");
        let mut after = during;
        for d in &mut after.devices {
            d.rssi = None;
        }
        u.bt_report(Report::State(after.clone()));
        let o = u.bt_options(Item::BtFound).unwrap();
        assert_eq!(o, vec!["Mouse \u{b7} mouse  -70 dBm".to_string(), "Phone  -40 dBm".to_string()]);
        // the mouse paired since, the phone gone from bluetoothd: the list says so
        after.devices[1].paired = true;
        after.devices.remove(0);
        u.bt_report(Report::State(after));
        assert!(u.bt_options(Item::BtFound).unwrap().is_empty());
        // a scan that heard nothing (no RSSI on any device) finds nothing, whatever bluetoothd still knows
        let mut quiet = with_devices();
        for d in &mut quiet.bt.link.devices {
            d.rssi = None;
        }
        quiet.bt_report(Report::Scanned);
        assert!(quiet.bt_found().is_empty());
    }

    #[test]
    fn a_passkey_is_shown_until_the_pairing_ends() {
        let mut u = with_devices();
        u.bt_report(Report::Prompt(Prompt::Passkey {
            device: "/org/bluez/hci0/dev_Keys".into(),
            passkey: 4321,
            entered: 0,
        }));
        assert_eq!(u.dialog.as_ref().unwrap()[1], "Type 004321 on it, then Enter");
        assert!(u.dialog.as_ref().unwrap()[0].contains("Keys"));
        u.bt_report(Report::Prompt(Prompt::Done));
        assert!(u.dialog.is_none() && u.bt.prompt.is_none());
    }

    /// Every Pair job ends with Done, with a prompt or without: it closes the passkey dialog only, not one the user
    /// opened since (or one open while a mouse pairs without a passkey).
    #[test]
    fn the_end_of_a_pairing_closes_only_its_own_dialog() {
        let mut u = with_devices();
        u.bt_activate(Item::BtInfo);
        let info = u.dialog.clone();
        assert!(info.is_some());
        u.bt_report(Report::Prompt(Prompt::Done));
        assert_eq!(u.dialog, info, "a pairing without a prompt leaves the details open");
        u.bt_report(Report::Prompt(Prompt::Pin { device: "/org/bluez/hci0/dev_Keys".into(), pin: "0000".into() }));
        u.bt_activate(Item::BtInfo);
        u.bt_report(Report::Prompt(Prompt::Done));
        assert_eq!(u.dialog, info, "the details opened over the PIN stay");
        assert!(u.bt.prompt.is_none());
    }

    /// The top bar counts devices, not inputs: a keyboard with a touchpad is one (as on the unit: two inputs, one
    /// address); a USB keyboard is none.
    #[test]
    fn connected_inputs_are_counted_by_device() {
        let i = |bus: &str, uniq: &str| (format!("{bus}\n"), format!("{uniq}\n"));
        let all = [
            i("0005", "00:11:22:33:44:0a"),
            i("0005", "00:11:22:33:44:0a"),
            i("0003", ""),
            i("0005", "00:11:22:33:44:0b"),
        ];
        assert_eq!(inputs_connected(&all), 2);
        assert_eq!(inputs_connected(&[i("0003", "")]), 0);
        assert_eq!(inputs_connected(&[]), 0);
    }

    /// The top bar: a box beside WLAN's while the stick is there, green with a device connected, a tap opens the page.
    #[test]
    fn the_top_bar_box_opens_the_page() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        (u.bt_stick, u.wlan_stick) = (true, true);
        let b = u.status_box("BT").unwrap();
        assert!(b.right() < u.status_box("WLAN").unwrap().x && u.top_right() == b.x);
        u.tap(b.x + 5, b.y + 5);
        assert!(u.menu_open && u.menu == MenuId::Bluetooth);
        let at = |u: &mut Ui| {
            let mut cv = Canvas::new(u.scr.w, u.scr.h);
            u.render(&mut cv);
            cv.px[((b.y + 15) * u.scr.w + b.x + 9) as usize] & 0xffffff
        };
        let grey = at(&mut u);
        u.bt_inputs = 1;
        assert_eq!(at(&mut u), GREEN & 0xffffff, "green with a device connected (not {grey:06x})");
    }

    #[test]
    fn without_a_stick_the_page_says_so() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert_eq!(u.bt_value(Item::BtState).unwrap(), "no stick in the front USB port");
        u.bt_activate(Item::BtScan);
        assert!(u.bt.busy.is_none());
        assert!(u.bt_lines()[1].starts_with("No stick"));
    }
}
