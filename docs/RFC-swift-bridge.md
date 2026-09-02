# RFC: Cider Bridge — Apple frameworks through a signed Swift helper

Status: draft, 2026-09-01. Tracking: beads `cider-bridge-*`.

## Why

cider reads Apple data through SQLite files and drives apps through AppleScript/JXA. That covers most apps but not all, and not always well:

- **HomeKit** has no file, no script interface, and no cloud API. The only door is `HomeKit.framework`, which only loads in a Mac Catalyst process signed with the `com.apple.developer.homekit` entitlement.
- **Weather** on modern macOS keeps an encrypted cache; the sanctioned source is `WeatherKit`, which needs a signed app with the WeatherKit capability.
- **Calendar, Reminders, Contacts** work today, but `EventKit`/`Contacts` give change notifications, `lastModifiedDate`, proper recurrence, and writes without an AppleScript round trip.

A Swift helper is the reliable, fast way in. It stays a helper: the Rust CLI remains the product and the library Alchemy links; the bridge is an optional accelerator that cider detects and uses when present.

## Constraints (Apple's, not ours)

- HomeKit: Catalyst only. Development-signed, per-device provisioning (each Mac registered in the team). Not Developer ID, not notarizable, not Homebrew-distributable. Every user builds it locally with their own team.
- WeatherKit: App ID with the WeatherKit capability, paid team, attribution required.
- EventKit/Contacts: native CLI is fine, TCC consent only, no paid team.

So there are two artifacts: a **Catalyst app** for HomeKit (and WeatherKit, since it needs the same signing), and a **native CLI** for everything else. cider works fully without either; the app adds HomeKit, the CLI upgrades what already works.

## Shape

```
cider (Rust) ──launch on demand──▶ Cider Bridge.app (Catalyst, HomeKit + WeatherKit)
      │       ◀── newline JSON over Unix socket ──┘   exits after 10 min idle
      └──subprocess, JSON stdio──▶ cider-bridge (native Swift CLI: EventKit, Contacts)
```

### Bridge app protocol

Socket: `$HOME/Library/Application Support/cider/bridge.sock`, mode 0600. One JSON object per line each way.

Request: `{"id": 1, "cmd": "home.scenes", "args": {"home": "2183 26th Ave"}}`
Reply: `{"id": 1, "ok": true, "data": [...]}` or `{"id": 1, "ok": false, "error": {"code": "not_found", "message": "..."}}`

Error codes: `not_found`, `invalid_args`, `homekit_denied`, `homekit_unavailable`, `timeout`, `internal`.

Homes, rooms, accessories, scenes, and triggers are addressed by name or UUID, case-insensitive on names; ambiguity is `invalid_args`.

| cmd | args | data |
|---|---|---|
| `ping` | | `{version, homekit_authorized: bool, homes: n}` |
| `home.homes` | | `[{id, name, primary}]` |
| `home.rooms` | `home?` | `[{id, name, home}]` |
| `home.accessories` | `home?, room?` | `[{id, name, room, manufacturer, model, reachable, services: [{id, name, type, characteristics: [{id, type, name, value, unit?, writable, readable}]}]}]` (live values) |
| `home.scenes` | `home?` | `[{id, name, home, kind, actions: n}]` |
| `home.run_scene` | `home?, scene` | `{ran: true}` |
| `home.set` | `home?, accessory, characteristic, value, service?` | `{accessory, characteristic, value}` |
| `home.triggers` | `home?` | `[{id, name, home, kind: "timer"|"event", enabled, fire_date?, recurrence?, scenes: [..], last_fire?}]` |
| `home.trigger_create_timer` | `home?, name, fire_at (RFC 3339 local), recurrence? ("daily"|"weekly"|{minutes:n}), scenes: [..]` | the trigger row |
| `home.trigger_set_enabled` | `home?, trigger, enabled` | the trigger row |
| `home.trigger_delete` | `home?, trigger` | `{deleted: true}` |
| `weather.current` | `lat, lon` | WeatherKit current + attribution |
| `weather.forecast` | `lat, lon, days?` | daily forecast + attribution |
| `quit` | | `{bye: true}` |

`home.trigger_create_timer` is the point of the whole thing: Home automations that fire on the home hub with the Mac asleep. `HMTimerTrigger` + `HMHome.addTrigger` + `addActionSet` + `enable`.

