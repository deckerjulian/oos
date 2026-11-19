// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Soak test: a script of typical use runs many times over, and the memory in use has to reach a plateau. The scope
//! runs for days; a buffer that is kept once per acquisition, per menu or per remote request shows in no short test.
//!
//! How it decides. The test build counts the heap bytes in use (`Counting`, the allocator of the test program only).
//! `CYCLE` runs `CYCLES` times on one demo screen; at the end of each cycle the count is noted. After `WARM_UP`
//! cycles the notes have to be equal, byte for byte (`plateau`): there is no allowance. What grows up to a bound by
//! design is brought to its bound before the first cycle (`Rig::new`), the rest fills in the warm-up (the history of
//! 200 records). The bound of the test is the number of cycles, never a time.
//!
//! The quiet point. A count is only comparable when no other thread holds a buffer for a moment. This cycle starts
//! none: on the demo screen the measurements are made in the frame (the `measure` thread is the unit's,
//! `Ui::measure_in_background`), and the workers of the analysers are left out. Those start for records above
//! 262 144 points (reference measurements, math, search, decoding, the bus trace, the eye), for the modules of the
//! measurement catalog, for pairs with more than 8192 edges and in SIM. The test does not trust that: the allocator
//! counts the calls of every other thread while the cycles run, and one call fails the test (`run`). A cycle that
//! starts a worker has to wait for the worker to let go of its job (an event or a join, not a sleep) before the
//! count is noted; a worker's answer does not say that, the job is dropped after it.
//!
//! Alone in a process. The other tests of this program run on threads next to each other and allocate too, so the
//! count of the process means nothing there. `memory_reaches_a_plateau` therefore starts this program again with
//! `cycles` as the only test (`child`), and `a_buffer_kept_per_cycle_is_reported` does the same with a buffer of
//! `KEPT` bytes kept per cycle on purpose: the check has to report it, else it proves nothing.
//!
//! Not covered: the web server's threads (the `/mcp` handler runs in the cycle; the sockets, the sender and the PNG
//! thread do not), the workers named above, the hardware source with its read-out and deposit threads. A list that
//! holds many entries before the first cycle hides a few more per cycle in its spare room; one that starts empty
//! shows (`CYCLES`). Memory that does not come from Rust's allocator is not counted; `oos-ui` has none on the host.
//!
//! By hand, with the count of every cycle:
//! `OOS_SOAK=plateau cargo test -- --ignored --exact --nocapture --test-threads=1 soak::cycles`

use crate::gfx::Canvas;
use crate::scpi::{Program, Scpi};
use crate::ui::testkit::Scratch;
use crate::ui::{Ui, KEY_NAMES, KNOB_NAMES};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering::Relaxed};

/// The system's allocator, counting while `COUNT` is set. Switched off (every other test) it costs one load of a
/// flag that nobody writes per call. Growing a block and a zeroed block come from `alloc` and `dealloc` (the trait's
/// own `realloc` and `alloc_zeroed`), so the count has two places only.
struct Counting;

#[global_allocator]
static COUNTING: Counting = Counting;

/// Set by the soak run at its start, never cleared.
static COUNT: AtomicBool = AtomicBool::new(false);
/// Heap bytes in use, of all threads, since `COUNT` was set.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// Calls of the allocator by threads other than the soak's, since `COUNT` was set.
static ELSEWHERE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// This thread runs the cycles. Without a destructor and with a constant start value: reading it allocates nothing.
    static SOAK: Cell<bool> = const { Cell::new(false) };
}

fn count(bytes: isize) {
    if COUNT.load(Relaxed) {
        LIVE.fetch_add(bytes, Relaxed);
        if !SOAK.get() {
            ELSEWHERE.fetch_add(1, Relaxed);
        }
    }
}

// SAFETY: every call goes to `System` with the layout it came with; the counters touch no memory of the caller.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size() as isize);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        count(-(layout.size() as isize));
        System.dealloc(block, layout)
    }
}

/// Cycles before the counts are compared: the history holds its 200 records after the second (`cycles` prints the
/// counts), the rest is margin.
const WARM_UP: usize = 4;
/// Cycles in all. More than twice the warm-up, because a list shows its growth only when it takes more room, and it
/// takes twice the room each time: a list that starts empty and gets an entry per cycle holds, at the end of the
/// warm-up, room for less than twice its entries, so it has to take more before the cycle count has doubled.
const CYCLES: usize = 10;
const _: () = assert!(CYCLES > 2 * WARM_UP);
/// Bytes kept per cycle on purpose by `a_buffer_kept_per_cycle_is_reported`.
const KEPT: usize = 16;

