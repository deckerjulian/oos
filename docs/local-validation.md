# Local validation checkpoint

This change connects reference waveforms to timed Math, decoder roles and software triggers; adds selected-source automatic NRZ Eye/Jitter; and fixes retained acquisition memory, measurement refresh and zoom controls. Plain wheel pans time, Ctrl-wheel adjusts vertical scale. Hardware Autoset affects ADC channels; references have explicit Fit record. The desktop hardware panel remains a separate window.

Processing changes include fused calibrated ADC conversion, bounded ordered workers, shared immutable captures, every-frame basic statistics and parallel persistence deposits. Rootfs deployment replaces telnet with provisioned key-only SSH and persistent host identity.

Validation is local only. Focused math, decoder, SCPI, capture, zoom, Autoset and SSH checks pass at their recorded checkpoints. ARM64/Linux Rust checking passed. Actual D-drive Gen7 files were exercised through timed Math, zoom, stacked/overlay rendering and Auto Eye. C-PHY electrical-symbol decoding is implemented; the actual capture did not yield a qualified word alignment, so payload parity is unproven.

This is an integration checkpoint. The full suite and new remote PR integration are being completed before release. The first full Windows run encountered sandbox socket/file denials and two behavioral regressions now under investigation. ARM image/Dropbear linking and physical scope throughput were not tested on this Windows host. No GitHub CI was requested or run by this change. Commits carry [skip ci].

Usage and limitations: [reference analysis](reference-analysis.md), [SSH provisioning](ssh.md).
