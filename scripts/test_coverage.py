import unittest
from coverage import inspect

FILE = "/workspace/crates/domain/src/lib.rs"


def region(line, count, kind=0, file_id=0):
    return [line, 1, line, 9, count, file_id, 0, kind]


def instantiation(*regions, filenames=(FILE, "/rustlib/core/src/macros.rs")):
    return {"filenames": list(filenames), "regions": list(regions)}


class CoverageGate(unittest.TestCase):
    def report(self, covered, count, functions=None):
        summary = {m: {"covered": covered, "count": count} for m in ("lines", "functions", "regions")}
        if functions is None:
            functions = [instantiation(*[region(i, int(i < covered)) for i in range(count)])]
        return {"data": [{"files": [{"filename": FILE, "summary": summary}], "functions": functions}]}

    def test_exact_counters_reject_a_rounded_hundred_percent(self):
        totals, missing = inspect(self.report(999999, 1000000))
        self.assertTrue(missing)
        self.assertEqual(totals["lines"], [999999, 1000000])
        self.assertEqual(totals["regions"], [999999, 1000000])

    def test_full_coverage_passes(self):
        self.assertFalse(inspect(self.report(4, 4))[1])

    def test_no_source_files_is_an_error(self):
        with self.assertRaises(ValueError):
            inspect({"data": [{"files": [], "functions": []}]})

    def test_a_region_any_instantiation_ran_is_covered(self):
        # The CLI's unit-test binary and the binary its integration tests run
        # each execute part of one function; llvm-cov's summary credits only
        # the better one.
        halves = [instantiation(region(1, 1), region(2, 0)), instantiation(region(1, 0), region(2, 3))]
        totals, missing = inspect(self.report(2, 2, halves))
        self.assertEqual(totals["regions"], [2, 2])
        self.assertFalse(missing)

    def test_a_region_no_instantiation_ran_is_missing(self):
        both_miss = [instantiation(region(1, 1), region(2, 0)), instantiation(region(1, 2), region(2, 0))]
        totals, missing = inspect(self.report(2, 2, both_miss))
        self.assertEqual(totals["regions"], [1, 2])
        self.assertIn((FILE, "regions", 1), missing)

    def test_branch_gap_and_macro_regions_are_not_counted(self):
        others = [instantiation(region(1, 1), region(2, 0, kind=4), region(3, 0, kind=3), region(4, 0, file_id=1))]
        self.assertEqual(inspect(self.report(1, 1, others))[0]["regions"], [1, 1])


if __name__ == "__main__":
    unittest.main()
