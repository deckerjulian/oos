// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! One setting must not change another (eTobey in the forum, after a scope whose trigger types changed each other).
//! Each test steps one setting through all its values and back, with steps of the screen in between, and checks that
//! the setup (every saved setting) is what it was.

use super::*;
use crate::wave::Source;

fn ui() -> Ui {
    // the hardware source: the demo source keeps presets that suit its signals
    let mut u = Ui::new(Source::Hw { shared: Default::default(), seen: 0 });
    u.m.ch[1].on = true;
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(1))];
    u.step(0.05);
    u
}

/// The keys whose values differ between two setups.
fn changed(a: &str, b: &str) -> Vec<String> {
    let map = |t: &str| t.lines().filter_map(|l| l.split_once(" = ")).map(|(k, v)| (k.to_string(), v.to_string())).collect::<std::collections::BTreeMap<_, _>>();
    let (a, b) = (map(a), map(b));
    a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()
}

/// Parameters that belong to one trigger type only: a type sets its own when it is chosen.
const TYPE_OWN: [&str; 14] = ["trig_t1", "trig_when", "trig_level2", "trig_pol", "pat", "setup", "hold", "win_mode", "trig_src2", "trig_slope2", "nth_idle", "nth_n", "trig_bus", "trig_bus_val"];

/// Steps `it` once round all `n` of its values with steps of the screen between, and checks the setup after (but
/// for the keys of `own`).
fn round(u: &mut Ui, it: Item, n: usize, own: &[&str]) {
    let before = crate::store::setup_text(&u.m);
    for _ in 0..n {
        u.adjust(it, 1);
        u.step(0.05);
    }
    let after = crate::store::setup_text(&u.m);
    let bad: Vec<String> = changed(&before, &after).into_iter().filter(|k| !own.contains(&k.as_str()) && !k.starts_with("ser.")).collect();
    assert!(bad.is_empty(), "{:?} round: {:?} changed", it, bad);
}

#[test]
fn every_trigger_type_and_back_leaves_the_rest() {
    let mut u = ui();
    round(&mut u, Item::TrigType, TRIG_TYPES.len(), &TYPE_OWN);
}

#[test]
fn every_acquisition_mode_and_back_leaves_the_rest() {
    let mut u = ui();
    round(&mut u, Item::AcqMode, 4, &[]);
}

#[test]
fn every_analyser_page_and_back_leaves_the_rest() {
    let mut u = ui();
    let cv = Canvas::new(u.scr.w, u.scr.h);
    let mut scpi = crate::scpi::Scpi::open(0);
    let before = crate::store::setup_text(&u.m);
    for page in ["SPECtrum", "EYE", "POWer", "BODE", "MAGnetic", "INVerter", "DPT", "SCOPe"] {
        scpi.exec_line(&mut u, &cv, &format!(":VIEW {}", page));
        u.step(0.05);
    }
    let after = crate::store::setup_text(&u.m);
    assert!(changed(&before, &after).is_empty(), "pages round: {:?} changed", changed(&before, &after));
}

#[test]
fn draw_path_bottom_bar_and_features_leave_the_acquisition() {
    let mut u = ui();
    let keep = |m: &Model| (m.tscale, m.trig_type, m.trig_level.to_bits(), m.acq, m.ch.iter().map(|c| (c.on, c.vscale)).collect::<Vec<_>>(), m.meas.clone());
    let before = keep(&u.m);
    for it in [Item::AcqDrawPath, Item::BottomBar] {
        u.adjust(it, 1);
        u.step(0.05);
        u.adjust(it, 1);
        u.step(0.05);
    }
    for i in 0..features::features().len() {
        u.adjust(Item::FeatOn(i), 1);
        u.step(0.05);
        u.adjust(Item::FeatOn(i), 1);
    }
    assert_eq!(keep(&u.m), before);
}

#[test]
fn every_decoder_protocol_and_back_leaves_the_channels_and_the_trigger() {
    // on the hardware a protocol sets its own parameters only: no channel is switched on, no trigger changes, and
    // the sources the protocols share stay (a new role takes a channel that is on, or none)
    let mut u = ui();
    u.m.dec_on = true;
    let n = u.protos_on().len();
    let before = crate::store::setup_text(&u.m);
    for _ in 0..n {
        let on: Vec<bool> = u.m.ch.iter().map(|c| c.on).collect();
        u.adjust(Item::DecProto, 1);
        u.step(0.05);
        assert_eq!(u.m.ch.iter().map(|c| c.on).collect::<Vec<_>>(), on, "{} switched a channel", u.m.dec_proto.name());
        let roles = u.m.dec_proto.roles();
        for (role, c) in [(roles[1].map(|_| u.m.dec_src2), ()), (roles[2].and(u.m.dec_cs), ()), (roles[3].and(u.m.dec_src4), ())].map(|x| x.0).into_iter().enumerate() {
            if let Some(c) = c.filter(|&c| c != u.m.dec_src) {
                assert!(c < NCH, "{} role {}: {c}", u.m.dec_proto.name(), role + 1);
            }
        }
    }
    let after = crate::store::setup_text(&u.m);
    // the decoder's own keys and the pane a protocol with a view of its own opens
    let bad: Vec<String> = changed(&before, &after).into_iter().filter(|k| !k.starts_with("dec") && !k.starts_with("ser.") && k != "pane").collect();
    assert!(bad.is_empty(), "protocols round: {bad:?} changed");
}

#[test]
fn a_new_decoder_role_takes_a_channel_that_is_on() {
    let mut u = ui();
    // CH1 and CH2 are on: UART decodes CH1; SPI adds clock/data, chip select
    u.m.dec_on = true;
    u.set_dec_proto(Proto::UART);
    u.m.dec_src = 0;
    u.set_dec_proto(Proto::SPI);
    assert_eq!(u.m.dec_src, 0, "the shared source stays");
    assert_eq!(u.m.dec_src2, 1, "the second role takes CH2, which is on");
    assert_eq!(u.m.dec_cs, None, "no channel left that is on: no chip select");
    assert!(!u.m.ch[2].on && !u.m.ch[3].on, "nothing switched on");
}

#[test]
fn leaving_bus_after_a_recall_leaves_the_recalled_decoder() {
    // what choosing Bus switched on is switched off when Bus is left (`bus_auto`), but only in the set-up it came
    // from: a recalled set-up with Bus and its decoder keeps them
    let mut u = ui();
    let dir = std::env::temp_dir().join(format!("oos-interplay-bus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir.clone();
    u.m.dec_on = false;
    u.set_trig_type(TrigType::Bus);
    u.step(0.05);
    assert!(u.m.dec_on, "Bus switches the decoder on");
    let mut saved = u.m.clone();
    for c in 0..NCH { saved.ch[c].on = c < n_ch(); }
    std::fs::write(u.setup_path(), crate::store::setup_text(&saved)).unwrap();
    u.recall_setup();
    u.step(0.05);
    let on: Vec<bool> = u.m.ch.iter().map(|c| c.on).collect();
    u.adjust(Item::TrigType, -1);
    u.step(0.05);
    assert_ne!(u.m.trig_type, TrigType::Bus);
    assert!(u.m.dec_on, "the decoder of the recalled set-up stays on");
    assert_eq!(u.m.ch.iter().map(|c| c.on).collect::<Vec<_>>(), on);
    // the same after the Default key
    u.set_trig_type(TrigType::Bus);
    u.key(key::DEFAULT);
    assert!(u.bus_auto.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
