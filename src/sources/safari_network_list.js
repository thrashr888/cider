function(config) {
    if (!window.performance || typeof performance.getEntriesByType !== "function")
        throw new Error("Resource Timing is unavailable in this document");
    const entries = performance.getEntriesByType("resource")
        .filter(e => !config.filter || e.name.includes(config.filter));
    return JSON.stringify({backend: "resource_timing", coverage: "retained_resource_timing",
        complete_history: false, headers_available: false, bodies_available: false,
        url: location.href, matched_count: entries.length,
        requests_truncated: entries.length > config.limit,
        requests: entries.slice(0, config.limit).map(e => ({
            url: e.name, initiator_type: e.initiatorType || "",
            start_time_ms: e.startTime, duration_ms: e.duration,
            transfer_size_bytes: e.transferSize || 0, encoded_body_size_bytes: e.encodedBodySize || 0
        })),
        limitations: "Partial document snapshot, not full historical HTTP traffic. The page can clear or overflow its timing buffer. Empty does not mean no requests. Cached/cross-origin sizes may be zero. No methods, statuses, request headers, or response bodies. Use monitor for future fetch/XHR; Cider native network capture owns a separate automation tab/session, not this regular tab's history."
    });
}
