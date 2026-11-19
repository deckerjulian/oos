# ADR 0068: The comparison of the programs for the scope leaves out the places of their panics and their symbol names

- Status: accepted (2026-11-19, by the maintainer)
- Date: 2026-11-19
- Category: process

## Context

ADR 0041 lets `unit` pass when the programs for the scope (oos-ui, oos-fpgad, oos-update) are byte-identical to the
base's. On 2026-11-19 three changes that do not change what the programs do still came out different:

- **A moved line.** One comment line added near the top of `hal/src/dho/seq/samples.rs` changed all three programs.
  Every panic in Rust carries the file, line and column of its place in the source as data in the program, and each
  line moved changes those numbers. The same was seen in stage 0a of ADR 0053: functions added in the middle of three
  files, never called by the programs, changed all three. The same code as a test module at the end of a file left
  them byte-identical.
- **Code moved out of a file.** A first version of #309 (not pushed) moved a simulator that the programs never call
  out of `hal/src/dho/seg.rs`, and all three programs differed (reported by the session that wrote it; not tried
  here with the new comparison).
- **A build script.** #302 adds build scripts to `oos-hal` and `oos-ui` that set the same `cfg` for the same code. All
  three programs differed.
- Most refactorings move lines. Under ADR 0041 they need a run on a unit even when the comparison of ADR 0032 and
  0042 finds every register sequence and every picture equal.

Tried on 2026-11-19 on the host, with the pinned Rust 1.100.0:

| Change | Byte for byte (ADR 0041) | Without panic places and symbols |
|---|---|---|
| a comment line in `seq/samples.rs` | all three differ | all three equal |
| #302 (build scripts of hal and ui) | all three differ | oos-fpgad and oos-update equal, oos-ui differs |

oos-ui of #302 still differs in most of its bytes, because the build script of `oos-ui` changes how the program is laid
out. A change of the build is not let through by this record.

## Decision

**`tools/scope-binaries.sh` builds both sides with `-Zlocation-detail=none` (no file, line or column in the panic
data) and `-Cstrip=symbols` (no symbol table), and compares those programs byte for byte. `unit` passes when they are
equal, as in ADR 0041.**

- Left out is only data the programs never act on. A panic place is printed when the program panics, and the scope
  builds with `panic = "abort"`, so it is printed and the program ends. A symbol name is read by a debugger. Every
  instruction, constant, table and string the programs use is still compared.
- Rust lets a program read a panic place too (`Location::caller()`, `#[track_caller]`, a panic hook). No code of the
  tree does, and `tools/tests/test_scope_binaries.py` fails when code does: such code needs this record looked at
  again before it comes in.
- `-Zlocation-detail` is an unstable flag. `RUSTC_BOOTSTRAP=1` lets the pinned release take it, for these two builds
  only. They are compared and thrown away. The image is built as before, without the flag.
- What else needs a unit is unchanged: the paths of the image (`boot/`, `kernel/`, `rootfs/`) are not compared, and
  programs that still differ need a run on a unit and `unit-tested`.

## Alternatives

- **Keep byte for byte on the shipped build (ADR 0041).** A refactoring that moves lines in a hardware path waits for a
  unit, or is written so that no line moves (code at the end of a file). The second bends the code to the check.
- **Compare the programs section by section, or map the panic places out of a stripped program.** It would need an
  ELF parser in the tree and its own tests. The two compiler flags give the same result with the tools that are
  already there.
- **Ship programs without panic places** (`-Zlocation-detail=none` in the image). A panic message on the serial console
  would no longer say where it came from, and the image would depend on an unstable flag.

## Consequences

- A refactoring, a lint fix or a test that moves lines in `hal/src/dho/`, `ui/src/board/` or `fpgad/` passes `unit`
  without a scope when the programs are otherwise equal. The run on a unit is kept for changes that do change them.
- The panic messages of a change are not compared. A change that only rewords a panic message (`panic!("...")`) still
  differs: the text is a string in the program.
- The flag belongs to the compiler release. A newer Rust comes in through a pull request that changes the pin (ADR
  0045). The weekly run with the newest Rust does not run this comparison. If a release drops or renames the flag, the
  next pull request whose job `scope` runs fails it ("the comparison could not be made"). Then `unit` stays pending,
  never passes, and this record needs a second look.
- ADR 0041 stays in force. This record changes what its comparison leaves out.

## References

ADR 0032, ADR 0041, ADR 0042, ADR 0045, ADR 0053, `tools/scope-binaries.sh`, `tools/tests/test_scope_binaries.py`,
`tools/pr-gate.sh`, #302.
