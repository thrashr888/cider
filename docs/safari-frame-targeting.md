# Safari frame targeting: regular tabs versus native automation

## Result for existing signed-in tabs

Cider's `DocumentTarget`, `eval_document`, and binary transfer APIs use Safari
Apple Events. They target the **top-level document**, not a cross-origin iframe.
An iframe's `src`, the parent's origin, or a frame's ordinal is not an executable
document handle. Do not substitute a frame UUID into a top-level handle.

The installed Safari 27.0.1 scripting definition
(`/Applications/Safari.app/Contents/Resources/Safari.sdef`) declares:

```xml
<command name="do JavaScript" code="sfridojs"
         description="Applies a string of JavaScript code to a document.">
  <direct-parameter type="text"/>
  <parameter name="in" code="dcnm" optional="yes">
    <type type="document"/>
    <type type="tab"/>
  </parameter>
  <result type="any"/>
</command>
```

It exposes no frame selection parameter or frame object. Same-origin page code
can use DOM frame APIs subject to normal browser security; Apple Events cannot
select a cross-origin frame when `contentDocument` is null. Cider does not
disable origin checks, change Safari security preferences, use token extraction,
or inject a `postMessage` bridge into an uncooperative application.

For the reported embedded Loop case, the Outlook parent origin
`https://outlook.cloud.microsoft` and iframe origin
`https://hosted.loop.cloud.microsoft` differ. Existing-tab eval and binary
transport do **not** unlock that iframe.

## Native safe frame API exists, in a different context

Read-only protocol probes on Safari 27.0.1, October 11, 2026, found that
`/usr/bin/safaridriver --mcp` advertises:

- `evaluate_javascript`: required string `expression`, optional string `frameId`;
  the expression is a function body with explicit `return`. `frameId` is the
  iframe element UID from `get_page_content`, not a URL or frame index.
- `get_page_content`: optional `includeSubframes: true` explicitly includes
  cross-origin subframes. Extraction UIDs can be used by native page tools.
- `create_tab`, `close_tab`, `list_tabs`, and `switch_tab(handle)`.

Thus **Safari as a platform is not categorically unable to evaluate frames**.
Its native tool is the appropriate security-preserving surface for an
automation-owned frame. However, there is no advertised attach/import command
or window-ID/document-UUID mapping for a regular Apple Events tab:

```text
Advertised tool names:
browser_console_messages, browser_dialogs, close_tab, create_tab,
evaluate_javascript, get_network_request, get_page_content,
list_network_requests, list_tabs, navigate_to_url, page_info,
page_interactions, screenshot, set_emulated_media, set_viewport_size,
switch_tab, wait_for_navigation

Fresh native session list_tabs: []
evaluate_javascript("return JSON.stringify({cider_synthetic_constant: 42});"):
  isError: true
  "Tool error: Command failed: No active tab"
```

Only initialization, tool schema listing, tab count, and the synthetic constant
were probed. No tab was created, switched, or navigated; no content extraction,
account action, or credential-bearing result was requested. Probe subprocesses
were stopped afterward. These observations establish an **existing-session
attachment blocker**, not proof that cross-origin native frame evaluation fails.
No native frame evaluation or authentication continuity was live-validated.

## Supported alternatives and future work

- Let the user open the embedded application as a genuine top-level tab and
  verify access/authentication there. Capture a fresh `DocumentTarget` for that
  tab. This works only if the application actually supports top-level use;
  opening an iframe URL alone does not establish a session.
- With explicit user permission, use a separate Safari native automation
  context, discover its iframe UID, and call native
  `evaluate_javascript(expression, frameId)`. The user must manage any required
  sign-in in that context. Cider's existing `safari network` capture already
  uses this isolated backend, but it is not a frame-evaluation/session API.
- A future native frame adapter must own its connection, verify the native tab,
  top-level document UUID, frame document UUID and exact origins **on every
  dispatch, poll and cleanup**, and invalidate on frame replacement/reload,
  movement or UID reuse. A frame UID alone is not a durable document identity.
  It must also establish result, cancellation and session-lifetime contracts
  with synthetic cross-origin pages before being advertised as working.

No adapter is provided that silently transfers the original user's credentials
or document handle into another browser context. Deterministic resolver tests
cover reload, window movement, exact-origin mismatch and frame-only identities;
they explicitly ensure that a nested identity cannot resolve as a top-level
document. They do not pretend to test Safari's cross-origin security model.
