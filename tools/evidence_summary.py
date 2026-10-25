#!/usr/bin/env python3
"""Evidence skip summary for CI (from X_ITE_R tools/evidence_summary.py at ffe080b1): python3 -I -B tools/evidence_summary.py LOG

LOG is the CM_EVIDENCE_LOG file that cm-evidence writes: one line per permitted skip, with the fields
test, reason, key, caller and detail, separated by tabs. Prints a Markdown table of every skip, and of the
tests that GPU_TESTS skips by name (`--skip NAME`). Exits 1 when a line is malformed or names a reason or
key outside CM_EVIDENCE_ALLOW, or when a line records a key mismatch. An absent or empty LOG prints
"0 evidence skips" and exits 0."""
import os, sys


def allowed(allow, reason, key):
    if reason == "key-mismatch":
        return False
    if reason == "key-absent":
        return key != "-" and key in allow
    return reason in allow


def main(argv):
    if len(argv) != 1:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        return 1
    allow = {a.strip() for a in os.environ.get("CM_EVIDENCE_ALLOW", "").split(",") if a.strip()}
    lines = []
    if os.path.exists(argv[0]):
        with open(argv[0], encoding="utf-8") as f:
            lines = [l.rstrip("\n") for l in f if l.strip()]
    bad = []
    print(f"### Evidence skips: {len(lines)}\n")
    if lines:
        print("| test | reason | key | caller | detail | allowed |\n| --- | --- | --- | --- | --- | --- |")
    for l in lines:
        c = l.split("\t")
        if len(c) != 5:
            bad.append(f"malformed line: {l!r}")
            continue
        ok = allowed(allow, c[1], c[2])
        if not ok:
            bad.append(f"not allowed: {c[0]} {c[1]} {c[2]}")
        cells = [x.replace("|", "\\|") for x in c]
        print("| " + " | ".join(cells) + f" | {'yes' if ok else '**NO**'} |")
    if not lines:
        print("0 evidence skips")
    skipped = os.environ.get("GPU_TESTS", "").split()
    names = [skipped[i + 1] for i, a in enumerate(skipped[:-1]) if a == "--skip"]
    print(f"\n### Tests skipped by name (GPU_TESTS): {len(names)}\n")
    for n in names:
        print(f"- `{n}`")
    print(f"\nAllow list: `{','.join(sorted(allow)) or '(empty)'}`")
    for b in bad:
        print(f"\n**FAIL** {b}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
