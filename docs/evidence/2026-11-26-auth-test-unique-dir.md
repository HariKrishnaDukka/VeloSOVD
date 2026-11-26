# Evidence: auth-service flaky test fix, unique temp dir per test (2026-11-26)

**Scope:** test-only fix for the intermittent failure of `auth-service::server::tests::same_error_for_wrong_password_and_unknown_user` (`store.save(&path).unwrap()` panicking with `No such file or directory (os error 2)`). Every unit test that needs a temp directory now gets its own, named from the process id plus a process-wide counter, instead of from a clock reading.
**Environment:** Linux developer laptop, Rust workspace built with `--locked`. Toolchain version was not printed in this run.
**Commit under test:** `01f8c53` on branch `fix/auth-test-unique-dir`, based on `f2a5d8d` (main, merge of PR #28). Changed files: `crates/auth-service/src/main.rs`, `server.rs`, `store.rs`, `test_util.rs` (new). No product code, no Cargo file changed.
**Requirements:** `PRJ-SEC-11` and `PRJ-SEC-12` (supporting). Mapped by test name in `docs/requirements/requirements.csv` at `f2a5d8d`: `store::tests::save_load_roundtrip_and_management` is listed under PRJ-SEC-11; `server::tests::same_error_for_wrong_password_and_unknown_user` and `server::tests::disabling_a_user_applies_without_restart` are listed under PRJ-SEC-12. No requirement row, status or test name changed.

This record retains the observed results of one manual run on one machine. It does not establish ISO 17978, ISO 14229, ISO/SAE 21434 or UNECE R155 conformance. A run of 200 passes without a failure does not prove that the test can never fail again.

## Expected vs observed

All 14 checks matched.

| # | Check | Expected | Observed | Requirement |
|---|---|---|---|---|
| 1 | `git rev-parse --short HEAD` on the new branch; `git status --short` before it | `f2a5d8d`, empty status | `f2a5d8d`, empty status | - |
| 2 | `git am --keep-cr` of the patch, then `git show --stat` | applies, 4 files, +27, -6 | `Applying: test(auth): ...`, HEAD `01f8c53`, 4 files, 27 insertions, 6 deletions (exit code not echoed) | - |
| 3 | files changed outside `crates/auth-service/src/` | 0 | 0 | - |
| 4 | diff lines in `Cargo.lock`, `Cargo.toml`, `crates/*/Cargo.toml` | 0 | 0 | - |
| 5 | `cargo fmt -p auth-service -- --check` | exit 0 | exit 0 | - |
| 6 | `cargo clippy -p auth-service --all-targets --locked -- -D warnings` | exit 0 | exit 0 | - |
| 7 | `cargo test -p auth-service --locked` | 12 passed, 0 failed | 12 passed, 0 failed (0.40 s) | PRJ-SEC-11, PRJ-SEC-12 |
| 8 | `python3 tools/check_traceability.py` | exit 0, 57 references, 0 problems | exit 0, 57 test references checked, 0 problems | - |
| 9 | 200 runs of the test binary, `--test-threads=16` | `failed=0` | `runs=200 failed=0` | PRJ-SEC-11, PRJ-SEC-12 |
| 10 | temp dirs left after the 200 runs | 800 (4 per run) | `dirs=800` | - |
| 11 | processes that created other than 4 dirs | 0 | `bad_pids=0` | - |
| 12 | fresh clone of the pushed branch, `git log --oneline -2` | top `test(auth)...`, second `f2a5d8d` | top `01f8c53 test(auth): ...`, second `f2a5d8d` | - |
| 13 | `cargo build -p auth-service --locked`, warning count on the fresh clone | 0 | 0 | - |
| 14 | 5 x `cargo test -p auth-service --locked` on the fresh clone | 5 x 12 passed, 0 failed | 5 x `12 passed; 0 failed` (0.46, 0.49, 0.48, 0.39, 0.39 s) | PRJ-SEC-11, PRJ-SEC-12 |

Checks 10 and 11 together show that each of the 200 test processes created exactly 4 distinct `velo-auth-<pid>-<n>` directories, one per test that calls `serve_plain()`.

## Audit lines

This change has no server audit log. The retained lines are the raw terminal output of the run.

```text
fmt exit=0
clippy exit=0
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s
[TRACE] 57 test references checked, 0 problem(s)
trace exit=0
runs=200 failed=0
dirs=800
bad_pids=0
01f8c53 (HEAD -> fix/auth-test-unique-dir, origin/fix/auth-test-unique-dir) test(auth): give every test its own temp dir (fix flaky same_error_for_wrong_password_and_unknown_user)
f2a5d8d Merge pull request #28 from HariKrishnaDukka/docs/m1-live-evidence
```

## Deviation from the test sheet

After the sheet was complete, a second `git am --keep-cr` of the same patch was run on the local `main` branch (output: `Applying: test(auth): ...`, no error). The push in check 12 had already been done, and its output shows only the new branch `fix/auth-test-unique-dir` being created on origin, so `origin/main` was not changed. Recovery output: before the reset, `git status` showed local `main` ahead of `origin/main` by 1 commit (`a1ad4c7`, a second copy of `01f8c53`); `origin/fix/auth-test-unique-dir` still pointed at `01f8c53`. After `git reset --hard origin/main`: HEAD `f2a5d8d`, `## main...origin/main`, 0 commits ahead. At the next `git fetch`, `origin/main` had moved from `f2a5d8d` to `8eac61d`, the merge of PR #29 (`fix/auth-test-unique-dir`, containing `01f8c53`); checks 1 to 14 ran against `f2a5d8d`. On the later main `65b13a9` (merge of PR #30, this evidence file) the 12 auth-service tests passed (0.37 s) and traceability reported 57 references, 0 problems.

## Not observed in this run

- The failure before the fix was not reproduced on this machine and no before/after run was made here. The cause (two tests getting the same directory name, then racing on `users.toml.tmp` and `rename`) was established separately, not in this run.
- The exit code of `git am` (check 2) was not echoed. Success is shown by the `Applying:` line, the new HEAD and the stat.
- Rust toolchain version and CPU count were not printed.
- Test directories under `/tmp` are still not deleted by the server tests (4 per run, check 10). This is unchanged behaviour, not a regression.
- `sovd-server` tests, which use their own temp directories in `audit.rs`, were not run or inspected.

## Requirement coverage

| Requirement | What this run shows |
|---|---|
| PRJ-SEC-11 | `store::tests::save_load_roundtrip_and_management` passes with the new directory helper in a single run (check 7) and in 200 parallel runs (check 9). |
| PRJ-SEC-12 | `server::tests::same_error_for_wrong_password_and_unknown_user` and `disabling_a_user_applies_without_restart` pass in a single run (check 7) and in 200 parallel runs (check 9). |
