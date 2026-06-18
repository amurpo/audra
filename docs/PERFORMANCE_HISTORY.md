# Performance history

This file is the changelog for Audra's performance work, and the home of every
measured number. The [Performance section of the README](../README.md#performance)
describes the app's behavior qualitatively; this file holds the actual figures,
the test rig, the methodology, and the dated record of *how they got there* and
what each change bought — so regressions and gains stay traceable.

## Test rig

All runs below use the same machine and library:

- **Machine:** Fedora, 8 logical CPUs, 16 GiB RAM, GTK4 Vulkan renderer.
- **Library:** 3,831 tracks / 380 albums / 179 artists (real collection).

## Reproducing the measurements

Build a profiling binary with frame pointers and line info:

```bash
RUSTFLAGS="-C force-frame-pointers=yes -C debuginfo=1" cargo build --release
```

Then, on Fedora (`sudo dnf install sysprof`):

```bash
# Honest memory cost of a running instance (PSS, private dirty)
grep -E 'Rss|Pss|Private_Dirty' /proc/$(pidof audra)/smaps_rollup

# CPU profile
sysprof-cli cpu.syscap -- ./target/release/audra

# Frame timings (GTK frame clock + main loop)
sysprof-cli --gtk --speedtrack ui.syscap -- ./target/release/audra

# Allocation profile
sysprof-cli --memprof mem.syscap -- ./target/release/audra
```

Open the resulting `.syscap` files with `sysprof`, or read them without the GUI:
`sysprof-cat <file>.syscap` dumps a capture to text. The CPU profile comes out as
a callgraph with `self`/`total` per node; aggregate self-samples per symbol with
`awk`. Force numeric (`$2+0`) and run under `LC_ALL=C` — a comma-decimal locale
otherwise sorts/prints the numbers as text.

---

## 2026-06-15 — Album grid: `GtkFlowBox` → virtualized `GtkGridView`

Validation of commit `9c9c14b` (the album grid migration). The goal was never
raw RAM — it was killing the load-time hitch and the O(n) layout cost of a
`FlowBox` that realizes every card up front. Confirmed.

| Metric | Before (FlowBox) | After (GridView) | Notes |
| --- | --- | --- | --- |
| Layout cost | `gtk_widget_allocate` ≈ 48% of tracked retained allocations | layout family **0.27%** of CPU self-samples (`gtk_widget_allocate` 0.01%) | layout is no longer a hotspot |
| Load hitch | ~518 ms longest `Frameclock cycle` | **291 ms** | ~40% lower; only the first frame after the library loads |
| PSS | ~178 MiB | **163–168 MiB** | ~10–15 MiB less; fewer widgets realized |
| RSS | ~294 MB | **282–289 MB** | |
| Frame cycle median | ~2.8 ms | **3.05 ms** | unchanged; scrolling stays smooth |

**Where CPU goes now** (126,062 self-samples): album-art decode/scale
(`scale_line`, `zune_jpeg`, `idct`, `fdeflate`) **10.78%** and CSS matching
(`gtk_css_selector_tree_match`, style updates) **9.15%**, with a lot of
`intel_idle`. As predicted, the texture branch did **not** shrink:
`cover_textures` still caches every decoded cover so it can repaint on scroll
without re-fetching, so texture RAM stays linear in album count.

**New finding — residual scroll jank moved off layout.** Of 1,212 frames, ~120
(~10%) exceeded the 16.6 ms budget, but layout is no longer the cause (layout
p99 = 3.75 ms). The residual hitches are `Validate CSS` (max 132 ms, p99 36 ms,
70 frames over budget) plus cover decode: recycling a `GridView` cell
revalidates the CSS tree. The medians are excellent, so this is a future
optimization target, not a defect.

**Verdict:** the migration did what it was meant to — layout cost gone
(~48% → 0.27%), load hitch down (~518 → 291 ms), widget RAM down (~10–15 MiB).
Remaining cost is the expected one (decoded covers, all cached). The artists
grid is still a `FlowBox` + batched appends, so its share of the cost is
unchanged.

---

## 2026-06-13/14 — Baseline (`GtkFlowBox` album grid)

The reference profile, before the GridView migration. Album and artist grids
both on `FlowBox`; album appends batched across the main loop (commit `75ac134`)
to soften, but not eliminate, the load hitch.

- **Memory:** PSS ~178 MiB, Private_Dirty ~146 MiB. Of ~146 MiB private dirty,
  ~86 MB tracked by malloc (≈48% `gtk_widget_allocate` for the 380 realized
  cards + ≈52% cover pixel buffers via `scale_to_pixels`), the rest GPU
  textures and internal GTK allocations. Flat over 15 min — no leak.
- **CPU:** ~5.85% of system CPU during recording; top app function
  `scale_to_pixels` 1.65% (cover decode/scale, transient at grid load). Audio
  `decoder::next_block` only 0.27% — playback is essentially free.
- **Rendering:** Vulkan (`GskGpuRenderer`). Typical frames 1–3 ms. But the
  `FlowBox` laid out all 380 cards at once on library open: longest
  `Frameclock cycle` 518 ms, layout 232 ms — a ~0.5 s one-shot hitch at load,
  not scroll jank. This hitch is what the GridView migration targeted.
