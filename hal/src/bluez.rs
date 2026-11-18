// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Bluetooth through bluetoothd on the system bus (docs/bluetooth.md, ADR 0026): the adapter, the devices it knows,
//! discovery, pairing, connecting and removing, and the pairing agent that shows a keyboard's passkey. `/bin/oos-bt`
//! runs bluetoothd; this module talks to it over `dbus.rs`. The parsing (`objects`, `agent`) is pure and tested on
//! the host; the calls need the bus of a unit.

use crate::dbus::{Conn, Handler, Message, Value, SYSTEM_BUS};
use std::time::Duration;

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const AGENT: &str = "org.bluez.Agent1";
const PROPS: &str = "org.freedesktop.DBus.Properties";
/// Our pairing agent's object.
pub const AGENT_PATH: &str = "/org/oos/agent";
/// What the scope can do in a pairing: show a passkey (a keyboard types it) and confirm one.
const CAPABILITY: &str = "KeyboardDisplay";
/// A plain call; pairing and connecting take their own, longer times.
const CALL: Duration = Duration::from_secs(5);
const PAIR: Duration = Duration::from_secs(60);
const CONNECT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub name: String,
    pub powered: bool,
    pub discovering: bool,
}

/// A device bluetoothd knows: one found by a scan, or one paired before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub address: String,
    /// the name it gives itself, or its address when it gives none
    pub name: String,
    /// `input-mouse`, `input-keyboard`, `phone` ... (from its class or appearance), empty when unknown
    pub icon: String,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    /// signal strength of the last advertisement (dBm); None for a device out of reach
    pub rssi: Option<i16>,
    /// the device gave a name of its own
    pub named: bool,
}

impl Device {
    /// A mouse, keyboard, tablet or game controller: what the scope can use.
    pub fn is_input(&self) -> bool {
        self.icon.starts_with("input-")
    }
}

/// The first adapter and the devices, from the answer of `ObjectManager.GetManagedObjects` (`a{oa{sa{sv}}}`).
pub fn objects(body: &[Value]) -> (Option<Adapter>, Vec<Device>) {
    let (mut adapter, mut devices) = (None, Vec::new());
    let Some(all) = body.first() else { return (None, devices) };
    for (path, ifaces) in all.entries() {
        let path = path.as_str().unwrap_or_default().to_string();
        if let Some(a) = ifaces.get(ADAPTER) {
            if adapter.is_none() {
                adapter = Some(Adapter {
                    path: path.clone(),
                    name: a.get("Alias").and_then(Value::as_str).unwrap_or_default().into(),
                    powered: a.get("Powered").and_then(Value::as_bool).unwrap_or(false),
                    discovering: a.get("Discovering").and_then(Value::as_bool).unwrap_or(false),
                });
            }
        }
        if let Some(d) = ifaces.get(DEVICE) {
            let address = d.get("Address").and_then(Value::as_str).unwrap_or_default().to_string();
            let name = d.get("Name").and_then(Value::as_str).map(str::to_string);
            devices.push(Device {
                path,
                named: name.is_some(),
                name: name
                    .or_else(|| d.get("Alias").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_else(|| address.clone()),
                address,
                icon: d.get("Icon").and_then(Value::as_str).unwrap_or_default().into(),
                paired: d.get("Paired").and_then(Value::as_bool).unwrap_or(false),
                trusted: d.get("Trusted").and_then(Value::as_bool).unwrap_or(false),
                connected: d.get("Connected").and_then(Value::as_bool).unwrap_or(false),
                rssi: d.get("RSSI").and_then(Value::as_i64).map(|v| v as i16),
            });
        }
    }
    (adapter, devices)
}

/// What the agent tells the user during a pairing.
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// type this number on the keyboard, then Enter (`entered`: the keys typed so far)
    Passkey { device: String, passkey: u32, entered: u16 },
    /// type this PIN on the device (old keyboards)
    Pin { device: String, pin: String },
    /// the pairing ended or was cancelled: the prompt goes
    Done,
}

/// The fixed PIN for a legacy device that asks the scope for one: it is typed on the keyboard as well.
pub const LEGACY_PIN: &str = "0000";

