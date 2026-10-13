"""Summarise run_tools.py: per tool run, packages found, with a license, and agreement with feluda on the same subject.
python3 -I tools_summary.py  -> results/tools.jsonl, report/tools_table.md, report/tools_disagreements.md"""
import json, os, re, glob, statistics, random
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = f"{ROOT}/results/raw"
if not os.path.isdir(RAW) or not os.listdir(RAW):
    raise SystemExit(f"{RAW} is empty: run cells first. The committed results/*.jsonl are left as they are.")
ALIAS = {"mit license": "mit", "the mit license": "mit", "expat": "mit", "apache software license": "apache-2.0", "apache 2.0": "apache-2.0", "apache license 2.0": "apache-2.0",
         "apache license, version 2.0": "apache-2.0", "apache-2": "apache-2.0", "apache license": "apache-2.0", "apache2": "apache-2.0", "bsd license": "bsd", "bsd-3": "bsd-3-clause", "bsd 3-clause": "bsd-3-clause",
         "new bsd": "bsd-3-clause", "3-clause bsd": "bsd-3-clause", "bsd-2": "bsd-2-clause", "mpl 2.0": "mpl-2.0", "mpl-2": "mpl-2.0", "isc license": "isc", "python software foundation license": "psf-2.0", "psf": "psf-2.0",
         "gnu lgpl": "lgpl", "mit/x11": "mit"}
def same(a, b):
    """any license in common, 'bsd' without a clause count matching any bsd-*"""
    for x in a:
        for y in b:
            if x == y or (x == "bsd" and y.startswith("bsd")) or (y == "bsd" and x.startswith("bsd")): return True
    return False
def toks(s):
    out = set()
    for t in re.split(r"\s+(?:AND|OR|WITH)\s+|[()]|\s+and\s+|\s+or\s+|/|,", str(s or ""), flags=re.I):
        t = t.strip().lower().replace("-only", "").replace("-or-later", "").replace("_", "-").rstrip("+")
        t = ALIAS.get(t, t)
        if t and t not in ("noassertion", "unknown", "none", "n/a", "no license"): out.add(t)
    return out
def lic_cdx(c):
    out = set()
    for l in c.get("licenses") or []:
        if "expression" in l: out |= toks(l["expression"])
        else: out |= toks((l.get("license") or {}).get("id") or (l.get("license") or {}).get("name"))
    return out
def parse(tool, path):
    """name -> set(license tokens); None if unparseable"""
    try:
        if tool == "syft":
            d = json.load(open(path)); m = {}
            for a in d.get("artifacts", []):
                s = set()
                for l in a.get("licenses", []): s |= toks(l.get("spdxExpression") or l.get("value"))
                m.setdefault(a["name"], set()).update(s)
            return m
        if tool == "trivy":
            d = json.load(open(path)); m = {}
            for r in d.get("Results", []) or []:
                for l in r.get("Licenses", []) or []:
                    m.setdefault(l.get("PkgName") or l.get("FilePath") or "?", set()).update(toks(l.get("Name")))
            return m
        if tool == "cdxgen":
            d = json.load(open(path)); m = {}
            for c in d.get("components", []):
                n = (c.get("group") + "/" if c.get("group") else "") + c["name"]
                m.setdefault(n, set()).update(lic_cdx(c))
                m.setdefault(c["name"], set()).update(lic_cdx(c))
            return m
        if tool == "scancode":
            d = json.load(open(path)); m = {}
            for f in d.get("files", []):
                for l in f.get("detected_license_expression_spdx") and [f["detected_license_expression_spdx"]] or []:
                    m.setdefault(f["path"], set()).update(toks(l))
            return m
        if tool == "cargo-deny":
            d = json.load(open(path)); m = {}
            for lic, crates in d.get("licenses", []):
                for c in crates: m.setdefault(c.split()[0], set()).update(toks(lic))
            return m
        if tool == "go-licenses":
            m = {}
            for line in open(path.replace(".json", ".csv")):
                p = line.rstrip("\n").split(",")
                if len(p) >= 3: m.setdefault(p[0], set()).update(toks(p[2]))
            return m
        if tool in ("pip-licenses", "license-checker"):
            d = json.load(open(path)); m = {}
            if isinstance(d, list):
                for p in d: m.setdefault(p["Name"].lower(), set()).update(toks(p.get("License")))
            else:
                for k, v in d.items():
                    m.setdefault(k.rsplit("@", 1)[0], set()).update(toks(v.get("licenses")))
            return m
    except Exception:
        return None
    return None
