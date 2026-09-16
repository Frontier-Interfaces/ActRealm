#!/bin/zsh
# Packages ActRealm.app: SwiftUI frontend + the Rust actrealm backend
# bundled as Contents/Helpers/actrealm.
#
# Usage: Scripts/package-app.sh [output-dir]
#   ACTREALM_REPO   path to the Rust checkout (default: monorepo root)
#   ACTREALM_SKIP_RUST_BUILD=1 reuses the existing release backend binary
#   ACTREALM_SWIFT_CONFIGURATION=debug packages a debug Swift build
#   ACTREALM_SIGN_IDENTITY signing identity; defaults to the first available
#                          Apple Development identity, then ad-hoc for local QA
#   ACTREALM_REQUIRE_RELEASE_SIGNING=1 rejects ad-hoc output
#   ACTREALM_VERSION / ACTREALM_BUILD_NUMBER override bundle version metadata
#   SDKROOT           macOS SDK (default: installed 26.0 SDK, then xcrun)
set -euo pipefail

SCRIPT_DIR="${0:a:h}"
PACKAGE_DIR="${SCRIPT_DIR:h}" # apps/macos/
REPO_ROOT="${PACKAGE_DIR:h:h}"
RUST_REPO="${ACTREALM_REPO:-$REPO_ROOT}"
GIT_COMMIT="$(git -C "$REPO_ROOT" rev-parse HEAD)"
SOURCE_MODIFIED=false
if [[ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=normal)" ]]; then
  SOURCE_MODIFIED=true
fi
SWIFT_CONFIGURATION="${ACTREALM_SWIFT_CONFIGURATION:-release}"
if [[ "$SWIFT_CONFIGURATION" == "release" ]]; then
  SWIFT_DEBUG_INFO="${ACTREALM_SWIFT_DEBUG_INFO:-none}"
else
  SWIFT_DEBUG_INFO="${ACTREALM_SWIFT_DEBUG_INFO:-dwarf}"
fi
OUT_DIR="${1:-$PACKAGE_DIR/dist}"
APP="$OUT_DIR/ActRealm.app"
INFO_PLIST="$PACKAGE_DIR/Resources/Info.plist"
ENTITLEMENTS="$PACKAGE_DIR/Resources/ActRealm.entitlements"
RELEASE_ENTITLEMENTS="$PACKAGE_DIR/Resources/ActRealm.release.entitlements"
APP_ICON="$PACKAGE_DIR/Resources/ActRealm.icns"
APP_ICON_PNG="$PACKAGE_DIR/Resources/ActRealmIcon.png"
if [[ -n "${ACTREALM_SIGN_IDENTITY+x}" ]]; then
  SIGN_IDENTITY="$ACTREALM_SIGN_IDENTITY"
else
  SIGN_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' \
      | head -1
  )"
  SIGN_IDENTITY="${SIGN_IDENTITY:--}"
fi
REQUIRE_RELEASE_SIGNING="${ACTREALM_REQUIRE_RELEASE_SIGNING:-0}"
BUNDLE_VERSION="${ACTREALM_VERSION:-}"
if [[ -n "$BUNDLE_VERSION" ]]; then
  BUNDLE_VERSION="${BUNDLE_VERSION#v}"
  if [[ ! "$BUNDLE_VERSION" =~ '^[0-9]+([.][0-9]+){0,2}$' ]]; then
    echo "error: ACTREALM_VERSION must be a numeric bundle version (for example 1.2.3)" >&2
    exit 1
  fi
fi
if [[ "$REQUIRE_RELEASE_SIGNING" == "1" && "$SIGN_IDENTITY" == "-" ]]; then
  echo "error: release packaging requires ACTREALM_SIGN_IDENTITY" >&2
  exit 1
fi
if [[ "$REQUIRE_RELEASE_SIGNING" == "1" &&
      "$SIGN_IDENTITY" != "Developer ID Application:"* ]]; then
  echo "error: release packaging requires a Developer ID Application identity" >&2
  exit 1
fi
if [[ "$SIGN_IDENTITY" == "Developer ID Application:"* ]]; then
  ENTITLEMENTS="$RELEASE_ENTITLEMENTS"
