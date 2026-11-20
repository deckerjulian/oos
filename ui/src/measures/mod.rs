// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement modules: a set of measurements with everything the scope needs around them, behind one registration.
//!
//! A measurement module is a type that implements [`MeasModule`] and one line in [`MODULES`]. That is all: the tiles
//! on the measurement page (name, description, drawn mark, the category and group they sit in), the source rows of
//! the page, saved setups (`cat` rows), SCPI (`CAT<id>` in `:MEAS:STRIP?`), the logger columns, the statistics and
//! the background worker that computes the values all come from the trait. The signal processing itself usually
//! lives in `oos-dsp` and the module calls it.
//!
//! A module computes all of its measurements that read the same sources in one go ([`MeasModule::compute`]), so
//! measurements that share an expensive intermediate (an eye diagram, a switching analysis) belong in one module. The
//! result is cached until a source, the gate or one of the module's [`settings`](MeasModule::settings) changes.
//!
//! The 60 standard measurements (`oos_dsp::measure::Meas`) and the 17 two-source variants are computed by the scope
//! itself with their own statistics and per-measurement settings; [`standard`] and [`pairs`] only place them on the
//! page. `docs/measurement-modules.md` walks through writing a module, and `example.rs` (compiled in the tests) uses
//! every hook.

use crate::model::{Model, WaveId, NCH};
use oos_dsp::measure::Meas;

pub mod double_pulse;
#[cfg(test)]
pub mod example;
pub mod jitter;
pub mod magnetic;
pub mod motor;
pub mod pairs;
pub mod power;
pub mod rails;
pub mod serial;
pub mod standard;

/// The registered modules. Within a group of the page, tiles appear in this order and then in the order of each
/// module's [`MeasModule::defs`]. Never reuse or change a measurement's `id`: saved setups and SCPI refer to it.
pub static MODULES: &[&dyn MeasModule] = &[
    &serial::Rate,
    &standard::Standard,
    &pairs::Pairs,
    &jitter::Jitter,
    &power::Quality,
    &power::Harmonics,
    &power::Inrush,
    &power::InputC,
    &rails::Rails,
    &power::Turn,
    &power::Switching,
    &magnetic::Magnetic,
    &power::Ripple,
    &power::LineRipple,
    &power::SwRipple,
    &power::Ratio,
    &motor::Motor,
    &motor::Mechanical,
    &double_pulse::DoublePulse,
    #[cfg(test)]
    &example::Example,
];

/// The drawn sign of a tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Pulse,
    Square,
    Edge,
    Eye,
    Bars,
    Loop,
    Slope,
}

/// Who computes a measurement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// a standard measurement of the scope (strip, statistics, per-measurement settings)
    Standard(Meas),
    /// a two-source variant of the scope (`pairs::PAIRS` index)
    Pair(usize),
    /// the module's [`MeasModule::compute`]; stored as a `cat` row with its `id`
    Module,
}

/// A place on the measurement page: category ([`BASIC`] ...) and the group's title.
pub type Place = (u8, &'static str);

/// One measurement a module offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Def {
    /// stable number in setups (`cat` rows) and over SCPI (`CAT<id>`), unique over all modules, never reused
    pub id: u16,
    /// name on the tile, in the Selected list and in messages
    pub name: &'static str,
    /// one sentence: what it measures
    pub blurb: &'static str,
    pub mark: Mark,
    /// SI unit of the value ("" for a count or a ratio)
    pub unit: &'static str,
    pub kind: Kind,
    /// the source slots it reads: 0 = A (Source, Voltage, Phase A ...), 1 = B, 2 = C, 3 = D
    pub roles: &'static [usize],
    /// where its tile is; a measurement may sit in more than one category
    pub places: &'static [Place],
}

/// A measurement the module computes.
pub const fn module(
    id: u16,
    name: &'static str,
    unit: &'static str,
    mark: Mark,
    roles: &'static [usize],
    places: &'static [Place],
    blurb: &'static str,
) -> Def {
    Def { id, name, blurb, mark, unit, kind: Kind::Module, roles, places }
}

/// A standard measurement of the scope on source A.
pub const fn standard(
    id: u16,
    name: &'static str,
    unit: &'static str,
    mark: Mark,
    k: Meas,
    places: &'static [Place],
    blurb: &'static str,
) -> Def {
    Def { id, name, blurb, mark, unit, kind: Kind::Standard(k), roles: &[0], places }
}

