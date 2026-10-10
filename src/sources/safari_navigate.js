// Runs in JXA, not in the page. app/win/target are selected by TabTarget.
function(config) {
    const key = JSON.stringify(config.key);
    app.doJavaScript("window[" + key + "] = true", {in: target});
    const initialURL = target.url();
    const deadline = Date.now() + config.timeout * 1000;
    let ready = false;
    try {
        target.url = config.url;
        while (Date.now() < deadline) {
            const raw = app.doJavaScript(
                "(function(){return JSON.stringify({old:!!window[" + key +
                "],url:location.href,ready:document.readyState});})()", {in: target});
            const state = JSON.parse(raw);
            if ((!state.old || state.url !== initialURL) &&
                /^https?:/.test(state.url) && state.ready === "complete") {
                ready = true;
                break;
            }
            delay(0.2);
        }
        if (!ready) throw new Error("Safari navigation timed out; selected tab left open at its current URL. Read cider safari tabs to inspect it");
        return JSON.stringify({ok: true, action: "navigate", requested_url: config.url,
            url: target.url(), title: target.name() || "", window: config.window,
            tab: config.tab, window_id: win.id()});
    } finally {
        try { app.doJavaScript("delete window[" + key + "]", {in: target}); } catch (error) {}
    }
}
