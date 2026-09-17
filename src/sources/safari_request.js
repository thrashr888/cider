function(config) {
    const url = new URL(config.url);
    if (!/^https?:$/.test(location.protocol) || url.origin !== location.origin) {
        throw new Error("invalid request: URL must have the same origin as the selected Safari tab. Run cider safari tabs --pretty and select a tab on the requested scheme, host, and port with --window and --tab");
    }
    const state = {controller: new AbortController(), result: null};
    window[config.key] = state;
    state.timer = setTimeout(() => {
        state.controller.abort();
        state.result = {error: "Safari request timed out"};
        // Cleanup even if the osascript process is killed by its caller.
        setTimeout(() => { delete window[config.key]; }, 2000);
    }, config.timeout * 1000);
    (async () => {
        try {
            const response = await fetch(url.href, {
                method: "GET", credentials: "same-origin", mode: "same-origin",
                redirect: "error", signal: state.controller.signal
            });
            const reader = response.body && response.body.getReader();
            const decoder = new TextDecoder();
            let body = "", bytes = 0, truncated = false;
            if (reader) {
                try {
                    while (true) {
                        const {done, value} = await reader.read();
                        if (done) break;
                        const available = config.max_bytes - bytes;
                        const part = value.subarray(0, available);
                        body += decoder.decode(part, {stream: true});
                        bytes += part.length;
                        if (value.length > available) { truncated = true; break; }
                    }
                } finally { await reader.cancel(); }
            }
            body += decoder.decode();
            state.result = {ok: response.ok, action: "request", url: response.url,
                status: response.status, status_text: response.statusText,
                content_type: response.headers.get("content-type") || "", body, truncated};
        } catch (error) {
            state.result = {error: error.name === "AbortError" ? "Safari request timed out" :
                "Safari request failed (network, CSP, or redirect). Open the URL in Safari to complete login or follow redirects, then retry with its final same-origin URL. If the site blocks in-page requests, use cider safari content instead. Details: " + error.message};
        }
    })();
    return "started";
}
