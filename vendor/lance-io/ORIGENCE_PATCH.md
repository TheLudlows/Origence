# lance-io 13.0.0: local shutdown error backport

Source: the official [crates.io archive](https://static.crates.io/crates/lance-io/lance-io-13.0.0.crate).
Archive SHA-256: `224730479e781d24b81a0afecfe230b5b21a86a4f767ba7655b974402d0c67c5`.
Published VCS commit: `0923349baa19fd9af776b7d4626dc758dc610c80`.

All 53 archive files are retained, including 44 source files and packaging metadata.
Only `src/object_writer.rs` differs: the shutdown branch from official
[commit e492dcac](https://github.com/lance-format/lance/commit/e492dcac2c3b506918a6a4cfd166a2c5e443cc81)
returns the underlying write error and enters `Poisoned` before persistence.
The local modification is marked in that file. No other changes from upstream
PR #9609 are included. The full Apache-2.0 LICENSE is copied unchanged from the
published VCS commit (Git blob `cd4c38213a96441ccad9b5ce049e397179feabc3`).

The root `[patch.crates-io]` selects this source for the existing 13.0.0 node.
Its root lock entry loses only registry source/checksum; the optional direct
root edge enables focused public-API tests without new packages or features.
The retained upstream Cargo.lock is provenance, not the lock used by Origence.
Do not add this crate as a workspace member or pull its upstream dev-dependency
closure into the application. This is third-party code, not authored project code.

`tests/local_writer_recovery.rs` checks a real read-only-file flush failure,
cleanup, repeat-shutdown/write rejection, and successful exact-byte publication.
The Linux native-write matrix additionally tests actual Lance maintenance with
positive-length synthetic ENOSPC and SIGKILL. Neither proves power-loss safety
or repairs files already corrupted by the unpatched version.

Remove this patch when a compatible official release includes the fix, after
rerunning the same regressions. Keep the existing storage format and write path.
