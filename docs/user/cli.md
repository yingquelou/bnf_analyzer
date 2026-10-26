# 命令行：把源码变成 JSON

命令行是"一份配置 + 不带动作"的那条路：读 `postscript.json`（或 `-c` 指定的配置），
把源码分析成带位置信息的语法树。它**不做求值**——要把树变成值，得把 crate 当库用，
见 `docs/user/api.md`。

写自己的配置看 `docs/user/config.md`；本文只讲怎么把 `analyzer` 跑起来、输出长什么样。

## 0. 拿到可执行文件

可执行文件不在默认 feature 里，两种方式都行：

```text
$ cargo install --path . --features cli      # 装进 ~/.cargo/bin，之后直接 analyzer …
$ cargo run --features cli -- sample.ps      # 不安装，就地跑
```

下文为简洁起见直接写 `analyzer …`，前者等价于安装后的命令，后者把 `analyzer` 换成
`cargo run --features cli --` 即可。

---

## 1. 用法

```text
usage: analyzer [options] [file]

Analyze a source file with the lexical rules and grammar from a configuration file
and print the result as JSON (the built-in postscript.json is used by default).

arguments:
  <file>                file to analyze; omit it or pass - to read standard input

options:
  -c, --config <file>   use this configuration file (default: built-in postscript.json)
  -t, --tokens          print the token stream only, without parsing
      --bnf             print the current grammar as BNF (read-only view)
      --compact         print compact JSON (two-space indentation by default)
  -h, --help            print this help
  -V, --version         print version
```

- 选项与文件位置无关：`analyzer sample.ps -t` 与 `analyzer -t sample.ps` 等价。
- `-` 表示标准输入，也可以干脆不写文件参数。
- `--bnf` 不需要源码：它只渲染文法，给了文件也不会读。
- 配置默认是**编译期内嵌**的 `postscript.json`，与工作目录无关；`-c` 才读磁盘。

---

## 2. 三种输出

输出有三种形态，字段名与配置里的键是同一套命名习惯（同一套 `Value` 模型，
配 `-t` / 默认都只是同一棵树的不同投影）。

### 2.1 默认：带位置的语法树

输入 `1 2`，输出（为省版面把 `span` 写成一行；实际默认两格缩进、每个字段一行）：

```json
{
  "kind": "rule",
  "rule": "program",
  "span": { "start": 0, "end": 3, "line": 1, "column": 1 },
  "children": [
    {
      "kind": "rule",
      "rule": "value",
      "span": { "start": 0, "end": 1, "line": 1, "column": 1 },
      "children": [
        { "kind": "number", "text": "1",
          "span": { "start": 0, "end": 1, "line": 1, "column": 1 } }
      ]
    },
    {
      "kind": "rule",
      "rule": "value",
      "span": { "start": 2, "end": 3, "line": 1, "column": 3 },
      "children": [
        { "kind": "number", "text": "2",
          "span": { "start": 2, "end": 3, "line": 1, "column": 3 } }
      ]
    }
  ]
}
```

两种节点用 `kind` 区分：

| 节点 | 字段 | 说明 |
|---|---|---|
| 语法树节点 | `kind: "rule"`、`rule`、`span`、`children` | `rule` 是产生式左部 |
| 叶子 token | `kind`（类别）、`text`（原文）、`span` | 字面量终结符的 `kind` 就是原文，如 `"{"` |

`span` 四个字段：

| 字段 | 含义 |
|---|---|
| `start` / `end` | **字节**偏移，左闭右开，可直接用于切片 |
| `line` / `column` | 起点行列，**1 基**；`column` 按字符计，不是字节 |

`end` 不是独立位置，而是区间终点：多字节字符（中文、UTF-8）下
`end - start` 与字符数不相等，按字节切才安全。

### 2.2 `-t`：只输出 token 流

```text
$ printf '1 2' | analyzer -t --compact
[{"kind":"number","text":"1","span":{"start":0,"end":1,"line":1,"column":1}},{"kind":"number","text":"2","span":{"start":2,"end":3,"line":1,"column":3}}]
```

输出是一个数组，元素结构与树里的叶子相同。`skip` 规则（空白、注释）不产出 token，
所以上例只有两个元素。这条路径不做语法检查：括号不配对也能正常输出 token。

### 2.3 `--bnf`：当前文法的只读视图

```text
$ analyzer --bnf
program ::= value* ;
value ::= @number | @boolean | @string | @hexstring | @name | @immediate | @operator | procedure | array | dictionary ;
procedure ::= "{" value* "}" ;
array ::= "[" value* "]" ;
dictionary ::= "<<" entry* ">>" ;
entry ::= @name value ;
```

渲染约定：

- `rule` 引用 → 裸名（`value`）；
- `token` 引用 → `@类别`（`@number`）；
- `literal` → 带引号的原文（`"{"`）；
- 同名多条产生式合并成一行 `|`；空右部渲染成 `x ::= ;`（ε）；
- 文法内部用的辅助符号**不出现**，只留下你能在配置里写出来的名字。

这份文本只用来人读与评审，不能读回配置——配置里只有结构化对象。

---

## 3. 出错时

错误一律写 stderr，进程退出码为 1，并附上出错行与插入符：

```text
$ printf '/f { dup 1' | analyzer
line 1, column 11: unexpected end of input, expected one of: "<<", "[", "{", "}", array, boolean,
dictionary, hexstring and 7 more
  /f { dup 1
            ^
```

- 词法错误会指出具体字符：`line 1, column 1: no rule matches character '('`；
- 期望列表最多显示 8 项，超出后写 `and N more`；
- 列表里只会出现用户可见的符号——文法内部的辅助产生式被过滤掉；
- 配置错误发生在读源码之前，因此没有插入符：

```text
$ analyzer -c old.json -
configuration version 1 is no longer supported: patterns and right-hand sides must be structured
objects. Wrap a regex as { "regex": … }, rewrite BNF text as { "rule": … } / { "token": … } /
{ "seq": [ … ] }, and set version to 2
```

---

## 4. 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功（包括 `-h` / `-V` / `--bnf`） |
| 1 | 读文件失败、配置非法、词法或语法错误、JSON 序列化失败 |

没有"部分成功"：要么完整输出一份 JSON，要么什么都不输出。

---

## 5. 常见用法

```text
# 分析文件
analyzer sample.ps

# 从管道读，紧凑输出，再交给 jq
cat sample.ps | analyzer - --compact | jq '.children | length'

# 换一份配置试文法
analyzer -c examples/minimal.json --bnf
printf '{ 1 /x }' | analyzer -c examples/minimal.json --compact

# 只看词法，验证最长匹配与优先级
printf '1 /a true' | analyzer -t --compact
```

需要"分析 + 求值/建 AST"时，`-t` 与默认输出都不够用：命令行这条路不执行动作。
把 crate 当库用，按 `docs/user/api.md` 注册 `Semantics` 即可，用的是同一份 `Config`。

---

## 6. 接着读

| 你想 | 读 |
|---|---|
| 写一份自己的配置（换语言、加语法、调优先级） | `docs/user/config.md` |
| 把树变成值（求和、建字典、求值） | `docs/user/api.md` |
| 弄清某个键对应哪个文法节点 | `docs/user/grammar.md` |
| 改这个 crate 本身 | `docs/dev/architecture.md` |