/// A two-source variant of the scope from source A to source B.
pub const fn pair(
    id: u16,
    name: &'static str,
    unit: &'static str,
    mark: Mark,
    k: usize,
    places: &'static [Place],
    blurb: &'static str,
) -> Def {
    Def { id, name, blurb, mark, unit, kind: Kind::Pair(k), roles: &[0, 1], places }
}

/// A category of the page (the list on its left): its source rows and the order of its groups.
pub struct Category {
    pub name: &'static str,
    /// the labels of the source rows A, B, C, D
    pub slots: [&'static str; 4],
    /// label of row A when it is the only one
    pub one: &'static str,
    /// source rows always shown; more when the measurement picked reads more
    pub min: usize,
    /// group titles in their order; a group a module names that is not here follows them
    pub groups: &'static [&'static str],
}

pub const BASIC: u8 = 0;
pub const TWO: u8 = 1;
pub const SERIAL: u8 = 2;
pub const JITTER: u8 = 3;
pub const POWER: u8 = 4;
pub const MOTOR: u8 = 5;
pub const RAILS: u8 = 6;
pub const DPT: u8 = 7;

const AB: [&str; 4] = ["Source A", "Source B", "Source C", "Source D"];

pub const CATEGORIES: [Category; 8] = [
    Category {
        name: "Basic",
        slots: AB,
        one: "Source",
        min: 1,
        groups: &["AMPLITUDE", "AVERAGE & STATISTICS", "TIMING", "EDGES & COUNTS", "PER CYCLE"],
    },
    Category {
        name: "Two sources",
        slots: AB,
        one: "Source",
        min: 2,
        groups: &["DELAY", "PHASE", "SKEW", "SETUP & HOLD"],
    },
    Category {
        name: "Serial & PAM",
        slots: AB,
        one: "Source",
        min: 1,
        groups: &["CLOCK & UNIT INTERVAL", "PAM LEVELS"],
    },
    Category {
        name: "Jitter & eye",
        slots: AB,
        one: "Source",
        min: 1,
        groups: &["JITTER", "EYE", "AMPLITUDE", "TIMING"],
    },
    Category {
        name: "Power",
        slots: ["Voltage source", "Current source", "Source C", "Source D"],
        one: "Voltage source",
        min: 2,
        groups: &[
            "INPUT",
            "AMPLITUDE",
            "TIMING",
            "SWITCHING",
            "MAGNETICS",
            "OUTPUT",
            "PSRR & IMPEDANCE",
            "POWER FACTOR",
        ],
    },
    Category {
        name: "Motor drive",
        slots: ["Phase A", "Phase B", "Phase C", "Torque"],
        one: "Phase A",
        min: 3,
        groups: &["DQ0 TRANSFORM", "SHAFT & EFFICIENCY"],
    },
    Category {
        name: "Power rails",
        slots: ["Rail", "Input", "Source C", "Source D"],
        one: "Rail",
        min: 2,
        groups: &["RAIL SEQUENCING"],
    },
    Category {
        name: "Double pulse",
        slots: ["Voltage", "Current", "Gate", "Source D"],
        one: "Voltage",
        min: 3,
        groups: &["SWITCHING", "DIODE RECOVERY"],
    },
];

/// The sources of one computation, on one common time grid (the slowest source's sample spacing over the time all of
/// them cover).
pub struct Ctx<'a> {
    pub model: &'a Model,
    /// time of the first sample (s) and the spacing (s)
    pub t0: f64,
    pub dt: f64,
    /// the row's sources A, B, C, D
    pub row: [WaveId; 4],
    pub waves: &'a [(WaveId, Vec<f32>)],
}

impl<'a> Ctx<'a> {
    /// The samples of source `id`, if the computation has it.
    pub fn wave(&self, id: WaveId) -> Option<&'a [f32]> {
        self.waves.iter().find(|(source, _)| *source == id).map(|(_, v)| v.as_slice())
    }

    /// The samples of several sources (all on the same grid), or `None` when one is missing.
    pub fn align(&self, ids: &[WaveId]) -> Option<Vec<&'a [f32]>> {
        ids.iter().map(|&id| self.wave(id)).collect()
    }
}

/// What a computation gives: a value per measurement (NaN = no value) and a line on how it was obtained.
pub struct Values {
    pub values: Vec<(u16, f64)>,
    pub quality: &'static str,
}

/// The quality line of a value that is derived directly from the sources.
pub const DERIVED: &str = "Derived from scaled source roles";

