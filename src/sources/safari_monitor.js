function(config) {
    if (window.__ciderNetworkMonitor) throw new Error("A Cider monitor is already active in this tab. Wait for it to finish before retrying.");
    const state = {requests: [], matched: 0, bytes: 0, active: true, readers: new Set()};
    const originalFetch = window.fetch;
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    const xhrInfo = new WeakMap();
    const record = (url, method) => {
        if (!state.active) return null;
        url = new URL(url, location.href).href;
        if (config.filter && !url.toLowerCase().includes(config.filter.toLowerCase())) return null;
        state.matched++;
        if (state.requests.length >= config.limit) return null;
        const item = {request_id: 'page-' + state.matched, url, method,
            started_at_iso8601: new Date().toISOString(), body_state: config.bodies ? 'pending' : 'not_requested'};
        state.requests.push(item);
        return item;
    };
    const save = (item, body) => {
        if (!state.active) return;
        if (body.length > config.max_body_bytes) { item.body_state = 'over_limit'; return; }
        const bytes = new TextEncoder().encode(body).length;
        if (bytes > config.max_body_bytes || state.bytes + bytes > 16777216) { item.body_state = 'over_limit'; return; }
        state.bytes += bytes;
        item.response_body = body;
        item.body_state = 'captured';
    };
    const failed = (item, error) => {
        if (state.active && item && config.bodies) { item.body_state = 'error'; item.body_error = String(error); }
    };
    async function captureFetch(item, response) {
        item.status = response.status;
        item.mime_type = response.headers.get('content-type') || '';
        if (!config.bodies) return;
        if (!state.active) return;
        if (state.readers.size >= 8) { item.body_state = 'unavailable'; return; }
        if (!/json|text\/|javascript|xml/.test(item.mime_type)) { item.body_state = 'unavailable'; return; }
        const clone = response.clone();
        if (!clone.body) { save(item, ''); return; }
        const reader = clone.body.getReader();
        state.readers.add(reader);
        const decoder = new TextDecoder();
        let bytes = 0, body = '';
        try {
            while (state.active) {
                const part = await reader.read();
                if (part.done) { body += decoder.decode(); save(item, body); return; }
                bytes += part.value.length;
                if (bytes > config.max_body_bytes || bytes + state.bytes > 16777216) { item.body_state = 'over_limit'; return; }
                body += decoder.decode(part.value, {stream:true});
            }
        } finally {
            // Do not await cancellation of a tee: that can wait for the page's reader.
            reader.cancel().catch(() => {});
            state.readers.delete(reader);
        }
    }
    function wrappedFetch(input, init) {
        let item;
        try { item = record(typeof input === 'string' || input instanceof URL ? String(input) : input.url,
            String(init && init.method || input && input.method || 'GET').toUpperCase()); } catch (_) {}
        // Return the original promise and response, without consuming the page's body.
        const promise = originalFetch.apply(this, arguments);
        if (item) promise.then(response => captureFetch(item, response)).catch(error => failed(item, error));
        return promise;
    }
    function wrappedOpen(method, url) {
        const result = originalOpen.apply(this, arguments);
        xhrInfo.set(this, {method:String(method).toUpperCase(), url:String(url)});
        return result;
    }
    function wrappedSend() {
        const info = xhrInfo.get(this);
        let item;
        try { if (info) item = record(info.url, info.method); } catch (_) {}
        if (item) this.addEventListener('loadend', () => {
            if (!state.active) return;
            try {
                item.status = this.status;
                item.mime_type = this.getResponseHeader('content-type') || '';
                if (!config.bodies) return;
                if (this.responseType === '' || this.responseType === 'text') save(item, this.responseText);
                // JSON responseType is already parsed by the browser; never reserialize it.
                else item.body_state = 'unavailable';
            } catch (error) { failed(item, error); }
        }, {once:true});
        return originalSend.apply(this, arguments);
    }
    state.stop = () => {
        state.active = false;
        clearTimeout(state.timer);
        if (window.fetch === wrappedFetch) window.fetch = originalFetch;
        if (XMLHttpRequest.prototype.open === wrappedOpen) XMLHttpRequest.prototype.open = originalOpen;
        if (XMLHttpRequest.prototype.send === wrappedSend) XMLHttpRequest.prototype.send = originalSend;
        for (const reader of state.readers) reader.cancel().catch(() => {});
        for (const item of state.requests) if (item.body_state === 'pending') item.body_state = 'unavailable';
        const result = {ok:true, action:'monitor', backend:'page_fetch_xhr', url:state.url,
            page:{url:location.href, title:document.title}, matched_count:state.matched,
            requests_truncated:state.matched > state.requests.length, requests:state.requests,
            opened_tab:false, tab_closed:false};
        delete window.__ciderNetworkMonitor;
        delete window[config.key];
        return JSON.stringify(result);
    };
    state.url = location.href;
    window[config.key] = state;
    window.__ciderNetworkMonitor = state;
    window.fetch = wrappedFetch;
    XMLHttpRequest.prototype.open = wrappedOpen;
    XMLHttpRequest.prototype.send = wrappedSend;
    // Restore wrappers even if the caller is interrupted. Full navigation destroys them.
    state.timer = setTimeout(state.stop, (config.seconds + 10) * 1000);
    return 'started';
}
