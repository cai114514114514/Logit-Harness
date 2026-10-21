"""Author the small controlled corpus as reviewable files; stdout is an apply_patch document.

This script is not the evaluator. Frozen task/fixture files, rather than this authoring helper,
are the experiment's authority. Expected results below are fixed before any model trajectory.
"""
import json

TASKS = []
FILES = {}


def check(identity, group, data, expected, *, before=None, public=False):
    item = {"id": identity, "group": group, "input": {"op": "solve", "data": data}, "expected": expected, "public": public}
    if before is not None:
        item["before"] = before
    return item


def add(identity, language, kind, title, requirements, correction, checks, starter=None):
    split = "development" if len(TASKS) < 12 else "holdout"
    task = {
        "id": identity, "language": language, "kind": kind, "split": split, "title": title,
        "trigger": "active_check" if len(TASKS) % 2 else "first_write",
        "requirements": requirements,
        "correction": correction + " 调用者已更新 verify.py；不要修改该文件，保留其余要求和 NOTES.txt。",
        "notes": f"Caller notes for {identity}: preserve this file byte for byte.\n",
        "checks": checks,
    }
    TASKS.append(task)
    FILES[f"eval/corpus/tasks/{identity}.json"] = json.dumps(task, ensure_ascii=False, indent=2) + "\n"
    if starter is not None:
        root = f"eval/corpus/fixtures/{identity}/"
        if language == "python":
            FILES[root + "core.py"] = starter
            FILES[root + "main.py"] = f'''import json
import sys
from core import solve

for line in sys.stdin:
    request = json.loads(line)
    answer = {{"name": {identity!r}, "version": 1}} if request.get("op") == "about" else solve(request["data"])
    print(json.dumps(answer, ensure_ascii=False))
'''
        elif language == "typescript":
            FILES[root + "core.ts"] = starter
            FILES[root + "main.ts"] = f'''import readline from "node:readline";
import {{ solve }} from "./core.ts";

const input = readline.createInterface({{ input: process.stdin }});
for await (const line of input) {{
  const request = JSON.parse(line);
  const answer = request.op === "about" ? {{name: {json.dumps(identity)}, version: 1}} : solve(request.data);
  console.log(JSON.stringify(answer));
}}
'''
        else:
            FILES[root + "Cargo.toml"] = '[package]\nname = "task"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\nserde_json = "1"\n'
            FILES[root + "src/core.rs"] = starter
            FILES[root + "src/main.rs"] = f'''mod core;
use std::io::{{self, BufRead}};
use serde_json::{{Value, json}};

fn main() {{
    for line in io::stdin().lock().lines() {{
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let answer = if request["op"] == "about" {{ json!({{"name": "{identity}", "version": 1}}) }} else {{ core::solve(&request["data"]) }};
        println!("{{}}", answer);
    }}
}}
'''


# Development: two new and two existing projects per language.
add("dev-ledger", "python", "new", "Inventory ledger",
    ["data 是整数数组，按顺序求库存余额，返回每步余额数组。初始余额为 0。", "初始规则允许余额为负数；空数组返回空数组。"],
    "改为拒绝会使余额为负数的单笔变动：该步返回字符串 rejected，余额保持不变，继续处理后续变动。",
    [check("deposits", "core", [3, 4], [3, 7], public=True), check("withdraw", "core", [5, -2], [5, 3]), check("overdraw", "correction", [2, -3, 4], [2, "rejected", 6], before=[2, -1, 3], public=True), check("negative-first", "correction", [-1, 2], ["rejected", 2], before=[-1, 1]), check("empty", "edge", [], []), check("zero", "edge", [0, 1, -1], [0, 1, 0])])

add("dev-slug", "python", "existing", "Slug normalization",
    ["data 是字符串：转为小写，把非 ASCII 字母或数字的连续片段替换成一个下划线，去掉首尾分隔符。", "返回 slug 字符串，允许空结果。修复当前实现的连续分隔符问题。"],
    "分隔符从下划线改为连字符，其余规范化规则不变。",
    [check("plain", "core", "ABC123", "abc123", public=True), check("digits", "core", "42", "42"), check("spaces", "correction", "  Hi!!! World  ", "hi-world", before="hi_world", public=True), check("unicode", "correction", "你好Hello_世界X", "hello-x", before="hello_x"), check("empty", "edge", "", ""), check("punctuation", "edge", "!!!", "")],
    "import re\n\ndef solve(data):\n    return re.sub(r'[^a-z0-9]', '_', data.lower()).strip('_')\n")

add("dev-sales", "python", "new", "Daily sales totals",
    ["data 是含 day 字符串和 amount 整数的记录数组；按 day 汇总 amount，返回按 day 升序排列的 [day,total] 数组。", "初始规则保留汇总为零的日期；空输入返回空数组。"],
    "删除汇总金额恰为零的日期，负数汇总仍保留。",
    [check("sum", "core", [{"day":"b","amount":2},{"day":"a","amount":3},{"day":"b","amount":4}], [["a",3],["b",6]], public=True), check("negative", "core", [{"day":"a","amount":-2}], [["a",-2]]), check("cancel", "correction", [{"day":"a","amount":3},{"day":"a","amount":-3}], [], before=[["a",0]], public=True), check("mixed", "correction", [{"day":"b","amount":0},{"day":"a","amount":1}], [["a",1]], before=[["a",1],["b",0]]), check("empty", "edge", [], []), check("large", "edge", [{"day":"a","amount":1000000},{"day":"a","amount":1}], [["a",1000001]])])

