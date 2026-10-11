const {test} = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(require("node:path").join(__dirname,"../src/sources/safari_page_runner.js"),"utf8");
const resolver = fs.readFileSync(require("node:path").join(__dirname,"../src/sources/safari_document_target.js"),"utf8");

function fixture(options = {}) {
    let now = 0, polls = 0, writes = 0, aborted = 0;
    const page = {window:{__cider_document_identity:"original"}, location:{origin:"https://example.com"},clearTimeout(){}};
    const other = {window:{__cider_document_identity:"other"},location:{origin:"https://example.com"},clearTimeout(){}};
    let tabs = [page,other];
    const app = {running:()=>true, windows:()=> options.missing ? [] : [{id:()=>7,tabs:()=>tabs}],
        doJavaScript(code,{in:target}) {
        if (code === "start") {
            writes++;
            target.window.key = {result:null};
            if (options.request) target.window.key.controller={abort(){aborted++;}};
            return options.startup || "started";
        }
        if (code.startsWith("JSON.stringify((")) {
            polls++;
            if (polls === 1) {
                if (options.reorder) tabs.reverse();
                if (options.route) target.location.pathname="/new-route";
                if (options.replace) tabs=[other];
                if (options.origin) target.location.origin="https://evil.example";
            }
            if (polls === 2 && !options.timeout) target.window.key.result=options.error || {ok:true,value:42};
        }
        return vm.runInNewContext(code,target);
    }};
    const locate = vm.runInNewContext(`(${resolver})`)({window_id:7,document_id:"original",origin:"https://example.com"},app);
    const run = vm.runInNewContext(`(${source})`,{app,locate,Date:{now:()=>now},delay(){now+=100;}});
    const result = JSON.parse(run({timeout:1,key:'"key"',guard:'window.__cider_document_identity==="original" && location.origin==="https://example.com"',page:"start"}));
    return {result,writes,aborted,page,other};
}
test("same-origin SPA and reorder preserve original document without writing another tab",()=>{
    const f=fixture({reorder:true,route:true});
    assert.equal(f.result.value,42);
    assert.equal(f.writes,1);
    assert.equal(f.other.window.key,undefined);
    assert.equal(f.page.window.key,undefined);
});
test("replacement or cross-origin departure stops polling and cannot clean a different document",()=>{
    for(const options of [{replace:true},{origin:true}]) {
        const f=fixture(options);
        assert.equal(f.result.cider_operation_error.phase,"running");
        assert.equal(f.result.cider_operation_error.cancellation,"unknown");
        assert.equal(f.writes,1);
        assert.equal(f.other.window.key,undefined);
    }
});
test("before-dispatch selection failure performs no write",()=>{
    const f=fixture({missing:true});
    assert.equal(f.writes,0);
    assert.equal(f.result.cider_operation_error.phase,"before_dispatch");
    assert.equal(f.result.cider_operation_error.cancellation,"not_needed");
});
test("timeout distinguishes unsupported eval cancellation from requested fetch abort",()=>{
    assert.equal(fixture({timeout:true}).result.cider_operation_error.cancellation,"unsupported");
    const request=fixture({timeout:true,request:true});
    assert.equal(request.result.cider_operation_error.cancellation,"requested");
    assert.equal(request.aborted,1);
});
test("completed exception retains completion phase without implying rollback",()=>{
    const f=fixture({error:{error:"caller exception",phase:"completed",cancellation:"not_needed"}});
    assert.equal(f.result.cider_operation_error.phase,"completed");
    assert.equal(f.result.cider_operation_error.cancellation,"not_needed");
});
test("explicit startup refusal retains before-dispatch metadata",()=>{
    const f=fixture({startup:JSON.stringify({error:"Binary transfer handle mismatch",phase:"before_dispatch"})});
    assert.equal(f.result.cider_operation_error.phase,"before_dispatch");
    assert.equal(f.result.cider_operation_error.cancellation,"not_needed");
});
test("reload, window move, and frame-only markers cannot resolve a top-level document",()=>{
    const original={window:{__cider_document_identity:"original"},location:{origin:"https://example.com"}};
    const reloaded={window:{},location:{origin:"https://example.com"}};
    const frameOnly={window:{frames:[original.window]},location:{origin:"https://example.com"}};
    for (const windows of [
        [{id:()=>7,tabs:()=>[reloaded]}],
        [{id:()=>8,tabs:()=>[original]}],
        [{id:()=>7,tabs:()=>[frameOnly]}],
    ]) {
        const app={running:()=>true,windows:()=>windows,
            doJavaScript:(code,{in:target})=>vm.runInNewContext(code,target)};
        const locate=vm.runInNewContext(`(${resolver})`)({window_id:7,document_id:"original",origin:"https://example.com"},app);
        assert.throws(locate,/unavailable|changed/);
    }
});
