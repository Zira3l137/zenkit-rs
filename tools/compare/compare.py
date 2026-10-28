"""Compares the output of `zk-dump` (zenkit-rs) with that of `dump_zenkit.py` (ZenKit).

    python compare.py <zenkit-rs.jsonl> <zenkit.jsonl>

Floats are compared at f32 precision, since both sides read the same 32-bit values. Differences
zenkit-rs makes on purpose are listed in KNOWN and reported separately. Exits with 1 if anything
else differs.
"""

import json
import math
import re
import struct
import sys
from collections import Counter, defaultdict

# Where zenkit-rs deliberately differs from ZenKit, as (path pattern, reason).
KNOWN = [
    (r"polygons\[\]\.normal_axis$", "Gothic 1 normal_axis: ZenKit merges both bits into one"),
    (r"mipmaps\[\]\.(width|height)$", "ZenKit reports 0 for mipmaps smaller than a pixel"),
]

EXAMPLES_PER_PATH = 3


def f32(x):
    return struct.unpack("f", struct.pack("f", x))[0]


def walk(rs, zk, pattern, at, out):
    """Records every difference between `rs` and `zk` in `out` as (pattern, at, rs, zk), where
    `pattern` leaves out list indices so that differences group by field, and `at` keeps them."""
    if isinstance(zk, dict) and "__error__" in zk:
        out.append((pattern, at, "<skipped>", zk["__error__"]))
        return

    # JSON has no NaN or infinity. zk-dump (serde_json) writes them as null, Python as NaN/Infinity.
    if rs is None and isinstance(zk, float) and not math.isfinite(zk):
        return

    if isinstance(rs, bool) or isinstance(zk, bool):
        if bool(rs) != bool(zk):
            out.append((pattern, at, rs, zk))
    elif isinstance(rs, float) or isinstance(zk, float):
        if not isinstance(rs, (int, float)) or not isinstance(zk, (int, float)):
            out.append((pattern, at, rs, zk))
        elif f32(rs) != f32(zk) and not (rs != rs and zk != zk):
            out.append((pattern, at, rs, zk))
    elif isinstance(rs, dict) and isinstance(zk, dict):
        for key in rs.keys() | zk.keys():
            dot = "." if pattern else ""
            walk(rs.get(key), zk.get(key), f"{pattern}{dot}{key}", f"{at}{dot}{key}", out)
    elif isinstance(rs, list) and isinstance(zk, list):
        if len(rs) != len(zk):
            out.append((f"{pattern}.len", f"{at}.len", len(rs), len(zk)))
        for i, (a, b) in enumerate(zip(rs, zk)):
            walk(a, b, f"{pattern}[]", f"{at}[{i}]", out)
    elif zk is None and pattern.endswith("rgba"):
        pass  # ZenKit cannot decode this format; zenkit-rs can.
    elif rs != zk:
        out.append((pattern, at, rs, zk))


def load(path):
    entries = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            entry = json.loads(line)
            entries[entry["path"].lower()] = entry
    return entries


def known(path):
    for pattern, reason in KNOWN:
        if re.search(pattern, path):
            return reason
    return None


def main():
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2

    rs, zk = load(sys.argv[1]), load(sys.argv[2])
    stats = Counter()
    differences = defaultdict(list)
    expected = Counter()
    skipped = Counter()
    errors = []

    for name in sorted(rs.keys() | zk.keys()):
        a, b = rs.get(name), zk.get(name)
        if a is None or b is None:
            stats["only in " + ("ZenKit" if a is None else "zenkit-rs")] += 1
            continue
        if "error" in a and "error" in b:
            stats["fail to load on both sides"] += 1
            continue
        if "error" in a or "error" in b:
            stats["fail to load on one side"] += 1
            errors.append((name, a.get("error", "loaded"), b.get("error", "loaded")))
            continue

        found = []
        walk(a["result"], b["result"], "", "", found)
        unexpected = False
        for pattern, at, x, y in found:
            if x == "<skipped>":
                skipped[pattern] += 1
            elif reason := known(pattern):
                expected[reason] += 1
            else:
                unexpected = True
                if len(differences[pattern]) < EXAMPLES_PER_PATH:
                    differences[pattern].append((name, at, x, y))
                else:
                    differences[pattern].append(None)
        stats["differ" if unexpected else "identical"] += 1

    print(f"{len(rs.keys() | zk.keys())} files")
    for key, count in sorted(stats.items()):
        print(f"  {key}: {count}")

    if expected:
        print("\nKnown, intended differences:")
        for reason, count in expected.most_common():
            print(f"  {count:>8}  {reason}")

    if skipped:
        print("\nValues ZenKit's Python bindings could not read (not compared):")
        for path, count in skipped.most_common(10):
            print(f"  {count:>8}  {path or '(whole file)'}")

    if errors:
        print("\nLoaded by only one side (zenkit-rs | ZenKit):")
        for name, a, b in errors[:20]:
            print(f"  {name}\n      {a}\n      {b}")

    if differences:
        print("\nUnexpected differences (zenkit-rs vs ZenKit):")
        for path, examples in sorted(differences.items(), key=lambda d: -len(d[1])):
            print(f"  {path}: {len(examples)} values")
            for example in filter(None, examples):
                name, at, x, y = example
                print(f"      {name} {at}: {json.dumps(x)[:80]} vs {json.dumps(y)[:80]}")

    return 1 if differences or errors else 0


if __name__ == "__main__":
    sys.exit(main())