add("dev-cache", "python", "existing", "TTL cache simulation",
    ["data 含 ttl 非负整数和 ops 数组。操作为 [set,key,value,time] 或 [get,key,time]。仅 get 产生输出；缺失返回 null。每条输入独立从空缓存开始。", "修复键缺失时抛异常的问题。初始规则 age > ttl 才过期。"],
    "过期边界改为 age >= ttl；设置值不会产生输出，其他行为不变。",
    [check("hit", "core", {"ttl":5,"ops":[["set","a",7,0],["get","a",4]]}, [7], public=True), check("missing", "core", {"ttl":5,"ops":[["get","x",0]]}, [None]), check("boundary", "correction", {"ttl":5,"ops":[["set","a",1,0],["get","a",5]]}, [None], before=[1], public=True), check("zero-ttl", "correction", {"ttl":0,"ops":[["set","a",2,3],["get","a",3]]}, [None], before=[2]), check("overwrite", "edge", {"ttl":4,"ops":[["set","a",1,0],["set","a",3,4],["get","a",6]]}, [3]), check("empty", "edge", {"ttl":3,"ops":[]}, [])],
    "def solve(data):\n    cache, output = {}, []\n    for op in data['ops']:\n        if op[0] == 'set':\n            cache[op[1]] = (op[2], op[3])\n        else:\n            value, time = cache[op[1]]\n            output.append(value if op[2] - time <= data['ttl'] else None)\n    return output\n")

add("dev-intervals", "rust", "new", "Interval union",
    ["data 是 [start,end] 整数闭区间数组，start <= end。按起点终点排序并合并重叠区间，返回区间数组。", "初始规则也合并端点恰好相接的区间。"],
    "只合并真正重叠的区间；前一区间 end 等于后一区间 start 时保持分开。",
    [check("overlap", "core", [[3,8],[1,5]], [[1,8]], public=True), check("disjoint", "core", [[8,9],[1,2]], [[1,2],[8,9]]), check("touch", "correction", [[1,3],[3,5]], [[1,3],[3,5]], before=[[1,5]], public=True), check("chain", "correction", [[0,2],[1,3],[3,8]], [[0,3],[3,8]], before=[[0,8]]), check("empty", "edge", [], []), check("negative", "edge", [[-4,-1],[-3,0]], [[-4,0]])])

