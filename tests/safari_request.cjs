// Deterministic tests for the actual in-page request script. Run: node --test tests/safari_request.cjs
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const script = fs.readFileSync(`${__dirname}/../src/sources/safari_request.js`, 'utf8');
async function run({url = 'https://example.com/api', max_bytes = 10, response, failure} = {}) {
    const calls = [], timers = [];
    const context = {URL, AbortController, TextDecoder, location: new URL('https://example.com/page'), window: {},
        setTimeout(fn) {timers.push(fn); return timers.length;}, clearTimeout() {},
        async fetch(url, options) {
            calls.push({url, options});
            if (failure) throw failure;
            return response || new Response('hello world', {status: 200, headers: {'content-type': 'text/plain'}});
        }};
    const start = vm.runInNewContext(`(${script})`, context);
    start({url, max_bytes, timeout: 1, key: '__test'});
    for (let i = 0; i < 50 && !context.window.__test.result; i++) await new Promise(setImmediate);
    return {result: context.window.__test.result, calls, timers, context};
}
test('bounded GET includes session credentials and refuses redirects', async () => {
    const {result, calls} = await run();
    assert.equal(result.body, 'hello worl');
    assert.equal(result.truncated, true);
    assert.equal(calls[0].options.method, 'GET');
    assert.equal(calls[0].options.credentials, 'same-origin');
    assert.equal(calls[0].options.mode, 'same-origin');
    assert.equal(calls[0].options.redirect, 'error');
});
test('same-origin validation happens before any request', async () => {
    for (const url of ['https://evil.test', 'http://example.com', 'https://example.com:444/api']) {
        await assert.rejects(run({url}), /same origin/);
    }
});
test('exact limit and empty bodies are not falsely truncated', async () => {
    assert.equal((await run({max_bytes: 11})).result.truncated, false);
    const {result} = await run({response: new Response(null, {status: 204})});
    assert.equal(result.body, '');
    assert.equal(result.status, 204);
});
test('HTTP errors retain their body and status', async () => {
    const {result} = await run({response: new Response('no access', {status: 403})});
    assert.equal(result.ok, false);
    assert.equal(result.status, 403);
    assert.equal(result.body, 'no access');
});
test('network errors and deadline cleanup are explicit', async () => {
    const {result} = await run({failure: new Error('blocked')});
    assert.match(result.error, /network, CSP, or redirect/);
    const {timers, context} = await run();
    timers[0]();
    assert.equal(context.window.__test.controller.signal.aborted, true);
    assert.match(context.window.__test.result.error, /timed out/);
    timers[1]();
    assert.equal(context.window.__test, undefined);
});
test('UTF-8 split across chunks is preserved', async () => {
    const bytes = new TextEncoder().encode('你好');
    const response = new Response(new ReadableStream({start(controller) {
        controller.enqueue(bytes.slice(0, 2)); controller.enqueue(bytes.slice(2)); controller.close();
    }}));
    assert.equal((await run({response})).result.body, '你好');
});
