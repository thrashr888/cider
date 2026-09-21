const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/../src/sources/safari_monitor.js`, 'utf8');
function setup(options = {}, fetchImpl = async () => new Response('hello', {headers:{'content-type':'text/plain'}})) {
    class XHR extends EventTarget {
        open(method, url) { this.method = method; this.url = url; return 'open-result'; }
        send(body) { this.sentBody = body; return 'send-result'; }
        getResponseHeader() { return 'application/json'; }
        finish(body, type = '') { this.responseText = body; this.responseType = type; this.status = 200; this.dispatchEvent(new Event('loadend')); }
    }
    const timers = [];
    const context = {URL, TextEncoder, TextDecoder, XMLHttpRequest:XHR,
        location:new URL('https://example.com/profile'), document:{title:'Profile'}, fetch:fetchImpl,
        setTimeout(fn) { timers.push(fn); return timers.length; }, clearTimeout() {}};
    context.window = context;
    const originalOpen = XHR.prototype.open, originalSend = XHR.prototype.send;
    const start = vm.runInNewContext(`(${source})`, context);
    assert.equal(start({key:'__test',seconds:30,limit:100,max_body_bytes:1000000,bodies:true,...options}), 'started');
    const state = context.__test;
    return {context, state, XHR, timers, start, fetchImpl, originalOpen, originalSend, stop:()=>JSON.parse(state.stop())};
}
async function settle() { for(let i=0;i<20;i++) await new Promise(setImmediate); }

test('preserves original fetch promise, page response, JSON whitespace, and UTF-8', async () => {
    const body = '{ "data" : ["🐈", 1] }\n';
    const response = new Response(body, {headers:{'content-type':'application/json'}});
    const promise = Promise.resolve(response);
    const s = setup({}, () => promise);
    assert.equal(s.context.fetch('/api', {headers:{Authorization:'not-exported'}}), promise);
    await promise;
    assert.equal(await response.text(), body);
    await settle();
    const result = s.stop();
    assert.equal(result.requests[0].response_body, body);
    assert.equal(result.requests[0].body_state, 'captured');
    assert.equal(JSON.stringify(result).includes('not-exported'), false);
    assert.equal(s.context.fetch, s.fetchImpl);
    assert.equal(s.XHR.prototype.open, s.originalOpen);
    assert.equal(s.XHR.prototype.send, s.originalSend);
    assert.equal(s.context.__ciderNetworkMonitor, undefined);
});
test('captures original XHR text but never reserializes parsed JSON', () => {
    const s = setup();
    for (const type of ['', 'text', 'json', 'arraybuffer']) {
        const xhr = new s.XHR();
        assert.equal(xhr.open('POST','/api'), 'open-result');
        assert.equal(xhr.send('private-request-body'), 'send-result');
        xhr.finish('{ "original" : true }', type);
    }
    const result = s.stop();
    assert.deepEqual(result.requests.map(r=>r.body_state), ['captured','captured','unavailable','unavailable']);
    assert.equal(result.requests[0].response_body, '{ "original" : true }');
    assert.equal(JSON.stringify(result).includes('private-request-body'), false);
});
test('filters before limiting and reports omitted matching requests', () => {
    const s = setup({filter:'/API', limit:1, bodies:false});
    for (const url of ['/other','/api/one','/api/two']) { const x = new s.XHR(); x.open('get',url); x.send(); x.finish('body'); }
    const result = s.stop();
    assert.equal(result.matched_count, 2);
    assert.equal(result.requests.length, 1);
    assert.equal(result.requests_truncated, true);
    assert.equal(result.requests[0].body_state, 'not_requested');
    assert.equal(result.requests[0].response_body, undefined);
});
test('omits oversized bodies whole and leaves page fetch response untouched', async () => {
    const response = new Response('🐈🐈', {headers:{'content-type':'text/plain'}});
    const s = setup({max_body_bytes:5}, () => Promise.resolve(response));
    await s.context.fetch('/api');
    assert.equal(await response.text(), '🐈🐈');
    await settle();
    const result = s.stop();
    assert.equal(result.requests[0].body_state, 'over_limit');
    assert.equal(result.requests[0].response_body, undefined);
});
test('aggregate cap and pending XHR are explicit', () => {
    const s = setup();
    s.state.bytes = 16777216;
    const x = new s.XHR(); x.open('GET','/api'); x.send(); x.finish('x');
    const pending = new s.XHR(); pending.open('GET','/pending'); pending.send();
    const result = s.stop();
    assert.equal(result.requests[0].body_state, 'over_limit');
    assert.equal(result.requests[1].body_state, 'unavailable');
});
test('cleanup timer restores wrappers without overwriting a newer page wrapper', () => {
    const s = setup();
    const newer = () => {};
    s.context.fetch = newer;
    s.timers[0]();
    assert.equal(s.context.fetch, newer);
    assert.equal(s.XHR.prototype.open, s.originalOpen);
    assert.equal(s.context.__test, undefined);
});
test('a second capture is rejected without replacing the first', () => {
    const s = setup();
    const wrapped = s.context.fetch;
    assert.throws(()=>s.start({key:'other'}), /already active/);
    assert.equal(s.context.fetch, wrapped);
    s.stop();
});
test('fetch failure does not change the page rejection and is reported', async () => {
    const failure = new Error('network down');
    const s = setup({}, () => Promise.reject(failure));
    await assert.rejects(s.context.fetch('/api'), error => error === failure);
    await settle();
    const result = s.stop();
    assert.equal(result.requests[0].body_state, 'error');
    assert.match(result.requests[0].body_error, /network down/);
});
