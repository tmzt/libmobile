---
title: App icons from PNGs, made into Android resources
date: 2026-10-24
status: In implementation
covers:
  - "`gradle::Icon` and `Project::icon`: a master PNG (or PNGs at known
    sizes) declared in the app's build script"
  - "The generated resources: legacy and round `mipmap-*dpi/ic_launcher`,
    the adaptive icon (`mipmap-anydpi-v26`), an optional monochrome
    layer, and `android:icon` / `android:roundIcon` in the manifest"
  - "A host-only feature `icons` for the image dependency"
  - Vector sources and desktop icons, left to later plans
sources:
  - "Owner (2026-10-24, relayed by the handlr session): \"add icon
    support with png-based at various sizes (vector later) and convert
    them to Android resources automatically, should be compatible with
    desktop use also (later)\". For the owner's approval in the libmorph
    session."
  - "handlr's request: `.icon(Icon::png(\"icon/icon-1024.png\"))` through
    libmorph-build, or a set of PNGs; legacy `mipmap-*dpi/ic_launcher.png`
    (48–192 px) and round; the adaptive icon with a foreground PNG at
    108–432 px and a background colour or PNG; optionally monochrome; the
    manifest's `android:icon` and `android:roundIcon`. Until then handlr
    copies its own PNGs into the generated project from its build.rs."
  - "libmobile today: `gradle::Project` renders the Gradle project and its
    manifest (`codegen::GradleProject::render`), with no icon and no
    `res/` of its own; `Project::extra_script` applies an app's Gradle
    script last."
  - "Android: launcher icons are 48 dp (mdpi 48, hdpi 72, xhdpi 96, xxhdpi
    144, xxxhdpi 192 px). Adaptive icons (API 26) are two 108 dp layers
    (108 to 432 px), of which the centre 72 dp shows and a 66 dp circle is
    always visible; the `<monochrome>` layer (API 33) is used for themed
    icons. Generated projects have minSdk 29, so every device takes the
    adaptive icon; the legacy PNGs serve launchers that ask for them."
related:
  - libmorph docs/design/20261024_03_APP_ICON_PLAN.md (libmorph-build
    passes it through)
  - docs/design/20261022_05_URL_SCHEMES_PLAN.md (the last manifest
    addition)
---

# App icons from PNGs, made into Android resources

## 1. Declaring the icon

Feature `icons`, used only by build scripts (host). It adds the `image`
crate (0.25, `default-features = false`, `png` only) to libmobile, and
nothing to the Android build of an app.

```rust
// libmobile::gradle, feature `icons`
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Icon { /* source, background, monochrome, layout */ }

impl Icon {
    /// One square master PNG, at least 432 px (1024 recommended), with
    /// transparency around the artwork. Every size is made from it.
    pub fn png(path: impl AsRef<Path>) -> Self;
    /// PNGs at known sizes, each square; a size not given is made from
    /// the largest one given.
    pub fn pngs(sizes: &[(u32, &Path)]) -> Self;
    /// The adaptive icon's background: a colour (the default, white) ...
    pub fn background_color(self, rgb: [u8; 3]) -> Self;
    /// ... or a square PNG, used at full bleed.
    pub fn background_png(self, path: impl AsRef<Path>) -> Self;
    /// A single-colour PNG for themed icons (Android 13+), laid out like
    /// the foreground.
    pub fn monochrome_png(self, path: impl AsRef<Path>) -> Self;
    /// The master already fills the 108 dp adaptive canvas (its artwork
    /// inside the centre 66 dp); use it as the foreground unchanged.
    /// Without this, the master is scaled into the centre 66 dp.
    pub fn full_bleed(self) -> Self;
}

impl Project {
    #[cfg(feature = "icons")]
    pub fn icon(self, icon: Icon) -> Self;
}
```

- Paths are relative to the crate root, as `extra_script`'s are, and each
  gets a `cargo:rerun-if-changed`.
- A source that is not a PNG, not square, or cannot be read fails
  `generate()` with an error naming the file. A master smaller than 432 px
  is scaled up, with a `cargo:warning`.
- Scaling is Lanczos3, in linear light is not attempted (the PNGs are
  treated as sRGB, as Android does).

## 2. What is generated

Under the generated project's `app/src/main/res/`, written only when
changed (as the other files are):

| File | Sizes | Content |
|---|---|---|
| `mipmap-{m,h,xh,xxh,xxxh}dpi/ic_launcher.png` | 48, 72, 96, 144, 192 | The foreground over the background, as the adaptive icon looks unmasked, with rounded corners of 1/6 of the size, as legacy launchers expect |
| `mipmap-*dpi/ic_launcher_round.png` | the same | The same, masked to a circle |
| `mipmap-*dpi/ic_launcher_foreground.png` | 108, 162, 216, 324, 432 | The adaptive foreground |
| `mipmap-*dpi/ic_launcher_background.png` | the same | Only for `background_png` |
| `mipmap-*dpi/ic_launcher_monochrome.png` | the same | Only for `monochrome_png` |
| `values/ic_launcher_background.xml` | | `<color name="ic_launcher_background">`, for `background_color` |
| `mipmap-anydpi-v26/ic_launcher.xml`, `ic_launcher_round.xml` | | `<adaptive-icon>` with `<background>`, `<foreground>` and, if given, `<monochrome>` |

