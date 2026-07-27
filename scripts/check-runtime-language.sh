#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$repo_root" <<'PY'
import json
import pathlib
import re
import sys
from collections import Counter

root = pathlib.Path(sys.argv[1])
contract_path = root / "shared/contracts/runtime-messages.json"
contract = json.loads(contract_path.read_text(encoding="utf-8"))

if contract.get("schemaVersion") != 1:
    raise SystemExit("error: runtime message contract schemaVersion must be 1")
if contract.get("contract") != "actrealm.runtime-message":
    raise SystemExit("error: unexpected runtime message contract identifier")
if contract.get("locales") != ["en", "zh-Hans"]:
    raise SystemExit("error: runtime message locales must be en and zh-Hans")

code_pattern = re.compile(r"^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$")
placeholder_pattern = re.compile(r"\{([a-z][a-zA-Z0-9]*)\}")
han_pattern = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff]")
messages = contract.get("messages")
if not isinstance(messages, list) or not messages:
    raise SystemExit("error: runtime message contract must contain messages")

registered = set()
for index, message in enumerate(messages):
    if not isinstance(message, dict):
        raise SystemExit(f"error: runtime message #{index} is not an object")
    code = message.get("code")
    if not isinstance(code, str) or not code_pattern.fullmatch(code):
        raise SystemExit(f"error: invalid runtime message code: {code!r}")
    if code in registered:
        raise SystemExit(f"error: duplicate runtime message code: {code}")
    registered.add(code)
    parameters = message.get("parameters")
    if not isinstance(parameters, list) or any(
        not isinstance(item, str) or not item for item in parameters
    ):
        raise SystemExit(f"error: invalid parameters for {code}")
    if len(parameters) != len(set(parameters)):
        raise SystemExit(f"error: duplicate parameters for {code}")
    expected = set(parameters)
    for locale in ("en", "zh-Hans"):
        text = message.get(locale)
        if not isinstance(text, str) or not text.strip():
            raise SystemExit(f"error: missing {locale} reference text for {code}")
        actual = set(placeholder_pattern.findall(text))
        if actual != expected:
            raise SystemExit(
                f"error: {code} {locale} placeholders {sorted(actual)} "
                f"do not match parameters {sorted(expected)}"
            )
    if han_pattern.search(message["en"]):
        raise SystemExit(f"error: English reference text contains Han characters: {code}")

emitted = set()
literal_code_pattern = re.compile(
    r'"((?:session|attention|interaction|jump|quota)\.[a-z0-9_.]+)"'
)
for relative in (
    "crates/server/src/status_messages.rs",
    "crates/quota/src/lib.rs",
    "crates/runtime/src/waiter.rs",
):
    text = (root / relative).read_text(encoding="utf-8")
    emitted.update(literal_code_pattern.findall(text))

missing = sorted(emitted - registered)
if missing:
    raise SystemExit(
        "error: Runtime emits unregistered message codes:\n  " + "\n  ".join(missing)
    )

api_contract_path = root / "shared/contracts/api-errors.json"
api_contract = json.loads(api_contract_path.read_text(encoding="utf-8"))
if api_contract.get("schemaVersion") != 1:
    raise SystemExit("error: API error contract schemaVersion must be 1")
if api_contract.get("contract") != "actrealm.api-error":
    raise SystemExit("error: unexpected API error contract identifier")
if api_contract.get("locales") != ["en", "zh-Hans"]:
    raise SystemExit("error: API error locales must be en and zh-Hans")

api_code_pattern = re.compile(r"^[A-Z][A-Z0-9_]+$")
api_errors = api_contract.get("errors")
if not isinstance(api_errors, list) or not api_errors:
    raise SystemExit("error: API error contract must contain errors")