VER = {}
def feluda_map(subject):
    for st in ("cold", "warm"):
        p = f"{RAW}/{subject}/{st}/report.json"
        if os.path.exists(p):
            try:
                d = json.load(open(p)); m = {}
                for r in d:
                    m.setdefault(r["name"].lower(), set()).update(toks(r.get("license"))); VER[(subject, r["name"].lower())] = (r.get("version"), r.get("license"))
                return m
            except Exception: pass
    return None
rows = []; dis = []
random.seed(7)
for meta_p in sorted(glob.glob(f"{RAW}/**/tool-*/meta.json", recursive=True)):
    d = os.path.dirname(meta_p); meta = json.load(open(meta_p)); tool = meta["tool"]
    row = {"tool": tool, "subject": meta["subject"], "state": meta["scenario"].rsplit("-", 1)[1], "tier": meta.get("tier")}
    try:
        hf = json.load(open(d + "/hyperfine.json"))["results"][0]["measurements"]
        w = sorted(m["time_wall_clock"]["value"] for m in hf)
        row.update(samples=len(w), median_s=statistics.median(w), min_s=w[0], max_s=w[-1],
                   peak_rss_mb=round(max(m["memory_peak_resident"]["value"] for m in hf) / 1048576, 1))
    except Exception: pass
    row["exit"] = open(d + "/exit.txt").read().strip() if os.path.exists(d + "/exit.txt") else None
    row["dnf"] = row["exit"] == "124"
    out = next(iter(glob.glob(d + "/out.json")), None)
    tm = parse(tool, out) if out else (parse(tool, d + "/out.json") if tool == "go-licenses" else None)
    if tm is not None:
        row["packages"] = len(tm); row["with_license"] = sum(1 for v in tm.values() if v)
        fm = feluda_map(meta["subject"])
        if fm is not None:
            common = [n for n in tm if n.lower() in fm and fm[n.lower()]]
            sample = random.sample(common, min(50, len(common)))
            agree = [n for n in sample if tm[n] and same(tm[n], fm[n.lower()])]
            missing = [n for n in sample if not tm[n]]
            row.update(common_names=len(common), sampled=len(sample), agree=len(agree), tool_no_license=len(missing), disagree=len(sample) - len(agree) - len(missing))
            for n in sample:
                if tm[n] and not same(tm[n], fm[n.lower()]): dis.append((tool, meta["subject"], n, VER.get((meta["subject"], n.lower()), ("", ""))[0], sorted(tm[n]), VER.get((meta["subject"], n.lower()), ("", ""))[1]))
    rows.append(row)
with open(f"{ROOT}/results/tools.jsonl", "w") as f:
    for r in rows: f.write(json.dumps(r) + "\n")
with open(f"{ROOT}/report/tools_disagreements.md", "w") as f:
    f.write("| tool | subject | package | feluda version | tool says | feluda says (raw) |\n|---|---|---|---|---|---|\n")
    seen = set()
    for t in dis:
        k = (t[0], t[1], t[2])
        if k in seen: continue
        seen.add(k)
        f.write(f"| {t[0]} | {t[1]} | {t[2]} | {t[3]} | {', '.join(t[4])} | {str(t[5]).replace(chr(10), ' ').replace('|', '/')[:90]} |\n")
print(len(rows), "tool runs,", len(dis), "disagreements")
