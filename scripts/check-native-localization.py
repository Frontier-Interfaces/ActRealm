#!/usr/bin/env python3
"""Check static native UI copy without translating user/Provider content."""
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parent.parent
sources = root / "apps/macos/Sources"
catalog = sources / "ActRealmKit/Resources/en.lproj/Localizable.strings"
entries = dict(re.findall(r'^"((?:[^"\\]|\\.)*)"\s*=\s*"((?:[^"\\]|\\.)*)";', catalog.read_text(), re.M))
han = re.compile(r"[\u3400-\u9fff]")
literal = re.compile(r'\b(?:Text|Button|Label|Toggle|Section|Picker|TextField|Menu|Window|navigationTitle|help|alert|LocalizedStringKey)\(\s*"((?:[^"\\]|\\.)*)"', re.S)
errors = []
entry_line = re.compile(r'^"(?:[^"\\]|\\.)*"\s*=\s*"(?:[^"\\]|\\.)*";\s*$')
for locale in ("en", "zh-Hans"):
    resource = sources / f"ActRealmKit/Resources/{locale}.lproj/Localizable.strings"
    in_comment = False
    for number, line in enumerate(resource.read_text().splitlines(), 1):
        stripped = line.strip()
        if in_comment:
            in_comment = "*/" not in stripped
            continue
        if stripped.startswith("/*"):
            in_comment = "*/" not in stripped
            continue
        if not stripped or stripped.startswith("//"):
            continue
        if not entry_line.fullmatch(stripped):
            errors.append(f"{resource.relative_to(root)}:{number}: malformed strings entry")
for path in sorted(sources.rglob("*.swift")):
    if "SnapshotTool" in path.parts:
        continue
    text = path.read_text()
    for match in literal.finditer(text):
        key = match[1]
        if han.search(key) and "\\(" not in key and key not in entries:
            line = text.count("\n", 0, match.start()) + 1
            errors.append(f"{path.relative_to(root)}:{line}: missing English UI key {key!r}")
for key, value in entries.items():
    if han.search(value):
        errors.append(f"English catalog value contains Chinese: {key!r}")
if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
print(f"Native localization check passed ({len(entries)} English entries)")
