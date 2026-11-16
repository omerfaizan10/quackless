"""Download the sqllogictest subset I run, from a git mirror of SQLite's
sqllogictest repository pinned to one commit.

Usage: .venv/bin/python scripts/fetch_slt.py   (writes data/slt/...)
"""
import pathlib
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
MIRROR = "https://raw.githubusercontent.com/risinglightdb/sqllogictest-sqlite"
COMMIT = "4ab49f571a02e3d21d6f8d4d2cc11eb90d7729c9"

FILES = (
    [f"test/select{i}.test" for i in range(1, 6)]
    + [f"test/evidence/{n}.test" for n in (
        "in1", "in2", "slt_lang_aggfunc", "slt_lang_createview",
        "slt_lang_droptable", "slt_lang_dropview")]
    + [f"test/random/groupby/slt_good_{i}.test" for i in range(14)]
    + [f"test/random/{d}/slt_good_{i}.test" for d in ("aggregates", "expr", "select") for i in range(10)]
)


def main():
    for rel in FILES:
        dest = ROOT / "data" / "slt" / rel.removeprefix("test/")
        if dest.exists():
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        req = urllib.request.Request(f"{MIRROR}/{COMMIT}/{rel}", headers={"User-Agent": "quackless-research/0.1"})
        with urllib.request.urlopen(req, timeout=120) as r:
            body = r.read()
        tmp = dest.with_suffix(".partial")
        tmp.write_bytes(body)
        tmp.rename(dest)
        print(f"{rel}: {len(body):,} bytes")


if __name__ == "__main__":
    main()
