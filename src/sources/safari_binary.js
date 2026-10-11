function(config) {
    const registryKey = "__cider_binary_transfers";
    if (!window[registryKey]) Object.defineProperty(window, registryKey, {value: new Map()});
    const registry = window[registryKey];
    let transfer = registry.get(config.id);
    function failure(message, phase = "before_dispatch", cancellation = "not_needed") {
        return JSON.stringify({error: message, phase, cancellation});
    }
    if (config.action === "open") {
        if (registry.size >= 4 || transfer) return failure("Binary transfer capacity exceeded");
        transfer = {offset: 0, chunk_bytes: config.chunk_bytes, total_bytes: config.total_bytes,
            direction: config.direction, idle_timeout: config.idle_timeout,
            controller: new AbortController(), busy: false, adapter: null, terminal: false};
        registry.set(config.id, transfer);
    } else {
        if (!transfer || transfer.terminal) return failure("Binary transfer unavailable or expired");
        if (transfer.busy && config.action !== "abort") return failure("Binary transfer already has an operation in flight");
        if (transfer.offset !== config.offset || transfer.direction !== config.direction ||
            transfer.chunk_bytes !== config.chunk_bytes || transfer.total_bytes !== config.total_bytes ||
            transfer.idle_timeout !== config.idle_timeout) return failure("Binary transfer handle mismatch");
    }
    if (config.action === "abort" && transfer.busy) transfer.cancel();
    const state = {result: null};
    window[config.key] = state;
    function dispose() {
        if (transfer.terminal) return;
        transfer.terminal = true;
        transfer.cancel = null;
        registry.delete(config.id);
        clearTimeout(transfer.idle_timer);
        transfer.controller.abort();
        if (transfer.adapter && typeof transfer.adapter.abort === "function") {
            // Cleanup must never hold the transport open or leak adapter exceptions.
            Promise.resolve().then(() => transfer.adapter.abort()).catch(() => {});
        }
    }
    state.controller = {abort: dispose};
    transfer.cancel = () => {
        if (!state.result) state.result = {
            error: "Binary transfer aborted; dispatched writes may have completed",
            phase: "running", cancellation: "requested"
        };
        clearTimeout(state.timer);
        dispose();
        setTimeout(() => { delete window[config.key]; }, 2000);
    };
    transfer.busy = true;
    clearTimeout(transfer.idle_timer);
    state.timer = setTimeout(() => {
        state.result = {error: "Binary transfer timed out; dispatched writes may have completed",
            phase: "running", cancellation: "requested"};
        dispose();
        setTimeout(() => { delete window[config.key]; }, 2000);
    }, config.timeout * 1000);
    Promise.resolve().then(async () => {
        let result = {ok: true, offset: transfer.offset};
        if (config.action === "open") {
            const adapter = await ((0, eval)(config.javascript))(transfer.controller.signal);
            transfer.adapter = adapter;
            if (transfer.terminal) {
                if (adapter && typeof adapter.abort === "function") {
                    Promise.resolve().then(() => adapter.abort()).catch(() => {});
                }
                return;
            }
            const method = transfer.direction === "download" ? "read" : "write";
            if (!adapter || typeof adapter[method] !== "function" ||
                typeof adapter.close !== "function" || typeof adapter.abort !== "function") {
                throw new Error("invalid adapter");
            }
        } else if (config.action === "read") {
            if (transfer.direction !== "download") throw new Error("invalid direction");
            const max = Math.min(transfer.chunk_bytes, transfer.total_bytes - transfer.offset);
            const bytes = await transfer.adapter.read(max);
            if (transfer.terminal) return;
            if (bytes === null) {
                result.done = true;
                result.hex = "";
            } else {
                if (Object.prototype.toString.call(bytes) !== "[object Uint8Array]" || !ArrayBuffer.isView(bytes) || bytes.byteLength === 0 || bytes.byteLength > max) {
                    throw new Error("invalid binary chunk");
                }
                result.done = false;
                let hex = "";
                for (const byte of bytes) hex += byte.toString(16).padStart(2, "0");
                result.hex = hex;
                transfer.offset += bytes.byteLength;
            }
        } else if (config.action === "write") {
            if (transfer.direction !== "upload" || !/^(?:[0-9a-f]{2})+$/.test(config.hex) ||
                config.hex.length / 2 > transfer.chunk_bytes ||
                config.hex.length / 2 > transfer.total_bytes - transfer.offset) throw new Error("invalid binary chunk");
            const bytes = new Uint8Array(config.hex.length / 2);
            for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(config.hex.slice(i * 2, i * 2 + 2), 16);
            await transfer.adapter.write(bytes, transfer.offset);
            if (transfer.terminal) return;
            transfer.offset += bytes.byteLength;
        } else if (config.action === "finish") {
            await transfer.adapter.close();
            if (transfer.terminal) return;
            transfer.terminal = true;
            registry.delete(config.id);
        } else if (config.action === "abort") {
            dispose();
        } else {
            throw new Error("invalid binary operation");
        }
        if (state.result) return;
        result.next_offset = transfer.offset;
        state.result = result;
        delete state.controller;
        clearTimeout(state.timer);
        transfer.busy = false;
        transfer.cancel = null;
        if (!transfer.terminal) transfer.idle_timer = setTimeout(dispose, transfer.idle_timeout * 1000);
    }).catch(() => {
        if (!state.result) {
            state.result = {error: "Binary adapter failed or exceeded bounds; dispatched writes may have completed",
                phase: "completed", cancellation: "requested"};
            clearTimeout(state.timer);
        }
        dispose();
    }).finally(() => {
        setTimeout(() => { delete window[config.key]; }, 2000);
    });
    return "started";
}
