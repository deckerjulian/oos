// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 24: SIM mode plays offline refs (from a file or Add to R<n>) as a live simulation. OOS_SHOTS=<dir> writes
//! pictures (b24-*).
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    let dir = std::env::temp_dir().join(format!("oos-batch24-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.05);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..4 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b24-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

/// A CSV file in the store folder, loaded through the Reference import.
fn load_csv(u: &mut Ui, slot: usize, name: &str) {
    let mut csv = String::from("Time (s),Voltage (V)\n");
    for i in 0..20_000 { csv.push_str(&format!("{:e},{}\n", i as f64 * 1e-7, (i as f64 * 0.02).sin() + 0.2 * (i as f64 * 0.31).sin())); }
    std::fs::write(u.store_dir.join(name), &csv).unwrap();
    let data = std::fs::read(u.store_dir.join(name)).unwrap();
    u.load_waveform_bytes_at(&data, name, slot).expect("the CSV loads");
}

fn stat_n(u: &Ui, row: usize) -> u64 { u.measurement_statistics_snapshot_for_test()[row].0 }

fn play(u: &mut Ui, frames: usize) { for _ in 0..frames { frame(u); } }

#[test]
fn sim_mode_plays_a_file_ref_through_the_acquisition_path() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    // a Live session again, with the file ref only drawn over the channels
    u.return_to_live();
    assert!(!u.record_mode());
    u.refs[0].as_mut().unwrap().on = true;
    u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0)), (Meas::Freq, WaveId::Ref(0))];
    let before = u.refs[0].as_ref().unwrap().v.clone();
    let chans = (u.m.ch[0].on, u.m.ch[1].on);
    u.activate(Item::SimPlay);
    assert!(u.record_mode() && u.sim_mode() && !u.session_is_sim, "the ref plays, nothing is generated");
    assert!(!u.sim_generating());
    play(&mut u, 40);
    assert!(u.replay.captures > 1, "captures {}", u.replay.captures);
    assert!(stat_n(&u, 0) > 1, "n = {}", stat_n(&u, 0));
    assert_eq!((u.m.ch[0].on, u.m.ch[1].on), chans, "no channel switched off");
    assert!(std::sync::Arc::ptr_eq(&u.refs[0].as_ref().unwrap().v, &before), "the ref is not overwritten");
    let caption = u.record_caption();
    assert!(caption.starts_with("SIM") && caption.contains("R1 ref1.csv"), "{caption}");
    assert_eq!(u.studio_source().0, "SIM");
    shot(&mut u, "sim-file-ref");
    // Live restores the session and leaves the ref as it was
    u.return_to_live();
    assert!(!u.record_mode() && !u.sim_mode());
    assert!(std::sync::Arc::ptr_eq(&u.refs[0].as_ref().unwrap().v, &before));
    assert_eq!((u.m.ch[0].on, u.m.ch[1].on), chans);
}

#[test]
fn sim_mode_plays_a_file_ref_and_an_added_ref_together() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    u.return_to_live();
    u.sim_preset_set("SATA I");
    assert!(u.sim_slot_set(1));
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    for _ in 0..3 { frame(&mut u); }
    let added = u.refs[1].as_ref().expect("R2 holds the added signal").v.clone();
    let file = u.refs[0].as_ref().unwrap().v.clone();
    assert_eq!(u.refs[1].as_ref().unwrap().label, "SATA I");
    u.refs[0].as_mut().unwrap().on = true;
    u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0)), (Meas::Vpp, WaveId::Ref(1))];
    let names = (u.refs[0].as_ref().unwrap().label.clone(), u.refs[1].as_ref().unwrap().label.clone());
    let colours = (u.refs[0].as_ref().unwrap().color, u.refs[1].as_ref().unwrap().color);
    u.activate(Item::SimPlay);
    assert_eq!(u.sim_refs, vec![0, 1]);
    play(&mut u, 40);
    assert!(stat_n(&u, 0) > 1, "R1 n = {}", stat_n(&u, 0));
    assert!(stat_n(&u, 1) > 1, "R2 n = {}", stat_n(&u, 1));
    let caption = u.record_caption();
    assert!(caption.contains("R1 ref1.csv") && caption.contains("R2 SATA I"), "{caption}");
    shot(&mut u, "sim-two-refs");
    u.return_to_live();
    assert!(!u.sim_mode());
    let (a, b) = (u.refs[0].as_ref().unwrap(), u.refs[1].as_ref().unwrap());
    assert!(std::sync::Arc::ptr_eq(&a.v, &file) && std::sync::Arc::ptr_eq(&b.v, &added), "both refs unchanged");
    assert_eq!(((a.label.clone(), b.label.clone()), (a.color, b.color)), (names, colours), "names and colours kept");
}

#[test]
fn sim_mode_without_refs_plays_the_sheet_signal() {
    let mut u = studio();
    assert!(u.sim_shown_refs().is_empty());
    u.sim_preset_set("SATA I");
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    play(&mut u, 6);
    assert!(u.session_is_sim && u.sim_refs.is_empty() && u.record_mode(), "the sheet signal, as before");
    assert_eq!(u.refs[u.sim_slot()].as_ref().map(|r| r.label.as_str()), Some("SATA I"));
    u.return_to_live();
    assert!(!u.record_mode() && !u.sim_mode());
}

#[test]
fn a_hidden_ref_is_not_played() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    u.return_to_live();
    u.refs[0].as_mut().unwrap().on = false;
    assert!(u.sim_shown_refs().is_empty());
    u.sim_preset_set("SATA I");
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    assert!(u.session_is_sim && u.sim_refs.is_empty(), "no shown ref: the sheet signal plays");
}
