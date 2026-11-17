// Copyright (C) 2026 dekajoule
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
    let map = |t: &str| {
        t.lines()
            .filter_map(|l| l.split_once(" = "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let (a, b) = (map(a), map(b));
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What a trigger type may set when it is chosen on the hardware: Delay and Setup/Hold put their second source on a
/// channel that is on (CH4 is off here), Bus a value of its protocol, which may have changed since. The rest (width,
/// pattern, lower level, ...) stays: review U6, where these keys were left out because each type set its demo values.
const TYPE_OWN: [&str; 3] = ["trig_src2", "trig_bus", "trig_bus_val"];

/// Steps `it` once round all `n` of its values with steps of the screen between, and checks the setup after (but
/// for the keys of `own`).
fn round(u: &mut Ui, it: Item, n: usize, own: &[&str]) {
    let before = crate::store::setup_text(&u.m);
    for _ in 0..n {
        u.adjust(it, 1);
        u.step(0.05);
    }
    let after = crate::store::setup_text(&u.m);
    let bad: Vec<String> =
        changed(&before, &after).into_iter().filter(|k| !own.contains(&k.as_str()) && !k.starts_with("ser.")).collect();
    assert!(bad.is_empty(), "{:?} round: {:?} changed", it, bad);
}

#[test]
fn every_trigger_type_and_back_leaves_the_rest() {
    let mut u = ui();
    // the levels in the order the two-level types need (otherwise their lower level gives way, the next test)
    u.m.trig_level = 1.0;
    // values no type would choose for itself
    (u.m.trig_t1, u.m.trig_t2, u.m.trig_when, u.m.trig_pol) = (7e-6, 9e-6, WhenKind::Out, Pol::Neg);
    (u.m.win_mode, u.m.trig_slope2, u.m.trig_level2) = (WinMode::Exit, Slope::Fall, 0.25);
    (u.m.setup, u.m.hold, u.m.nth_idle, u.m.nth_n) = (3e-6, 4e-6, 70e-6, 5);
    u.m.pat = std::array::from_fn(|i| [Bit::L, Bit::H, Bit::F, Bit::X][i % 4]);
    round(&mut u, Item::TrigType, TRIG_TYPES.len(), &TYPE_OWN);
}

/// Review U6: on a source that is not the demo, a pulse width, a pattern and the runt thresholds set by the user stay
/// when the type changes and comes back; the demo keeps its starting values, and a lower level never ends above the
/// upper one.
#[test]
fn a_trigger_type_keeps_the_values_it_shares_away_from_the_demo() {
    let replay = Source::Replay { words: Vec::new(), xinc: 1e-9, yinc: 1.0, yorg: 0.0, yref: 0.0 };
    for (name, src) in [("hw", Source::Hw { shared: Default::default(), seen: 0 }), ("replay", replay)] {
        let mut u = Ui::new(src);
        u.set_trig_type(TrigType::Pulse);
        (u.m.trig_t1, u.m.trig_when, u.m.trig_pol) = (5e-6, WhenKind::Gt, Pol::Neg);
        u.set_trig_type(TrigType::Pattern);
        let pat: [Bit; NCH] = std::array::from_fn(|i| [Bit::L, Bit::H, Bit::F, Bit::X][i % 4]);
        u.m.pat = pat;
        u.set_trig_type(TrigType::Runt);
        (u.m.trig_level, u.m.trig_level2) = (1.2, 0.8);
        for t in [TrigType::Edge, TrigType::Pulse, TrigType::Window, TrigType::Pattern, TrigType::Runt] {
            u.set_trig_type(t);
        }
        assert_eq!((u.m.trig_t1, u.m.trig_when, u.m.trig_pol), (5e-6, WhenKind::Gt, Pol::Neg), "{name}: pulse width");
        assert_eq!(u.m.pat, pat, "{name}: pattern");
        assert_eq!((u.m.trig_level, u.m.trig_level2), (1.2, 0.8), "{name}: runt thresholds");
    }
    // the demo: each type comes with values that trigger on its signals
    let mut u = crate::ui::testkit::demo_ui();
    u.m.trig_t1 = 5e-6;
    u.set_trig_type(TrigType::Pulse);
    assert_eq!(u.m.trig_t1, 30e-6, "demo: the pulse preset");
    u.set_trig_type(TrigType::Runt);
    assert_eq!((u.m.trig_level, u.m.trig_level2), (2.0, 0.5), "demo: the runt preset");
}

#[test]
fn the_lower_trigger_level_never_ends_above_the_upper() {
    let mut u = ui();
    // Delay's level B above its level A: Runt takes it as its lower level, at most up to the shared upper one
    u.set_trig_type(TrigType::Delay);
    (u.m.trig_level, u.m.trig_level2) = (0.4, 1.6);
    u.set_trig_type(TrigType::Runt);
    assert_eq!((u.m.trig_level, u.m.trig_level2), (0.4, 0.4), "the lower level gives way to the upper");
    let ordered = |u: &Ui| u.m.trig_level2 <= u.m.trig_level;
    // the knob: each level stops at the other
    u.m.trig_level2 = -0.4;
    for _ in 0..200 {
        u.adjust(Item::TrigLevel, -1);
        assert!(ordered(&u), "upper turned down: {} < {}", u.m.trig_level, u.m.trig_level2);
    }
    assert_eq!(u.m.trig_level, -0.4, "the upper stopped at the lower");
    for _ in 0..200 {
        u.adjust(Item::TrigLevel2, 1);
        assert!(ordered(&u), "lower turned up");
    }
    // a level set from elsewhere (SCPI's :TRIGger:...:LEVel, 50 %): the lower one follows it down
    for t in [TrigType::Slope, TrigType::Runt, TrigType::Window] {
        u.set_trig_type(t);
        (u.m.trig_level, u.m.trig_level2) = (1.0, 0.5);
        u.m.trig_level = 0.2;
        u.mark_changed();
        assert_eq!((u.m.trig_level, u.m.trig_level2), (0.2, 0.2), "{t:?}");
    }
    // the other types keep two levels of their own, in any order
    u.set_trig_type(TrigType::Delay);
    (u.m.trig_level, u.m.trig_level2) = (0.2, 1.0);
    u.mark_changed();
    assert_eq!((u.m.trig_level, u.m.trig_level2), (0.2, 1.0));
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
    let keep = |m: &Model| {
        (
            m.tscale,
            m.trig_type,
            m.trig_level.to_bits(),
            m.acq,
            m.ch.iter().map(|c| (c.on, c.vscale)).collect::<Vec<_>>(),
            m.meas.clone(),
        )
    };
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
        for (role, c) in
            [(roles[1].map(|_| u.m.dec_src2), ()), (roles[2].and(u.m.dec_cs), ()), (roles[3].and(u.m.dec_src4), ())]
                .map(|x| x.0)
                .into_iter()
                .enumerate()
        {
            if let Some(c) = c.filter(|&c| c != u.m.dec_src) {
                assert!(c < NCH, "{} role {}: {c}", u.m.dec_proto.name(), role + 1);
            }
        }
    }
    let after = crate::store::setup_text(&u.m);
    // the decoder's own keys and the pane a protocol with a view of its own opens
    let bad: Vec<String> = changed(&before, &after)
        .into_iter()
        .filter(|k| !k.starts_with("dec") && !k.starts_with("ser.") && k != "pane")
        .collect();
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

// Break it catches: a recorded file (Replay) or a Live stream treated as the demo, whose decoder choice moves the
// sources to the demo's channels (U6's twin in the decoder, review of 0.1.13).
#[test]
fn a_decoder_on_a_replay_keeps_its_sources() {
    let mut u = ui();
    u.src = Source::Replay { words: Vec::new(), xinc: 1e-9, yinc: 1.0, yorg: 0.0, yref: 0.0 };
    u.m.dec_on = true;
    u.set_dec_proto(Proto::UART);
    u.m.dec_src = 0;
    u.set_dec_proto(Proto::SPI);
    assert_eq!((u.m.dec_src, u.m.dec_src2), (0, 1), "the sources follow the channels that are on, not the demo's");
    assert!(!u.m.ch[2].on && !u.m.ch[3].on, "nothing switched on");
}

#[test]
fn leaving_bus_after_a_recall_leaves_the_recalled_decoder() {
    // what choosing Bus switched on is switched off when Bus is left (`bus_auto`), but only in the set-up it came
    // from: a recalled set-up with Bus and its decoder keeps them
    let mut u = ui();
    let dir = crate::ui::testkit::Scratch::new("interplay-bus");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir.clone();
    u.m.dec_on = false;
    u.set_trig_type(TrigType::Bus);
    u.step(0.05);
    assert!(u.m.dec_on, "Bus switches the decoder on");
    let mut saved = u.m.clone();
    for c in 0..NCH {
        saved.ch[c].on = c < n_ch();
    }
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
