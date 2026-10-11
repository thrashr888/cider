# Bounded Safari binary transfers

`cider::sources::safari` exports:

- `BinaryTransferOptions`, `BinaryDirection::{Download, Upload}`;
- `open_binary_transfer(factory, &DocumentTarget, direction, &options)`;
- `read_binary_chunk(&mut BinaryTransfer) -> BinaryChunk`;
- `write_binary_chunk(&mut BinaryTransfer, &[u8])`;
- `finish_binary_transfer(&mut BinaryTransfer)`;
- `abort_binary_transfer(&mut BinaryTransfer)`.

These are additive APIs. Existing `eval`, `eval_document`, and `request` contracts
and limits are unchanged. This is **chunk transport between Rust and page code**,
not browser fetch request-body streaming. It requires the existing Safari
Apple Events JavaScript permission. It cannot access a cross-origin iframe;
see [frame targeting evidence and alternatives](safari-frame-targeting.md).

## Adapter contract

The caller supplies a trusted JavaScript **factory expression**:

```javascript
(signal) => ({
    // Download only: return a nonempty Uint8Array of at most maxBytes, or null for EOF.
    async read(maxBytes) { /* consume one bounded portion */ },
    // Upload only: acknowledge after consuming this chunk; do not retain all chunks.
    async write(bytes, offset) { /* submit one consumer-controlled chunk */ },
    async close() { /* finish and release readers/resources */ },
    async abort() { /* best-effort cancellation and resource release */ }
})
```

The factory can be async. The factory and both lifecycle methods are required;
only the method for the selected direction is required. A download's
`read(0)` must return `null`; any bytes beyond the total budget fail rather than
being silently truncated. EOF remains open until `finish` or `abort` (or expiry).
`total_bytes` is a ceiling, not an exact expected length; the consumer may finish
early and should independently validate lengths or digests when needed.

Keep URLs, credentials, authorization headers, upload-session URLs and server
metadata **inside the page factory/closure**. Cider neither discovers nor returns
them. Pass `signal` to fetch calls and implement `abort` for any stream reader or
other resource. For downloads, read responses incrementally; do not use
`arrayBuffer()`/`blob()` on an unbounded response. A consumer adapting a
`ReadableStream` must bound any retained upstream chunk and split it as needed
to honor `maxBytes`, cancelling if its own upstream buffer budget is exceeded.

For uploads, Safari support for streaming `fetch` request bodies is not assumed.
Implement `write` as a consumer-controlled, bounded request (such as one upload
session range) or another bounded sink. Protocol-specific chunk sizes, headers,
server offset reconciliation, session finalization and recovery are the
consumer's responsibility. If a service requires request chunks larger than
Cider's transport chunk, accumulate only a separately bounded service chunk
inside the adapter. No endpoint-specific logic is built into Cider.

## Bounds and lifetime

`allow_eval: true` is mandatory because caller code can mutate a page or server.
There is no implicit total-byte budget. Options:

| Field | Bound |
|---|---|
| `chunk_bytes` | 1–128000 bytes |
| `total_bytes` | 0–9007199254740991 bytes (exact JavaScript integer) |
| `timeout` | 1–120 seconds per operation |
| `idle_timeout` | 1–120 seconds between acknowledged operations |

Cider retains one bounded chunk per operation, not a whole transfer. Transport
uses internal lowercase hex, not UTF-8 decoding or lossy string conversions;
library/CLI byte arrays preserve every value including 0 and 255. Binary
acknowledgements are never truncated or accepted as previews.

There are at most four registered live transfers per document. Every operation
re-resolves the original window ID, document UUID, and exact origin. Same-origin
SPA routes and reordering remain valid; reload, origin changes, closure and
moving into another window invalidate the target. Offsets and bounds are
checked both locally and against page session state. A session permits only one
in-flight operation; cloned/stale handles cannot replay acknowledged writes.
An explicit abort with the last acknowledged offset can interrupt an in-flight
chunk; that chunk receives a `running/requested` error, not a write acknowledgement.

Successful close removes session state. Explicit abort, operation timeout,
adapter failure, and idle expiry remove it and request `AbortSignal`/adapter
cleanup. Normal operation result slots are removed by the runner and have a
short fallback expiry. Cleanup never falls back to a different document.
If identity disappears or transport dies, immediate cleanup can be unknown;
the original page's timers provide best-effort cleanup if it remains alive.
Suspended pages can delay timers. A dropped Rust handle has no asynchronous
`Drop`; explicitly finish/abort or rely on expiry.

The adapter is arbitrary trusted page code: Cider cannot force it to respect
its own memory budget, terminate a synchronous loop or an unresolved promise,
cancel server writes, or erase resources that it hides from `abort`. Bounds
apply to Cider's transport, not to arbitrary code or Safari's network buffers.

## Failure and retry rules

Operational failures use `PageOperationError` and the existing CLI error
envelope with `phase` and `cancellation`. A known stale/busy/expired startup
refusal is `before_dispatch/not_needed`. An adapter exception is
`completed/requested`, which does **not** mean its server write did not happen.
A timeout is `running/requested`; transport loss or a malformed acknowledgement
is `unknown/unknown`. Adapter exception messages are redacted, since they may
contain credentials or preauthenticated URLs.

After an operational error the Rust handle is marked closed, and after an
adapter error/timeout the page session is terminal. Do not restore `closed`,
replay stale CLI handles, or automatically retry writes after uncertain dispatch.
For an ambiguous upload, the consumer must inspect/reconcile the service's
state via its own protocol before deciding what to do. Cider never retries
requests or adapter invocations.

## CLI and synthetic example

The CLI processes **one chunk per invocation** so consumers control output and
backpressure:

```sh
cider safari binary open --file factory.js --direction download \
  --total-bytes 10000000 --chunk-bytes 128000 --tab 2 --allow-eval
# Save the returned plain JSON handle to handle.json using your consumer.
cider safari binary read --handle-file handle.json
# Result: {"handle":{...advanced handle...},"chunk":{"offset":0,"bytes":[0,255],"done":false}}
# Replace handle.json with result.handle after each acknowledged read.
cider safari binary finish --handle-file handle.json

cider safari binary open --file sink.js --direction upload \
  --total-bytes 10000000 --tab 2 --allow-eval
cider safari binary write --handle-file handle.json --file one-chunk.bin
# Write/finish/abort return the updated plain handle.
cider safari binary abort --handle-file handle.json
```

Handle files are read with a 16384-byte cap, source files with the existing
1000000-byte cap, and raw chunk files with the configured chunk cap. `--dry-run`
validates inputs without selecting a document or running the adapter.
With `--envelope`, extract the payload before saving a handle.
Do not write back an error envelope as a successful acknowledgement.

A network-free `factory.js`:

```javascript
() => {
    let offset = 0;
    return {
        read(maxBytes) {
            if (offset === 750001) return null;
            const length = Math.min(maxBytes, 750001 - offset);
            const chunk = Uint8Array.from({length}, (_, i) => (offset + i) % 256);
            offset += length;
            return chunk;
        },
        close() {},
        abort() {}
    };
}
```

Use a budget at least 750001 for this fixture. Library consumers similarly loop
over `read_binary_chunk`, immediately send `chunk.bytes` to their own bounded
sink, stop on `done`, and finish the transfer. They need not buffer the file.
Synthetic Rust and Node tests cover all byte values, multi-chunk transfers,
empty downloads, total/chunk bounds, stale handles, single-flight dispatch,
timeouts, late completion, abandoned startup, idle cleanup, redacted failures,
and no write retries. No network or real account content is used by these tests.
