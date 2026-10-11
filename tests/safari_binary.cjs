// Synthetic adapters only: node --test tests/safari_binary.cjs
const {test} = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(require("node:path").join(__dirname, "../src/sources/safari_binary.js"), "utf8");
const settle = () => new Promise(resolve => setImmediate(resolve));

function fixture(extra = {}, injectedFactory = null) {
    const timers = new Map();
    let next = 0;
    const page = {window: {}, AbortController, Uint8Array,
        setTimeout(fn, milliseconds) { const id = ++next; timers.set(id, {fn, milliseconds}); return id; },
        clearTimeout(id) { timers.delete(id); }, ...extra};
    const script = injectedFactory === null ? source :
        source.replace("(0, eval)(config.javascript)", `(\n${injectedFactory}\n)`);
    const run = vm.runInContext(`(${script})`, vm.createContext(page));
    const defaults = {id: "transfer", direction: "download", chunk_bytes: 128000,
        total_bytes: 1000000, offset: 0, timeout: 1, idle_timeout: 2};
    let count = 0;
    return {
        page, timers,
        async op(action, config = {}) {
            const key = `operation${++count}`;
            const started = run({...defaults, action, key, ...config});
            await settle();
            return started === "started" ? page.window[key].result : JSON.parse(started);
        },
        fire(milliseconds) {
            const timer = [...timers.values()].find(timer => timer.milliseconds === milliseconds);
            assert.ok(timer, `missing timer ${milliseconds}`);
            timer.fn();
        },
        size() { return page.window.__cider_binary_transfers.size; },
    };
}

test("downloads >128000 bytes in bounded chunks with every byte including 0/255 intact", async () => {
    let position = 0, closed = 0, aborted = 0;
    const bytes = Uint8Array.from({length: 750001}, (_, i) => i % 256);
    const f = fixture({adapter: {
        read(max) {
            assert.ok(max <= 128000);
            if (position === bytes.length) return null;
            const chunk = bytes.slice(position, position + max);
            position += chunk.length;
            return chunk;
        },
        close() { closed++; },
        abort() { aborted++; },
    }});
    assert.equal((await f.op("open", {javascript: "() => adapter"})).ok, true);
    let offset = 0;
    while (true) {
        const chunk = await f.op("read", {offset});
        assert.equal(chunk.ok, true);
        assert.equal(chunk.offset, offset);
        const data = Buffer.from(chunk.hex, "hex");
        assert.deepEqual(data, Buffer.from(bytes.slice(offset, offset + data.length)));
        offset = chunk.next_offset;
        if (chunk.done) break;
    }
    assert.equal(offset, bytes.length);
    await f.op("finish", {offset});
    assert.equal(closed, 1);
    assert.equal(aborted, 0);
    assert.equal(f.size(), 0);
});

test("uploads >128000 bytes acknowledge one exact chunk at a time, without accumulation", async () => {
    let writes = 0, expectedOffset = 0;
    const bytes = Uint8Array.from({length: 128000}, (_, i) => i % 256);
    const f = fixture({adapter: {
        async write(chunk, offset) {
            assert.equal(offset, expectedOffset);
            assert.deepEqual(chunk, bytes);
            writes++;
            expectedOffset += chunk.length;
        },
        close() {}, abort() {},
    }});
    const config = {direction: "upload", javascript: "() => adapter"};
    await f.op("open", config);
    for (let i = 0; i < 5; i++) {
        const result = await f.op("write", {...config, offset: i * bytes.length, hex: Buffer.from(bytes).toString("hex")});
        assert.equal(result.ok, true);
        assert.equal(result.next_offset, (i + 1) * bytes.length);
    }
    assert.equal(writes, 5);
    await f.op("finish", {...config, offset: expectedOffset});
    assert.equal(f.size(), 0);
});

test("zero-length downloads, total budgets and oversized/invalid chunks are explicit", async () => {
    const empty = fixture({adapter: {read(max) { assert.equal(max, 0); return null; }, close() {}, abort() {}}});
    await empty.op("open", {javascript: "() => adapter", total_bytes: 0});
    const end = await empty.op("read", {total_bytes: 0});
    assert.equal(end.done, true);
    assert.equal(end.hex, "");
    for (const data of [new Uint8Array([0, 255]), new Uint8Array(), "binary"]) {
        let aborts = 0;
        const f = fixture({adapter: {read: () => data, close() {}, abort() {aborts++;}}});
        await f.op("open", {javascript: "() => adapter", total_bytes: 1});
        assert.match((await f.op("read", {total_bytes: 1})).error, /bounds/);
        assert.equal(f.size(), 0);
        assert.equal(aborts, 1);
    }
});

test("writes reject invalid hex/chunk/total bounds before invoking the sink", async () => {
    for (const hex of ["", "f", "zz", "00ff", "00".repeat(128001)]) {
        let writes = 0, aborts = 0;
        const f = fixture({adapter: {write() {writes++;}, close() {}, abort() {aborts++;}}});
        const config = {direction: "upload", total_bytes: 1};
        await f.op("open", {...config, javascript: "() => adapter"});
        const result = await f.op("write", {...config, hex});
        assert.equal(writes, 0);
        assert.match(result.error, /bounds/);
        assert.equal(aborts, 1);
        assert.equal(f.size(), 0);
    }
});