registered_api_errors = set()
api_reference = {}
for index, item in enumerate(api_errors):
    if not isinstance(item, dict):
        raise SystemExit(f"error: API error #{index} is not an object")
    code = item.get("code")
    if not isinstance(code, str) or not api_code_pattern.fullmatch(code):
        raise SystemExit(f"error: invalid API error code: {code!r}")
    if code in registered_api_errors:
        raise SystemExit(f"error: duplicate API error code: {code}")
    registered_api_errors.add(code)
    api_reference[code] = item
    for locale in ("en", "zh-Hans"):
        text = item.get(locale)
        if not isinstance(text, str) or not text.strip():
            raise SystemExit(f"error: missing {locale} API error text for {code}")
    if han_pattern.search(item["en"]):
        raise SystemExit(f"error: English API error text contains Han characters: {code}")

server_text = (root / "crates/server/src/server.rs").read_text(encoding="utf-8")
server_production = server_text.split("\n#[cfg(test)]\nmod tests {", 1)[0]
emitted_api_errors = set(
    re.findall(
        r'api_error(?:_detail)?\s*\(.{0,240}?"([A-Z][A-Z0-9_]+)"',
        server_production,
        flags=re.DOTALL,
    )
)
missing_api_registration = sorted(emitted_api_errors - registered_api_errors)
if missing_api_registration:
    raise SystemExit(
        "error: Runtime emits unregistered API error codes:\n  "
        + "\n  ".join(missing_api_registration)
    )

localized_by_locale = {}
strings_pattern = re.compile(
    r'^"((?:[^"\\]|\\.)*)"\s*=\s*"((?:[^"\\]|\\.)*)";',
    flags=re.MULTILINE,
)
for locale in ("en", "zh-Hans"):
    strings_path = (
        root
        / "apps/macos/Sources/ActRealmKit/Resources"
        / f"{locale}.lproj/Localizable.strings"
    )
    strings_text = strings_path.read_text(encoding="utf-8")
    entries = strings_pattern.findall(strings_text)
    keys = [key for key, _ in entries]
    duplicates = sorted(key for key, count in Counter(keys).items() if count > 1)
    if duplicates:
        raise SystemExit(
            f"error: duplicate macOS {locale} localization keys:\n  "
            + "\n  ".join(duplicates)
        )
    localized = dict(entries)
    localized_by_locale[locale] = localized
    missing_localizations = sorted(registered - localized.keys())
    if missing_localizations:
        raise SystemExit(
            f"error: macOS {locale} is missing Runtime message codes:\n  "
            + "\n  ".join(missing_localizations)
        )
    missing_api_errors = sorted(registered_api_errors - localized.keys())
    if missing_api_errors:
        raise SystemExit(
            f"error: macOS {locale} is missing API error codes:\n  "
            + "\n  ".join(missing_api_errors)
        )
    drifted_api_errors = sorted(
        code
        for code in registered_api_errors
        if localized[code] != api_reference[code][locale]
    )
    if drifted_api_errors:
        raise SystemExit(
            f"error: macOS {locale} API error wording differs from the shared contract:\n  "
            + "\n  ".join(drifted_api_errors)
        )

web_text = (root / "web/app.js").read_text(encoding="utf-8")
try:
    web_registry = web_text.split("const RUNTIME_MESSAGES_ZH = {", 1)[1].split(
        "\n};", 1
    )[0]
except IndexError as error:
    raise SystemExit("error: Web Runtime message registry was not found") from error
web_codes = set(re.findall(r'^\s*"([^"]+)":', web_registry, flags=re.MULTILINE))
missing_web = sorted(registered - web_codes)
if missing_web:
    raise SystemExit(
        "error: Web zh-Hans is missing Runtime message codes:\n  "
        + "\n  ".join(missing_web)
    )

try:
    web_api_registry = web_text.split("const API_ERRORS_ZH = {", 1)[1].split(
        "\n};", 1
    )[0]
except IndexError as error:
    raise SystemExit("error: Web API error registry was not found") from error
web_api_entries = dict(
    re.findall(
        r'^\s*([A-Z][A-Z0-9_]+):\s*"([^"]*)",',
        web_api_registry,
        flags=re.MULTILINE,
    )
)
missing_web_api_errors = sorted(registered_api_errors - web_api_entries.keys())
if missing_web_api_errors:
    raise SystemExit(
        "error: Web zh-Hans is missing API error codes:\n  "
        + "\n  ".join(missing_web_api_errors)
    )
