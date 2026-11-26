# victory-amd: r13 for AMD cards

This is VIctory (GPL-3.0, [Marvisak/VIctory](https://github.com/Marvisak/VIctory)) as merged from
[fxceless/victory-fork](https://github.com/fxceless/victory-fork), with local fixes for AMD (RDNA4, RX 9060 XT).
No game files, saves, caches or binaries are in this repository. You need your own console dump.

What differs from fxceless main (ceb9e1f):
- `VICTORY_BIT_EXACT_VIEWS` (default **off**): integer typed buffer views as float turned lit skin black on AMD.
- AMD descriptor fix (constant buffer slot kind), large-compute skip list, own merges of several pull requests.

Scripts here:
- `start.ps1`: settings measured on an RX 9060 XT / Ryzen 5 7500F / 32 GB. Put it next to `victory.exe`.
- `starter.cs`: a small Windows starter for `start.ps1` (desktop shortcut).
- `check-updates.sh`: lists new commits in the forks we follow, and new PortWatch news.

Build: `cargo build --release --locked -p victory`.