/// One cycle, in the text commands of `cmd.rs`. `scpi - <message>` goes through the interpreter (the `-` stands for
/// the file of the script runner), `mcp <body>` through the web server's `/mcp` handler. The cycle ends where it
/// began: on the default set-up, with an empty storage folder.
const CYCLE: &str = r#"
# the default set-up running: CH1, edge trigger, sweep Auto
frames 2
# two more channels, one off again (select, menu, off)
key ch2
key ch3
frames 1
key ch3
key ch3
close
# sweep Normal, a Single with the wait for its record, Auto again
scpi - :TRIGger:SWEep NORMal;:TRIGger:SWEep?
frames 1
scpi - :SINGle;*OPC?
key runstop
scpi - :TRIGger:SWEep AUTO
frames 1
# another trigger type and back, from the menu
menu Trigger
set Type = Pulse
frames 1
set Type = Edge
close
# three measurements from the page, with statistics, and off again
key measure
do Amplitude
do Period
do Fall Time
close
menu Measure settings
set Statistics = On
close
frames 2
key measure
do Amplitude
do Period
do Fall Time
close
# cursors on and off
key cursor
frames 1
key cursor
# zoom, moved, and back; time base and vertical scale by the knobs
knob hscale push
knob hpos +5
frames 1
knob hscale push
knob hscale -2
knob vscale +1
frames 1
knob hscale +2
knob vscale -1
# persistence
menu Display
set Persistence = 1 s
close
frames 1
# math
menu Math
set Category = Arithmetic
set Operation = A+B
close
frames 1
# a reference, shown and cleared
key ref
do Save to next R
frames 1
do Clear all
close
# history: one record back, and live again
key navback
frames 1
key navplay
frames 1
# storage: set-up, CSV, PNG
menu Storage
do Save setup
do Save waveform (CSV)
do Save screenshot (PNG)
frames 1
close
# remote: queries, a record, an unknown command and its error; a tool list, a command and a screenshot over MCP
scpi - *IDN?
scpi - :MEASure:ITEM? VPP,CHANnel1
scpi - :MEASure:ITEM? FREQuency,CHANnel2
scpi - :WAVeform:SOURce CHANnel1;:WAVeform:MODE NORMal;:WAVeform:FORMat ASCii
scpi - :WAVeform:PREamble?
scpi - :WAVeform:DATA?
scpi - :NOPE?
scpi - :SYSTem:ERRor?
mcp {"jsonrpc":"2.0","id":1,"method":"tools/list"}
mcp {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":TIMebase:SCALe?"}}}
mcp {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"screenshot"}}
# the default set-up
key default
frames 1
"#;

/// The demo screen with what the script runner of `main.rs` has around it: a canvas, an SCPI interpreter, a storage
/// folder.
struct Rig {
    ui: Ui,
    cv: Canvas,
    scpi: Scpi,
    shots: u32,
}

impl Rig {
    fn new(store: &std::path::Path) -> Rig {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.store_dir = store.to_path_buf();
        // What grows to a bound by design, at its bound before the first cycle. The receipts of finished operations:
        // a Single adds one, up to 128 (`Ui::scpi_finish_operation`), one per cycle here.
        ui.scpi_receipts_fill();
        // The time zone is looked up once a minute of the screen's time and stored at the first look
        // (`Ui::ntp_poll`): a frame a minute long.
        ui.step(60.0);
        Rig { ui, cv: Canvas::new(crate::ui::W, crate::ui::H), scpi: Scpi::open(0), shots: 0 }
    }

    /// One frame as the script runner makes it (`run_script` in `main.rs`): step, render, a requested screenshot.
    fn frame(&mut self) {
        self.ui.step(1.0 / 30.0);
        self.ui.render(&mut self.cv);
        if let Some(path) = self.ui.shot_request.take() {
            crate::save_shot(&mut self.ui, &self.cv, path, &mut self.shots);
        }
    }

