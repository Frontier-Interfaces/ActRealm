# Post-M14 macOS interface localization

## Scope

The native macOS client supports three interface-language choices:

- System Default;
- Simplified Chinese;
- English.

System Default reads the first macOS preferred language. A language beginning
with `zh` selects Simplified Chinese; all other preferred languages fall back
to English. An explicit selection is saved only in the app's local
`UserDefaults` and applies immediately without restarting the Runtime.

## Product boundary

Localization is a client presentation concern. It does not add a Runtime
language setting, modify persisted Runtime facts, or translate Provider- and
user-authored content. Runtime-generated presentation state now includes
stable message codes and arguments; macOS renders those codes using the
selected language and keeps the Runtime's English text only as a compatibility
fallback. The shared bilingual registry is
`shared/contracts/runtime-messages.json`.

Runtime API failures likewise expose stable error codes and English diagnostic
detail. macOS and Web own the user-facing wording through
`shared/contracts/api-errors.json`; a client must not show the diagnostic
detail as its primary error message. Display-field IDs are the stable contract,
while their labels and descriptions are client-owned.

The main window, Settings, menu-bar popover, HUD, transient notices, generated
counts and durations, task state, and quota presentation use the selected
language.

The packaged app declares `en` and `zh-Hans` in `Info.plist`. Packaging copies
the localization tables used by SwiftUI into the main app resources and also
embeds the Swift Package resource bundle used by dynamic presentation code.

## Automated verification

On 2026-07-25:

```text
apps/macos/Scripts/test.sh
115 tests in 21 suites passed
```

The localization tests cover:

- macOS language resolution and English fallback;
- Runtime message-code localization and argument substitution;
- fixed and formatted string resources;
- bilingual compact duration formatting;
- local language-selection persistence;
- English menu-bar lane presentation.

The SnapshotTool additionally renders 23 English and 23 Simplified Chinese
artifacts for the main workspace, expanded task, interactive question, Agent
setup, Agent Focus settings, HUD, menu-bar popover, and Runtime monitor. The
English run loaded the same main-bundle localization table used by the
packaged app. Fixed client controls rendered in English while source task
titles, prompts, commands, and model names remained verbatim.

The narrow compact-quota artifact fixes the main workspace at its 1160-point
minimum width. English reserves a 360-point quota column instead of the
Chinese 300-point minimum. When the remaining percentage, progress bar, and
reset label still cannot fit one row, the reset label moves below them rather
than expanding or clipping the card.

The ad-hoc package passed deep code-sign verification, launched its arm64 app
and bundled Runtime helper on macOS 27, and returned `overall: pass` from
`doctor --json`. Its `Info.plist` and resources contain both declared
localizations.

The ordinary Rust workspace run passed 199 tests with three explicitly
manual/resource tests ignored. Format, zero-warning Clippy, release build,
JavaScript syntax, the shared language contract, and M0 end-to-end transport
also passed. The explicit 120-second release resource gate recorded 0.003%
average idle CPU and 7,024 KiB maximum Runtime RSS across 118 samples.

## Remaining acceptance

- Use the hosted packaged app to switch Settings between English and
  Simplified Chinese and visually confirm the live update. Deterministic
  bilingual surface snapshots already pass, but the local interactive runner
  was unavailable for this final click-through.
- Commit and push only after separate user authorization.
