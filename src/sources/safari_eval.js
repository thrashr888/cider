function(config) {
    const state = {result: null};
    window[config.key] = state;
    state.timer = setTimeout(() => {
        state.result = {error: "Safari eval timed out; JavaScript may still be running and side effects are not rolled back"};
        setTimeout(() => { delete window[config.key]; }, 2000);
    }, config.timeout * 1000);
    Promise.resolve().then(() => (0, eval)(config.javascript)).then(value => {
        if (state.result) return;
        let serialized;
        try {
            serialized = JSON.stringify(value);
            if (serialized === undefined && value !== undefined) {
                throw new Error("result is not JSON-serializable");
            }
        } catch (error) {
            throw new Error("Safari eval result serialization failed: " + String(error.message || error));
        }
        const chars = Array.from(serialized === undefined ? "" : serialized);
        const truncated = chars.length > config.max_chars;
        state.result = {ok: true, action: "eval", value_type: value === undefined ? "undefined" : "json",
            truncated};
        if (truncated) state.result.preview = chars.slice(0, config.max_chars).join("");
        else if (serialized !== undefined) state.result.value = JSON.parse(serialized);
        clearTimeout(state.timer);
    }).catch(error => {
        if (!state.result) {
            state.result = {error: "Safari eval failed: " + String(error && error.message || error)};
            clearTimeout(state.timer);
        }
    }).finally(() => {
        setTimeout(() => { delete window[config.key]; }, 2000);
    });
    return "started";
}
