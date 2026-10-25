#!/usr/bin/env python3
"""Deterministic content digest for a reference source tree.

The identity gate must check reference sources by content, not only by a stored
SHA string. A stored SHA proves what someone wrote down. A content digest proves
the bytes that are actually on disk.

Usage:
  reference-digest.py <dir> [--sub a] [--sub b] [--exclude name]

Exit codes:
  0  digest printed on stdout as: <file_count> <sha256>
  2  usage error
  3  the directory does not exist
"""
import hashlib
import os
import sys


def collect(root, subs, excludes):
    """Return {relative_path: sha256} for every regular file under the given roots."""
    bases = [root] if not subs else [os.path.join(root, s) for s in subs]
    out = {}
    for base in bases:
        if not os.path.isdir(base):
            sys.stderr.write("reference-digest: not a directory: %s\n" % base)
            sys.exit(3)
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d not in excludes)
            for name in sorted(filenames):
                path = os.path.join(dirpath, name)
                if os.path.islink(path) or not os.path.isfile(path):
                    continue
                rel = os.path.relpath(path, base)
                digest = hashlib.sha256()
                with open(path, "rb") as handle:
                    for chunk in iter(lambda: handle.read(1 << 20), b""):
                        digest.update(chunk)
                out[rel] = digest.hexdigest()
    return out


def main(argv):
    args = argv[1:]
    if not args:
        sys.stderr.write(__doc__)
        return 2
    subs, excludes = [], {".git", "node_modules"}
    while args:
        arg = args.pop(0)
        if arg == "--sub":
            if not args:
                sys.stderr.write("reference-digest: --sub needs a value\n")
                return 2
            subs.append(args.pop(0))
        elif arg == "--exclude":
            if not args:
                sys.stderr.write("reference-digest: --exclude needs a value\n")
                return 2
            excludes.add(args.pop(0))
        else:
            root = arg
    if not os.path.isdir(root):
        sys.stderr.write("reference-digest: not a directory: %s\n" % root)
        return 3

    files = collect(root, subs, excludes)
    # Fold in the path names too, so a rename or a moved file changes the digest
    # even when the bytes themselves are identical.
    overall = hashlib.sha256()
    for rel in sorted(files):
        overall.update(rel.encode("utf-8"))
        overall.update(b"\0")
        overall.update(files[rel].encode("ascii"))
        overall.update(b"\n")
    print("%d %s" % (len(files), overall.hexdigest()))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))