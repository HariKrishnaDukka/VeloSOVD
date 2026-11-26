#!/usr/bin/env python3
"""Dependency age audit (14-day rule) for the Studio frontend and the Tauri shell.

Audits EVERY entry of sovd-studio/package-lock.json (npm registry) and EVERY registry package of
sovd-studio/src-tauri/Cargo.lock (crates.io) and lists versions published less than N days ago.

  python3 tools/check_dep_age.py [--days 14] [--npm-only | --cargo-only] [--refresh]

Exit code: 0 = all old enough, 1 = young or unknown versions found, 2 = network or file error.
Network: registry.npmjs.org (8 parallel requests) and the crates.io API (1 request per second,
as the crates.io crawler policy asks; about 400 crates = roughly 7 minutes the first time).
crates.io answers are cached in ~/.cache/velosovd-dep-age/crates.json and reused (--refresh
ignores the cache). HINT = newest stable version of the same major (0.x: same minor) that is old
enough. It does not check what the dependents allow: pin it, rebuild, run this script again.
"""
import argparse
import datetime
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NPM_LOCK = os.path.join(ROOT, "sovd-studio", "package-lock.json")
CARGO_LOCK = os.path.join(ROOT, "sovd-studio", "src-tauri", "Cargo.lock")
CACHE_DIR = os.path.join(os.environ.get("XDG_CACHE_HOME", os.path.expanduser("~/.cache")), "velosovd-dep-age")
UA = "velosovd-dep-age/1.0 (https://github.com/HariKrishnaDukka/VeloSOVD)"
UTC = datetime.timezone.utc


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(2)


def when(text):
    m = re.match(r"(\d{4})-(\d\d)-(\d\d)[T ](\d\d):(\d\d):(\d\d)", text or "")
    return datetime.datetime(*map(int, m.groups()), tzinfo=UTC) if m else None


def get_json(url):
    for attempt in range(6):
        req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            time.sleep(int(e.headers.get("Retry-After", "5")) + 1 if e.code == 429 else 2 * (attempt + 1))
        except OSError:
            time.sleep(2 * (attempt + 1))
    fail(f"cannot reach {url}")


def vkey(v):
    return tuple(int(x) if x.isdigit() else 0 for x in re.split(r"[-+]", v)[0].split("."))


def same_series(cur, cand):
    a, b = vkey(cur), vkey(cand)
    return a[0] == b[0] if a[0] > 0 else a[:2] == b[:2]


def hint(current, published, cutoff, yanked=()):
    ok = [v for v, d in published.items()
          if d and d <= cutoff and "-" not in v.split("+")[0] and v not in yanked and same_series(current, v)]
    return max(ok, key=vkey) if ok else "-"


def audit_npm(cutoff):
    lock = json.load(open(NPM_LOCK, encoding="utf-8"))
    entries = set()
    for path, e in lock.get("packages", {}).items():
        if path and not e.get("link") and e.get("version"):
            entries.add((e.get("name") or path.split("node_modules/")[-1], e["version"]))
    names = sorted({n for n, _ in entries})

    def fetch(name):
        doc = get_json("https://registry.npmjs.org/" + urllib.parse.quote(name, safe="@"))
        times = (doc or {}).get("time", {})
        return name, {v: when(t) for v, t in times.items() if v not in ("created", "modified")}

    with ThreadPoolExecutor(8) as pool:
        times = dict(pool.map(fetch, names))
    return [("npm", n, v, times.get(n, {}).get(v), hint(v, times.get(n, {}), cutoff)) for n, v in sorted(entries)]


def cargo_packages():
    pkgs, cur = [], {}
    for line in open(CARGO_LOCK, encoding="utf-8"):
        line = line.strip()
        if line == "[[package]]":
            if cur:
                pkgs.append(cur)
            cur = {}
        else:
            m = re.match(r'(name|version|source) = "(.*)"$', line)
            if m:
                cur[m.group(1)] = m.group(2)
    if cur:
        pkgs.append(cur)
    return sorted({(p["name"], p["version"]) for p in pkgs if p.get("source", "").startswith("registry+")})


def crate_versions(name):
    versions, url = [], f"https://crates.io/api/v1/crates/{name}/versions?per_page=100"
    while url:
        doc = get_json(url)
        if doc is None:
            break
        versions += [[v["num"], v["created_at"], v["yanked"]] for v in doc["versions"]]
        nxt = (doc.get("meta") or {}).get("next_page")
        url = f"https://crates.io/api/v1/crates/{name}/versions{nxt}" if nxt else None
        time.sleep(1.0)
    return versions


def audit_cargo(cutoff, refresh):
    pkgs = cargo_packages()
    os.makedirs(CACHE_DIR, exist_ok=True)
    cache_file = os.path.join(CACHE_DIR, "crates.json")
    cache = {} if refresh or not os.path.exists(cache_file) else json.load(open(cache_file))
    known = {n: {x[0] for x in cache.get(n, [])} for n, _ in pkgs}
    todo = sorted({n for n, v in pkgs if v not in known[n]})
    for i, name in enumerate(todo, 1):
        cache[name] = crate_versions(name)
        if i % 20 == 0 or i == len(todo):
            json.dump(cache, open(cache_file, "w"))
            print(f"  crates.io: {i}/{len(todo)} crates fetched", file=sys.stderr, flush=True)
    rows = []
    for n, v in pkgs:
        info = {x[0]: when(x[1]) for x in cache.get(n, [])}
        yanked = {x[0] for x in cache.get(n, []) if x[2]}
        rows.append(("cargo", n, v, info.get(v), hint(v, info, cutoff, yanked)))
    return rows


def main():
    ap = argparse.ArgumentParser(description="Dependency age audit (14-day rule)")
    ap.add_argument("--days", type=int, default=14)
    group = ap.add_mutually_exclusive_group()
    group.add_argument("--npm-only", action="store_true")
    group.add_argument("--cargo-only", action="store_true")
    ap.add_argument("--refresh", action="store_true", help="ignore the crates.io cache")
    a = ap.parse_args()
    now = datetime.datetime.now(UTC)
    cutoff = now - datetime.timedelta(days=a.days)
    for path in ([] if a.cargo_only else [NPM_LOCK]) + ([] if a.npm_only else [CARGO_LOCK]):
        if not os.path.exists(path):
            fail(f"missing {path}")
    rows = []
    if not a.cargo_only:
        rows += audit_npm(cutoff)
    if not a.npm_only:
        rows += audit_cargo(cutoff, a.refresh)
    bad = []
    for eco in ("npm", "cargo"):
        mine = [r for r in rows if r[0] == eco]
        if mine:
            young = [r for r in mine if r[3] and r[3] > cutoff]
            unknown = [r for r in mine if not r[3]]
            print(f"{eco:5} {len(mine):4} packages audited | younger than {a.days} days: {len(young)} | date unknown: {len(unknown)}")
            bad += young + unknown
    if bad:
        print(f"\n{'eco':5} {'package':34} {'version':22} {'published':10} {'age':>5}  hint")
        for eco, n, v, d, h in bad:
            age = f"{(now - d).days}d" if d else "-"
            print(f"{eco:5} {n:34} {v:22} {d.strftime('%Y-%m-%d') if d else 'UNKNOWN':10} {age:>5}  {h}")
    print("\nRESULT: " + ("FAIL" if bad else "PASS"))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
