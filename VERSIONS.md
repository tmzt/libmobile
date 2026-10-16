# VERSIONS

Each change of the crate version and the public API changes it carries
(AGENTS.md, "Versions and tags"). Internal changes are not listed, and there
is no entry without a version change. Entries are appended, oldest first,
and never edited.

## 0.1.0 — 2026-10-13 (`21938c4`, tag `v0.1.0`)

Initial release, and every change up to `3da1e1f`. Recorded retroactively on
2026-10-16; the tag was added on 2026-10-16.

## 0.1.1 — 2026-10-16 (tag `v0.1.1`)

cpal on Android (`docs/design/20261016_01_CPAL_FEATURE_PLAN.md`). The commit
is the one tagged `v0.1.1`: the entry is written in that commit, which
cannot name its own hash.

- Added the `cpal` feature: cpal 0.18 as an optional Android dependency,
  re-exported as `libmobile::android::cpal`. It turns on `ndk`'s `audio`
  and `api-level-26` features.