test("stale offsets, altered budgets and concurrent calls do not dispatch adapters", async () => {
    let reads = 0, resolve;
    const f = fixture({adapter: {read() {reads++; return new Promise(r => {resolve = r;});}, close() {}, abort() {}}});
    await f.op("open", {javascript: "() => adapter"});
    for (const config of [{offset: 1}, {total_bytes: 2}, {chunk_bytes: 2}, {idle_timeout: 3}, {direction: "upload"}]) {
        const result = await f.op("read", config);
        assert.match(result.error, /mismatch/);
        assert.equal(result.phase, "before_dispatch");
        assert.equal(reads, 0);
    }
    assert.equal(await f.op("read"), null);
    assert.match((await f.op("read")).error, /in flight/);
    assert.equal(reads, 1);
    resolve(new Uint8Array([0, 255]));
    await settle();
    assert.equal(f.page.window.operation7.result.next_offset, 2);
});

test("timeout and interruption abort once, discard late acknowledgements and never retry writes", async () => {
    let writes = 0, aborts = 0, resolve, signal;
    const f = fixture({factory(s) {
        signal = s;
        return {write() {writes++; return new Promise(r => {resolve = r;});}, close() {}, abort() {aborts++;}};
    }});
    await f.op("open", {direction: "upload", javascript: "s => factory(s)"});
    await f.op("write", {direction: "upload", hex: "00ff"});
    f.fire(1000);
    await settle();
    const error = f.page.window.operation2.result;
    assert.equal(error.phase, "running");
    assert.equal(error.cancellation, "requested");
    assert.equal(signal.aborted, true);
    assert.equal(f.size(), 0);
    resolve();
    await settle();
    assert.equal(f.page.window.operation2.result, error);
    assert.equal(writes, 1);
    assert.equal(aborts, 1);
    assert.match((await f.op("write", {direction: "upload", hex: "00ff"})).error, /unavailable/);
});

test("explicit abort can interrupt an in-flight chunk without acknowledging its write", async () => {
    let finish, writes = 0, aborts = 0;
    const f = fixture({adapter: {
        write() {writes++; return new Promise(resolve => {finish = resolve;});},
        close() {}, abort() {aborts++;},
    }});
    await f.op("open", {direction: "upload", javascript: "() => adapter"});
    await f.op("write", {direction: "upload", hex: "00ff"});
    const abort = await f.op("abort", {direction: "upload"});
    assert.equal(abort.ok, true);
    assert.equal(abort.next_offset, 0);
    const interrupted = f.page.window.operation2.result;
    assert.equal(interrupted.phase, "running");
    assert.equal(interrupted.cancellation, "requested");
    finish();
    await settle();
    assert.equal(f.page.window.operation2.result, interrupted);
    assert.equal(writes, 1);
    assert.equal(aborts, 1);
    assert.equal(f.size(), 0);
});

test("idle expiry, explicit abort and abandoned startup release page state", async () => {
    for (const action of ["idle", "abort", "runner"]) {
        let aborted = 0;
        const f = fixture({adapter: {read: () => null, close() {}, abort() {aborted++;}}});
        await f.op("open", {javascript: "() => adapter"});
        if (action === "idle") f.fire(2000);
        else if (action === "abort") await f.op("abort");
        else {
            // Transport cleanup aborts a still-running operation, not acknowledged ones.
            f.page.window.__cider_binary_transfers.get("transfer").adapter.read = () => new Promise(() => {});
            await f.op("read");
            f.page.window.operation2.controller.abort();
        }
        await settle();
        assert.equal(aborted, 1);
        assert.equal(f.size(), 0);
    }
    let finish, aborts = 0;
    const f = fixture({factory: () => new Promise(resolve => {finish = resolve;})});
    await f.op("open", {javascript: "() => factory()"});
    f.fire(1000);
    finish({read: () => null, close() {}, abort() {aborts++;}});
    await settle();
    assert.equal(f.size(), 0);
    assert.equal(aborts, 1);
});

test("adapter failures redact exceptions and terminate without claiming a server rollback", async () => {
    let calls = 0;
    const f = fixture({adapter: {write() {calls++; throw new Error("private URL and credential");}, close() {}, abort() {throw new Error("cleanup secret");}}});
    await f.op("open", {direction: "upload", javascript: "() => adapter"});
    const error = await f.op("write", {direction: "upload", hex: "ff"});
    assert.equal(calls, 1);
    assert.equal(error.phase, "completed");
    assert.equal(error.cancellation, "requested");
    assert.doesNotMatch(error.error, /private|credential|secret/);
    assert.equal(f.size(), 0);
});

test("per-document live session capacity is finite and operation timers are cleared", async () => {
    const f = fixture({adapter: {read: () => null, close() {}, abort() {}}});
    for (let i = 0; i < 4; i++) assert.equal((await f.op("open", {id: `t${i}`, javascript: "() => adapter"})).ok, true);
    const result = await f.op("open", {id: "overflow", javascript: "() => adapter"});
    assert.match(result.error, /capacity/);
    assert.equal(f.size(), 4);
    assert.equal([...f.timers.values()].filter(t => t.milliseconds === 1000).length, 0);
});

test("native source embedding works without CSP-sensitive indirect eval", async () => {
    const f = fixture({eval() {throw new Error("eval disabled");}},
        "() => ({read: () => new Uint8Array([0,255]), close() {}, abort() {}})");
    assert.equal((await f.op("open")).ok, true);
    const chunk = await f.op("read");
    assert.equal(chunk.hex, "00ff");
    assert.equal(chunk.next_offset, 2);
});
