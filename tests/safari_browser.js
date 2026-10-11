// Deterministic page-operation tests: node --test tests/safari_browser.js
// No Safari, network access, or real browser credentials are used.
const {test} = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

function pageOperation(name, extra = {}) {
    const timers = new Map();
    let nextTimer = 0;
    const page = {
        window: {},
        location: new URL("https://example.com/page"),
        URL, AbortController, TextDecoder,
        setTimeout(fn, milliseconds) {
            const id = ++nextTimer;
            timers.set(id, {fn, milliseconds});
            return id;
        },
        clearTimeout(id) { timers.delete(id); },
        ...extra
    };
    const context = vm.createContext(page);
    const operation = vm.runInContext(
        "(" + fs.readFileSync(`${__dirname}/../src/sources/safari_${name}.js`, "utf8") + ")",
        context
    );
    return {
        page, timers,
        start(config) {
            return operation({key: "test", timeout: 1, max_chars: 100, max_bytes: 100,
                method: "GET", headers: {}, body: null, ...config});
        },
        result() { return JSON.parse(JSON.stringify(page.window.test.result)); }
    };
}

async function settle() {
    await new Promise(resolve => setImmediate(resolve));
}

test("eval awaits async completion and returns JSON and undefined honestly", async () => {
    for (const [javascript, expected, type] of [
        ["Promise.resolve({n:42})", {n:42}, "json"],
        ["(async () => { await Promise.resolve(); return null; })()", null, "json"],
        ["undefined", undefined, "undefined"],
    ]) {
        const operation = pageOperation("eval");
        assert.equal(operation.start({javascript}), "started");
        await settle();
        const result = operation.result();
        assert.equal(result.ok, true);
        assert.equal(result.value_type, type);
        assert.deepEqual(result.value, expected);
        assert.equal(result.truncated, false);
    }
});

test("eval does not fake success for exceptions, rejection, or nonserializable values", async () => {
    for (const javascript of [
        "throw new Error('boom')",
        "Promise.reject(new Error('boom'))",
        "(() => { const a = {}; a.a = a; return a; })()",
        "1n", "(() => {})", "Symbol('x')",
    ]) {
        const operation = pageOperation("eval");
        operation.start({javascript});
        await settle();
        assert.match(operation.result().error, /Safari eval/);
        assert.equal(operation.result().ok, undefined);
    }
});

test("eval bounds Unicode preview without presenting partial JSON as a value", async () => {
    const operation = pageOperation("eval");
    operation.start({javascript: "'a😀b'", max_chars: 3});
    await settle();
    assert.equal(operation.result().truncated, true);
    assert.equal(operation.result().preview, '"a😀');
    assert.equal(operation.result().value, undefined);
});

test("eval timeout is an error even if the promise later resolves", async () => {
    const operation = pageOperation("eval");
    operation.start({javascript: "new Promise(resolve => window.finish = resolve)"});
    await settle();
    [...operation.timers.values()].find(t => t.milliseconds === 1000).fn();
    operation.page.window.finish(42);
    await settle();
    assert.match(operation.result().error, /may still be running/);
});

test("request rejects cross-origin startup and forwards gated request options without retries", async () => {
    let calls = 0, received;
    const operation = pageOperation("request", {
        async fetch(url, options) {
            calls++;
            received = {url, options};
            return {ok: false, url, status: 401, statusText: "Unauthorized",
                headers: {get: () => "application/json"}, body: null};
        }
    });
    assert.throws(() => operation.start({url: "https://other.example/api"}), /same origin/);
    assert.equal(calls, 0);
    operation.start({url: "https://example.com/api", method: "POST",
        headers: {"X-App": "test"}, body: "{}"});
    await settle();
    assert.equal(calls, 1);
    assert.equal(received.options.method, "POST");
    assert.equal(received.options.body, "{}");
    assert.equal(received.options.headers["X-App"], "test");
    assert.equal(received.options.redirect, "error");
    assert.equal(received.options.mode, "same-origin");
    assert.equal(received.options.credentials, "same-origin");
    assert.equal(operation.result().status, 401);
    assert.equal(operation.result().ok, false);
    assert.equal(operation.result().headers, undefined);
});

test("request bounds streamed response bytes and reports truncation", async () => {
    let cancelled = false;
    const operation = pageOperation("request", {
        async fetch(url) {
            return {ok: true, url, status: 200, statusText: "OK",
                headers: {get: () => "text/plain"},
                body: {getReader: () => ({
                    read: async () => ({done: false, value: new Uint8Array([97, 98, 99, 100])}),
                    cancel: async () => { cancelled = true; }
                })}};
        }
    });
    operation.start({url: "https://example.com/api", max_bytes: 3});
    await settle();
    assert.equal(operation.result().body, "abc");
    assert.equal(operation.result().truncated, true);
    assert.equal(cancelled, true);
});

