Added:

- **System > Probe optional memory** probes the memory above 256 MiB for every segment size of the Acquire menu's
  depths and switches it on when every size held; **System > Opt. memory** switches its use off and on again (on
  without a probe's yes probes first). Off by default; kept in system.conf (`mem.upper`, `mem.upper.found`)
  ([First Steps](wiki/First-Steps.md#optional-memory)).
