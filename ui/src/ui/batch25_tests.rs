//! Batch 25: SIM hides the live channel pills, the bottom bar shares its width by pill count, and Load file is on the
//! Reference and SIM sheets. OOS_SHOTS=<dir> writes pictures (b25-*).
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    let dir = std::env::temp_dir().join(format!("oos-batch25-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b25-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn csv_bytes() -> Vec<u8> {
    let mut csv = String::from("Time (s),Voltage (V)\n");
    for i in 0..4_000 { csv.push_str(&format!("{:e},{}\n", i as f64 * 1e-7, (i as f64 * 0.02).sin())); }
    csv.into_bytes()
}

fn load_csv(u: &mut Ui, slot: usize, name: &str) {
    std::fs::write(u.store_dir.join(name), csv_bytes()).unwrap();
    u.load_waveform_bytes_at(&csv_bytes(), name, slot).expect("the CSV loads");
}

/// Every pill rectangle of the bar that is on it.
fn pills(u: &Ui) -> Vec<Rect> {
    let b = u.studio_bottom();
    b.ch.iter().chain(b.refs.iter()).chain(b.math.iter()).copied().filter(|r| r.w > 0).collect()
}

/// No pill overlaps another or reaches the time base, and the chips after them clear the time base too.
fn assert_clear(u: &Ui) {
    let b = u.studio_bottom();
    let mut all = pills(u);
    all.extend([b.add].into_iter().filter(|r| r.w > 0));
    all.extend(b.la);
    all.extend(b.gen);
    all.sort_by_key(|r| r.x);
    for w in all.windows(2) { assert!(w[0].right() <= w[1].x, "{:?} overlaps {:?}", (w[0].x, w[0].w), (w[1].x, w[1].w)); }
    let last = all.last().unwrap();
    assert!(last.right() <= b.time.x - 5, "the pills run into the time base: {} > {}", last.right(), b.time.x - 5);
}

#[test]
fn sim_hides_the_live_channel_pills_and_live_restores_them() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    u.return_to_live();
    u.sim_preset_set("SATA I");
    assert!(u.sim_slot_set(1));
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    for _ in 0..3 { frame(&mut u); }
    u.refs[0].as_mut().unwrap().on = true;
    let before = u.studio_bottom();
    assert!(before.ch[0].w > 0 && before.ch[1].w > 0, "the live pills are on the bar in Live");
    let live_rects = (before.ch[0], before.ch[1]);
    u.activate(Item::SimPlay);
    assert_eq!(u.sim_refs, vec![0, 1]);
    for _ in 0..6 { frame(&mut u); }
    let b = u.studio_bottom();
    assert!(b.ch.iter().all(|r| r.w == 0), "CH pills are off the bar while SIM plays refs");
    assert!(b.refs[0].w > 0 && b.refs[1].w > 0, "the playing R pills stay");
    assert!(b.add.w > 0, "+ CH stays");
    assert!(u.m.ch[0].on && u.m.ch[1].on, "the channels themselves are untouched");
    assert_clear(&u);
    shot(&mut u, "sim-two-refs");
    u.return_to_live();
    assert!(!u.sim_mode());
    let b = u.studio_bottom();
    assert_eq!((b.ch[0], b.ch[1]), live_rects, "Live restores the pills unchanged");
}

#[test]
fn pills_share_the_bar_by_count_and_never_reach_the_right_controls() {
    let mut u = studio();
    u.m.ch[1].on = false;
    load_csv(&mut u, 0, "wave1.csv");
    u.refs[0].as_mut().unwrap().on = true;
    let two = pills(&u);
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].w, 170, "few pills: as wide as a channel pill was");
    assert_clear(&u);
    shot(&mut u, "bar-2-pills");
    let mut last = two[0].w;
    for n in 2..=10 {
        for i in 0..n { if u.refs[i].is_none() { load_csv(&mut u, i, &format!("wave{}.csv", i + 1)); } }
        for r in u.refs.iter_mut().flatten() { r.on = true; }
        let p = pills(&u);
        assert_eq!(p.len(), n + 1);
        assert!(p.iter().all(|r| r.w == p[0].w), "equal widths");
        assert!(p[0].w <= last, "{n}: {} > {last}", p[0].w);
        last = p[0].w;
        assert_clear(&u);
    }
    // every channel and every ref: the smallest pills still clear the controls
    for c in 0..n_ch() { u.m.ch[c].on = true; }
    assert_clear(&u);
    assert!(pills(&u)[0].w >= 18 && pills(&u)[0].w < 60);
    shot(&mut u, "bar-full");
    // about 8 pills for the picture
    for i in 5..NREF { u.refs[i] = None; }
    for c in 0..n_ch() { u.m.ch[c].on = c < 3; }
    assert_eq!(pills(&u).len(), 8);
    assert_clear(&u);
    shot(&mut u, "bar-8-pills");
}

