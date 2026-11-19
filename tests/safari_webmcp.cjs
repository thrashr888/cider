const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");
const vm = require("node:vm");
const source = fs.readFileSync(path.join(__dirname, "../src/sources/safari_webmcp.js"), "utf8");

async function discover(document, isSecureContext = true) {
    return JSON.parse(JSON.stringify(await vm.runInNewContext(source, { document, isSecureContext })));
}

test("metadata only, explicitly no other origins or invocation", async () => {
    let discoveryCalls = 0;
    const registry = {
        async getTools(...args) {
            assert.equal(this, registry);
            assert.deepEqual(args, []);
            ++discoveryCalls;
            return [{
                name: "lookup", description: "Look up an item",
                inputSchema: { type: "object", properties: { query: { type: "string" } } },
                execute() { assert.fail("do not call tools"); },
                extraPrivateData: "excluded",
            }];
        },
        executeTool() { assert.fail("do not call tools"); },
    };
    assert.deepEqual(await discover({ modelContext: registry }), {
        available: true, truncated: false,
        tools: [{
            name: "lookup", description: "Look up an item",
            input_schema: { type: "object", properties: { query: { type: "string" } } },
        }],
    });
    assert.equal(discoveryCalls, 1);
});

test("unavailable, empty, and partial evidence are distinct", async () => {
    const unavailable = { available: false, truncated: false, tools: [] };
    assert.deepEqual(await discover({}), unavailable);
    let insecureRead = false;
    assert.deepEqual(await discover({
        get modelContext() { insecureRead = true; return {}; },
    }, false), unavailable);
    assert.equal(insecureRead, false);
    assert.deepEqual(await discover({ modelContext: { async getTools() { return []; } } }), {
        available: true, truncated: false, tools: [],
    });
    assert.deepEqual(await discover({
        modelContext: { async getTools() { throw new Error("private page details"); } },
    }), unavailable);

    const valid = { name: "lookup", description: "Look up an item" };
    const reply = await discover({
        modelContext: { async getTools() { return [{ ...valid, inputSchema: { title: "x".repeat(33000) } },
            null, ...Array.from({ length: 100 }, () => valid)]; } },
    });
    assert.equal(reply.available, true);
    assert.equal(reply.truncated, true);
    assert.equal(reply.tools.length, 99);
    assert.deepEqual(reply.tools[0], valid);
});
