#!/usr/bin/env bash
set -euo pipefail

old_prefix='f''low'
role='a''gent'
old_short='F''low'
old_control_room='控''制室'
old_zh_name_one='枢''界'
old_zh_name_two='成''事界'

legacy_pattern="${old_prefix}([ _-]?${role})|act[ _-]?room|${role}[ _-]?workspace|${role} 工作区|${old_control_room}|${old_zh_name_one}|${old_zh_name_two}"

failed=0
while IFS= read -r -d '' file; do
  if grep -I -H -n -i -E "$legacy_pattern" "$file"; then
    failed=1
  fi
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
