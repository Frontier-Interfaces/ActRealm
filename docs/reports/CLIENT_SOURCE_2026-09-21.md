# Client source coverage — 2026-09-21

## Problem

The native task detail labelled missing client metadata as a provider CLI.
The current desktop task had no `environment`, so the UI rendered "Codex CLI"
even though the provider process was hosted by the desktop application.
Provider identity, client origin and managed-control capability are separate.

## Change

- External Hooks inspect at most 24 same-user process ancestors and the real
  host application's bundle metadata on macOS. Executable paths stay transient;
  events retain only bounded client name, bundle identifier, surface and PID.
  A provider-named ancestor is preferred; Node/Python-backed Hooks retain their
  verified direct parent instead of losing their process-liveness evidence.
- Resource binaries bundled with a desktop app do not establish desktop origin.
  A Codex binary launched from Terminal remains a Terminal-origin task. Host
  bundle identity takes precedence over packaged executable/app names.
- A shared classifier covers Codex/ChatGPT, Claude and Kimi desktop identities;
  Grok app names; VS Code/Insiders, Cursor, Windsurf, Zed, Xcode and JetBrains
  hosts; Terminal, iTerm2, Warp, Ghostty, WezTerm, Alacritty, kitty and multiplexers.
  Known environment hints cover SSH and terminal fallbacks. Other real app
  bundles can retain their own bounded name and bundle identifier.
- Provider-generated/internal events do not adopt Runtime's own client context.
  Explicit ACP launches preserve their host source or identify the ACP entry
  point when no frontend is known. Observing a shared/web service does not prove
  which browser or remote frontend the user is using.
- Client host and provider process identity remain independent, including
  one desktop agent launching a different provider and Node/Python backends.
  They are not marked disconnected solely because the host app has another name.
- Client identity is updated coherently. A changed source clears old bundle,
  terminal-locator and PID information instead of combining fields from two
  clients. Internal/unspecified events retain the last observed source.
- Native and web task details use "Client source" and "Source unidentified".
  Neither provider name, project name nor connection state substitutes for a
  client identity. Missing historical origins are not inferred from today's
  foreground app. Existing display-setting keys remain compatible.
- Safe observed app bundle identifiers can be used for app-level jump fallback;
  remote-only sources do not claim a locally available original window.

## Verification scope

Classifier fixtures cover the named entry points, unknown/malformed metadata,
bundled CLI versus real host ancestry and actual bundle identity precedence.
Storage tests cover internal-event preservation and switching between desktop,
terminal and SSH sources without stale locators. Native/web regressions cover
unknown sources across every provider and independence from managed connection.

Real-device acceptance verifies the installed desktop source. Entries not
installed or active on this machine have classification/protocol coverage, not
separate live-client acceptance. Source labels do not grant approvals or imply
that a connector owns the original client's running turn.

No Display package changes or provider model probes are included.

## Installed verification — build 135

The active desktop task changed from a missing Runtime `environment` (rendered
as "Codex CLI" by the old UI) to `environment: "Codex app"`. The installed
native detail displays "客户端来源：Codex 桌面端". The task remains running and
its separate managed connection state remains `owned_elsewhere`.

The installed app and shared helper pass strict signatures and matching hashes.
Display build 86 is unchanged. Native and web unknown-source regressions pass;
nonblocking Hook p95 remains below the existing 50 ms gate. Broader fixture
coverage does not assert that every named third-party client was run locally.

The candidate is included in PR #10. Build 135 was packaged before submission
and retains the source-modified marker and its base-commit provenance.

Final validation: 450 Rust tests passed (3 intentionally ignored), 223 Swift
tests and 9 web tests passed. Clippy, release, localization, format, CI pin,
JavaScript syntax, Info.plist, documentation links and diff checks passed.
