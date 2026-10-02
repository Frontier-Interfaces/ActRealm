#!/usr/bin/env bash
set -euo pipefail

old_prefix='f''low'
role='a''gent'
old_short='F''low'
old_control_room='控''制室'
old_zh_name_one='枢''界'
old_zh_name_two='成''事界'

legacy_pattern="${old_prefix}([ _-]?${role})|act[ _-]?room|${role}[ _-]?workspace|${role} 工作区|${old_control_room}|${old_zh_name_one}|${old_zh_name_two}"

# These exact references remove an earlier installation's Hooks. Keep the
# compatibility check readable without exempting either file from naming checks.
legacy_binary="/.${old_prefix}-${role}/bin/${old_prefix}-${role}"
is_legacy_hook_migration_reference() {
  local file="$1" line="$2"
  line="${line#"${line%%[![:space:]]*}"}"
  case "$file" in
    crates/installer/src/lib.rs)
      [[ "$line" == "|| normalized.ends_with(\"$legacy_binary\")" ]]
      ;;
    crates/installer/tests/m3_installer.rs)
      [[ "$line" == "\"command\": \"/Users/example$legacy_binary hook --provider claude\"," ||
         "$line" == ".contains(\"$legacy_binary\"));" ]]
      ;;
    *) return 1 ;;
  esac
}

failed=0
while IFS= read -r -d '' file; do
  [[ -f "$file" ]] || continue
  while IFS= read -r match; do
    if is_legacy_hook_migration_reference "$file" "${match#*:}"; then
      continue
    fi
    printf '%s:%s\n' "$file" "$match"
    failed=1
  done < <(grep -I -n -i -E "$legacy_pattern" "$file" || true)
  if grep -I -H -n -w "$old_short" "$file"; then
    failed=1
  fi
done < <(git ls-files -z --cached --others --exclude-standard)

if git ls-files --cached --others --exclude-standard | grep -E -i "$legacy_pattern"; then
  failed=1
fi

if (( failed )); then
  printf '%s\n' "error: legacy product language detected; use ActRealm naming only" >&2
  exit 1
fi

printf '%s\n' "ActRealm language check passed"
"$(dirname "$0")/check-runtime-language.sh"

python3 "$(dirname "$0")/check-native-localization.py"