### Launch on demand

cider looks for `Cider Bridge.app` in `~/Applications`, `/Applications`, then `$CIDER_BRIDGE_APP`. If the socket does not answer `ping` within 200 ms it runs `open -gj -a <path>` and polls the socket for up to 10 s. The app has no Dock icon (`LSUIElement`), one menu-bar item with the authorization state, and quits after 10 minutes without a request. No daemon: nothing runs unless cider asked, and nothing keeps running.

### Native CLI (`cider-bridge`)

Plain SwiftPM executable, ad-hoc signed with an embedded Info.plist carrying the usage strings, so TCC prompts name it. Invoked by cider as `cider-bridge <cmd> [json-args]`, JSON on stdout, same envelope as the socket. Commands: `calendar.list {since?, from?, to?, calendar?}` with `modified_at`, `calendar.create|update|delete`, `reminders.list {since?, list?, include_completed?}`, `reminders.create|complete|update|delete`, `contacts.list {since?}`, and `watch {sources}` streaming `EKEventStoreChanged` / `CNContactStoreDidChange` as lines. cider prefers the CLI for writes when installed and keeps the SQLite fast path for bulk reads.

## Rust side

`src/sources/bridge.rs`: `Bridge::connect() -> Result<Bridge>` (find, launch, ping), `call(cmd, args) -> serde_json::Value`, typed errors mapped from the envelope, `is_installed()`. `cider home` gains `--live` (fail if no bridge) and otherwise uses the bridge when it answers, the cache when it does not, saying which in `--envelope` metadata. New subcommands: `home state [--room]` (accessories with live values), `home run --scene`, `home set --accessory --characteristic --value`, `home triggers`, `home triggers create-timer --name --at --repeat --scene`, `home triggers enable|disable|delete`. `cider doctor` gains `bridge_app` and `bridge_cli` checks. `cider bridge build` wraps XcodeGen + xcodebuild with the user's team (`CIDER_TEAM_ID` or `.env.local`), `cider bridge install` copies to `~/Applications`.

## Layout

```
bridge/
  project.yml                 XcodeGen: app target (Catalyst) + cider-bridge CLI target
  Package.swift               SwiftPM for the CLI + shared core (swift build works without Xcode signing)
  Sources/BridgeCore/         envelope, socket framing, command router, HomeKit/EventKit/Contacts/WeatherKit handlers
  Sources/CiderBridgeApp/     Catalyst app: AppDelegate, menu bar, socket server, idle timer
  Sources/cider-bridge/       native CLI main
  Resources/                  entitlements, Info.plist, icon
  Tests/                      envelope + router tests; HomeKit handlers behind a protocol with a fake
```

Bundle ID `dev.thrasher.cider.bridge`. Team from `.env.local` (`CIDER_TEAM_ID=...`), never committed.

## What the user must do once

Enable HomeKit (and WeatherKit, if wanted) on the App ID `dev.thrasher.cider.bridge` in the developer portal. Xcode's automatic signing registers the device and creates the profile but cannot add the HomeKit capability to a Catalyst App ID; that step is manual. After that `cider bridge build` is hands-off.

## Staging

1. **Protocol + skeleton**: BridgeCore envelope/router/socket with tests; app builds unsigned (`CODE_SIGNING_ALLOWED=NO`); Rust `bridge.rs` client with a fake socket test. Gate: `ping` round-trips through a real socket from `cargo test` with a stub server.
2. **HomeKit read**: homes, rooms, accessories with live values, scenes, triggers. Gate: `cider home state` shows the office lights' real on/off state and matches the Home app.
3. **HomeKit write**: run scene, set characteristic, timer triggers. Gate: `cider home triggers create-timer` produces an automation visible in the Home app that fires with the Mac asleep.
4. **Native CLI**: EventKit/Contacts reads with `modified_at`, writes, `watch`. Gate: `cider reminders complete` goes through the CLI when present; `cider watch` emits an EventKit change within a second.
5. **WeatherKit**: current + forecast with attribution. Gate: `cider weather` is back, honestly sourced.

## Not doing

Distributing the signed app. A Homebrew formula for cider stays Rust-only; the bridge is `cider bridge build` on the user's own Mac. No background daemon: the app is launched by cider and exits on idle. No Notes, Messages, or Find My: no framework exists.
