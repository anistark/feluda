"""Warm layer B for Java and Ruby without running any package code.
usage: python3 -I install_fetch.py <java|ruby> <report.json> <state_dir>
Java: download <name>-<version>.jar into the Maven layout under <state>/m2.
Ruby: download the .gem and unpack its data.tar.gz into <state>/gem/gems/<name>-<version>/."""
import io, json, os, sys, tarfile, gzip, urllib.request, concurrent.futures as cf

kind, report, state = sys.argv[1:4]
rows = json.load(open(report))

def fetch(url):
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "feluda-bench"}), timeout=60) as r:
            return r.read()
    except Exception:
        return None

def java(row):
    name, ver = row["name"], row["version"]
    if ":" not in name or not ver or ver == "RELEASE":
        return 0
    g, a = name.split(":", 1)
    rel = f"{g.replace('.', '/')}/{a}/{ver}/{a}-{ver}.jar"
    data = fetch(f"https://repo1.maven.org/maven2/{rel}")
    if data is None:
        return 0
    p = os.path.join(state, "m2", rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "wb").write(data)
    return 1

def ruby(row):
    name, ver = row["name"], row["version"]
    data = fetch(f"https://rubygems.org/downloads/{name}-{ver}.gem")
    if data is None:
        return 0
    dest = os.path.join(state, "gem", "gems", f"{name}-{ver}")
    os.makedirs(dest, exist_ok=True)
    outer = tarfile.open(fileobj=io.BytesIO(data))
    inner = outer.extractfile("data.tar.gz")
    tf = tarfile.open(fileobj=io.BytesIO(gzip.decompress(inner.read())))
    for m in tf.getmembers():
        if not (m.isfile() or m.isdir()):
            continue
        target = os.path.realpath(os.path.join(dest, m.name))
        if not target.startswith(os.path.realpath(dest) + os.sep) and target != os.path.realpath(dest):
            continue
        if m.isdir():
            os.makedirs(target, exist_ok=True)
        else:
            os.makedirs(os.path.dirname(target), exist_ok=True)
            open(target, "wb").write(tf.extractfile(m).read())
    return 1

fn = java if kind == "java" else ruby
with cf.ThreadPoolExecutor(8) as ex:
    done = sum(ex.map(fn, rows))
print(f"{kind}: fetched {done} of {len(rows)}")