    /// One line of `CYCLE`. A line that does nothing would make the cycle smaller without notice (`cmd::run` only
    /// prints what it does not know): a command or a name that is not there stops the test.
    fn line(&mut self, line: &str) {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let known = |found: bool| if found { Ok(()) } else { Err(format!("no such {}", word)) };
        let checked = match word {
            "menu" => self.ui.script_menu(rest),
            "do" => self.ui.script_do(rest),
            "set" => rest.split_once(" = ").map_or(Err("no value".into()), |(l, v)| self.ui.script_set(l, v)),
            "mcp" => known(
                crate::site::mcp(rest, &mut crate::site::Ctx::new(&mut self.ui, &self.cv, &mut self.scpi))
                    .is_some_and(|a| a.contains("\"result\"")),
            ),
            "key" => known(KEY_NAMES.iter().any(|(name, _)| *name == rest)),
            "knob" => known(KNOB_NAMES.iter().any(|(name, _)| rest.starts_with(name))),
            "frames" | "scpi" | "close" => Ok(()),
            _ => Err("not a command of the cycle".into()),
        };
        if let Err(e) = checked {
            panic!("{}: {}", line, e);
        }
        if matches!(word, "menu" | "do" | "set" | "mcp") {
            return;
        }
        match crate::cmd::run(&mut self.ui, line) {
            crate::cmd::Out::Frames(n) => (0..n).for_each(|_| self.frame()),
            crate::cmd::Out::Scpi(_, message) => {
                // an answer that waits for an operation gets frames until it is there, 600 at most (as `run_script`)
                let mut program = Program::new(&message);
                let mut answer = self.scpi.poll_program(&mut self.ui, &self.cv, &mut program);
                for _ in 0..600 {
                    if answer.is_some() {
                        break;
                    }
                    self.frame();
                    answer = self.scpi.poll_program(&mut self.ui, &self.cv, &mut program);
                }
                assert!(answer.is_some(), "{}: no answer after 600 frames", line);
            }
            crate::cmd::Out::Shot(_) | crate::cmd::Out::None => {}
        }
    }

    /// `CYCLE` once; the storage folder is emptied, so the next cycle writes the same names.
    fn cycle(&mut self) {
        let records = self.ui.records;
        CYCLE.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).for_each(|line| self.line(line));
        assert!(self.ui.records > records, "the cycle acquired nothing");
        let stored: Vec<_> = std::fs::read_dir(&self.ui.store_dir).expect("storage folder").flatten().collect();
        assert_eq!(stored.len(), 3, "a set-up, a CSV and a PNG: {:?}", stored);
        for file in stored {
            std::fs::remove_file(file.path()).expect("remove a stored file");
        }
    }
}

/// The counts after the warm-up are equal; else the first cycle that differs from the one before it, and all counts.
/// `live[k]` is the count at the end of cycle `k + 1`.
fn plateau(live: &[isize], warm_up: usize) -> Result<(), String> {
    assert!(warm_up >= 1 && live.len() > warm_up);
    let Some(k) = (warm_up..live.len()).find(|&k| live[k] != live[k - 1]) else { return Ok(()) };
    let mut text = format!(
        "{:+} bytes in cycle {}, after a warm-up of {} cycles. Bytes in use at the end of each cycle:\n",
        live[k] - live[k - 1],
        k + 1,
        warm_up
    );
    for (k, bytes) in live.iter().enumerate() {
        let step = if k == 0 { String::new() } else { format!(" ({:+})", bytes - live[k - 1]) };
        text += &format!("  {:2}: {}{}{}\n", k + 1, bytes, step, if k < warm_up { " warm-up" } else { "" });
    }
    Err(text)
}