The manifest's `<application>` gains `android:icon="@mipmap/ic_launcher"`
and `android:roundIcon="@mipmap/ic_launcher_round"`. Without `icon()`,
nothing changes: no `res/`, no attributes, the system's default icon.

The image work is in pure functions of the decoded master
(`codegen::icon_images`) and the XML in `codegen::adaptive_icon_xml`,
tested on the host: sizes, the 66 dp placement, the circle mask's
corners being transparent, and the XML.

## 3. Until this ships

The supported hook today is `Project::extra_script`: an app's script can
add a resource directory with
`android { sourceSets["main"].res.srcDir("<dir>") }`. It cannot set
`android:icon` on the generated manifest, so the launcher keeps the
default icon unless the resources override `@mipmap/ic_launcher`, which
the generated manifest does not reference yet.

## 4. Steps

1. **The feature and the generator.** `Icon`, `Project::icon`, the
   resources and manifest attributes of section 2, and the host tests.
   **Done when:** `cargo test --workspace --features icons` and clippy
   pass, and the `wgpu` example, declaring a test icon (a 1024 px PNG in
   the example, with a background colour and a monochrome layer), builds
   with `gradle assembleDebug` and `aapt dump badging` lists
   `application-icon-*` entries.
2. **On the S23.**
   **Done when:** on the owner's S23 Ultra the launcher shows the icon,
   masked by One UI's icon shape; with themed icons on, the monochrome
   layer is used; and a launcher shortcut or the app info page shows the
   round or legacy icon without a white square around it.
3. **Release** in the next libmobile release, on the owner's word.
   Additive: a new feature, type and builder method.

## 5. Later (separate plans)

- **Vector sources:** an SVG made into a VectorDrawable, or rendered into
  the same PNG layers.
- **Desktop:** the same `Icon` for the macOS app bundle's `.icns` (with
  the bundle plan URL schemes left for later) and the window icon, built
  in libdevice and libmorph-build.

## 6. Open questions for the owner

1. **Placement default.** Scale the master into the centre 66 dp (safe for
   any artwork), with `full_bleed()` for artwork made for adaptive icons.
   Recommended: yes.
2. **Legacy PNGs.** Generated projects have minSdk 29, so every device
   uses the adaptive icon; the legacy PNGs are a fallback for launchers
   that ask for them. Recommended: generate them anyway; they are small.
3. **The `image` crate** behind `icons`, host-only. Recommended: yes; the
   alternative, `png` plus a hand-written resampler, is more code to own.

## Amendment — 2026-10-24 13:55 EDT: approved; sources are PNGs, outputs are what the platform takes

The owner approved this plan with libmorph's
`docs/design/20261024_03_APP_ICON_PLAN.md` on 2026-10-24: "approve. the
format can be whatever the platform needs, png was meant to mean the
source files would be pngs if bitmaps".

- **Sources and outputs.** "PNG-based" describes the sources: bitmap
  sources are PNG files. What is generated is whatever each platform
  takes. For Android that is the table of section 2, written as PNG
  mipmaps (Android also accepts WebP; the generator may switch to it
  without an amendment if size matters). Later platforms take their own
  formats (`.icns` on macOS).
- **Section 6**, with the recommendations: the master is scaled into the
  centre 66 dp by default, with `full_bleed()` for adaptive artwork; the
  legacy PNGs are generated; the `image` crate is used behind `icons`.
- **Wording.** Section 1's last bullet reads: scaling is Lanczos3 on the
  sRGB values, as Android's own tools do, without converting to linear
  light. Section 5's desktop bullet reads: the same `Icon` for the macOS
  app bundle's `.icns` and for the window icon, built in libdevice and
  libmorph-build; the window icon would be embedded from the build
  script's output (an `app_icon!()` macro over `include_bytes!` of a
  file in the app's `OUT_DIR`), so the app declares the icon once.

The status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-24 14:07 EDT: implementation has started; the document is now immutable

Step 1 (the `icons` feature, `gradle::Icon`, `Project::icon`, the generated
resources) is implemented on `main` from this commit, on the owner's word.

## Amendment — 2026-10-24 14:30 EDT: released in 0.2.5; step 2 still open

Step 1 is `1ed44b3` and `2acdfc6` (the `wgpu` example), with the
departure in `03feb9d` (`icon_images` in a private `src/icon.rs`, since
`build.rs` compiles `codegen.rs` without `image`); `aapt dump badging`
lists the adaptive icon at every density. Step 3 is libmobile 0.2.5
(`411ba59`, tag `v0.2.5`), released on the owner's word ("add this to 0.4.2
if that's the next build, don't hold up the current work") before step 2
is complete: the S23 resolves the launcher icon to `mipmap/ic_launcher`,
but the launcher, themed icons and app info need the owner. The status
stays `In implementation` until they pass.
