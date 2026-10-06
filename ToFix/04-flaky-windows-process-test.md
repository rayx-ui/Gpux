# 04: Flaky Windows test `test_drop_terminates_grandchildren`

## What Happened

CI run [37521002906](https://github.com/rayx-ui/Gpux/actions/runs/37521002906), job
`test (windows)`, on PR #2, whose change touches only WGSL:

```
test process::windows_tests::test_drop_terminates_grandchildren ... FAILED
panicked at crates\util\src\process.rs:230:13:
timed out waiting for grandchild pid file
test result: FAILED. 113 passed; 1 failed
error: test failed, to rerun pass `-p gpux-zed-util --lib`
```

The same job passed on both earlier `main` runs (37500248162 and 37487767802) and on PR #1's run.
**The re-run of the failed job passed**, which confirms a timing flake. Upstream Zed `main` still
uses the same 5 second deadline (checked 2026-10-06; last change to the file `c642b422de`,
2026-06-15), so there is no upstream fix to import.

## Why It Is Probably A Timing Flake

The test (`crates/util/src/process.rs`, `windows_tests::spawn_process_tree`, from upstream Zed and
unchanged in gpux) launches `powershell.exe -NoProfile -Command "Start-Process ping.exe ...;
Set-Content <pid file> ..."`. It then polls for the pid file with a **5 second** deadline. A cold
PowerShell start plus `Start-Process` on a loaded `windows-latest` runner can exceed 5 seconds. CI
runs tests with `--test-threads=1` after a long build, while Defender scans new binaries.

## Options

- Leave it and re-run on failure (it is upstream test code).
- Harden it as a small gpux patch: raise the deadline to 30 seconds. Replace PowerShell with a
  quicker launcher if the grandchild semantics allow it, for example
  `cmd /c start /b ping -n 60 127.0.0.1` together with recording the pid. Record the patch in the
  import guide so the next Zed import keeps it.
- Check upstream Zed for a newer version of this test before patching.

Validate with `cargo test -p gpux-zed-util process::windows_tests -- --test-threads=1` on Windows,
several times in a row.
