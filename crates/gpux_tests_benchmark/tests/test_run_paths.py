"""The paths run.py works with from crates/gpux_tests_benchmark.

    python3 -m unittest discover -s crates/gpux_tests_benchmark/tests -p "test_*.py" -v

Imports run.py without running it: the checkout it finds, where results go, which changes make
the gpux tree dirty, the artifact names the build produces, and the Playwright install the web
runner loads. Needs git and cargo on PATH.
"""

import importlib.util
import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent


def load_run():
    spec = importlib.util.spec_from_file_location("gpux_bench_run", BENCH / "run.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


run = load_run()


def lib_name():
    """`[lib] name` of the benchmark's Cargo.toml."""
    manifest = (BENCH / "Cargo.toml").read_text()
    section = manifest.split("[lib]", 1)[1].split("\n[", 1)[0]
    return re.search(r'^name = "([^"]+)"', section, re.MULTILINE).group(1)


class Paths(unittest.TestCase):
    def test_root_is_the_checkout(self):
        self.assertEqual(run.BENCH, BENCH)
        self.assertEqual(run.ROOT, BENCH.parent.parent)
        self.assertTrue((run.ROOT / "Cargo.toml").is_file())
        self.assertTrue((run.ROOT / "crates" / "gpux_tests_conformance").is_dir())
        self.assertEqual(run.Tree().root, run.ROOT)

    def test_results_stay_in_the_benchmark_folder(self):
        self.assertEqual(run.RESULTS, run.ROOT / "crates" / "gpux_tests_benchmark" / "results")

    def test_artifact_names_follow_the_library(self):
        self.assertEqual(run.LIB, lib_name())
        self.assertEqual(run.LIB, "gpux_tests_benchmark")
        source = (BENCH / "run.py").read_text()
        self.assertIn('f"lib{LIB}.a"', source)
        self.assertIn('f"{LIB}.wasm"', source)

    def test_the_page_imports_the_module(self):
        page = (BENCH / "web" / "index.html").read_text()
        self.assertIn(f"import('./{run.LIB}.js')", page)

    def test_the_web_runner_finds_playwright_in_tools_browser(self):
        script = (BENCH / "web" / "run.mjs").read_text()
        match = re.search(r"createRequire\(path\.join\(here, ([^)]*)\)\)", script)
        self.assertIsNotNone(match, "the createRequire call")
        parts = re.findall(r"'([^']*)'", match.group(1))
        package = (BENCH / "web").joinpath(*parts).resolve()
        self.assertEqual(package, run.ROOT / "tools" / "browser" / "package.json")
        self.assertTrue(package.is_file())
        self.assertIn("@playwright/test", json.loads(package.read_text())["devDependencies"])

    def test_the_root_workspace_does_not_build_the_benchmark(self):
        metadata = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
            cwd=run.ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        packages = {package["name"] for package in json.loads(metadata)["packages"]}
        self.assertIn("gpux-tests-conformance", packages)
        self.assertNotIn("gpux-tests-benchmark", packages)
        self.assertIn("[workspace]", (BENCH / "Cargo.toml").read_text())


def git(cwd, *args):
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True)


class Dirty(unittest.TestCase):
    """`Tree.dirty()` in a scratch repository shaped like the checkout."""

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)
        self.bench = self.root / "crates" / "gpux_tests_benchmark"
        self.gpui = self.root / "crates" / "gpui"
        for folder in (self.bench / "src", self.gpui):
            folder.mkdir(parents=True)
        (self.root / "Cargo.toml").write_text("[workspace]\n")
        (self.bench / "src" / "scenario.rs").write_text("// a scenario\n")
        (self.gpui / "lib.rs").write_text("// gpux\n")
        git(self.root, "init", "-q")
        git(self.root, "add", ".")
        git(
            self.root,
            "-c", "user.name=bench", "-c", "user.email=bench@example.invalid",
            "commit", "-q", "-m", "scratch",
        )
        self.tree = run.Tree()
        self.tree.root = self.root
        self.tree.bench = self.bench

    def tearDown(self):
        self.scratch.cleanup()

    def test_a_clean_tree_is_not_dirty(self):
        self.assertFalse(self.tree.dirty())

    def test_benchmark_edits_and_results_do_not_count(self):
        (self.bench / "src" / "scenario.rs").write_text("// an edited scenario\n")
        (self.bench / "results" / "run").mkdir(parents=True)
        (self.bench / "results" / "run" / "meta.json").write_text("{}\n")
        self.assertFalse(self.tree.dirty())

    def test_a_change_in_another_crate_counts(self):
        (self.gpui / "lib.rs").write_text("// gpux, changed\n")
        self.assertTrue(self.tree.dirty())


if __name__ == "__main__":
    unittest.main()