/// `cycles` cycles on this thread. At the end of each: the bytes in use, and how many of them are kept on purpose
/// (`keep`: a buffer of `KEPT` bytes more per cycle, and the list that holds them). Only alone in a process: stops
/// when another thread used the heap meanwhile.
fn run(cycles: usize, keep: bool) -> Vec<(isize, isize)> {
    SOAK.set(true);
    COUNT.store(true, Relaxed);
    let store = Scratch::new("soak");
    std::fs::create_dir_all(&*store).expect("storage folder");
    let mut rig = Rig::new(&store);
    let mut kept: Vec<Vec<u8>> = Vec::new();
    // room for all counts now: the list of counts must not grow between them
    let mut live = Vec::with_capacity(cycles);
    let elsewhere = ELSEWHERE.load(Relaxed);
    for _ in 0..cycles {
        rig.cycle();
        if keep {
            kept.push(vec![0; KEPT]);
        }
        let on_purpose = kept.len() * KEPT + kept.capacity() * std::mem::size_of::<Vec<u8>>();
        live.push((LIVE.load(Relaxed), on_purpose as isize));
    }
    let calls = ELSEWHERE.load(Relaxed) - elsewhere;
    assert_eq!(
        calls, 0,
        "another thread used the heap during the cycles ({} calls): the counts are not taken at a quiet point. A \
         worker that the cycle starts has to be waited for before the count (soak.rs, the quiet point)",
        calls
    );
    live
}

/// The variable that tells `cycles` what to do (`plateau` or `kept`); without it `cycles` does nothing.
const MODE: &str = "OOS_SOAK";

/// This test program again, with `cycles` as its only test, alone in a process. Its output, when it passed.
fn child(mode: &str) -> String {
    let name = format!("{}::cycles", module_path!().split_once("::").expect("a module of the crate").1);
    let out = std::process::Command::new(std::env::current_exe().expect("the test program"))
        .args(["--exact", &name, "--ignored", "--nocapture", "--test-threads=1"])
        .env(MODE, mode)
        // the clock on the screen and in file names stands still: a count must not change with the time of day
        .env("OOS_FIXED_TIME", "1762862400")
        .output()
        .expect("start the test program again");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{} failed:\n{}", name, text);
    // a filter that matches nothing passes too
    assert!(text.contains("1 passed"), "{} did not run:\n{}", name, text);
    text
}

/// The line of `cycles` with its counts, for a run with `--nocapture`.
fn counts(output: &str) -> &str {
    output.lines().find(|line| line.starts_with("soak:")).unwrap_or("soak: no counts")
}

#[test]
fn memory_reaches_a_plateau() {
    println!("{}", counts(&child("plateau")));
}

/// The proof that the check sees what it is for: the same cycle with a small buffer kept each time.
#[test]
fn a_buffer_kept_per_cycle_is_reported() {
    println!("{}", counts(&child("kept")));
}

#[test]
#[ignore = "started by the other soak tests, alone in a process"]
fn cycles() {
    let Ok(mode) = std::env::var(MODE) else {
        eprintln!("soak: {} is not set, nothing to do (see soak.rs)", MODE);
        return;
    };
    let keep = mode == "kept";
    // the kept buffer shows in the first cycle after the warm-up; more cycles would only show it again
    let counts = run(if keep { WARM_UP + 2 } else { CYCLES }, keep);
    let live: Vec<isize> = counts.iter().map(|(live, _)| *live).collect();
    let verdict = plateau(&live, WARM_UP);
    eprintln!("soak: {:?}", live);
    if keep {
        let report = verdict.expect_err("a buffer kept per cycle went unnoticed");
        assert!(report.contains(&format!("in cycle {},", WARM_UP + 1)), "{}", report);
        // and it is this buffer that was reported: without the bytes kept on purpose the counts are a plateau
        let rest: Vec<isize> = counts.iter().map(|(live, on_purpose)| live - on_purpose).collect();
        plateau(&rest, WARM_UP).expect("only the bytes kept on purpose grow");
    } else if let Err(report) = verdict {
        panic!("the memory in use grows: {}", report);
    }
}

#[test]
fn plateau_wants_equal_counts_after_the_warm_up() {
    assert!(plateau(&[10, 70, 90, 90, 90, 90], 3).is_ok());
    // still filling after the warm-up
    assert!(plateau(&[10, 70, 90, 90], 2).unwrap_err().starts_with("+20 bytes in cycle 3,"));
    // a buffer per cycle
    let report = plateau(&[10, 90, 90, 106, 122, 138], 2).unwrap_err();
    assert!(report.starts_with("+16 bytes in cycle 4,"), "{}", report);
    // a list that took more room once
    assert!(plateau(&[10, 90, 90, 90, 90, 122, 122], 2).unwrap_err().starts_with("+32 bytes in cycle 6,"));
    // less is not a plateau either: the cycles are not the same then
    assert!(plateau(&[10, 90, 90, 90, 74], 2).unwrap_err().starts_with("-16 bytes in cycle 5,"));
}