/// Answers a call to the pairing agent; `tell` gets what is to be shown. None for a call that is not the agent's.
///
/// `pairing` is the device of the Pair job that runs (the user picked it on the scope). Only that device gets a
/// passkey shown, the PIN and a yes; every other device is refused, so nothing around can pair with the scope or
/// connect a service while Find devices leaves it pairable.
pub fn agent(m: &Message, pairing: Option<&str>, tell: &mut dyn FnMut(Prompt)) -> Option<Message> {
    if m.kind != crate::dbus::METHOD_CALL
        || m.path.as_deref() != Some(AGENT_PATH)
        || m.interface.as_deref() != Some(AGENT)
    {
        return None;
    }
    let dev = m.body.first().and_then(Value::as_str).unwrap_or_default().to_string();
    let num = |i: usize| m.body.get(i).and_then(Value::as_i64).unwrap_or(0);
    let member = m.member.as_deref()?;
    let ours = pairing.is_some_and(|p| !dev.is_empty() && p == dev);
    let asks = matches!(
        member,
        "DisplayPasskey"
            | "DisplayPinCode"
            | "RequestPinCode"
            | "RequestConfirmation"
            | "RequestAuthorization"
            | "AuthorizeService"
    );
    if asks && !ours {
        return Some(Message::error(m, "org.bluez.Error.Rejected", "not the device being paired"));
    }
    Some(match member {
        "DisplayPasskey" => {
            tell(Prompt::Passkey { device: dev, passkey: num(1) as u32, entered: num(2) as u16 });
            Message::reply(m, vec![])
        }
        "DisplayPinCode" => {
            tell(Prompt::Pin { device: dev, pin: m.body.get(1).and_then(Value::as_str).unwrap_or_default().into() });
            Message::reply(m, vec![])
        }
        "RequestPinCode" => {
            tell(Prompt::Pin { device: dev, pin: LEGACY_PIN.into() });
            Message::reply(m, vec![Value::Str(LEGACY_PIN.into())])
        }
        // the user started this pairing on the scope: numeric comparison and authorisations are accepted
        "RequestConfirmation" | "RequestAuthorization" | "AuthorizeService" | "Release" => Message::reply(m, vec![]),
        "Cancel" => {
            tell(Prompt::Done);
            Message::reply(m, vec![])
        }
        // the scope has no keypad to type a passkey shown elsewhere
        "RequestPasskey" => Message::error(m, "org.bluez.Error.Rejected", "no keypad"),
        _ => return None,
    })
}

/// A connection to bluetoothd with our agent registered.
pub struct Bluez {
    c: Conn,
}

impl Bluez {
    pub fn open() -> Result<Bluez, String> {
        let mut c = Conn::open(SYSTEM_BUS)?;
        let mut none = |_: &Message| None;
        let reg = Message::call(
            BLUEZ,
            "/org/bluez",
            "org.bluez.AgentManager1",
            "RegisterAgent",
            vec![Value::Path(AGENT_PATH.into()), Value::Str(CAPABILITY.into())],
        );
        c.call(reg, CALL, &mut none)?;
        let def = Message::call(
            BLUEZ,
            "/org/bluez",
            "org.bluez.AgentManager1",
            "RequestDefaultAgent",
            vec![Value::Path(AGENT_PATH.into())],
        );
        c.call(def, CALL, &mut none)?;
        Ok(Bluez { c })
    }

    pub fn state(&mut self, h: Handler) -> Result<(Option<Adapter>, Vec<Device>), String> {
        let m = Message::call(BLUEZ, "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", vec![]);
        Ok(objects(&self.c.call(m, CALL, h)?))
    }

    fn set(&mut self, path: &str, iface: &str, prop: &str, v: Value, h: Handler) -> Result<(), String> {
        let m = Message::call(
            BLUEZ,
            path,
            PROPS,
            "Set",
            vec![Value::Str(iface.into()), Value::Str(prop.into()), Value::Variant(Box::new(v))],
        );
        self.c.call(m, CALL, h).map(drop)
    }

