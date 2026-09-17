# Native English UI and publication validation — 2026-09-16

## Language behavior

The default remains Follow system. An explicit Chinese or English choice is
saved locally. Invalid or absent preferences fall back to system language.
Window titles and app-owned menu commands use the selected language. AppKit's
standard menus pick it up on launch via a process-only override; the macOS
language preference is never changed. The settings footer explains when a
restart is needed for system menus.

The native pass fixes missing English picker labels, mixed-language usage
source/coverage strings, known legacy risk messages, menu-bar activity text,
selected-locale dates/chart axes, and long detail values. Common terminology and
status copy is shorter and consistent. User task titles, questions and other
Provider-authored content remain verbatim.

A native localization guard now checks static UI literals, malformed .strings
entries and Chinese text accidentally left in English catalog values. This
runs with the existing language/CI gates.

## Publication scope

The public PR branch starts at agent/v1-full (7d1532699ac67ece43809ee07755e957a24ae43f)
and brings over the current local-only app and Runtime. It includes the prior
usage/accounting, quota recovery, Fable parsing, approval capability and semantic
activity work. No private Cloud commit history is used as a PR parent.

Existing public website and support assets are preserved. Only their CI action
runtime pins are refreshed where needed; no website content change is intended.
The retired embedded Web files are carried from the existing local candidate,
without new Web implementation work in this localization pass.
Workstation-specific documentation paths were removed from the public copy.
No credential-pattern findings were found in publishable source files.

## Additional gates

The clean-checkout pass exposed a two-second Runtime-startup race in a Hook
undo integration test. Its setup now uses an isolated directory, checks early
child exit, gives process launch a separate bounded readiness budget and always
cleans up its child. The actual one-second Hook deadline and 250 ms test undo
window remain unchanged.

Dependency audit identified RUSTSEC-2026-0285 in rustls 0.23.43. The lockfile is
updated to the patched 0.23.45 release. Audit and full tests are rerun on that
lockfile before publication.

Validation results and installed-app observations are appended in the local
artifact copy after completion. Real long-sleep/expired-Claude-credential
recovery and public release qualification remain outside this English UI pass.

## 2026-09-17 follow-up — build 122

Live inspection of build 121 found missing English text in Data settings and
an untranslated quota refresh tooltip. The follow-up fills those entries plus
the Token dashboard pricing-source label, corrects singular quota windows such
as `Fable · 1 week`, and clarifies the local Companion access-token description.
The localization guard now also checks app-owned view wrapper arguments and
metric labels, which previously escaped the static SwiftUI literal scan.

The updated source passed 404 Rust tests (3 explicitly ignored), 211 Swift
tests, formatting, Clippy with warnings denied, the release build, dependency
audit with warnings denied, language/workflow contracts and resource parsing.

The build 122 candidate was installed and inspected locally on September 17.
The live main window showed singular weekly limits and the translated refresh
label. Data settings showed English backup help and UI latency labels with no
clipping in the inspected window. Settings and native menus opened in English,
and the local Runtime reconnected. Existing task titles and questions remained
in their original language. A regression test covers absent/invalid language
preferences falling back to System and explicit English overriding Chinese
system preferences.

The public review is [PR #10](https://github.com/Frontier-Interfaces/ActRealm/pull/10)
against `agent/v1-full`; current CI and the final installed commit are recorded
in that PR's validation section. This local inspection does not qualify a
public release or the deferred long-sleep credential-recovery scenarios.
