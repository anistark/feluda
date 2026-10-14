# Feluda benchmark harness

The harness behind the [Performance](https://feluda.readthedocs.io/en/latest/performance/) docs
page. It times one feluda binary on a synthetic ladder of projects and on 37 real repositories
pinned to a commit, under controlled cache states, and writes the docs page from the results.

The study's data is committed (`results/*.jsonl`), so the page can be regenerated without running
anything. The raw output of each run stays local.

## Layout

```
bench/
├── harness/              # every script; all of them source harness/env.sh
├── corpus/
│   ├── subjects.json     # every subject: id, kind, languages, tier, dir (relative to $BENCH_WORK)
│   ├── repos.toml        # the real repositories and the commit each is pinned to
│   ├── synthetic.tar.gz  # the synthetic ladder: manifests and lockfiles only
│   ├── pools/            # ranked package lists the ladder was drawn from
│   └── tier_overrides.json
├── report/
│   └── performance.rst.tmpl   # the docs page, with @@PLACEHOLDERS@@ filled from results/
└── results/
    ├── runs.jsonl        # one row per feluda cell (and per other tool run)
    ├── tools.jsonl       # other tools: packages found, agreement with feluda
    ├── env.json          # machine, toolchains, binary hash
    └── raw/              # per cell output (hyperfine, time, report, request counts); not committed
```

The synthetic ladder is a tarball rather than files on purpose. Committed as files, its hundred
manifests and lockfiles would put thousands of pinned, old packages in front of GitHub's dependency
graph and Dependabot, and in front of feluda itself when it scans this repository.

## Rules

Cloned repositories are untrusted code. Nothing in the harness runs their scripts:

- Dependencies are installed with `--ignore-scripts` (npm, pnpm, yarn), `cargo fetch`, `go mod download`
  and `dotnet restore`. Java jars and Ruby gems are downloaded and unpacked by `install_fetch.py`.
- Build wrappers (`mvnw`, `gradlew`, `.mvn`, `gradle/wrapper`) are deleted from the work copy before a
  tool runs, and cdxgen runs with `--no-install-deps` and a `PATH` of `node`, itself, `/usr/bin` and `/bin`, so it
  cannot start a build tool.
- Python helpers run as `python3 -I`. Git clones run with hooks off and no global config.

Every cell gets its own `HOME`, `XDG_CACHE_HOME` and Cargo, Go, Maven, npm, pnpm, uv, NuGet, Gradle,
gem and R directories under `$BENCH_WORK/state/<cell>`, so your own caches are never read or written.

The GitHub token is read from `GITHUB_TOKEN`, or from `gh auth token` when that is unset. It is only
ever an environment variable: never a flag, never written to a file, and scrubbed from logs.

## Requirements

macOS for now (`/usr/bin/time -l`, APFS clones with `cp -c`); a Linux pass is planned.

- `hyperfine`, `jq`, `timeout` and `ts` (coreutils, moreutils), `git`, Python 3
- matplotlib, for `charts.py` only (any Python that has it)
- The toolchains of the ecosystems being run: Rust, Node with npm and pnpm, Go, uv, .NET, Ruby, R,
  and Maven or Gradle for regenerating the Java ladder
- For `run_tools.py`: syft, trivy, cdxgen, scancode, cargo-deny, go-licenses,
  license-checker-rseidelsohn (pip-licenses is installed per run)

## Running

From the repository root, three recipes cover the usual cases:

```sh
export BENCH_WORK=/path/with/50GB/free   # outside this repository
just bench                               # the whole study: setup, ladder, real, flags, polyglot, then the page
just bench-cell python/requests warm --no-local   # one cell
just bench-page                          # data, charts and docs page from results/ (no network)
```

`just bench` builds a release binary into `$BENCH_WORK/feluda` the first time and keeps it, so a resumed
run measures the same build; delete it (or set `BENCH_BIN`) to benchmark a new one. The runs without a
token and the other tools are left out, since they take three hours by design and need extra tools.

The same steps by hand:

```sh
export BENCH_WORK=/path/with/50GB/free     # outside this repository
cargo build --release
cp target/release/feluda "$BENCH_WORK/feluda"   # or set BENCH_BIN; a copy stays fixed while you work

bench/harness/setup.sh          # unpack the ladder, clone every repository at its pinned commit
bench/harness/envinfo.sh        # record machine, toolchains and the binary's hash in results/env.json

python3 -I bench/harness/run_matrix.py ladder     # synthetic ladder, every language
python3 -I bench/harness/run_matrix.py real       # real repositories
python3 -I bench/harness/run_matrix.py flags      # --no-local, --no-clearlydefined, --no-vendor-scan
python3 -I bench/harness/run_matrix.py polyglot   # projects with several languages
bench/harness/unauth_rounds.sh                    # no token: 3 rounds, an hour apart
python3 -I bench/harness/run_tools.py             # other tools on the same corpus
```

`run_matrix.py` is resumable: a cell with a `meta.json` is skipped, so delete a cell's raw directory to
run it again. `--only <text>` limits it to matching subjects or scenarios, `--limit N` to N cells.
Progress goes to `results/PROGRESS.md`. One cell on its own:

```sh
bench/harness/run_cell.sh synthetic/ruby/n10 cold
bench/harness/run_cell.sh python/requests warm --no-local
```

### Scenarios

Feluda's speed depends on its own cache (A) and on the ecosystem's installed state (B), and each
scenario sets them separately:

| scenario | feluda cache | installed dependencies | notes |
|---|---|---|---|
| `cold` | empty | none | a fresh CI runner |
| `cold-installed` | empty | installed | |
| `warm-net` | filled | none | |
| `warm` | filled | installed | a developer's machine |
| `offline` | filled | installed | network refused, `--no-clearlydefined` |
| `unauth-cold` | empty | none | no GitHub token |

Settings, all optional:

| variable | default | |
|---|---|---|
| `BENCH_BIN` | `$BENCH_WORK/feluda` | binary under test |
| `BENCH_PROXY` | unset | proxy for every request, for example `http://127.0.0.1:18888` |
| `SAMPLES`, `WARMUP` | per scenario | hyperfine runs per cell |
| `CELL_BUDGET` | 1200 | seconds a cell may spend sampling |
| `CELL_TIMEOUT` | 1200 (L, XL), 3600 | seconds before one scan counts as *did not finish* |
| `KEEP_STATE` | 0 | 1 keeps the cell's state and work copy for inspection |

Request counts come from feluda itself: with `FELUDA_REQUEST_STATS=<file>` set, it writes a per host
tally of requests, statuses and times as JSON when it exits. `run_cell.sh` sets it on every run.

### Networks that drop IPv6

feluda's HTTP client has no connect timeout, so an address that never answers costs the full 30 s
request timeout. On a network where IPv6 or some addresses are dead, run the IPv4 proxy and point the
harness at it:

```sh
(cd bench/harness && python3 -I v4proxy.py &)
export BENCH_PROXY=http://127.0.0.1:18888
```

## Regenerating the docs page

`just bench-page` runs these in order, skipping the first two when `results/raw` is absent, and draws
the charts with the pinned matplotlib through `uv`, so the same data gives the same SVGs:

```sh
python3 -I bench/harness/collect.py        # results/raw -> results/runs.jsonl
python3 -I bench/harness/tools_summary.py  # results/raw -> results/tools.jsonl
python -I bench/harness/charts.py          # report/charts/*.svg (needs matplotlib 3.11.2)
python3 -I bench/harness/docs_page.py      # docs/source/performance.rst and its charts
```

Every chart is drawn twice by `charts.py`, `<name>.svg` for the light theme and `<name>_dark.svg`
for the dark one, both on a transparent background. The page places each pair as two figures, one
with `:figclass: light-only` and one with `:figclass: dark-only`, and the docs theme shows the one
that matches. A new chart in the template needs both.

`collect.py` and `tools_summary.py` refuse to run without `results/raw`, so the committed data is
never replaced by an empty set. With only the committed data, run `charts.py` and `docs_page.py`.
Every number on the page, in tables and in sentences, is computed from `results/` by `docs_page.py` (the `F` facts); the template, `report/performance.rst.tmpl`, holds only wording and `@@PLACEHOLDERS@@`. After a new run, the numbers update themselves; reread the sentences that interpret them ("barely change", "within the noise") in case the data now says otherwise. If a fact cannot be computed, for example a scenario that was not run, `docs_page.py` stops with an error rather than writing a stale number.
Edit the template, never `performance.rst`, then check the docs with `just docs-check`.

`tables.py` writes every table to `report/tables_all.md` for reading; it feeds nothing.

## Changing the corpus

- `gen_synthetic.py <lang> [N ...]` writes ladder rungs to `$BENCH_WORK/synthetic` from `corpus/pools`,
  locking where the ecosystem locks, without running package code. `fetch_pools.py` and
  `fetch_pools_extra.py` refresh the pools.
- A new repository goes in `PRIMARY` in `make_subjects.py` (with its primary language), then
  `clone.sh owner/name` and `scan_once.sh owner/name` to size it.
- `make_subjects.py` rebuilds `subjects.json` and `repos.toml` from the clones and the ladder. Run it
  with every clone present: a repository without a clone is left out. One without a `scan_once.sh`
  report keeps the size it already had.
- Pack the ladder back up afterwards, without macOS metadata files:
  `COPYFILE_DISABLE=1 tar -C "$BENCH_WORK" -czf bench/corpus/synthetic.tar.gz synthetic`.
