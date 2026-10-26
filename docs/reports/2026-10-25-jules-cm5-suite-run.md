# CodeMirror 5 Suite Run Report
**Date:** 2026-10-25
**Commit:** ffe080b193f031605095f87ab4cafe443e7b50c5
**Toolchain:** rustc 1.95.0 (59807616e 2026-04-14)

## Reference Pin Verification
```
reference
  root               /app
  X_ITE              34cdf9f97145b3cd2b8e250f8c88dfdd083c0517 (version 16.5.0, 857 files)
  CodeMirror         4ca3d0551cf9125114e0802877480599472dc186 (version 5.65.21, 440 files)
  CodeMirror live    not present (not checked)
  result             reference ACCEPTED
identity
  result             not checked (no --expect); this is not a candidate approval
```

## Test Results per Crate
| Crate | Passed | Failed | Ignored | Filtered |
| --- | --- | --- | --- | --- |
| cm-core | 73 | 0 | 0 | 0 |
| cm-modes | 190 | 4 | 0 | 0 |
| cm-view | 65 | 0 | 0 | 0 |

## Failures
The following tests failed in `cm-modes` (mostly due to missing upstream files not in the repo context):
- `css_tokens_match_cm5_runmode`
- `htmlmixed_tokens_match_cm5_runmode`
- `javascript_tokens_match_cm5_runmode`
- `xml_tokens_match_cm5_runmode`

### Failure output details
```
failures:

---- css_tokens_match_cm5_runmode stdout ----

thread '<unnamed>' (33685) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/addon/dialog/dialog.css: No such file or directory (os error 2)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '<unnamed>' (33686) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/addon/display/fullscreen.css: No such file or directory (os error 2)

thread '<unnamed>' (33687) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/addon/fold/foldgutter.css: No such file or directory (os error 2)

thread '<unnamed>' (33689) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/addon/lint/lint.css: No such file or directory (os error 2)

thread '<unnamed>' (33688) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/addon/hint/show-hint.css: No such file or directory (os error 2)

thread '<unnamed>' (33690) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
--
failures:
    css_tokens_match_cm5_runmode

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s

error: test failed, to rerun pass `-p cm-modes --test css_oracle`
     Running tests/htmlmixed_oracle.rs (target/debug/deps/htmlmixed_oracle-de7fccc654c2a365)

running 1 test
test htmlmixed_tokens_match_cm5_runmode ... FAILED

failures:

---- htmlmixed_tokens_match_cm5_runmode stdout ----

thread '<unnamed>' (33699) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/activeline.html: No such file or directory (os error 2)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '<unnamed>' (33701) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/bidi.html: No such file or directory (os error 2)

thread '<unnamed>' (33702) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/btree.html: No such file or directory (os error 2)

thread '<unnamed>' (33704) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/changemode.html: No such file or directory (os error 2)

thread '<unnamed>' (33703) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/buffers.html: No such file or directory (os error 2)

thread '<unnamed>' (33705) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
--
failures:
    htmlmixed_tokens_match_cm5_runmode

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s

error: test failed, to rerun pass `-p cm-modes --test htmlmixed_oracle`
     Running tests/javascript_oracle.rs (target/debug/deps/javascript_oracle-09a453bdefc63614)

running 1 test
test javascript_tokens_match_cm5_runmode ... FAILED

failures:

---- javascript_tokens_match_cm5_runmode stdout ----

thread '<unnamed>' (33719) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/src/display/Display.js: No such file or directory (os error 2)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '<unnamed>' (33713) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/src/display/operations.js: No such file or directory (os error 2)

thread 'javascript_tokens_match_cm5_runmode' (33712) panicked at crates/cm-modes/tests/common/mod.rs:313:41:
called `Result::unwrap()` on an `Err` value: Any { .. }

thread '<unnamed>' (33722) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/src/display/highlight_worker.js: No such file or directory (os error 2)

thread '<unnamed>' (33718) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/src/codemirror.js: No such file or directory (os error 2)

thread '<unnamed>' (33721) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
--
failures:
    javascript_tokens_match_cm5_runmode

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.23s

error: test failed, to rerun pass `-p cm-modes --test javascript_oracle`
     Running tests/toml_oracle.rs (target/debug/deps/toml_oracle-e39662ae8b33e172)

running 1 test
test toml_tokens_match_cm5_runmode ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

     Running tests/upstream_mode_tests.rs (target/debug/deps/upstream_mode_tests-5a1efb8087dece32)

running 174 tests
test css_atmediaunknownfeaturevaluekeyword ... ok
test css_atmediaunknownproperty ... ok
test css_atmediafeaturevaluekeyword ... ok
test css_atmediamaxwidthnested ... ok
test css_atmediaunknowntype ... ok
--
failures:

---- xml_tokens_match_cm5_runmode stdout ----

thread '<unnamed>' (33924) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/btree.html: No such file or directory (os error 2)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread '<unnamed>' (33925) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/buffers.html: No such file or directory (os error 2)

thread '<unnamed>' (33923) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/bidi.html: No such file or directory (os error 2)

thread '<unnamed>' (33921) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/activeline.html: No such file or directory (os error 2)

thread '<unnamed>' (33922) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
/app/crates/cm-modes/../../upstream/codemirror5/demo/anywordhint.html: No such file or directory (os error 2)

thread '<unnamed>' (33928) panicked at crates/cm-modes/tests/common/mod.rs:179:60:
--
failures:
    xml_tokens_match_cm5_runmode

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

error: test failed, to rerun pass `-p cm-modes --test xml_oracle`
   Doc-tests cm_modes

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 4 targets failed:
    `-p cm-modes --test css_oracle`
    `-p cm-modes --test htmlmixed_oracle`
    `-p cm-modes --test javascript_oracle`
    `-p cm-modes --test xml_oracle`
```
