function(config, app) {
    return function locate() {
        if (!app.running()) throw new Error("Safari is not running");
        const windows = app.windows().filter(w => w.id() === config.window_id);
        if (windows.length !== 1) throw new Error("Original Safari window is unavailable");
        const candidates = [];
        for (const tab of windows[0].tabs()) {
            try {
                const identity = JSON.parse(app.doJavaScript(
                    "JSON.stringify({id:window.__cider_document_identity,origin:location.origin})", {in:tab}));
                if (identity.id === config.document_id && identity.origin === config.origin) candidates.push(tab);
            } catch (error) {}
        }
        if (candidates.length !== 1) throw new Error("Original document unavailable or origin changed; recapture the target");
        return candidates[0];
    };
}