    /// Discovery on or off; while it runs the adapter also takes pairings.
    pub fn discovery(&mut self, adapter: &str, on: bool, h: Handler) -> Result<(), String> {
        if on {
            self.set(adapter, ADAPTER, "Pairable", Value::Bool(true), h)?;
        }
        let m = Message::call(BLUEZ, adapter, ADAPTER, if on { "StartDiscovery" } else { "StopDiscovery" }, vec![]);
        match self.c.call(m, CALL, h) {
            // already in the state asked for
            Err(e)
                if e.contains("InProgress")
                    || e.contains("NotReady") && !on
                    || e.contains("Failed: No discovery started") =>
            {
                Ok(())
            }
            r => r.map(drop),
        }
    }

    /// Pairs, trusts (so it reconnects by itself) and connects a device.
    pub fn pair(&mut self, dev: &str, h: Handler) -> Result<(), String> {
        let m = Message::call(BLUEZ, dev, DEVICE, "Pair", vec![]);
        match self.c.call(m, PAIR, h) {
            Err(e) if e.contains("AlreadyExists") => {}
            r => r.map(drop)?,
        }
        self.set(dev, DEVICE, "Trusted", Value::Bool(true), h)?;
        self.connect(dev, h)
    }

    pub fn connect(&mut self, dev: &str, h: Handler) -> Result<(), String> {
        self.c.call(Message::call(BLUEZ, dev, DEVICE, "Connect", vec![]), CONNECT, h).map(drop)
    }

    pub fn disconnect(&mut self, dev: &str, h: Handler) -> Result<(), String> {
        self.c.call(Message::call(BLUEZ, dev, DEVICE, "Disconnect", vec![]), CONNECT, h).map(drop)
    }

    /// Forgets a device: its pairing goes.
    pub fn remove(&mut self, adapter: &str, dev: &str, h: Handler) -> Result<(), String> {
        self.c
            .call(Message::call(BLUEZ, adapter, ADAPTER, "RemoveDevice", vec![Value::Path(dev.into())]), CALL, h)
            .map(drop)
    }

