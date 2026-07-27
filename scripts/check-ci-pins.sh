#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WORKFLOWS="$ROOT/.github/workflows"
EXPECTED_RUST_TOOLCHAIN='1.97'
EXPECTED_XCODE_VERSION='26.6'
EXPECTED_CARGO_AUDIT_VERSION='0.22.2'
errors=0

fail() {
  printf 'CI pin check: %s\n' "$*" >&2
  errors=$((errors + 1))
}

for workflow in "$WORKFLOWS"/*.yml "$WORKFLOWS"/*.yaml; do
  [ -f "$workflow" ] || continue
  relative=${workflow#"$ROOT/"}

  while IFS= read -r use; do
    [ -n "$use" ] || continue
    case "$use" in
      ./*) ;;
      *@????????????????????????????????????????)
        revision=${use##*@}
        case "$revision" in
          *[!0-9a-f]*) fail "$relative uses a non-hex action revision: $use" ;;
        esac
        ;;
      *) fail "$relative uses a mutable action reference: $use" ;;
    esac
  done <<EOF
$(sed -n 's/^[[:space:]-]*uses:[[:space:]]*\([^[:space:]#]*\).*$/\1/p' "$workflow")
EOF

  if grep -Eq '(^|[^-[:alnum:]_])(latest-stable|stable)([^-[:alnum:]_]|$)' "$workflow"; then
    fail "$relative contains a mutable stable/latest-stable selector"
  fi

  if grep -Eq '(^|[[:space:]])(curl|wget)[[:space:]].*\|[[:space:]]*(sh|bash)' "$workflow"; then
    fail "$relative contains an unpinned remote installer"
  fi

  while IFS= read -r install; do
    [ -n "$install" ] || continue
    case "$install" in
      *"cargo install cargo-audit"*)
        case "$install" in
          *"--version \"$EXPECTED_CARGO_AUDIT_VERSION\""*"--locked"*|\
          *"--version '$EXPECTED_CARGO_AUDIT_VERSION'"*"--locked"*|\
          *"--version $EXPECTED_CARGO_AUDIT_VERSION"*"--locked"*) ;;
          *) fail "$relative must install cargo-audit $EXPECTED_CARGO_AUDIT_VERSION with --locked" ;;
        esac
        ;;
      *) fail "$relative contains an unapproved cargo install: $install" ;;
    esac
  done <<EOF
$(grep -E '(^|[[:space:]])cargo[[:space:]]+install[[:space:]]' "$workflow" || true)
EOF
done

if grep -R -Eq 'xcode-version:[[:space:]]*(latest-stable|latest|stable)' "$WORKFLOWS"; then
  fail "Xcode must use the exact supported version $EXPECTED_XCODE_VERSION"
fi

xcode_workflows=$(grep -Rl 'maxim-lobanov/setup-xcode@' "$WORKFLOWS" || true)
for workflow in $xcode_workflows; do
  grep -Eq "xcode-version:[[:space:]]*[\"']?$EXPECTED_XCODE_VERSION[\"']?([[:space:]#]|$)" "$workflow" ||
    fail "${workflow#"$ROOT/"} does not pin Xcode $EXPECTED_XCODE_VERSION"
done

rust_workflows=$(grep -Rl 'dtolnay/rust-toolchain@' "$WORKFLOWS" || true)
for workflow in $rust_workflows; do
  grep -Eq "toolchain:[[:space:]]*[\"']?$EXPECTED_RUST_TOOLCHAIN[\"']?([[:space:]#]|$)" "$workflow" ||
    fail "${workflow#"$ROOT/"} does not pin Rust $EXPECTED_RUST_TOOLCHAIN"
done

if grep -R -Eq '(x86_64|macos-(13|14|15)|ACTREALM_EXPECTED_ARCHS:[[:space:]]*[^#]*x86)' "$WORKFLOWS"; then
  fail "workflow expands beyond the supported Apple Silicon target"
fi

if [ "$errors" -ne 0 ]; then
  printf 'CI pin check failed with %s issue(s).\n' "$errors" >&2
  exit 1
fi

printf 'CI pin check passed: immutable actions, Rust %s, Xcode %s, cargo-audit %s, Apple Silicon only.\n' \
  "$EXPECTED_RUST_TOOLCHAIN" "$EXPECTED_XCODE_VERSION" "$EXPECTED_CARGO_AUDIT_VERSION"