/// The values of the measurements `ids`, each `value(id)`, derived directly from the sources.
pub fn derived(ids: &[u16], value: impl Fn(u16) -> f64) -> Values {
    Values { values: ids.iter().map(|&id| (id, value(id))).collect(), quality: DERIVED }
}

/// A measurement module. Only `key` and `defs` are required; a module of [`Kind::Module`] measurements also needs
/// `compute`.
pub trait MeasModule: Sync {
    /// Stable name of the module (tests, docs): letters, digits and '_' only.
    #[cfg_attr(not(test), allow(dead_code))] // only the registry's tests read it; every module still names itself
    fn key(&self) -> &'static str;

    /// Its measurements.
    fn defs(&self) -> &'static [Def];

    /// The settings its values depend on (as bits): a change computes them again. Push them onto `v`.
    fn settings(&self, _m: &Model, _v: &mut Vec<u64>) {}

    /// More sources than the roles of `id`, and checks of the row, before anything is computed: push extra sources
    /// onto `ids`, or say why the row cannot be measured.
    fn sources(&self, _id: u16, _row: [WaveId; 4], _m: &Model, _ids: &mut Vec<WaveId>) -> Result<(), &'static str> {
        Ok(())
    }

    /// The values of the measurements `ids` (all of this module, all reading the same roles) on these sources. Runs
    /// on the worker thread, never on the render thread; a panic counts as "Catalog analysis failed".
    fn compute(&self, _x: &Ctx, ids: &[u16]) -> Values {
        derived(ids, |_| f64::NAN)
    }

    /// The quality line while a value of `id` is still computing.
    fn pending_quality(&self, _id: u16) -> &'static str {
        DERIVED
    }

    /// Why `id` has no value when it came out NaN (`None` = the empty value says enough).
    fn missing_reason(&self, _id: u16) -> Option<&'static str> {
        None
    }
}

/// The number of a source in settings keys.
pub fn wave_number(id: WaveId) -> u64 {
    match id {
        WaveId::Ch(c) => c as u64,
        WaveId::Ref(r) => (NCH + r) as u64,
    }
}

/// A setting as bits for [`MeasModule::settings`].
pub fn put(v: &mut Vec<u64>, x: f64) {
    v.push(x.to_bits());
}

/// Every measurement with the index of its module, in registration order.
pub fn all() -> impl Iterator<Item = (usize, &'static Def)> {
    MODULES.iter().enumerate().flat_map(|(i, m)| m.defs().iter().map(move |d| (i, d)))
}

/// The measurement `id` and the index of its module.
pub fn find(id: u16) -> Option<(usize, &'static Def)> {
    all().find(|(_, d)| d.id == id)
}

/// The measurement `id`.
pub fn def(id: u16) -> Option<&'static Def> {
    find(id).map(|x| x.1)
}

/// Module `i` of [`MODULES`].
pub fn get(i: usize) -> &'static dyn MeasModule {
    MODULES[i]
}

/// Whether `id` is stored as a `cat` row (a module's own measurement).
pub fn persisted(id: u16) -> bool {
    def(id).is_some_and(|d| d.kind == Kind::Module)
}

/// The groups of category `cat` with their tiles, in the order of the page.
pub fn groups(cat: u8) -> &'static [(&'static str, Vec<&'static Def>)] {
    type Groups = Vec<(&'static str, Vec<&'static Def>)>;
    static G: std::sync::OnceLock<Vec<Groups>> = std::sync::OnceLock::new();
    let all = G.get_or_init(|| {
        (0..CATEGORIES.len() as u8)
            .map(|c| {
                let mut names: Vec<&'static str> = CATEGORIES[c as usize].groups.to_vec();
                for (_, d) in all() {
                    for &(pc, g) in d.places {
                        if pc == c && !names.contains(&g) {
                            names.push(g);
                        }
                    }
                }
                names
                    .into_iter()
                    .map(|g| (g, all().filter(|(_, d)| d.places.contains(&(c, g))).map(|(_, d)| d).collect::<Vec<_>>()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect()
            })
            .collect()
    });
    &all[(cat as usize).min(all.len() - 1)]
}

/// The number of the tile of standard measurement `k`.
pub fn std_id(k: Meas) -> u16 {
    all().find(|(_, d)| d.kind == Kind::Standard(k)).map_or(0, |(_, d)| d.id)
}

/// The number of the tile of pair variant `k`.
#[cfg(test)]
pub fn pair_id(k: usize) -> u16 {
    all().find(|(_, d)| d.kind == Kind::Pair(k)).map_or(0, |(_, d)| d.id)
}

/// The name a standard measurement has on the page ("Peak-to-Peak"; the strip keeps the short one).
pub fn meas_name(k: Meas) -> &'static str {
    def(std_id(k)).map_or(k.name(), |d| d.name)
}

