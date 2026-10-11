function(config) {
    const deadline = Date.now() + config.timeout * 1000 + 1000;
    let result;
    let phase = "before_dispatch";
    let cancellation = "not_needed";
    let failure;
    try {
        let target = locate();
        phase = "running";
        const started = app.doJavaScript(config.page, {in: target});
        if (started !== "started") {
            let message = "Safari page operation could not start";
            try { message = JSON.parse(started).error || message; } catch (error) {}
            throw new Error(message);
        }
        while (Date.now() < deadline) {
            target = locate();
            const raw = app.doJavaScript('JSON.stringify((' + config.guard + ') && window[' + config.key + '] && window[' + config.key + '].result || null)', {in: target});
            if (raw && raw !== "null") { result = JSON.parse(raw); break; }
            delay(0.1);
        }
        if (!result) throw new Error("Safari page operation timed out; caller JavaScript may continue and side effects are not rolled back");
        if (result.error) {
            phase = result.phase || "running";
            cancellation = result.cancellation || "unknown";
            throw new Error(result.error);
        }
        phase = "completed";
    } catch (error) {
        failure = {message:String(error.message || error), phase, cancellation};
    } finally {
        try {
            const target = locate();
            const cleanup = app.doJavaScript('(function(){if(!(' + config.guard + '))return "unknown";const s=window[' + config.key + '];if(!s)return "unknown";const status=s.controller?"requested":"unsupported";if(s.controller)s.controller.abort();clearTimeout(s.timer);delete window[' + config.key + '];return status;})()', {in: target});
            if (failure && failure.phase === "running" && failure.cancellation === "not_needed") failure.cancellation = cleanup;
        } catch (error) {
            if (failure && failure.phase !== "before_dispatch") failure.cancellation = "unknown";
        }
    }
    return JSON.stringify(failure ? {cider_operation_error:failure} : result);
}