#[test]
fn load_file_on_the_desktop_asks_the_native_explorer_for_the_selected_slot() {
    let mut u = studio();
    u.desktop_panel = Some(false);
    u.ref_sel = 3;
    u.activate(Item::RefImport);
    assert!(u.open_wave_request && u.waveform_destination() == Some(3));
    assert!(!u.menu_open);
    u.open_wave_request = false;
    assert!(u.sim_slot_set(5));
    u.activate(Item::SimLoad);
    assert!(u.open_wave_request && u.waveform_destination() == Some(5), "the SIM sheet loads into the SIM slot");
}

#[test]
fn the_reference_and_sim_sheets_carry_a_load_file_row() {
    let mut u = studio();
    u.ref_sel = 1;
    u.open_menu(MenuId::Ref);
    let spec = u.sheet_spec(MenuId::Ref).expect("a sheet");
    assert!(spec.tabs[0].rows.contains(&Item::RefImport), "Load file is on the first tab of Reference");
    assert_eq!(u.sheet_label(Item::RefImport), "Load file into R2\u{2026}");
    shot(&mut u, "ref-sheet");
    u.open_menu(MenuId::Sim);
    let spec = u.sheet_spec(MenuId::Sim).expect("a sheet");
    assert!(spec.tabs[0].rows.contains(&Item::SimLoad), "and of SIM");
    assert!(u.sheet_label(Item::SimLoad).starts_with("Load file into R"));
    shot(&mut u, "sim-sheet");
    // the classic layouts keep their SIM menu
    u.m.layout = Layout::Default;
    assert!(!u.sim_items().contains(&Item::SimLoad));
}

#[test]
fn load_file_on_the_scope_lists_the_storage_files_and_loads_the_tapped_one() {
    let mut u = studio();
    assert!(u.desktop_panel.is_none());
    for n in ["alpha.csv", "beta.csv", "gamma.csv"] { std::fs::write(u.store_dir.join(n), csv_bytes()).unwrap(); }
    std::fs::write(u.store_dir.join("notes.txt"), b"not a waveform").unwrap();
    u.ref_sel = 2;
    u.open_menu(MenuId::Ref);
    u.activate(Item::RefImport);
    assert!(u.menu == MenuId::Pick && u.pick.is_some_and(|p| p.it == Item::RefImport), "the list of files opens");
    assert!(u.sheet_active(), "as a sheet, not the classic picker");
    let names = u.options(Item::RefImport);
    assert_eq!(names, ["alpha.csv", "beta.csv", "gamma.csv"], "waveform files only");
    shot(&mut u, "load-file-list");
    let k = names.iter().position(|n| n == "beta.csv").unwrap();
    u.activate(Item::PickOpt(k));
    assert_eq!(u.refs[2].as_ref().map(|r| r.label.as_str()), Some("beta.csv"), "loaded into the selected slot R3");
    // an empty storage says so
    let dir = u.store_dir.join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir;
    u.open_menu(MenuId::Ref);
    u.activate(Item::RefImport);
    assert!(u.menu != MenuId::Pick);
}

#[test]
fn a_file_loaded_while_sim_plays_refs_joins_the_playback() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    u.return_to_live();
    u.refs[0].as_mut().unwrap().on = true;
    u.activate(Item::SimPlay);
    assert_eq!(u.sim_refs, vec![0]);
    for _ in 0..10 { frame(&mut u); }
    std::fs::write(u.store_dir.join("second.csv"), csv_bytes()).unwrap();
    std::fs::write(u.store_dir.join("third.csv"), csv_bytes()).unwrap();
    u.open_menu(MenuId::Sim);
    assert!(u.sim_slot_set(1));
    u.activate(Item::SimLoad);
    let names = u.options(Item::RefImport);
    let k = names.iter().position(|n| n == "second.csv").unwrap();
    u.activate(Item::PickOpt(k));
    assert!(u.sim_mode() && u.record_mode(), "SIM keeps playing");
    assert_eq!(u.sim_refs, vec![0, 1], "R2 is in the playback");
    for _ in 0..30 { frame(&mut u); }
    assert!(u.replay.captures > 1);
    let b = u.studio_bottom();
    assert!(b.ch.iter().all(|r| r.w == 0) && b.refs[0].w > 0 && b.refs[1].w > 0);
    shot(&mut u, "sim-loaded");
    u.return_to_live();
    assert!(!u.sim_mode() && u.studio_bottom().ch[0].w > 0);
}