test("request network/redirect failures are errors, not success-shaped responses", async () => {
    let calls = 0;
    const operation = pageOperation("request", {
        async fetch() { calls++; throw new Error("redirect disallowed"); }
    });
    operation.start({url: "https://example.com/api"});
    await settle();
    assert.equal(calls, 1);
    assert.match(operation.result().error, /redirect/);
    assert.equal(operation.result().ok, undefined);
});

test("network list filters retained entries, caps results, and never claims full history", () => {
    const entries = [
        {name: "https://example.com/api/a", initiatorType: "fetch", startTime: 1, duration: 2, transferSize: 10},
        {name: "https://example.com/style.css", initiatorType: "link", startTime: 2, duration: 3},
        {name: "https://example.com/api/b", initiatorType: "xmlhttprequest", startTime: 3, duration: 4},
    ];
    const performance = {getEntriesByType(type) { assert.equal(type, "resource"); return entries; }};
    const operation = pageOperation("network_list", {window: {performance}, performance});
    const result = JSON.parse(operation.start({filter: "/api/", limit: 1}));
    assert.equal(result.matched_count, 2);
    assert.equal(result.requests_truncated, true);
    assert.equal(result.requests.length, 1);
    assert.equal(result.requests[0].url, "https://example.com/api/a");
    assert.equal(result.complete_history, false);
    assert.equal(result.headers_available, false);
    assert.equal(result.bodies_available, false);
    assert.equal(result.requests[0].method, undefined);
    assert.equal(result.requests[0].response_body, undefined);
    assert.equal(result.requests[0].headers, undefined);
    const empty = JSON.parse(operation.start({filter: "/missing", limit: 10}));
    assert.equal(empty.requests.length, 0);
    assert.match(empty.limitations, /Empty does not mean no requests/);
    assert.equal(empty.complete_history, false);
    assert.throws(() => pageOperation("network_list").start({limit: 10}), /unavailable/);
});

function navigation({permission = true, complete = true, sameDocument = false} = {}) {
    const documentPage = vm.createContext({
        window: {}, document: {readyState: "complete"},
        location: {href: "https://example.com/old"}
    });
    let now = 0, navigationCount = 0, waits = 0;
    const target = new Proxy({}, {
        get(_, name) {
            if (name === "url") return () => documentPage.location.href;
            if (name === "name") return () => "final page";
            throw new Error("Unexpected tab operation: " + name);
        },
        set(_, name, value) {
            assert.equal(name, "url");
            navigationCount++;
            if (sameDocument) documentPage.location.href = value;
            return true;
        }
    });
    const operation = pageOperation("navigate", {
        app: {doJavaScript(code, selection) {
            assert.equal(selection.in, target);
            if (!permission) throw new Error("JavaScript permission denied");
            return vm.runInContext(code, documentPage);
        }},
        win: {id: () => 99}, target,
        Date: {now: () => now},
        delay() {
            now += 200;
            waits++;
            if (complete && !sameDocument) {
                documentPage.window = {};
                documentPage.location.href = "https://example.com/final";
                documentPage.document.readyState = "complete";
            }
        }
    });
    return {
        run() { return JSON.parse(operation.start({url: "https://example.com/new", window: 1, tab: 2})); },
        counts() { return {navigationCount, waits}; },
        documentPage
    };
}

test("navigation waits past the old ready document, reports final URL and never closes the tab", () => {
    const operation = navigation();
    const result = operation.run();
    assert.equal(result.url, "https://example.com/final");
    assert.equal(result.requested_url, "https://example.com/new");
    assert.equal(result.window_id, 99);
    assert.equal(result.tab, 2);
    assert.deepEqual(operation.counts(), {navigationCount: 1, waits: 1});
    assert.equal(operation.documentPage.window.test, undefined);
    assert.equal(navigation({sameDocument: true}).run().url, "https://example.com/new");
});

test("navigation checks permission before mutation and leaves failed navigation open", () => {
    const denied = navigation({permission: false});
    assert.throws(() => denied.run(), /permission denied/);
    assert.equal(denied.counts().navigationCount, 0);
    const timeout = navigation({complete: false});
    assert.throws(() => timeout.run(), /timed out.*left open/);
    assert.equal(timeout.counts().navigationCount, 1);
    assert.equal(timeout.documentPage.window.test, undefined);
});