    /// Answers the agent calls that come in within `t` (a device that reconnects and asks).
    pub fn serve(&mut self, t: Duration, h: Handler) -> Result<(), String> {
        self.c.serve(t, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }
    fn var(v: Value) -> Value {
        Value::Variant(Box::new(v))
    }
    fn dict(kv: Vec<(&str, Value)>, vsig: &str) -> Value {
        Value::Array(
            format!("{{s{}}}", vsig),
            kv.into_iter().map(|(k, v)| Value::Entry(Box::new(s(k)), Box::new(v))).collect(),
        )
    }

    #[test]
    fn the_managed_objects_give_the_adapter_and_the_devices() {
        let adapter = dict(
            vec![
                ("Alias", var(s("dho924s-000000"))),
                ("Powered", var(Value::Bool(true))),
                ("Discovering", var(Value::Bool(false))),
            ],
            "v",
        );
        let mouse = dict(
            vec![
                ("Address", var(s("d1:11:22:33:44:01"))),
                ("Name", var(s("MX Anywhere 3"))),
                ("Alias", var(s("MX Anywhere 3"))),
                ("Icon", var(s("input-mouse"))),
                ("Paired", var(Value::Bool(true))),
                ("Connected", var(Value::Bool(true))),
                ("RSSI", var(Value::I16(-58))),
            ],
            "v",
        );
        let anon = dict(vec![("Address", var(s("5a:11:22:33:44:02"))), ("Alias", var(s("5A-00-00-00-00-02")))], "v");
        let ifaces = |name: &str, props: Value| {
            Value::Array("{sa{sv}}".into(), vec![Value::Entry(Box::new(s(name)), Box::new(props))])
        };
        let all = Value::Array(
            "{oa{sa{sv}}}".into(),
            vec![
                Value::Entry(Box::new(Value::Path("/org/bluez/hci0".into())), Box::new(ifaces(ADAPTER, adapter))),
                Value::Entry(Box::new(Value::Path("/org/bluez/hci0/dev_D1".into())), Box::new(ifaces(DEVICE, mouse))),
                Value::Entry(Box::new(Value::Path("/org/bluez/hci0/dev_5A".into())), Box::new(ifaces(DEVICE, anon))),
            ],
        );
        // through the wire and back, as bluetoothd sends it
        let mut m = Message::call("x", "/", "i", "m", vec![all]);
        m.serial = 1;
        let body = Message::decode(&m.encode()).unwrap().body;
        let (a, d) = objects(&body);
        assert_eq!(
            a,
            Some(Adapter {
                path: "/org/bluez/hci0".into(),
                name: "dho924s-000000".into(),
                powered: true,
                discovering: false
            })
        );
        assert_eq!(d.len(), 2);
        assert!(d[0].is_input() && d[0].paired && d[0].connected && !d[0].trusted && d[0].named);
        assert_eq!((d[0].name.as_str(), d[0].rssi), ("MX Anywhere 3", Some(-58)));
        assert!(!d[1].named && !d[1].is_input() && d[1].rssi.is_none());
        assert_eq!(d[1].name, "5A-00-00-00-00-02");
    }

    fn agent_call(member: &str, body: Vec<Value>) -> Message {
        let mut m = Message::call("x", AGENT_PATH, AGENT, member, body);
        m.serial = 5;
        m.sender = Some(":1.1".into());
        m
    }

    #[test]
    fn the_agent_shows_a_passkey_and_accepts_what_it_can() {
        let call = agent_call;
        let k = Some("/org/bluez/hci0/dev_K");
        let mut told = Vec::new();
        let dev = Value::Path("/org/bluez/hci0/dev_K".into());
        let shown = call("DisplayPasskey", vec![dev.clone(), Value::U32(123456), Value::U16(2)]);
        let r = agent(&shown, k, &mut |p| told.push(p)).unwrap();
        assert_eq!((r.kind, r.reply_serial), (crate::dbus::METHOD_RETURN, Some(5)));
        assert_eq!(told, vec![Prompt::Passkey { device: "/org/bluez/hci0/dev_K".into(), passkey: 123456, entered: 2 }]);
        let r = agent(&call("RequestPinCode", vec![dev.clone()]), k, &mut |_| {}).unwrap();
        assert_eq!(r.body, vec![s(LEGACY_PIN)]);
        let r = agent(&call("RequestPasskey", vec![dev.clone()]), k, &mut |_| {}).unwrap();
        assert_eq!(r.kind, crate::dbus::ERROR);
        let yes = agent(&call("RequestConfirmation", vec![dev, Value::U32(1)]), k, &mut |_| {}).unwrap();
        assert_eq!(yes.kind, crate::dbus::METHOD_RETURN);
        let mut other = call("DisplayPasskey", vec![]);
        other.path = Some("/elsewhere".into());
        assert!(agent(&other, k, &mut |_| {}).is_none());
    }

    const REJECTED: &str = "org.bluez.Error.Rejected";

    /// Only the device the user is pairing gets a yes: not one that asks by itself, not while nothing is paired.
    #[test]
    fn the_agent_refuses_every_device_but_the_one_being_paired() {
        let dev = Value::Path("/org/bluez/hci0/dev_X".into());
        let calls = [
            agent_call("RequestConfirmation", vec![dev.clone(), Value::U32(1)]),
            agent_call("RequestAuthorization", vec![dev.clone()]),
            agent_call("AuthorizeService", vec![dev.clone(), s("00001124-0000-1000-8000-00805f9b34fb")]),
            agent_call("RequestPinCode", vec![dev.clone()]),
            agent_call("DisplayPasskey", vec![dev.clone(), Value::U32(1), Value::U16(0)]),
            agent_call("DisplayPinCode", vec![dev.clone(), s("1234")]),
            agent_call("RequestConfirmation", vec![]),
        ];
        for pairing in [None, Some("/org/bluez/hci0/dev_K")] {
            for c in &calls {
                let mut told = Vec::new();
                let r = agent(c, pairing, &mut |p| told.push(p)).unwrap();
                let what = (c.member.as_deref(), pairing);
                assert_eq!((r.kind, r.error.as_deref()), (crate::dbus::ERROR, Some(REJECTED)), "{:?}", what);
                assert!(told.is_empty(), "nothing shown for {:?}", c.member);
            }
        }
        for c in &calls[..6] {
            assert_eq!(agent(c, Some("/org/bluez/hci0/dev_X"), &mut |_| {}).unwrap().kind, crate::dbus::METHOD_RETURN);
        }
    }
}
