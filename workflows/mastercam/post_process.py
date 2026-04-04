#!/usr/bin/env python3

"""Stitch Mastercam outputs into a combined CSV.

Combines three sources:
  - toolpaths CSV   : toolpath names, one per line (written by WriteOutput)
  - simulator output: Operation Info CSV (may have UTF-8 BOM)

Output format (overwrites simulator output file):
  Toolpath,<simulator columns>
  <toolpath>,<simulator row>
  ...
"""

import argparse
import csv
import os


def csv_field(value: str) -> str:
    """Minimal CSV quoting — quote the field only if it contains a comma."""
    if "," in value:
        return f'"{value}"'
    return value


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--simulator-output",
        required=True,
        metavar="PATH",
        help="Simulator Operation Info CSV (read then overwritten with stitched result).",
    )
    parser.add_argument(
        "--toolpaths-csv",
        required=True,
        metavar="PATH",
        help="Toolpath names CSV written by WriteOutput (one quoted name per line).",
    )
    args = parser.parse_args()

    # Read toolpath names.
    # WriteOutput quotes each value: "toolpath name" — use csv.reader to unescape.
    # Filter out blank/whitespace-only entries (trailing empty row from WriteOutput).
    with open(args.toolpaths_csv, newline="", encoding="utf-8") as f:
        toolpaths = [row[0].strip() for row in csv.reader(f) if row and row[0].strip()]

    # Read simulator output as raw lines.
    # encoding='utf-8-sig' automatically strips the BOM if present.
    with open(args.simulator_output, encoding="utf-8-sig") as f:
        sim_lines = f.read().splitlines()

    if not sim_lines:
        os.remove(args.toolpaths_csv)
        raise SystemExit("simulator output is empty")

    sim_header, *sim_data = sim_lines

    with open(args.simulator_output, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"Toolpath,{sim_header}\n")
        for toolpath, sim_row in zip(toolpaths, sim_data):
            f.write(f"{csv_field(toolpath)},{sim_row}\n")

    os.remove(args.toolpaths_csv)


if __name__ == "__main__":
    main()
