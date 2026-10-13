"""Markdown tables for REPORT.md -> report/tables_all.md. python3 -I tables.py"""
import os, json, math, collections
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
rows = [json.loads(l) for l in open(f"{ROOT}/results/runs.jsonl")]
tools = [json.loads(l) for l in open(f"{ROOT}/results/tools.jsonl")] if __import__("os").path.exists(f"{ROOT}/results/tools.jsonl") else []
out = []
def f(x, n=1): return "" if x is None else f"{x:.{n}f}"
def key(r): return (r["subject"], r["scenario"], tuple(r["flags"]), r.get("nouv", 0))
# 1. ladder
out.append("## Synthetic ladder (median seconds; unique name+version pairs in brackets)\n")
out.append("| language | rung | cold | cold-installed | warm-net | warm | offline | unique deps (cold) | requests (cold / warm) | install step failed in |\n|---|---|---|---|---|---|---|---|---|---|")
syn = collections.defaultdict(dict)
for r in rows:
    if r["subject"].startswith("synthetic/") and "polyglot" not in r["subject"] and not r.get("nouv") and not r["flags"]:
        syn[r["subject"]][r["scenario"]] = r
def num(k):
    return int(k.split("/n")[-1])
for k in sorted(syn, key=lambda k: (k.split("/")[1], num(k))):
    d = syn[k]
    def c(s):
        r = d.get(s); return "" if not r else ("DNF" if r.get("dnf") else f(r["median_s"], 2 if r["median_s"] < 10 else 1))
    cw = d.get("cold"); ww = d.get("warm")
    out.append(f"| {k.split('/')[1]} | {num(k)} | {c('cold')} | {c('cold-installed')} | {c('warm-net')} | {c('warm')} | {c('offline')} | {cw.get('deps_unique') if cw else ''} | {(cw or {}).get('requests','')} / {(ww or {}).get('requests','')} | {', '.join(sc for sc, r in d.items() if r.get('install') == 'FAILED')} |")
# 2. slope: seconds per unique dep, log-log exponent per language
out.append("\n## Growth per language (least squares on log time against log unique deps, cold and warm)\n")
out.append("| language | cold exponent | warm exponent | cold s per dep at largest rung | warm s per dep at largest rung |\n|---|---|---|---|---|")
for lang in ("rust", "node", "python", "go", "dotnet", "ruby", "r", "cpp", "c", "java"):
    res = {}
    for sc in ("cold", "warm"):
        pts = [(r.get("deps_unique") or r["deps_total"], r["median_s"]) for k, d in syn.items() if k.split("/")[1] == lang for s, r in d.items() if s == sc and r.get("median_s") and not r.get("dnf")]
        if len(pts) >= 2:
            xs = [math.log(p[0]) for p in pts]; ys = [math.log(p[1]) for p in pts]; mx = sum(xs) / len(xs); my = sum(ys) / len(ys)
            den = sum((x - mx) ** 2 for x in xs)
            res[sc] = (sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den if den else None, max(pts)[1] / max(pts)[0])
    if res: out.append(f"| {lang} | {f(res.get('cold',(None,))[0],2)} | {f(res.get('warm',(None,))[0],2)} | {f(res.get('cold',(0,None))[1],3)} | {f(res.get('warm',(0,None))[1],3)} |")
# 3. real repos
out.append("\n## Real repositories (median seconds)\n")
out.append("| subject | tier | rows | unique | cold | warm | cold / warm | requests cold | peak RSS MB (cold) | notes |\n|---|---|---|---|---|---|---|---|---|---|")
real = collections.defaultdict(dict)
for r in rows:
    if r["kind"] == "real" and not r["flags"] and not r["scenario"].startswith(("ecotool", "tool-", "unauth")):
        real[r["subject"]][r["scenario"]] = r
NOTES = {"java/mybatis-3": "search.maven.org hangs add 30 s each"}
FIX_BIN = "d80ff64a85f1"  # feluda-fix1: the cells whose results the three post study fixes change
for k in sorted(real):
    d = real[k]; c = d.get("cold"); w = d.get("warm")
    def t(r):
        if not r: return ""
        if r.get("skipped_cold_dnf"): return "skipped, cold did not finish"
        if r.get("dnf"): return f"DNF (over {int(r.get('dnf_after_s') or 0)} s)"
        return f(r["median_s"], 2 if r["median_s"] < 10 else 1)
    ratio = f(c["median_s"] / w["median_s"], 1) if c and w and not c.get("dnf") and not w.get("dnf") and w["median_s"] else ""
    out.append(f"| {k} | {(c or w or {}).get('tier')} | {(c or {}).get('deps_total','')} | {(c or {}).get('deps_unique','')} | {t(c)} | {t(w)} | {ratio} | {(c or {}).get('requests','')} | {(c or {}).get('peak_rss_mb','')} | {NOTES.get(k,'')}{'rerun with the fix build' if (c or {}).get('feluda_sha256','') and (c or {}).get('feluda_sha256','').startswith(FIX_BIN) else ''}{' ; warm install step FAILED, ecosystem state not really warm' if (w or {}).get('install') == 'FAILED' else ''} |")
# 4. flags
out.append("\n## Flag variants on M tier repos (warm; ratio to the default warm run)\n")
out.append("| subject | flag | median s | default s | ratio |\n|---|---|---|---|---|")
for r in rows:
    if r["kind"] == "real" and r["flags"] and r["scenario"] == "warm":
        b = real.get(r["subject"], {}).get("warm")
        if b and not r.get("dnf"): out.append(f"| {r['subject']} | {' '.join(r['flags'])} | {f(r['median_s'],2)} | {f(b['median_s'],2)} | {f(r['median_s']/b['median_s'],2)} |")
# 5. unauth
out.append("\n## Unauthenticated first run (one scan per round, rounds 61 minutes apart)\n")
out.append("| subject | round | seconds | GitHub requests (statuses) | OSI Unknown rows |\n|---|---|---|---|---|")
for r in rows:
    if r["scenario"] == "unauth-cold":
        gh = (r.get("request_hosts") or {}).get("api.github.com", {})
        out.append(f"| {r['subject']} | {r['flags'] or ''} | {f(r['median_s'],1)} | {gh.get('n','')} {gh.get('statuses','')} | |")
# 6. tools
if tools:
    out.append("\n## Other tools (median seconds; packages found; packages with a license)\n")
    out.append("| tool | subject | tier | state | median s | peak RSS MB | packages | with license | sampled | agree | tool has no license | disagree |\n|---|---|---|---|---|---|---|---|---|---|---|---|")
    for r in sorted(tools, key=lambda r: (r["tool"], r["subject"], r["state"])):
        out.append(f"| {r['tool']} | {r['subject']} | {r.get('tier','')} | {r['state']} | {'DNF' if r.get('dnf') else f(r.get('median_s'),2)} | {r.get('peak_rss_mb','')} | {r.get('packages','')} | {r.get('with_license','')} | {r.get('sampled','')} | {r.get('agree','')} | {r.get('tool_no_license','')} | {r.get('disagree','')} |")
# 7. DNF list
out.append("\n## DNF and skipped cells\n")
out.append("| subject | scenario | flags | what |\n|---|---|---|---|")
for r in rows:
    if r.get("dnf"):
        out.append(f"| {r['subject']} | {r['scenario']} | {' '.join(r['flags'])} | {'skipped, cold did not finish' if r.get('skipped_cold_dnf') else 'did not finish within ' + str(int(r.get('dnf_after_s') or 0)) + ' s'} |")
open(f"{ROOT}/report/tables_all.md", "w").write("\n".join(out) + "\n")
print(len(out), "lines")
