(async () => {
    const unavailable = { available: false, truncated: false, tools: [] };
    try {
        if (!globalThis.isSecureContext) return unavailable;
        const registry = document.modelContext;
        if (!registry || typeof registry.getTools !== "function") return unavailable;
        // Do not request other origins, invoke tools, or pass user input into the page.
        const entries = await registry.getTools();
        if (!Array.isArray(entries)) return unavailable;
        const tools = [];
        let truncated = entries.length > 100;
        for (let index = 0; index < 100 && index < entries.length; ++index) {
            const { name, description, inputSchema } = entries[index] || {};
            if (typeof name !== "string" || !name.trim() || name.length > 4096
                || typeof description !== "string" || !description.trim() || description.length > 4096) {
                truncated = true;
                continue;
            }
            const tool = { name, description };
            if (inputSchema && typeof inputSchema === "object" && !Array.isArray(inputSchema)) {
                // Detach metadata so the eval transport does not run a
                // page-provided schema's toJSON/getters outside this guard.
                const serialized = JSON.stringify(inputSchema);
                if (typeof serialized !== "string" || serialized.length > 32768) {
                    truncated = true;
                } else {
                    const schema = JSON.parse(serialized);
                    if (schema && typeof schema === "object" && !Array.isArray(schema)) {
                        tool.input_schema = schema;
                    } else {
                        truncated = true;
                    }
                }
            }
            tools.push(tool);
        }
        return { available: true, truncated, tools };
    } catch {
        return unavailable;
    }
})()