/// The name a pair variant has on the page.
pub fn pair_name(k: usize) -> &'static str {
    all().find(|(_, d)| d.kind == Kind::Pair(k)).map_or(pairs::pair_short_name(k), |(_, d)| d.name)
}

/// Persisted catalog IDs are only the extra Wave rows; ordinary Meas/pairs retain their existing formats.
pub fn catalog_id_valid(id: u16) -> bool {
    persisted(id)
}

// Which catalog rows the model may keep is the registry's to say (ADR 0049)
impl crate::model::Model {
    /// `fit_platform` for the measurement, catalogue, analyser and math sources on a missing input.
    pub(crate) fn fit_meas_sources(&mut self, n: usize, mut changed: bool) -> bool {
        for e in &mut self.meas {
            if let WaveId::Ch(c) = e.1 {
                if c >= n {
                    e.1 = WaveId::Ch(0);
                    changed = true;
                }
            }
        }
        for e in &mut self.meas2 {
            for id in [&mut e.1, &mut e.2] {
                if let WaveId::Ch(c) = *id {
                    if c >= n {
                        *id = WaveId::Ch(0);
                        changed = true;
                    }
                }
            }
        }
        let previous_cat = self.cat.len();
        self.cat.retain(|row| catalog_id_valid(row.0));
        self.cat.truncate(crate::model::MEAS_SELECTION_MAX.saturating_sub(self.meas.len() + self.meas2.len()));
        changed |= previous_cat != self.cat.len();
        for row in &mut self.cat {
            for source in [&mut row.1, &mut row.2, &mut row.3, &mut row.4] {
                if let WaveId::Ch(c) = *source {
                    if c >= n {
                        *source = WaveId::Ch(0);
                        changed = true;
                    }
                } else if matches!(*source, WaveId::Ref(i) if i >= crate::model::NREF) {
                    *source = WaveId::Ref(0);
                    changed = true;
                }
            }
        }
        for s in [
            &mut self.inv_tq,
            &mut self.inv_ev,
            &mut self.inv_ei,
            &mut self.eye_b,
            &mut self.eye_rail,
            &mut self.pw_vin,
            &mut self.pw_clk,
        ] {
            if let Some(WaveId::Ch(c)) = *s {
                if c >= n {
                    *s = None;
                    changed = true;
                }
            }
        }
        // math sources: channels 0..NCH, then math channels (always there)
        for s in [&mut self.math_a, &mut self.math_b]
            .into_iter()
            .chain(self.maths.iter_mut().flat_map(|m| [&mut m.a, &mut m.b]))
        {
            if !crate::model::src_ok(*s) {
                *s = 0;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Numbers and names are unique; keys are safe; every measurement has a place in a category, roles in range and
    /// a description; every standard measurement and every pair variant has exactly one tile.
    #[test]
    fn modules_are_well_formed() {
        let (mut ids, mut names, mut keys) = (HashSet::new(), HashSet::new(), HashSet::new());
        for m in MODULES {
            assert!(
                !m.key().is_empty() && m.key().chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "key {:?}",
                m.key()
            );
            assert!(keys.insert(m.key()), "key {} twice", m.key());
            assert!(!m.defs().is_empty(), "{}: no measurements", m.key());
            for d in m.defs() {
                assert!(ids.insert(d.id), "{}: id {} twice", m.key(), d.id);
                assert!(names.insert(d.name), "{}: name {} twice", m.key(), d.name);
                assert!(
                    !d.places.is_empty() && d.places.iter().all(|p| (p.0 as usize) < CATEGORIES.len()),
                    "{}: place",
                    d.name
                );
                assert!(!d.roles.is_empty() && d.roles.iter().all(|&r| r < 4), "{}: roles", d.name);
                assert!(d.blurb.ends_with('.'), "{}: description", d.name);
            }
        }
        for &k in oos_dsp::measure::ALL.iter() {
            assert_eq!(all().filter(|(_, d)| d.kind == Kind::Standard(k)).count(), 1, "{k:?}");
        }
        for k in 0..crate::model::PAIRS_N {
            assert_eq!(all().filter(|(_, d)| d.kind == Kind::Pair(k)).count(), 1, "pair {k}");
        }
        // what setups store is what the setup reader takes
        for id in 0..=u16::MAX {
            assert_eq!(catalog_id_valid(id), persisted(id), "id {id}");
        }
    }

    /// A pair tile shows the unit its strip row and SCPI readout carry (Setup + Hold is a count of violations).
    #[test]
    fn pair_tiles_have_the_unit_of_their_value() {
        for k in 0..crate::model::PAIRS_N {
            let d = all().find(|(_, d)| d.kind == Kind::Pair(k)).unwrap().1;
            assert_eq!(d.unit, pairs::pair_unit(k), "{}", d.name);
        }
    }

    /// Every category has tiles, and its groups come in the order it names.
    #[test]
    fn categories_have_their_groups() {
        for c in 0..CATEGORIES.len() as u8 {
            let g = groups(c);
            assert!(!g.is_empty(), "{}", CATEGORIES[c as usize].name);
            let named: Vec<_> = g.iter().map(|x| x.0).filter(|n| CATEGORIES[c as usize].groups.contains(n)).collect();
            let order: Vec<_> = CATEGORIES[c as usize].groups.iter().copied().filter(|n| named.contains(n)).collect();
            assert_eq!(named, order);
        }
    }

    /// Every module's measurement gives a number (or NaN) on a plain square wave, without panicking, for every id
    /// group it can be asked for.
    #[test]
    fn modules_compute_without_panicking() {
        let m = Model::default();
        let n = 4000;
        let sq: Vec<f32> = (0..n).map(|k| if (k / 100) % 2 == 0 { 1.0 } else { 0.0 }).collect();
        let row = [WaveId::Ch(0), WaveId::Ch(1), WaveId::Ch(2), WaveId::Ch(3)];
        let waves: Vec<(WaveId, Vec<f32>)> = row.iter().map(|&id| (id, sq.clone())).collect();
        let x = Ctx { model: &m, t0: 0.0, dt: 1e-6, row, waves: &waves };
        for md in MODULES {
            let ids: Vec<u16> = md.defs().iter().filter(|d| d.kind == Kind::Module).map(|d| d.id).collect();
            if ids.is_empty() {
                continue;
            }
            let v = md.compute(&x, &ids);
            assert_eq!(
                v.values.iter().map(|x| x.0).collect::<Vec<_>>(),
                ids,
                "{}: one value per id, in order",
                md.key()
            );
        }
    }

    /// The groups the table of docs/reference-analysis.md lists for category `cat`, in lower case.
    fn documented_groups(doc: &str, cat: &str) -> Vec<String> {
        let row = doc.lines().find(|l| l.starts_with(&format!("| {} |", cat))).unwrap_or("");
        let cell = row.split('|').nth(2).unwrap_or("");
        cell.split(',').map(|g| g.split('(').next().unwrap_or("").trim().to_lowercase()).collect()
    }

    /// What a measurement needs besides its `Def`, each failure with its name and the part that is missing: a name
    /// and a description, its group in the table of docs/reference-analysis.md, and, when setups store its number,
    /// its line in ui/tests/golden/measurement-ids.txt (number, module key, name). The numbers of that list never
    /// change: `OOS_BLESS=1 cargo test --locked -q measures::tests` adds the line of a new measurement and no other.
    #[test]
    fn every_measurement_is_complete() {
        let doc = crate::ui::testkit::document("docs/reference-analysis.md");
        let mut stored = Vec::new();
        // the example module is compiled in the tests only
        for m in MODULES.iter().filter(|m| m.key() != "example") {
            for d in m.defs() {
                assert!(!d.name.is_empty() && d.blurb.len() > 1, "{} {}: a name and a description", m.key(), d.id);
                for &(c, group) in d.places {
                    let cat = CATEGORIES[c as usize].name;
                    assert!(
                        documented_groups(&doc, cat).contains(&group.to_lowercase()),
                        "{}: its group {} is not in the row of {} in docs/reference-analysis.md",
                        d.name,
                        group,
                        cat
                    );
                }
                if d.kind == Kind::Module {
                    stored.push(format!("{} {} {}", d.id, m.key(), d.name));
                }
            }
        }
        crate::ui::testkit::stored_names("measurement-ids.txt", &stored);
    }
}