fi
if [[ "$REQUIRE_RELEASE_SIGNING" == "1" && -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=normal)" ]]; then
  echo "error: release packaging requires a clean Git worktree so embedded commit metadata is exact" >&2
  exit 1
fi
if [[ -z "${SDKROOT:-}" ]]; then
  export SDKROOT="$("$SCRIPT_DIR/resolve-sdk.sh")"
fi

if [[ ! -f "$RUST_REPO/Cargo.toml" ]]; then
  echo "error: actrealm checkout not found at $RUST_REPO" >&2
  echo "       set ACTREALM_REPO=/path/to/actrealm" >&2
  exit 1
fi
[[ -f "$INFO_PLIST" ]] || { echo "error: missing $INFO_PLIST" >&2; exit 1; }
[[ -f "$ENTITLEMENTS" ]] || { echo "error: missing $ENTITLEMENTS" >&2; exit 1; }
[[ -f "$RELEASE_ENTITLEMENTS" ]] || { echo "error: missing $RELEASE_ENTITLEMENTS" >&2; exit 1; }
[[ -f "$APP_ICON" ]] || { echo "error: missing $APP_ICON" >&2; exit 1; }
[[ -f "$APP_ICON_PNG" ]] || { echo "error: missing $APP_ICON_PNG" >&2; exit 1; }
plutil -lint "$ENTITLEMENTS"
plutil -lint "$RELEASE_ENTITLEMENTS"
if [[ "${ACTREALM_SKIP_RUST_BUILD:-0}" == "1" ]]; then
  echo "==> Reusing existing Rust backend (release)"
  [[ -x "$RUST_REPO/target/release/actrealm" ]] || {
    echo "error: reusable Rust backend not found at $RUST_REPO/target/release/actrealm" >&2
    exit 1
  }
else
  echo "==> Building Rust backend (release)"
  (cd "$RUST_REPO" && ACTREALM_GIT_COMMIT="$GIT_COMMIT" cargo build --release -p actrealm)
fi

echo "==> Building SwiftUI app ($SWIFT_CONFIGURATION)"
(cd "$PACKAGE_DIR" && swift build --disable-sandbox -c "$SWIFT_CONFIGURATION" -debug-info-format "$SWIFT_DEBUG_INFO" --product ActRealmApp)
SWIFT_BIN="$(cd "$PACKAGE_DIR" && swift build --disable-sandbox -c "$SWIFT_CONFIGURATION" -debug-info-format "$SWIFT_DEBUG_INFO" --product ActRealmApp --show-bin-path)/ActRealmApp"
if [[ ! -x "$SWIFT_BIN" ]]; then
  # The swiftbuild backend lays products out differently; fall back to a search.
  SWIFT_BIN="$(find "$PACKAGE_DIR/.build" -type f -name ActRealmApp -perm +111 2>/dev/null | grep -i "$SWIFT_CONFIGURATION" | head -1)"
fi
[[ -x "$SWIFT_BIN" ]] || { echo "error: built ActRealmApp binary not found" >&2; exit 1; }
SWIFT_RESOURCE_DIR="${SWIFT_BIN:h}"
SWIFT_RESOURCE_BUNDLE="$SWIFT_RESOURCE_DIR/ActRealmMac_ActRealmKit.bundle"
[[ -d "$SWIFT_RESOURCE_BUNDLE" ]] || {
  echo "error: built localization resource bundle not found at $SWIFT_RESOURCE_BUNDLE" >&2
  exit 1
}

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"
mkdir -p "$APP/Contents/Resources/ProviderIcons"

cp "$SWIFT_BIN" "$APP/Contents/MacOS/ActRealm"
cp "$RUST_REPO/target/release/actrealm" "$APP/Contents/Helpers/actrealm"
cp "$INFO_PLIST" "$APP/Contents/Info.plist"
cp "$APP_ICON" "$APP/Contents/Resources/ActRealm.icns"
cp "$APP_ICON_PNG" "$APP/Contents/Resources/ActRealmIcon.png"
for resource_bundle in "$SWIFT_RESOURCE_DIR/"*.bundle(N); do
  cp -R "$resource_bundle" "$APP/Contents/Resources/"
done
for localization in "$PACKAGE_DIR/Sources/ActRealmKit/Resources/"*.lproj; do
  cp -R "$localization" "$APP/Contents/Resources/"
done
cp "$REPO_ROOT/web/assets/claude.png" "$APP/Contents/Resources/ProviderIcons/claude.png"
cp "$REPO_ROOT/web/assets/codex.png" "$APP/Contents/Resources/ProviderIcons/codex.png"
BUILD_NUMBER="${ACTREALM_BUILD_NUMBER:-$(git -C "$REPO_ROOT" rev-list --count HEAD)}"
BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
plutil -replace CFBundleVersion -string "$BUILD_NUMBER" "$APP/Contents/Info.plist"
plutil -replace ActRealmGitCommit -string "$GIT_COMMIT" "$APP/Contents/Info.plist"
plutil -replace ActRealmSourceModified -bool "$SOURCE_MODIFIED" "$APP/Contents/Info.plist"
plutil -replace ActRealmBuildDate -string "$BUILD_DATE" "$APP/Contents/Info.plist"
if [[ -n "$BUNDLE_VERSION" ]]; then
  plutil -replace CFBundleShortVersionString -string "$BUNDLE_VERSION" "$APP/Contents/Info.plist"
fi
plutil -lint "$APP/Contents/Info.plist"

SIGN_ARGS=(--force --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
  SIGN_ARGS+=(--options runtime)
fi
if [[ "$SIGN_IDENTITY" == "Developer ID Application:"* ]]; then
  SIGN_ARGS+=(--timestamp)
fi
echo "==> Codesigning ($SIGN_IDENTITY)"
codesign "${SIGN_ARGS[@]}" "$APP/Contents/Helpers/actrealm"
codesign "${SIGN_ARGS[@]}" --entitlements "$ENTITLEMENTS" "$APP/Contents/MacOS/ActRealm"
codesign "${SIGN_ARGS[@]}" --entitlements "$ENTITLEMENTS" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

EXPECTED_ARCHS="${ACTREALM_EXPECTED_ARCHS:-arm64}"
for binary in "$APP/Contents/MacOS/ActRealm" "$APP/Contents/Helpers/actrealm"; do
  ARCHS="$(lipo -archs "$binary")"
  for expected in ${(z)EXPECTED_ARCHS}; do
    if [[ " $ARCHS " != *" $expected "* ]]; then
      echo "error: $binary is missing required architecture $expected (found: $ARCHS)" >&2
      exit 1
    fi
  done
done

echo "==> Done: $APP"
echo "    commit: $GIT_COMMIT"
echo "    build:  $BUILD_NUMBER"
echo "    arch:   $EXPECTED_ARCHS"
echo "    open \"$APP\""
