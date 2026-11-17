# System Sliver

A small system monitor for a narrow strip along your screen edge. Built with Rust, iced 0.14 (wgpu), and iced-themer.

```sh
cargo run --release
```

The window opens at **220 × 820** logical pixels and resizes down to 120 px wide. Position or tile it with your window manager; it is a normal window, not a reserved desktop panel. Short windows scroll vertically.

- **CPU:** overall utilization across all logical CPUs (0–100%).
- **Memory:** used RAM, percentage and used/total bytes.
- **Swap:** used/total, or an explicit “No swap configured”.
- **Network:** aggregate receive (blue) and send (amber) rates, excluding loopback. VPNs and bridges may count the same traffic more than once. The plot auto-scales to the visible peak; its ceiling is shown above it.

All charts share a time axis: oldest on the left, newest on the right. CPU and swap use a fixed 0–100% scale. Memory auto-scales to the visible minimum and maximum with 10% padding on each side and a minimum span of two percentage points. Its plotted range is labeled above the chart; numeric usage stays unchanged. Select **1m / 5m / 15m / 1h** to change the visible history without losing samples. History accumulates while the app runs; uncollected history stays blank. The row above the history controls selects the update interval: **1s / 3s / 10s / 1m** (default 1s). Polls run on a background worker, never overlap, and do not scan processes. Up to one hour of history is retained, even when switching intervals; rates use actual elapsed time. CPU and network need two observations before showing values.

**Ctrl + / Ctrl =**, **Ctrl −**, **Ctrl 0** zoom the whole interface in, out, and back to 100%. History, zoom and window geometry are not persisted.

## License

Licensed under the GNU Affero General Public License, version 3 or later
(`AGPL-3.0-or-later`). See [LICENSE](LICENSE) for the full license text.

## Development

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

The theme is embedded from `theme.toml`; rebuild after editing it. `iced-themer` uses the sibling checkout at `../iced-themer`. Without that checkout, replace its path dependency with:

```toml
iced-themer = { git = "https://github.com/cjrh/iced-themer" }
```

The renderer prefers the low-power GPU. Override with `WGPU_POWER_PREF=high cargo run --release` if needed. A working GPU driver is required; there is deliberately no silent software-rendering fallback.
