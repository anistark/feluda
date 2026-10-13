"""Charts and tables from results/runs.jsonl -> report/charts/*.svg,png and report/tables.md
Every chart is drawn twice, <name>.svg for a light page and <name>_dark.svg for a dark one, both on a
transparent background; the docs page shows the one matching its theme.
Run with a Python that has matplotlib: python -I charts.py"""
import json, os, collections
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
plt.rcParams["svg.hashsalt"] = "feluda-bench"  # stable element ids, so unchanged data gives unchanged SVGs
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = f"{ROOT}/report/charts"; os.makedirs(OUT, exist_ok=True)
rows = [json.loads(l) for l in open(f"{ROOT}/results/runs.jsonl")]
all_rows = rows
rows = [r for r in rows if not r.get("dnf")]  # DNF and skipped cells are tabulated, never plotted as times
# colour-blind safe categorical palette, one colour per language; on a dark page the two darkest
# (Go's blue and C's grey) are lightened so they keep their contrast
LIGHT_PAL = {"rust": "#E69F00", "node": "#56B4E9", "python": "#009E73", "go": "#0072B2", "dotnet": "#CC79A7", "ruby": "#D55E00", "r": "#999999", "cpp": "#8B6BBF", "c": "#444444", "java": "#B8860B"}
DARK_PAL = {**LIGHT_PAL, "go": "#4A9BE0", "c": "#C8C8C8"}
THEMES = {
    "light": {"suffix": "", "ink": "#24292f", "muted": "#57606a", "pal": LIGHT_PAL},
    "dark": {"suffix": "_dark", "ink": "#e6edf3", "muted": "#9da7b3", "pal": DARK_PAL},
}
def lang(r): return r["subject"].split("/")[1] if r["subject"].startswith("synthetic/") else None
def save(fig, name):
    fig.tight_layout()
    fig.savefig(f"{OUT}/{name}{T['suffix']}.svg", metadata={"Date": None}, transparent=True)
    if not T["suffix"]: fig.savefig(f"{OUT}/{name}.png", dpi=140)
    plt.close(fig)
def style(ax, title, xl, yl):
    ax.set_title(title, loc="left", fontsize=11); ax.set_xlabel(xl); ax.set_ylabel(yl); ax.grid(alpha=.25, color=T["muted"])
    for s in ("top", "right"): ax.spines[s].set_visible(False)