drifted_web_api_errors = sorted(
    code
    for code in registered_api_errors
    if web_api_entries[code] != api_reference[code]["zh-Hans"]
)
if drifted_web_api_errors:
    raise SystemExit(
        "error: Web zh-Hans API error wording differs from the shared contract:\n  "
        + "\n  ".join(drifted_web_api_errors)
    )

swift_sources = "\n".join(
    path.read_text(encoding="utf-8")
    for path in sorted((root / "apps/macos/Sources").rglob("*.swift"))
)
swift_key_pattern = re.compile(
    r'\b(?:localized|localizedFormat|l10n|l10nFormat|'
    r'AppLocalization\.localized|AppLocalization\.formatted)'
    r'\(\s*"((?:[^"\\]|\\.)*)"',
    flags=re.DOTALL,
)
swift_keys = set(swift_key_pattern.findall(swift_sources))
missing_swift_keys = sorted(swift_keys - localized_by_locale["en"].keys())
if missing_swift_keys:
    raise SystemExit(
        "error: macOS English resources are missing explicit localization keys:\n  "
        + "\n  ".join(missing_swift_keys)
    )

client_message_codes = set(
    re.findall(r'RuntimeMessage\(\s*code:\s*"(client\.[a-z0-9_.]+)"', swift_sources)
)
for locale in ("en", "zh-Hans"):
    missing_client_codes = sorted(
        client_message_codes - localized_by_locale[locale].keys()
    )
    if missing_client_codes:
        raise SystemExit(
            f"error: macOS {locale} is missing client message codes:\n  "
            + "\n  ".join(missing_client_codes)
        )

display_fields_match = re.search(
    r"const TASK_CARD_DISPLAY_FIELDS:.*?=\s*&\[(.*?)\];",
    server_production,
    flags=re.DOTALL,
)
if not display_fields_match:
    raise SystemExit("error: Runtime task-card display-field registry was not found")
display_field_ids = set(re.findall(r'"([^"]+)"', display_fields_match.group(1)))
try:
    web_display_registry = web_text.split("const DISPLAY_FIELDS_ZH = {", 1)[1].split(
        "\n};", 1
    )[0]
except IndexError as error:
    raise SystemExit("error: Web display-field registry was not found") from error
web_display_ids = set(
    re.findall(r"^\s*([a-z][a-zA-Z0-9]+):", web_display_registry, flags=re.MULTILINE)
)
missing_web_display_fields = sorted(display_field_ids - web_display_ids)
if missing_web_display_fields:
    raise SystemExit(
        "error: Web zh-Hans is missing display-field copy:\n  "
        + "\n  ".join(missing_web_display_fields)
    )

mac_localization_source = (
    root / "apps/macos/Sources/ActRealmKit/Localization.swift"
).read_text(encoding="utf-8")
try:
    mac_display_registry = mac_localization_source.split(
        "private static let displayFieldKeys:", 1
    )[1].split("\n    ]", 1)[0]
except IndexError as error:
    raise SystemExit("error: macOS display-field registry was not found") from error
mac_display_ids = set(
    re.findall(r'^\s*"([^"]+)":', mac_display_registry, flags=re.MULTILINE)
)
missing_mac_display_fields = sorted(display_field_ids - mac_display_ids)
if missing_mac_display_fields:
    raise SystemExit(
        "error: macOS is missing display-field copy:\n  "
        + "\n  ".join(missing_mac_display_fields)
    )

violations = []
for path in sorted((root / "crates").glob("*/src/**/*.rs")):
    text = path.read_text(encoding="utf-8")
    production = text.split("#[cfg(test)]", 1)[0]
    for line_number, line in enumerate(production.splitlines(), 1):
        if han_pattern.search(line):
            violations.append(
                f"{path.relative_to(root)}:{line_number}:{line.strip()}"
            )

if violations:
    raise SystemExit(
        "error: Han characters found in Runtime production source; "
        "use a registered message code and English fallback:\n"
        + "\n".join(violations)
    )

print(
    f"Runtime language contract passed "
    f"({len(registered)} messages, {len(registered_api_errors)} API errors, "
    f"{len(emitted)} emitted message codes)"
)
PY
