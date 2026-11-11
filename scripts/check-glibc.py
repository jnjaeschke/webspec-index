#!/usr/bin/env python3
"""Reject ELF binaries requiring GLIBC symbols newer than the release baseline."""

import argparse
import os
import re
import subprocess


def version(value):
    return tuple(int(part) for part in value.split("."))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    parser.add_argument("maximum", type=version, help="maximum GLIBC version, e.g. 2.28")
    args = parser.parse_args()
    result = subprocess.run(
        ["readelf", "--version-info", "--wide", args.binary],
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, "LC_ALL": "C"},
    )
    requirements = set(re.findall(r"Name: (GLIBC_\S+)", result.stdout))
    if not requirements:
        parser.exit(1, "error: no GLIBC version requirements found in GNU binary\n")
    unsupported = sorted(
        name
        for name in requirements
        if not re.fullmatch(r"GLIBC_\d+(?:\.\d+)+", name)
        or version(name.removeprefix("GLIBC_")) > args.maximum
    )
    if unsupported:
        parser.exit(
            1, "error: unsupported GLIBC requirements: " + ", ".join(unsupported) + "\n"
        )
    highest = max(requirements, key=lambda name: version(name.removeprefix("GLIBC_")))
    print(f"{args.binary}: highest requirement is {highest}")


if __name__ == "__main__":
    main()
