#!/usr/bin/env python3
"""Inspect exact LLVM counters for workspace source files, without rounding.

Lines and functions come from llvm-cov's file summaries. Regions do not: for
each function llvm-cov credits only its best single instantiation, and the CLI
is compiled twice (the unit-test binary and the `admit` binary its integration
tests run), so a region one binary ran and the other did not reads as
uncovered. A region here is covered when any instantiation ran it.
"""
import json
import sys
from pathlib import Path

CODE_REGION = 0


def workspace_source(filename):
    return "/crates/" in filename and "/src/" in filename


def regions(report):
    """{filename: {span: covered}} over every instantiation's code regions."""
    out = {}
    for d in report["data"]:
        for fn in d["functions"]:
            for start_line, start_col, end_line, end_col, count, file_id, _, kind, *_ in fn["regions"]:
                filename = fn["filenames"][file_id]
                # file_id 0 is the function's own file; others are macro
                # expansions, which llvm-cov's summaries leave out too.
                if kind != CODE_REGION or file_id != 0 or not workspace_source(filename):
                    continue
                spans = out.setdefault(filename, {})
                span = (start_line, start_col, end_line, end_col)
                spans[span] = spans.get(span, False) or count > 0
    return out


def inspect(report):
    files = [f for d in report["data"] for f in d["files"] if workspace_source(f["filename"])]
    if not files:
        raise ValueError("coverage report contains no workspace source files")
    spans = regions(report)
    totals = {metric: [0, 0] for metric in ("lines", "functions", "regions")}
    missing = []
    for f in files:
        counts = {metric: (f["summary"][metric]["covered"], f["summary"][metric]["count"]) for metric in ("lines", "functions")}
        r = spans.get(f["filename"], {})
        counts["regions"] = (sum(r.values()), len(r))
        for metric, (covered, count) in counts.items():
            totals[metric][0] += covered
            totals[metric][1] += count
            if covered != count:
                missing.append((f["filename"], metric, count - covered))
    return totals, missing


def main():
    totals, missing = inspect(json.loads(Path(sys.argv[1]).read_text()))
    for metric, (covered, count) in totals.items():
        print(f"{metric}: {covered}/{count} ({100 * covered / count if count else 100:.6f}%)")
    if "--check" in sys.argv:
        for filename, metric, count in missing:
            print(f"{filename}: {count} uncovered {metric}")
        return bool(missing)
    return 0


if __name__ == "__main__":
    sys.exit(main())
