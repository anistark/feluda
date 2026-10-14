"""Rebuild results/runs.jsonl from results/raw. Run: python3 -I collect.py"""
import json, os, re, statistics
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = f"{ROOT}/results/raw"
if not os.path.isdir(RAW) or not os.listdir(RAW):
    raise SystemExit(f"{RAW} is empty: run cells first. The committed results/*.jsonl are left as they are.")
env = json.load(open(f"{ROOT}/results/env.json")) if os.path.exists(f"{ROOT}/results/env.json") else {}
subj = {s["id"]: s for s in json.load(open(f"{ROOT}/corpus/subjects.json"))}

def pct(xs, p):
    xs = sorted(xs)
    if not xs: return None
    k = (len(xs) - 1) * p
    f, c = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)

def num(rx, text):
    m = re.search(rx, text, re.M)
    return float(m.group(1)) if m else None

rows = []
for dirpath, dirs, files in os.walk(RAW):
    if "hyperfine.json" not in files or "/_" in dirpath:
        continue
    rel = os.path.relpath(dirpath, RAW).split(os.sep)
    scen = rel[-1]
    subject = "/".join(rel[:-1])
    scenario, _, ftag = scen.partition("+")
    meta = json.load(open(f"{dirpath}/meta.json")) if "meta.json" in files else {}
    try:
        hf = json.load(open(f"{dirpath}/hyperfine.json"))["results"][0]
        ms = hf["measurements"]
        t = [m["time_wall_clock"]["value"] for m in ms]
    except Exception:
        continue
    exit_codes = [m.get("exit_code", m.get("exit_status")) for m in ms]
    sm = hf["summary"]
    hf["mean"] = sm["time_wall_clock"]["mean"]; hf["stddev"] = sm["time_wall_clock"].get("stddev")
    hf["user"] = sm["time_user"]["mean"]; hf["system"] = sm["time_system"]["mean"]
    hf_rss = sm.get("memory_peak_resident", {}).get("max")
    row = {"subject": subject, "kind": meta.get("kind") or subj.get(subject, {}).get("kind"), "tier": meta.get("tier") or subj.get(subject, {}).get("tier"),
           "languages": meta.get("languages"), "scenario": scenario, "flags": meta.get("flags", []), "nouv": meta.get("nouv", 0),
           "samples": len(t), "median_s": statistics.median(t), "mean_s": hf["mean"], "stddev_s": hf.get("stddev"),
           "p10_s": pct(t, .10), "p90_s": pct(t, .90), "min_s": min(t), "max_s": max(t), "user_s": hf.get("user"), "sys_s": hf.get("system"),
           "times": t, "exit_codes": exit_codes, "install": meta.get("install"), "feluda_sha256": meta.get("feluda_sha256"), "date": meta.get("date")}
    row["dnf_stage"] = meta.get("dnf_stage"); row["skipped_cold_dnf"] = meta.get("dnf_stage") == "skipped_cold_dnf"; row["dnf"] = bool(meta.get("dnf")); row["dnf_after_s"] = meta.get("dnf_after_s")
    row["flag_p90_over_2x_median"] = bool(row["p90_s"] and row["median_s"] and row["p90_s"] > 2 * row["median_s"])
    if hf_rss: row["peak_rss_mb_hyperfine_max"] = round(hf_rss / 1048576, 1)
    if os.path.exists(f"{dirpath}/time.txt"):
        tt = open(f"{dirpath}/time.txt").read()
        rss = num(r"^\s*(\d+)\s+maximum resident set size", tt)
        row["peak_rss_mb"] = round(rss / 1048576, 1) if rss else None
        row["recorded_real_s"] = num(r"^\s*([\d.]+)\s+real", tt)
    if os.path.exists(f"{dirpath}/exit.txt"):
        row["exit"] = int(open(f"{dirpath}/exit.txt").read().strip() or -1)
    if os.path.exists(f"{dirpath}/requests.json"):
        try:
            rq = json.load(open(f"{dirpath}/requests.json"))
            row["requests"] = rq["total_requests"]
            row["request_hosts"] = {h: {"n": v["requests"], "errors": v["transport_errors"], "statuses": v["statuses"], "total_s": round(v["total_ms"] / 1000, 2), "max_s": round(v["max_ms"] / 1000, 2)} for h, v in rq["hosts"].items()}
            row["transport_errors"] = sum(v["transport_errors"] for v in rq["hosts"].values())
            row["http_4xx_5xx"] = sum(c for v in rq["hosts"].values() for s, c in v["statuses"].items() if int(s) >= 400)
        except Exception:
            pass
    if os.path.exists(f"{dirpath}/report.json"):
        try:
            rp = json.load(open(f"{dirpath}/report.json"))
            row["deps_total"] = len(rp)
            row["deps_unique"] = len({(r.get("name"), r.get("version")) for r in rp})
            row["unknown"] = sum(1 for r in rp if re.search(r"nknown|NOASSERTION|^$|No License", str(r.get("license") or ""), re.I) or r.get("license") is None)
            row["ecosystems"] = {}
            for r in rp:
                row["ecosystems"][r.get("ecosystem")] = row["ecosystems"].get(r.get("ecosystem"), 0) + 1
        except Exception:
            pass
    s = subj.get(subject, {})
    if s.get("kind") == "synthetic": row["rung"] = s.get("rung")
    row["feluda_git"] = env.get("git_head_short"); row["feluda_diff_hash"] = env.get("git_diff_hash")
    rows.append(row)
rows.sort(key=lambda r: (r["subject"], r["scenario"], str(r["flags"])))
with open(f"{ROOT}/results/runs.jsonl", "w") as f:
    for r in rows:
        f.write(json.dumps(r) + "\n")
print(len(rows), "cells ->", f"{ROOT}/results/runs.jsonl")
