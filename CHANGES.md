# CHANGES

Planned work, rule changes, and departures from frozen plans, appended
oldest first under timestamped headings. Entries are never edited.

## 2026-10-16 09:15 EDT — Versions and tags

- Owner's instruction (2026-10-16): apply libcontrols' versioning rules to
  libmobile, recorded here rather than in a plan. AGENTS.md gains
  "Versions and tags": the version commit is tagged `v<version>`,
  `VERSIONS.md` lists the public API changes of each version, `0.x`
  bumps the minor version for anything not strictly additive and the patch
  version otherwise, and incompatible dependency bumps propagate up to
  libmorph and libmorph-build.
- `VERSIONS.md` starts with 0.1.0, recorded retroactively, and the tag
  `v0.1.0` is added on `21938c4`, the commit that set it.
- The `cpal` feature (`docs/design/20261016_01_CPAL_FEATURE_PLAN.md`,
  `ee0f098`) is strictly additive, so it is released as 0.1.1.

## 2026-10-22 07:05 EDT — Runtime permission requests

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` (Draft).

Requested by the owner after libmorph's audiocapture example ran on the
Galaxy S23 with `RECORD_AUDIO` granted through `adb`, since libmobile
cannot ask for a permission at runtime. A new core
`bridge.PermissionPlugin` and an `onRequestPermissionsResult` hook on
`MobileActivityPlugin` let Rust call
`android::request_permissions` and `permission_granted`, with the
answer delivered as `MobileEvent::Permissions`. libmorph's audio capture
plan depends on it. To be released as 0.1.2 (strictly additive).

## 2026-10-22 07:18 EDT — Runtime permissions on the S23, and a stale Kotlin build

Plan: `docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md` (In
implementation), step 3.

On the owner's Galaxy S23 Ultra the `wgpu` example showed the system
dialog on a tap and logged `permissions [("android.permission.RECORD_AUDIO",
true)]` when allowed. libmorph's audiocapture example, on this branch, saw
both answers: an interrupted request (the app reinstalled while the dialog
was up) arrived as denied, and a later one as granted. An explicit "Don't
allow" is not yet checked.

Found while building libmorph against this branch: when a build switches
to another copy of libmobile, its `OUT_DIR` and so the staged Kotlin
source root (`libmobile.gradle`'s `java.srcDir`) move. Gradle's
incremental Kotlin compile in the generated project then fails with
"Unresolved reference 'WindowInsetsPlugin'" (and the other bridges) until
`gradle clean`. Not fixed here; a fix would belong in its own plan.