add("dev-priority", "rust", "existing", "Stable priority ordering",
    ["data 是 {id,priority} 记录数组。初始按 priority 从大到小排序并返回 id 数组，相同优先级保持输入次序。", "修复当前实现把同优先级项目按 id 排序的问题。"],
    "优先级方向改为从小到大，相同优先级仍保持输入次序。",
    [check("ties", "core", [{"id":"z","priority":2},{"id":"a","priority":2}], ["z","a"], public=True), check("one", "core", [{"id":"x","priority":1}], ["x"]), check("order", "correction", [{"id":"a","priority":3},{"id":"b","priority":1}], ["b","a"], before=["a","b"], public=True), check("negative", "correction", [{"id":"a","priority":0},{"id":"b","priority":-1},{"id":"c","priority":2}], ["b","a","c"], before=["c","a","b"]), check("empty", "edge", [], []), check("zero-ties", "edge", [{"id":"b","priority":0},{"id":"a","priority":0}], ["b","a"])],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value {\n    let mut rows=data.as_array().unwrap().clone();\n    rows.sort_by(|a,b| b["priority"].as_i64().cmp(&a["priority"].as_i64()).then(a["id"].as_str().cmp(&b["id"].as_str())));\n    json!(rows.iter().map(|r|r["id"].clone()).collect::<Vec<_>>())\n}\n')

add("dev-checkout", "rust", "new", "Integer checkout",
    ["data 含 cents、discount、tax，均为非负整数，discount 不超过 cents，tax 是整数百分比。税额用整数运算向下取整。", "初始规则：先按原价计税，再减固定折扣，返回最终 cents 整数。"],
    "改为先减固定折扣，再对折后金额计税。",
    [check("no-tax", "core", {"cents":100,"discount":20,"tax":0}, 80, public=True), check("no-discount", "core", {"cents":100,"discount":0,"tax":10}, 110), check("discount-tax", "correction", {"cents":1000,"discount":200,"tax":10}, 880, before=900, public=True), check("round", "correction", {"cents":101,"discount":50,"tax":10}, 56, before=61), check("zero", "edge", {"cents":0,"discount":0,"tax":10}, 0), check("free", "edge", {"cents":99,"discount":99,"tax":0}, 0)])

add("dev-decoder", "rust", "existing", "Percent decoding",
    ["data 是 ASCII 字符串。把合法 %HH 十六进制字节解码，输出有效 UTF-8 字符串；无效转义或无效 UTF-8 返回 {error:invalid}。", "初始规则把 + 转为空格；修复无效转义被当成普通文本保留的问题。"],
    "+ 改为保持字面加号，其余解码规则不变。",
    [check("encoded", "core", "A%20B", "A B", public=True), check("utf8", "core", "%E4%B8%AD", "中"), check("plus", "correction", "A+B", "A+B", before="A B", public=True), check("mixed", "correction", "%2B+", "++", before="+ "), check("invalid", "edge", "%Q0", {"error":"invalid"}), check("short", "edge", "%", {"error":"invalid"})],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value {\n let text=data.as_str().unwrap(); let raw=text.as_bytes(); let mut out=Vec::new(); let mut i=0;\n while i<raw.len() { if raw[i]==b\'%\' && i+2<raw.len() { if let Ok(n)=u8::from_str_radix(&text[i+1..i+3],16) {out.push(n);i+=3;continue;} } out.push(if raw[i]==b\'+\' {b\' \'} else {raw[i]});i+=1; }\n match String::from_utf8(out) { Ok(s)=>json!(s),Err(_)=>json!({"error":"invalid"}) }\n}\n')

add("dev-todos", "typescript", "new", "Todo command interpreter",
    ["data 是操作数组：[add,title] 添加未完成任务，[toggle,id] 切换已有任务，[list] 输出当前任务数组。仅 list 产生结果，任务含 id、title、done。", "初始 id 是从 1 递增的整数；不存在的 toggle 忽略。每条输入从空列表开始。"],
    "id 改为字符串 t1、t2…，toggle 也用字符串 id。",
    [check("empty-list", "core", [["list"]], [[]], public=True), check("no-list", "core", [["add","x"]], []), check("add", "correction", [["add","A"],["list"]], [[{"id":"t1","title":"A","done":False}]], before=[[{"id":1,"title":"A","done":False}]], public=True), check("toggle", "correction", [["add","A"],["toggle","t1"],["list"]], [[{"id":"t1","title":"A","done":True}]], before=[[{"id":1,"title":"A","done":False}]]), check("empty", "edge", [], []), check("unknown", "edge", [["toggle","none"],["list"]], [[]])])

add("dev-acl", "typescript", "existing", "Access-rule precedence",
    ["data 含 path 字符串和 rules 数组，每条规则含 prefix 和 allow 布尔值。匹配前缀；初始采用第一条匹配规则，缺省拒绝。", "修复当前实现总是允许没有匹配规则的请求。"],
    "改为最长匹配前缀优先，长度相同时仍采用输入中第一条。",
    [check("single", "core", {"path":"/a/x","rules":[{"prefix":"/a","allow":True}]}, True, public=True), check("missing", "core", {"path":"/x","rules":[]}, False), check("specific", "correction", {"path":"/a/private/x","rules":[{"prefix":"/a","allow":True},{"prefix":"/a/private","allow":False}]}, False, before=True, public=True), check("specific-allow", "correction", {"path":"/a/b","rules":[{"prefix":"/","allow":False},{"prefix":"/a","allow":True}]}, True, before=False), check("tie", "edge", {"path":"x","rules":[{"prefix":"x","allow":False},{"prefix":"x","allow":True}]}, False), check("empty-prefix", "edge", {"path":"abc","rules":[{"prefix":"","allow":False}]}, False)],
    'export function solve(data: any): boolean {\n  const rule = data.rules.find((rule: any) => data.path.startsWith(rule.prefix));\n  return rule ? rule.allow : true;\n}\n')

add("dev-schedule", "typescript", "new", "Dependency ordering",
    ["data 是 {id,deps} 数组，deps 引用输入中的 id。返回拓扑序；每次从就绪节点选 id 字典序最小者。循环返回 {error:cycle}。", "输入 id 唯一，空图返回空数组。"],
    "同时就绪的节点改为选输入次序最靠前者，不再按 id 排序。",
    [check("chain", "core", [{"id":"a","deps":[]},{"id":"b","deps":["a"]}], ["a","b"], public=True), check("cycle", "core", [{"id":"a","deps":["b"]},{"id":"b","deps":["a"]}], {"error":"cycle"}), check("ties", "correction", [{"id":"z","deps":[]},{"id":"a","deps":[]}], ["z","a"], before=["a","z"], public=True), check("unlock", "correction", [{"id":"z","deps":["a"]},{"id":"b","deps":[]},{"id":"a","deps":[]}], ["b","a","z"], before=["a","b","z"]), check("empty", "edge", [], []), check("self", "edge", [{"id":"a","deps":["a"]}], {"error":"cycle"})])

add("dev-pages", "typescript", "existing", "Cursor pagination",
    ["data 含升序唯一整数 items、整数 cursor 和非负整数 limit。初始选 items 中 >= cursor 的前 limit 项。", "修复 limit=0 时返回全部数据的问题。"],
    "cursor 改为排他边界，仅选择 > cursor 的项目。",
    [check("between", "core", {"items":[1,3,5],"cursor":2,"limit":2}, [3,5], public=True), check("zero", "core", {"items":[1,2],"cursor":0,"limit":0}, []), check("boundary", "correction", {"items":[1,2,3],"cursor":2,"limit":2}, [3], before=[2,3], public=True), check("last", "correction", {"items":[1,2],"cursor":2,"limit":4}, [], before=[2]), check("empty", "edge", {"items":[],"cursor":0,"limit":1}, []), check("negative", "edge", {"items":[-4,-2,0],"cursor":-3,"limit":9}, [-2,0])],
    'export function solve(data: any): number[] {\n  const rows = data.items.filter((value: number) => value >= data.cursor);\n  return data.limit ? rows.slice(0, data.limit) : rows;\n}\n')

# Held-out Python projects.
add("hold-layers", "python", "new", "Layered configuration",
    ["data 是对象数组，依次递归合并对象。非对象值覆盖旧值，数组整体覆盖。初始规则 null 删除该键。", "返回合并后的对象，不修改输入对象。"],
    "null 不再删除键，而是作为普通值保留。",
    [check("nested", "core", [{"a":{"x":1}},{"a":{"y":2}}], {"a":{"x":1,"y":2}}, public=True), check("array", "core", [{"a":[1]},{"a":[2,3]}], {"a":[2,3]}), check("null", "correction", [{"a":1},{"a":None}], {"a":None}, before={}, public=True), check("nested-null", "correction", [{"a":{"x":1}},{"a":{"x":None}}], {"a":{"x":None}}, before={"a":{}}), check("empty", "edge", [], {}), check("replace", "edge", [{"a":1},{"a":{"x":2}}], {"a":{"x":2}})])

add("hold-dedup", "python", "new", "Record deduplication",
    ["data 是含字符串 key 和任意 JSON value 的记录数组；key 用 Python casefold 规范化后去重。初始保留每组第一条记录。", "输出原始记录；各组顺序由规范化 key 首次出现位置决定。"],
    "每组改为保留最后一条记录，但各组的输出顺序仍由首次出现位置决定。",
    [check("distinct", "core", [{"key":"A","value":1},{"key":"B","value":2}], [{"key":"A","value":1},{"key":"B","value":2}], public=True), check("one", "core", [{"key":"x","value":None}], [{"key":"x","value":None}]), check("duplicate", "correction", [{"key":"A","value":1},{"key":"a","value":2}], [{"key":"a","value":2}], before=[{"key":"A","value":1}], public=True), check("ordering", "correction", [{"key":"B","value":1},{"key":"A","value":2},{"key":"b","value":3}], [{"key":"b","value":3},{"key":"A","value":2}], before=[{"key":"B","value":1},{"key":"A","value":2}]), check("empty", "edge", [], []), check("unicode", "edge", [{"key":"Straße","value":1},{"key":"STRASSE","value":1}], [{"key":"STRASSE","value":1}], before=[{"key":"Straße","value":1}])])

add("hold-limit", "python", "new", "Sliding-window admission",
    ["data 含递增 times 整数数组、正整数 window 和正整数 limit。依次决定每次请求是否接纳，返回布尔数组；只有获准请求占额度。", "初始窗口保留 age <= window 的历史请求。"],
    "窗口改为只保留 age < window 的历史请求，恰好到边界的请求释放额度。",
    [check("within", "core", {"times":[0,1,2],"window":10,"limit":2}, [True,True,False], public=True), check("gap", "core", {"times":[0,20],"window":10,"limit":1}, [True,True]), check("boundary", "correction", {"times":[0,10],"window":10,"limit":1}, [True,True], before=[True,False], public=True), check("repeat-boundary", "correction", {"times":[0,5,10],"window":5,"limit":1}, [True,True,True], before=[True,False,True]), check("empty", "edge", {"times":[],"window":2,"limit":1}, []), check("same", "edge", {"times":[0,0,0],"window":1,"limit":2}, [True,True,False])])

add("hold-allocation", "python", "new", "Weighted integer allocation",
    ["data 含非负整数 total 和正整数 weights 数组；按比例分配整数，先向下取整，剩余单位初始按索引从小到大轮流分配。", "返回分配数组，总和等于 total；weights 非空。"],
    "剩余单位改为按分数余数从大到小分配，余数相同按索引升序。每个位置最多补一单位。",
    [check("exact", "core", {"total":10,"weights":[1,1]}, [5,5], public=True), check("one", "core", {"total":7,"weights":[9]}, [7]), check("remainder", "correction", {"total":2,"weights":[1,2]}, [1,1], before=[1,1]), check("priority", "correction", {"total":1,"weights":[1,3]}, [0,1], before=[1,0], public=True), check("zero", "edge", {"total":0,"weights":[1,2]}, [0,0]), check("tie", "edge", {"total":1,"weights":[1,1]}, [1,0])])

add("hold-paths", "python", "existing", "Root-relative path normalization",
    ["data 是路径字符串，忽略重复 / 和 .，处理 ..，返回以 / 开头的规范化路径。", "初始规则遇到越过根的 .. 时忽略该段；修复空路径应返回 / 的问题。"],
    "任何试图越过根的 .. 都返回 {error:escape}，不能继续归一化。",
    [check("normal", "core", "a//b/../c", "/a/c", public=True), check("empty", "core", "", "/"), check("escape", "correction", "../a", {"error":"escape"}, before="/a", public=True), check("later", "correction", "a/../../b", {"error":"escape"}, before="/b"), check("root", "edge", "/./", "/"), check("literal", "edge", ".../x", "/.../x")],
    "def solve(data):\n    parts = []\n    for item in data.split('/'):\n        if item in ('', '.'): continue\n        if item == '..':\n            if parts: parts.pop()\n        else: parts.append(item)\n    return '/' + '/'.join(parts) if parts else ''\n")

add("hold-backoff", "python", "existing", "Retry delay schedule",
    ["data 含非负整数 base、cap、attempts。输出 attempts 个延迟，公式 min(cap, base*2**i)，i 从 0 开始。", "修复 attempts=0 的结果应为空数组，而不是一项。"],
    "指数因子从 2 改为 3，cap 和 attempts 语义不变。",
    [check("one", "core", {"base":2,"cap":100,"attempts":1}, [2], public=True), check("zero-attempts", "core", {"base":2,"cap":10,"attempts":0}, []), check("growth", "correction", {"base":2,"cap":100,"attempts":4}, [2,6,18,54], before=[2,4,8,16], public=True), check("cap", "correction", {"base":3,"cap":10,"attempts":4}, [3,9,10,10], before=[3,6,10,10]), check("zero-base", "edge", {"base":0,"cap":10,"attempts":3}, [0,0,0]), check("below-base", "edge", {"base":10,"cap":2,"attempts":2}, [2,2])],
    "def solve(data):\n    return [min(data['cap'], data['base'] * 2 ** i) for i in range(max(1, data['attempts']))]\n")

add("hold-join", "python", "existing", "Stable keyed join",
    ["data 含 left 和 right 记录数组，每项含 key 和 value。初始执行内连接，返回 [key,leftValue,rightValue] 数组，左侧顺序优先，右侧顺序次之。", "修复重复右键只保留一条的问题，重复键必须形成笛卡尔积。"],
    "改为左连接：没有匹配的左项也输出一条，其 rightValue 为 null。",
    [check("match", "core", {"left":[{"key":"a","value":1}],"right":[{"key":"a","value":2}]}, [["a",1,2]], public=True), check("duplicates", "core", {"left":[{"key":"a","value":1}],"right":[{"key":"a","value":2},{"key":"a","value":3}]}, [["a",1,2],["a",1,3]]), check("missing", "correction", {"left":[{"key":"x","value":1}],"right":[]}, [["x",1,None]], before=[], public=True), check("mixed", "correction", {"left":[{"key":"b","value":1},{"key":"a","value":2}],"right":[{"key":"a","value":3}]}, [["b",1,None],["a",2,3]], before=[["a",2,3]]), check("empty", "edge", {"left":[],"right":[]}, []), check("null-value", "edge", {"left":[{"key":1,"value":None}],"right":[{"key":1,"value":None}]}, [[1,None,None]])],
    "def solve(data):\n    right = {item['key']: item['value'] for item in data['right']}\n    return [[item['key'], item['value'], right[item['key']]] for item in data['left'] if item['key'] in right]\n")

add("hold-brackets", "python", "existing", "Delimiter validation",
    ["data 是字符串，检查 () [] {} 是否正确嵌套。初始忽略双引号字符串内的括号，支持反斜杠转义；未闭合字符串返回 false。", "修复当前实现不区分括号种类的问题。"],
    "增加忽略 // 行注释；注释在换行结束。字符串中的 // 不是注释。",
    [check("balanced", "core", "[{}()]", True, public=True), check("mismatch", "core", "([)]", False), check("comment", "correction", "() // [", True, before=False, public=True), check("next-line", "correction", "// }\n[]", True, before=False), check("quoted", "edge", '"[//]"{}', True), check("unclosed", "edge", '"abc', False)],
    "def solve(data):\n    depth, quoted, escaped = 0, False, False\n    for ch in data:\n        if quoted:\n            if escaped: escaped = False\n            elif ch == '\\\\': escaped = True\n            elif ch == '\"': quoted = False\n        elif ch == '\"': quoted = True\n        elif ch in '([{': depth += 1\n        elif ch in ')]}':\n            depth -= 1\n            if depth < 0: return False\n    return depth == 0 and not quoted\n")

# Held-out Rust projects.
add("hold-runs", "rust", "new", "Unicode run encoding",
    ["data 是字符串，按 Unicode 标量值进行游程编码，返回 [字符,次数] 数组。初始每个连续游程只输出一项。", "空字符串返回空数组，不能按 UTF-8 字节拆分字符。"],
    "单项次数最多为 3；更长的游程拆为相邻的多项，前面的项尽量填满。",
    [check("short", "core", "aab", [["a",2],["b",1]], public=True), check("unicode", "core", "中中a", [["中",2],["a",1]]), check("split", "correction", "aaaa", [["a",3],["a",1]], before=[["a",4]], public=True), check("six", "correction", "xxxxxx", [["x",3],["x",3]], before=[["x",6]]), check("empty", "edge", "", []), check("emoji", "edge", "😀😀", [["😀",2]])])

add("hold-reach", "rust", "new", "Graph reachability",
    ["data 含 edges 有向边数组和 start 字符串。返回可达节点的广度优先访问顺序，含 start；邻居初始按字典序遍历。", "重复边和环不能导致重复输出。"],
    "邻居改为按边首次出现在输入中的次序遍历，其余 BFS 规则不变。",
    [check("chain", "core", {"edges":[["a","b"],["b","c"]],"start":"a"}, ["a","b","c"], public=True), check("cycle", "core", {"edges":[["a","b"],["b","a"]],"start":"a"}, ["a","b"]), check("neighbors", "correction", {"edges":[["a","z"],["a","b"]],"start":"a"}, ["a","z","b"], before=["a","b","z"], public=True), check("layers", "correction", {"edges":[["a","z"],["a","b"],["z","x"],["b","y"]],"start":"a"}, ["a","z","b","x","y"], before=["a","b","z","y","x"]), check("alone", "edge", {"edges":[],"start":"a"}, ["a"]), check("duplicate", "edge", {"edges":[["a","b"],["a","b"]],"start":"a"}, ["a","b"])])

add("hold-slice", "rust", "new", "Offset slicing",
    ["data 含 items 数组、整数 offset 和非负整数 limit。返回从 offset 起最多 limit 个元素；初始负 offset 按 0 处理。", "offset 超出长度或 limit=0 返回空数组，不得整数溢出。"],
    "负 offset 不再截断到 0，而是返回 {error:offset}。",
    [check("normal", "core", {"items":[1,2,3],"offset":1,"limit":2}, [2,3], public=True), check("zero-limit", "core", {"items":[1],"offset":0,"limit":0}, []), check("negative", "correction", {"items":[1,2],"offset":-1,"limit":1}, {"error":"offset"}, before=[1], public=True), check("negative-empty", "correction", {"items":[],"offset":-9,"limit":3}, {"error":"offset"}, before=[]), check("beyond", "edge", {"items":[1],"offset":999999,"limit":999999}, []), check("empty", "edge", {"items":[],"offset":0,"limit":2}, [])])

add("hold-rotate", "rust", "new", "Rectangular matrix rotation",
    ["data 是矩形二维整数数组。初始顺时针旋转 90 度并返回新矩阵。", "空矩阵返回空数组，非空矩阵每行至少一项。"],
    "方向改为逆时针旋转 90 度；仍支持非正方形矩阵。",
    [check("single", "core", [[7]], [[7]], public=True), check("equal", "core", [[1,1],[1,1]], [[1,1],[1,1]]), check("square", "correction", [[1,2],[3,4]], [[2,4],[1,3]], before=[[3,1],[4,2]], public=True), check("rectangle", "correction", [[1,2,3],[4,5,6]], [[3,6],[2,5],[1,4]], before=[[4,1],[5,2],[6,3]]), check("empty", "edge", [], []), check("column", "edge", [[1],[2]], [[1,2]], before=[[2,1]])])

add("hold-dates", "rust", "existing", "Civil date validation",
    ["data 是 YYYY-MM-DD 字符串，年份范围 0001..9999；严格验证格式和公历闰年。无效输入返回 null。", "有效输入初始原样返回，修复把所有能被 4 整除的年份当作闰年的问题。"],
    "有效日期改为返回 DD/MM/YYYY，验证规则不变。",
    [check("invalid-century", "core", "1900-02-29", None, public=True), check("invalid-day", "core", "2026-04-31", None), check("ordinary", "correction", "2026-10-22", "22/10/2026", before="2026-10-22", public=True), check("leap", "correction", "2000-02-29", "29/02/2000", before="2000-02-29"), check("format", "edge", "2026-1-01", None), check("year-zero", "edge", "0000-01-01", None)],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value {\n let s=data.as_str().unwrap(); let parts:Vec<_>=s.split(\'-\').collect();\n if parts.len()!=3 || parts[0].len()!=4 || parts[1].len()!=2 || parts[2].len()!=2 {return Value::Null;}\n let nums:Option<Vec<u32>>=parts.iter().map(|p|p.parse().ok()).collect();let Some(n)=nums else{return Value::Null;};\n let (y,m,d)=(n[0],n[1],n[2]);if y==0 || !(1..=12).contains(&m){return Value::Null;}\n let days=[31,if y%4==0{29}else{28},31,30,31,30,31,31,30,31,30,31];\n if d==0 || d>days[(m-1)as usize]{Value::Null}else{json!(s)}\n}\n')

add("hold-versions", "rust", "existing", "Numeric version comparison",
    ["data 是两个点分非负整数版本字符串数组。比较数字段，返回 -1、0 或 1；初始相同前缀时较短版本更小。", "修复按字符串比较数字段的问题，不包含 prerelease 或 metadata。"],
    "末尾缺省段视为 0，因此 1、1.0、1.0.0 等价。",
    [check("numeric", "core", ["1.10","1.2"], 1, public=True), check("different", "core", ["2","3"], -1), check("zero-tail", "correction", ["1","1.0"], 0, before=-1, public=True), check("long-zero", "correction", ["2.0.0","2"], 0, before=1), check("equal", "edge", ["0","0"], 0), check("nonzero-tail", "edge", ["1","1.0.1"], -1)],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value {\n let a:Vec<_>=data[0].as_str().unwrap().split(\'.\').collect();let b:Vec<_>=data[1].as_str().unwrap().split(\'.\').collect();\n json!(match a.cmp(&b){std::cmp::Ordering::Less=>-1,std::cmp::Ordering::Equal=>0,std::cmp::Ordering::Greater=>1})\n}\n')

add("hold-reservations", "rust", "existing", "Atomic reservation batches",
    ["data 含 stock 非负整数和 batches 二维非负整数数组。每批申请依次扣库存；有一项不足则整批回滚并输出 rejected，否则输出剩余库存。", "修复当前实现在拒绝批次后仍保留部分扣减的问题。"],
    "拒绝后不再回滚整批，而是跳过不足的单项、继续批内其余项；每批始终输出剩余库存，不再输出 rejected。",
    [check("enough", "core", {"stock":10,"batches":[[2,3],[1]]}, [5,4], public=True), check("empty-batch", "core", {"stock":4,"batches":[[]]}, [4]), check("partial", "correction", {"stock":5,"batches":[[2,9,1]]}, [2], before=["rejected"], public=True), check("continue", "correction", {"stock":5,"batches":[[2,8],[3]]}, [3,0], before=["rejected",2]), check("empty", "edge", {"stock":0,"batches":[]}, []), check("zero", "edge", {"stock":0,"batches":[[0]]}, [0])],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value {\n let mut stock=data["stock"].as_i64().unwrap();let mut out=Vec::new();\n for batch in data["batches"].as_array().unwrap(){let mut ok=true;for item in batch.as_array().unwrap(){let n=item.as_i64().unwrap();if n>stock{ok=false;break;}stock-=n;}out.push(if ok{json!(stock)}else{json!("rejected")});}json!(out)\n}\n')

add("hold-glob", "rust", "existing", "Wildcard matching",
    ["data 含 pattern 和 text，仅含 ASCII；? 匹配任意一个字符，* 匹配任意长度（含零）的字符序列，全串匹配。", "初始大小写敏感。修复当前实现不支持 * 的问题；没有转义语法。"],
    "ASCII 字母改为忽略大小写比较，? 和 * 的语义不变。",
    [check("star", "core", {"pattern":"a*c","text":"abbbc"}, True, public=True), check("question", "core", {"pattern":"a?c","text":"abc"}, True), check("case", "correction", {"pattern":"HELLO","text":"hello"}, True, before=False, public=True), check("mixed", "correction", {"pattern":"A*Z","text":"abz"}, True, before=False), check("empty", "edge", {"pattern":"*","text":""}, True), check("mismatch", "edge", {"pattern":"a?","text":"a"}, False)],
    'use serde_json::{Value,json};\npub fn solve(data:&Value)->Value{let p=data["pattern"].as_str().unwrap().as_bytes();let t=data["text"].as_str().unwrap().as_bytes();json!(p.len()==t.len() && p.iter().zip(t).all(|(a,b)|*a==b\'?\' || a==b))}\n')

# Held-out TypeScript projects.
add("hold-tree", "typescript", "new", "Tree flattening",
    ["data 是 {name,children?} 树。按前序遍历输出每个节点的路径字符串数组，名称用 / 连接。", "初始输出全部节点，名称不含 /，缺少 children 视为空数组。"],
    "改为只输出叶节点路径；路径仍包含所有祖先名称。",
    [check("leaf", "core", {"name":"x"}, ["x"], public=True), check("empty-child", "core", {"name":"x","children":[]}, ["x"]), check("branch", "correction", {"name":"a","children":[{"name":"b"},{"name":"c"}]}, ["a/b","a/c"], before=["a","a/b","a/c"], public=True), check("deep", "correction", {"name":"a","children":[{"name":"b","children":[{"name":"c"}]}]}, ["a/b/c"], before=["a","a/b","a/b/c"]), check("unicode", "edge", {"name":"中"}, ["中"]), check("empty-name", "edge", {"name":""}, [""])])

add("hold-buckets", "typescript", "new", "Time-bucket aggregation",
    ["data 含 events 数组（每项 {time,value} 整数）和正整数 width。按 floor(time/width)*width 桶起点汇总 value，返回按桶起点升序的 [start,sum] 数组。", "初始计算忽略 data.offset（可缺省）；负时间必须使用向负无穷取整。"],
    "加入 offset（缺省 0）：桶起点变为 floor((time-offset)/width)*width+offset。",
    [check("normal", "core", {"width":10,"events":[{"time":1,"value":2},{"time":9,"value":3}]}, [[0,5]], public=True), check("negative", "core", {"width":10,"events":[{"time":-1,"value":1}]}, [[-10,1]]), check("offset", "correction", {"width":10,"offset":5,"events":[{"time":1,"value":2},{"time":9,"value":3}]}, [[-5,2],[5,3]], before=[[0,5]], public=True), check("edge-offset", "correction", {"width":4,"offset":2,"events":[{"time":2,"value":1}]}, [[2,1]], before=[[0,1]]), check("empty", "edge", {"width":1,"events":[]}, []), check("zero", "edge", {"width":1,"events":[{"time":0,"value":0}]}, [[0,0]])])

add("hold-distance", "typescript", "new", "Weighted edit distance",
    ["data 是两个 ASCII 字符串。插入和删除成本为 1，初始替换成本为 2；返回最小编辑成本整数。", "必须处理空字符串和重复字符。"],
    "替换成本改为 1，其余成本不变。",
    [check("same", "core", ["abc","abc"], 0, public=True), check("insert", "core", ["a","ab"], 1), check("replace", "correction", ["a","b"], 1, before=2, public=True), check("classic", "correction", ["kitten","sitting"], 3, before=5), check("empty", "edge", ["","abc"], 3), check("repeat", "edge", ["aaa","aa"], 1)])

add("hold-lines", "typescript", "new", "Chunked line splitting",
    ["data 是字符串分块数组，拼接后按 LF 或 CRLF 拆行；CRLF 可以跨分块，孤立 CR 保留。", "初始规则保留尾部空行，空输入等价于空字符串，返回 [\"\"]。"],
    "只删除由最后一个换行分隔符产生的那一个尾部空字符串；空输入仍返回 [\"\"]，中间空行保留。",
    [check("plain", "core", ["ab","cd"], ["abcd"], public=True), check("middle", "core", ["a\n\nb"], ["a","","b"]), check("tail", "correction", ["a\n"], ["a"], before=["a",""], public=True), check("crlf", "correction", ["a\r","\nb\r\n"], ["a","b"], before=["a","b",""]), check("empty", "edge", [], [""]), check("lone-cr", "edge", ["a\rb"], ["a\rb"])])

add("hold-median", "typescript", "existing", "Median with nullable data",
    ["data 是数字或 null 的数组，初始把 null 当作 0；返回中位数，偶数长度取中间两项的均值。", "空数组返回 null；修复当前实现排序时按字符串排序的问题。"],
    "null 改为忽略；忽略后没有数字时返回 null。",
    [check("numeric-sort", "core", [2,10,3], 3, public=True), check("even", "core", [1,4], 2.5), check("nullable", "correction", [None,4,8], 6, before=4, public=True), check("all-null", "correction", [None,None], None, before=0), check("empty", "edge", [], None), check("negative", "edge", [-5,-1,-3], -3)],
    'export function solve(data: (number|null)[]): number|null {\n  if (!data.length) return null;\n  const values = data.map(value => value ?? 0).sort();\n  const middle = Math.floor(values.length / 2);\n  return values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2;\n}\n')

add("hold-history", "typescript", "existing", "Undo and redo journal",
    ["data 是操作数组：[set,value]、[undo]、[redo]、[get]；初值 0，只有 get 产生输出，空 undo/redo 无操作。", "初始 set 保留 redo 栈；修复空 undo 把当前值变成 undefined 的问题。"],
    "新的 set 必须清空 redo 栈，其他行为不变。",
    [check("roundtrip", "core", [["set",1],["undo"],["redo"],["get"]], [1], public=True), check("empty-undo", "core", [["undo"],["get"]], [0]), check("branch", "correction", [["set",1],["undo"],["set",2],["redo"],["get"]], [2], before=[1], public=True), check("twice", "correction", [["set",1],["set",2],["undo"],["set",3],["redo"],["get"]], [3], before=[2]), check("empty", "edge", [], []), check("empty-redo", "edge", [["redo"],["get"]], [0])],
    'export function solve(data: any[]): any[] {\n  let value: any = 0; const past: any[] = [], future: any[] = [], out: any[] = [];\n  for (const op of data) {\n    if (op[0] === "set") {past.push(value); value = op[1];}\n    else if (op[0] === "undo") {future.push(value); value = past.pop();}\n    else if (op[0] === "redo" && future.length) {past.push(value); value = future.pop();}\n    else if (op[0] === "get") out.push(value);\n  }\n  return out;\n}\n')

add("hold-batches", "typescript", "existing", "Ordered capacity batches",
    ["data 含非负整数 weights 数组和正整数 capacity。保持输入顺序贪心分批，批内总重不能超过 capacity；初始超大单项单独成批。", "返回二维 weight 数组；修复空输入产生一个空批的问题。"],
    "只要存在大于 capacity 的单项，整个请求返回 {error:oversize}，不返回部分批次。",
    [check("normal", "core", {"weights":[2,3,1],"capacity":5}, [[2,3],[1]], public=True), check("empty", "core", {"weights":[],"capacity":3}, []), check("oversize", "correction", {"weights":[1,9,2],"capacity":5}, {"error":"oversize"}, before=[[1],[9],[2]], public=True), check("only-big", "correction", {"weights":[6],"capacity":5}, {"error":"oversize"}, before=[[6]]), check("zeros", "edge", {"weights":[0,0],"capacity":1}, [[0,0]]), check("exact", "edge", {"weights":[5,5],"capacity":5}, [[5],[5]])],
    'export function solve(data: any): number[][] {\n  const batches: number[][] = []; let batch: number[] = [], weight = 0;\n  for (const item of data.weights) {\n    if (batch.length && weight + item > data.capacity) {batches.push(batch); batch = []; weight = 0;}\n    batch.push(item); weight += item;\n    if (item > data.capacity) {batches.push(batch); batch = []; weight = 0;}\n  }\n  batches.push(batch);\n  return batches;\n}\n')

add("hold-router", "typescript", "existing", "Static and parameter routes",
    ["data 含 routes 字符串数组和 path；路径按 / 分段，:name 匹配一个非空段，静态段必须相等。初始采用第一条匹配路由。", "返回 {route,params}，无匹配返回 null；修复只比较段数而不比较静态段的问题。"],
    "匹配路由中静态段更多者优先，静态段数相同仍按输入顺序。",
    [check("parameter", "core", {"routes":["/users/:id"],"path":"/users/42"}, {"route":"/users/:id","params":{"id":"42"}}, public=True), check("missing", "core", {"routes":["/a"],"path":"/b"}, None), check("specific", "correction", {"routes":["/:id","/new"],"path":"/new"}, {"route":"/new","params":{}}, before={"route":"/:id","params":{"id":"new"}}, public=True), check("two-segments", "correction", {"routes":["/:a/:b","/x/:id"],"path":"/x/y"}, {"route":"/x/:id","params":{"id":"y"}}, before={"route":"/:a/:b","params":{"a":"x","b":"y"}}), check("no-routes", "edge", {"routes":[],"path":"/"}, None), check("root", "edge", {"routes":["/"],"path":"/"}, {"route":"/","params":{}})],
    'export function solve(data: any): any {\n  const path = data.path.split("/").filter(Boolean);\n  for (const route of data.routes) {\n    const parts = route.split("/").filter(Boolean); if (parts.length !== path.length) continue;\n    const params: Record<string,string> = {};\n    parts.forEach((part: string,index: number) => {if (part.startsWith(":")) params[part.slice(1)] = path[index];});\n    return {route,params};\n  }\n  return null;\n}\n')


if __name__ == "__main__":
    assert len(TASKS) == 36
    print("*** Begin Patch")
    for path, content in sorted(FILES.items()):
        print("*** Add File: " + path)
        for line in content.splitlines():
            print("+" + line)
    print("*** End Patch")