def draw():
    tbl = []
    syn = [r for r in rows if r["subject"].startswith("synthetic/") and "polyglot" not in r["subject"] and not r.get("nouv") and r["scenario"] in ("cold", "warm") and r.get("deps_total")]
    for scen in ("cold", "warm"):
        fig, ax = plt.subplots(figsize=(7.5, 4.8))
        by = collections.defaultdict(list)
        for r in syn:
            if r["scenario"] == scen: by[lang(r)].append((r.get("deps_unique") or r["deps_total"], r["median_s"]))
        for l, pts in sorted(by.items()):
            pts.sort(); ax.plot(*zip(*pts), marker="o", ms=4, color=PAL[l], label=l)
            ax.annotate(l, pts[-1], textcoords="offset points", xytext=(5, 0), fontsize=8, color=PAL[l])
        ax.set_xscale("log"); ax.set_yscale("log")
        style(ax, f"Scan time against dependencies reported, {scen} (median)", "unique name+version pairs in the report", "seconds")
        ax.legend(fontsize=7, ncol=2, frameon=False); save(fig, f"time_vs_deps_{scen}")

    # cold / warm ratio at n100
    fig, ax = plt.subplots(figsize=(7, 4))
    rat = []
    for l in PAL:
        c = [r for r in rows if r["subject"] == f"synthetic/{l}/n100" and r["scenario"] == "cold" and not r.get("nouv")]
        w = [r for r in rows if r["subject"] == f"synthetic/{l}/n100" and r["scenario"] == "warm" and not r.get("nouv")]
        if c and w: rat.append((l, c[0]["median_s"], w[0]["median_s"]))
    rat.sort(key=lambda t: -t[1] / t[2])
    ax.barh([t[0] for t in rat], [t[1] / t[2] for t in rat], color=[PAL[t[0]] for t in rat])
    for i, t in enumerate(rat): ax.text(t[1] / t[2], i, f" {t[1]:.0f}s / {t[2]:.1f}s", va="center", fontsize=8)
    style(ax, "Cold to warm ratio at 100 direct dependencies", "cold median / warm median", ""); ax.invert_yaxis(); save(fig, "cold_warm_ratio_n100")

    # scenarios at n100 for rust, node, python
    fig, ax = plt.subplots(figsize=(8, 4.2))
    sc = ["cold", "cold-installed", "warm-net", "warm", "offline"]
    w = 0.26
    for i, l in enumerate(("rust", "node", "python")):
        vals = []
        for s in sc:
            m = [r for r in rows if r["subject"] == f"synthetic/{l}/n100" and r["scenario"] == s and not r.get("nouv")]
            vals.append(m[0]["median_s"] if m else 0)
        ax.bar([x + i * w for x in range(len(sc))], vals, w, color=PAL[l], label=l)
    ax.set_xticks([x + w for x in range(len(sc))]); ax.set_xticklabels(sc); ax.set_yscale("log")
    style(ax, "The five states at 100 direct dependencies (median, log scale)", "", "seconds"); ax.legend(frameon=False); save(fig, "scenarios_n100")

    # polyglot actual vs sum vs max of the single language n100 (java n10) warm and cold
    fig, axs = plt.subplots(1, 2, figsize=(9, 4), sharey=False)
    sets = {"p2": ["node", "python"], "p3": ["node", "python", "ruby"], "p5": ["node", "python", "ruby", "go", "rust"], "pall": ["node", "python", "ruby", "go", "rust", "dotnet", "r", "cpp", "c", "java"]}
    for ax, scen in zip(axs, ("cold", "warm")):
        xs = []; act = []; sm = []; mx = []
        for name, ls in sets.items():
            a = [r for r in rows if r["subject"] == f"synthetic/polyglot/{name}" and r["scenario"] == scen]
            if not a: continue
            singles = []
            for l in ls:
                n = 10 if l == "java" else 100
                m = [r for r in rows if r["subject"] == f"synthetic/{l}/n{n}" and r["scenario"] == scen and not r.get("nouv")]
                if m: singles.append(m[0]["median_s"])
            if len(singles) != len(ls): continue
            xs.append(name); act.append(a[0]["median_s"]); sm.append(sum(singles)); mx.append(max(singles))
            tbl.append((scen, name, a[0]["median_s"], sum(singles), max(singles)))
        import numpy as np
        x = np.arange(len(xs)); ax.bar(x - .27, act, .27, label="actual", color=PAL["go"]); ax.bar(x, sm, .27, label="sum of singles", color=PAL["rust"]); ax.bar(x + .27, mx, .27, label="max of singles", color=PAL["python"])
        ax.set_xticks(x); ax.set_xticklabels(xs); ax.set_yscale("log"); style(ax, f"Polyglot, {scen}", "languages in one directory", "seconds"); ax.legend(frameon=False, fontsize=8)
    save(fig, "polyglot")

    # flag deltas on warm M-tier repos
    base = {r["subject"]: r for r in rows if r["scenario"] == "warm" and not r["flags"] and r["kind"] == "real"}
    fig, ax = plt.subplots(figsize=(8, 4)); fl = collections.defaultdict(list)
    for r in rows:
        if r["scenario"] == "warm" and r["flags"] and r["subject"] in base and r["kind"] == "real":
            fl[" ".join(r["flags"])].append((r["subject"], r["median_s"] / base[r["subject"]]["median_s"]))
    for i, (f, pts) in enumerate(sorted(fl.items())):
        ax.scatter([i] * len(pts), [p[1] for p in pts], color=PAL["go"])
        for j, p in enumerate(pts): ax.annotate(p[0].split("/")[1], (i, p[1]), textcoords="offset points", xytext=(6, 0), fontsize=6)
    ax.axhline(1, color=T["muted"], lw=1); ax.set_xticks(range(len(fl))); ax.set_xticklabels(sorted(fl)); ax.set_yscale("log")
    style(ax, "Flag variants against the default run (warm, M tier)", "", "time with flag / default"); save(fig, "flag_deltas")

    # ecosystem tool vs feluda
    eco = [r for r in rows if r["scenario"].startswith("ecotool")]
    if eco:
        fig, ax = plt.subplots(figsize=(8, 4)); names = []; tool = []; fel = []
        for r in eco:
            if r["scenario"] != "ecotool-warm": continue
            f = [x for x in rows if x["subject"] == r["subject"] and x["scenario"] == "warm" and not x["flags"]]
            if f: names.append(r["subject"]); tool.append(r["median_s"]); fel.append(max(f[0]["median_s"] - r["median_s"], 0))
        if names:
            ax.barh(names, tool, color=PAL["rust"], label="ecosystem tool (cargo, go, npm, uv, dotnet)"); ax.barh(names, fel, left=tool, color=PAL["go"], label="rest of feluda")
            style(ax, "Warm scan: ecosystem tool time and the rest", "seconds", ""); ax.legend(frameon=False, fontsize=8); save(fig, "ecosystem_tool_share")
    return tbl

for theme, T in THEMES.items():
    PAL = T["pal"]
    rc = {"text.color": T["ink"], "axes.labelcolor": T["ink"], "axes.edgecolor": T["muted"],
          "xtick.color": T["ink"], "ytick.color": T["ink"], "axes.titlecolor": T["ink"],
          "axes.facecolor": "none", "figure.facecolor": "none", "legend.labelcolor": T["ink"]}
    with plt.rc_context(rc):
        drawn = draw()
    if theme == "light": tbl = drawn
with open(f"{ROOT}/report/tables.md", "w") as f:
    f.write("| scope | state | actual s | sum of singles s | max of singles s |\n|---|---|---|---|---|\n")
    for t in tbl: f.write(f"| {t[1]} | {t[0]} | {t[2]:.1f} | {t[3]:.1f} | {t[4]:.1f} |\n")
print("charts ok", os.listdir(OUT))
