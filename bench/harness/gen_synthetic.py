"""Generate the synthetic ladder (manifests only) under $BENCH_WORK/synthetic/<lang>/n<N>/.
usage: python3 -I gen_synthetic.py <lang> [N ...]     (run inside harness/env.sh: bench_env gen)
Locks are produced where the ecosystem has them, without running any package code."""
import json, os, re, subprocess, sys, shutil, time, urllib.request, glob

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
POOLS = f"{ROOT}/corpus/pools"
OUT = os.path.join(os.environ["BENCH_WORK"], "synthetic")  # pack it into corpus/synthetic.tar.gz when done
RUNGS = [10, 50, 100, 250, 500, 1000]
UA = {"User-Agent": "feluda-bench (anirudhastark@gmail.com)"}

def pool(name):
    return json.load(open(f"{POOLS}/{name}.json"))["items"]

def get(url):
    for i in range(4):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
                return r.read().decode()
        except Exception as e:
            err = e
            time.sleep(2 + 3 * i)
    raise err

def run(cmd, cwd, timeout=1800):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout + p.stderr

def mk(lang, n):
    d = f"{OUT}/{lang}/n{n}"
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    return d

def rungs(argv, limit):
    ns = [int(a) for a in argv] if argv else RUNGS
    return [n for n in ns if n <= limit]

def gen_rust(argv):
    names = [n for n in pool("crates")]
    bad = set()
    for n in rungs(argv, 1000):
        d = mk("rust", n)
        os.makedirs(f"{d}/src")
        open(f"{d}/src/lib.rs", "w").write("")
        while True:
            chosen = [x for x in names if x not in bad][:n]
            toml = '[package]\nname = "bench-rust-n%d"\nversion = "0.1.0"\nedition = "2021"\nlicense = "MIT"\n\n[dependencies]\n' % n
            toml += "".join(f'{c} = "*"\n' for c in chosen)
            open(f"{d}/Cargo.toml", "w").write(toml)
            rc, out = run(["cargo", "generate-lockfile"], d)
            if rc == 0:
                break
            m = re.search(r"failed to select a version for `([\w\-]+)`", out) or re.search(r"package `([\w\-]+)`", out)
            print("  cargo failed:", out.strip().splitlines()[-1][:160], file=sys.stderr)
            if not m or m.group(1) not in chosen:
                # find the first direct crate named anywhere in the message
                cand = [c for c in chosen if f"`{c}`" in out or f"`{c} " in out]
                if not cand:
                    raise SystemExit("cannot find culprit:\n" + out[-2000:])
                bad.add(cand[0])
            else:
                bad.add(m.group(1))
        lock = open(f"{d}/Cargo.lock").read().count("[[package]]")
        print(f"rust n{n}: direct {len(chosen)} locked packages {lock} dropped {len(bad)}")

def gen_node(argv):
    names = pool("npm")
    for n in rungs(argv, 1000):
        d = mk("node", n)
        deps = {c: "*" for c in names[:n]}
        json.dump({"name": f"bench-node-n{n}", "version": "1.0.0", "license": "MIT", "dependencies": deps}, open(f"{d}/package.json", "w"), indent=2)
        rc, out = run(["npm", "install", "--package-lock-only", "--ignore-scripts", "--legacy-peer-deps", "--force", "--no-audit", "--no-fund"], d)
        if rc != 0:
            print("  npm failed:", out[-600:], file=sys.stderr)
            continue
        lock = json.load(open(f"{d}/package-lock.json"))
        # real manifests carry version ranges, not "*": pin each direct dependency as ^<locked version>
        new = {}
        for name in deps:
            ent = lock["packages"].get(f"node_modules/{name}")
            new[name] = "^" + ent["version"] if ent else "*"
        pj = json.load(open(f"{d}/package.json")); pj["dependencies"] = new
        lock["packages"][""]["dependencies"] = new
        json.dump(pj, open(f"{d}/package.json", "w"), indent=2); json.dump(lock, open(f"{d}/package-lock.json", "w"), indent=2)
        print(f"node n{n}: direct {n} locked packages {len(lock['packages'])-1}")

def gen_python(argv):
    names = pool("pypi")
    bad = set()
    for n in rungs(argv, 1000):
        d = mk("python", n)
        while True:
            chosen = [x for x in names if x not in bad][:n]
            open(f"{d}/requirements.txt", "w").write("".join(c + "\n" for c in chosen))
            rc, out = run(["uv", "pip", "compile", "--quiet", "--no-header", "requirements.txt"], d, 1800)
            if rc == 0:
                break
            norm = {x.lower().replace("_", "-"): x for x in chosen}
            words = re.findall(r"[A-Za-z0-9_.\-]+", out)
            culprit = next((w for w in words if w.lower().replace("_", "-") in norm and w.lower() not in ("and", "the")), None)
            print("  uv failed:", out.strip().splitlines()[0][:140], "->", culprit, file=sys.stderr)
            if culprit is None:
                raise SystemExit("no culprit:\n" + out[-1500:])
            bad.add(norm[culprit.lower().replace("_", "-")])
        total = len([l for l in out.splitlines() if re.match(r"^[A-Za-z0-9_.\-]+==", l)])
        open(f"{d}/resolved.txt", "w").write(out)
        print(f"python n{n}: direct {len(chosen)} resolved {total} dropped {len(bad)}")

def gen_go(argv):
    items = pool("go")
    # keep modules a real project requires; the highest version any of them asks for
    for n in rungs(argv, 1000):
        d = mk("go", n)
        chosen = items[:n]
        lines = ["module bench/go-n%d\n\ngo 1.23\n\nrequire (\n" % n]
        lines += [f"\t{m} {v}\n" for m, v, _ in chosen]
        lines.append(")\n")
        open(f"{d}/go.mod", "w").write("".join(lines))
        env = dict(os.environ, GOFLAGS="-mod=mod")
        p = subprocess.run(["go", "mod", "download"], cwd=d, capture_output=True, text=True, env=env, timeout=3600)
        g = subprocess.run(["go", "mod", "graph"], cwd=d, capture_output=True, text=True, timeout=3600)
        mods = {l.split()[1] for l in g.stdout.splitlines() if l.strip()}
        print(f"go n{n}: direct {len(chosen)} graph modules {len(mods)} download rc={p.returncode} graph rc={g.returncode} {g.stderr.strip()[:120]}")

def gen_gem(argv):
    names = pool("gem")
    skip = {"bundler", "rubygems-update"}
    bad = set()
    for n in rungs(argv, 500):
        d = mk("ruby", n)
        while True:
            chosen = [x for x in names if x not in skip and x not in bad][:n]
            open(f"{d}/Gemfile", "w").write('source "https://rubygems.org"\n\n' + "".join(f'gem "{c}"\n' for c in chosen))
            rc, out = run(["bundle", "lock"], d, 1800)
            if rc == 0:
                break
            m = re.search(r"Could not find gem '([\w\-.]+)", out) or re.search(r"Bundler could not find compatible versions for gem \"([\w\-.]+)\"", out)
            print("  bundle failed:", out.strip().splitlines()[-1][:140], file=sys.stderr)
            if not m or m.group(1) not in chosen:
                raise SystemExit("no culprit:\n" + out[-1500:])
            bad.add(m.group(1))
        lock = open(f"{d}/Gemfile.lock").read()
        specs = len(re.findall(r"^    [\w\-.]+ \(", lock, re.M))
        print(f"ruby n{n}: direct {len(chosen)} locked gems {specs} dropped {len(bad)}")

def nuget_version(pid):
    d = json.loads(get(f"https://api.nuget.org/v3-flatcontainer/{pid.lower()}/index.json"))
    vs = [v for v in d["versions"] if "-" not in v]
    return vs[-1] if vs else d["versions"][-1]

def gen_dotnet(argv):
    names = pool("nuget")
    cache = {}
    for n in rungs(argv, 1000):
        d = mk("dotnet", n)
        refs = []
        for pid in names[:n]:
            if pid not in cache:
                try:
                    cache[pid] = nuget_version(pid)
                except Exception:
                    cache[pid] = None
            if cache[pid]:
                refs.append(f'    <PackageReference Include="{pid}" Version="{cache[pid]}" />\n')
        open(f"{d}/Bench.csproj", "w").write('<Project Sdk="Microsoft.NET.Sdk">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n  <ItemGroup>\n' + "".join(refs) + "  </ItemGroup>\n</Project>\n")
        print(f"dotnet n{n}: direct {len(refs)}")

def gen_r(argv):
    names = pool("cran")
    for n in rungs(argv, 500):
        d = mk("r", n)
        imports = ",\n    ".join(names[:n])
        open(f"{d}/DESCRIPTION", "w").write(f"Package: benchr{n}\nType: Package\nTitle: Bench\nVersion: 0.1.0\nLicense: MIT + file LICENSE\nDescription: Synthetic ladder rung.\nImports:\n    {imports}\n")
        print(f"r n{n}: direct {n}")

def gen_cpp(argv):
    names = pool("vcpkg")
    for n in rungs(argv, 500):
        d = mk("cpp", n)
        json.dump({"name": f"bench-cpp-n{n}", "version": "0.1.0", "dependencies": names[:n]}, open(f"{d}/vcpkg.json", "w"), indent=1)
        print(f"cpp n{n}: direct {n}")

def gen_c(argv):
    names = pool("clibs")
    for n in rungs(argv, 100):
        d = mk("c", n)
        libs = " ".join(f"-l{x}" for x in names[:n])
        open(f"{d}/Makefile", "w").write(f"CC = gcc\nLDFLAGS = {libs}\nTARGET = bench\nSRC = main.c\n\nall: $(TARGET)\n\n$(TARGET): $(SRC)\n\t$(CC) $(CFLAGS) -o $@ $^ $(LDFLAGS)\n")
        open(f"{d}/main.c", "w").write("int main(void) { return 0; }\n")
        print(f"c n{n}: direct {n}")

def gen_java(argv):
    mv = pool("maven")  # [group, artifact, version]
    for n in rungs(argv, 500):
        d = mk("java", n)
        deps = "".join(f"    <dependency>\n      <groupId>{g}</groupId>\n      <artifactId>{a}</artifactId>\n      <version>{v}</version>\n    </dependency>\n" for g, a, v in mv[:n])
        open(f"{d}/pom.xml", "w").write(f'<project xmlns="http://maven.apache.org/POM/4.0.0">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>bench</groupId>\n  <artifactId>java-n{n}</artifactId>\n  <version>1.0</version>\n  <dependencies>\n{deps}  </dependencies>\n</project>\n')
        print(f"java n{n}: direct {n}")

def gen_gradle(argv):
    mv = pool("maven")
    for n in rungs(argv, 500):
        d = mk("gradle", n)
        deps = "".join(f"    implementation '{g}:{a}:{v}'\n" for g, a, v in mv[:n])
        open(f"{d}/build.gradle", "w").write(f"plugins {{ id 'java' }}\nrepositories {{ mavenCentral() }}\ndependencies {{\n{deps}}}\n")
        print(f"gradle n{n}: direct {n}")

gen_ruby = gen_gem

if __name__ == "__main__":
    lang = sys.argv[1]
    globals()["gen_" + lang](sys.argv[2:])
