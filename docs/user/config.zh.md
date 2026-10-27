# 配置参考：用一份 JSON 描述一门语言

面向"想分析自己的语言、但不写 Rust"的读者。`bnf-analyzer` 的全部语言知识都在一份配置里：
词法规则（`rules`）+ 文法产生式（`grammar`）。换一份配置就是换一门语言，二进制不用改。

配置只接受**结构化对象**：正则要包成 `{ "regex": "…" }`，右部要拆成
`{ "rule": … }` / `{ "token": … }` / `{ "seq": [ … ] }`。字符串写法（正则原文、BNF/EBNF 文本）
自 `version: 2` 起被明确拒绝——配置里不存在"再解析一小段文本"的隐藏步骤，读到什么就是什么。

> 本文只讲**写配置**。要把 crate 当库用（在 Rust 里挂动作、算值），看 `docs/user/api.zh.md`；
> 想看同一份文法在 JSON / Rust / BNF 三种形态之间的逐项对照，看 `docs/user/grammar.zh.md`。

配套文件：

| 文件 | 是什么 |
|---|---|
| `examples/postscript.json` | **内置配置**：PostScript 子集，命令行默认用的就是它；也是最完整的一份模板 |
| `examples/minimal.json` | 最小可用配置，演示"换一份配置就能分析别的语言" |
| `schema/config.schema.json` | **配置格式的 schema**（Draft 2020-12），与具体语言无关 |

---

## 1. 一份完整配置

```json
{
  "$schema": "./schema/config.schema.json",
  "version": 2,
  "rules": [
    { "name": "space",  "pattern": { "plus": { "chars": "\\s" } }, "skip": true },
    { "name": "comment", "pattern": { "regex": "%[^\\n]*" }, "skip": true },
    { "name": "number", "pattern": { "plus": { "chars": "0-9" } }, "priority": 10 },
    { "name": "name",   "pattern": { "regex": "/[^\\s()]*" }, "priority": 20 }
  ],
  "grammar": {
    "start": "program",
    "productions": [
      { "name": "program", "rhs": { "star": { "rule": "value" } } },
      { "name": "value", "rhs": { "alt": [
        { "token": "number" },
        { "token": "name" },
        { "rule": "procedure" }
      ] } },
      { "name": "procedure", "rhs": { "seq": [
        { "literal": "{" },
        { "star": { "rule": "value" } },
        { "literal": "}" }
      ] } }
    ]
  }
}
```

顶层只有四个键：

| 键 | 必填 | 含义 |
|---|---|---|
| `version` | 可省 | 配置格式版本，当前只有 `2`；省略按 `2` 处理 |
| `rules` | 可省 | 词法规则数组，省略等于空表（此时只能用字面量终结符） |
| `grammar` | 可省 | 文法段；省略后在装配时报 `empty grammar: at least one production is required` |
| `$schema` | 可省 | 编辑器用的 schema 路径，加载时忽略、回写时原样保留 |

**未知键一律报错**，和 schema 的 `additionalProperties: false` 一致。拼错 `productons`
不会静默失效，而是直接报 ``unknown field `productons`, expected one of: …``。

---

## 2. `rules`：词法

一条规则一个对象，键只有四个：

| 键 | 必填 | 含义 |
|---|---|---|
| `name` | 是 | 规则名，会成为 token 的 `kind`；文法里用 `{ "token": "…" }` 引用它 |
| `pattern` | 是 | 匹配模式，一个只写一个键的对象（见 §3） |
| `skip` | 否 | 为真时只消费源码、不产出 token（空白、注释）；默认 `false` |
| `priority` | 否 | 同长度竞争时的优先级，越大越优先；默认 `0` |

匹配永远从当前扫描位置**起锚**（相当于给正则加了 `\A`），所以 `pattern` 不用自己写 `^`。

裁决顺序是固定的三步：

1. **最长匹配**：能匹配更多字符的那条赢；
2. 长度相同时，**`priority` 大**的赢；
3. 还相同时，**声明顺序靠前**的赢。

`priority` 只在**长度相同**时才起作用，用途是让"更像专用 token"的规则压过通用规则。
内置配置里 `immediate`(30) > `name`(20) > `number`(10) > `boolean`(5) > `operator`(0)：
`1.5` 能被 `number` 与 `operator` 整段匹配，`number` 胜；`true` 同理归 `boolean`。
长度不同时优先级不参与——`/foo` 被 `name` 整段吃掉，`//x` 被 `immediate` 整段吃掉。

---

## 3. `pattern`：正则从哪来

`pattern` 是对象，**只写一个键**。一个键就是一个运算符，要组合就嵌套：

| 想匹配 | 写法 | 编译成 |
|---|---|---|
| 整段正则 | `{ "regex": "\\s+" }` | `\s+` |
| 左花括号 | `{ "literal": "{" }` | `\{` |
| 十进制数字 | `{ "chars": "0-9" }` | `[0-9]` |
| 非空白 | `{ "not_chars": "\\s" }` | `[^\s]` |
| 标识符 | `{ "seq": [{ "chars": "a-zA-Z_" }, { "star": { "chars": "a-zA-Z0-9_" } }] }` | `[a-zA-Z_](?:[a-zA-Z0-9_])*` |
| 关键字二选一 | `{ "alt": [{ "literal": "if" }, { "literal": "else" }] }` | `if\|else` |
| 零或多次 | `{ "star": … }` / `{ "plus": … }` / `{ "opt": … }` | `(?:…)*` / `(?:…)+` / `(?:…)?` |

约定：

- **`chars` / `not_chars` 的内容按正则字符集语法写**（`0-9`、`a-f` 是区间）。因此 `]`、`^`、`\`
  要自己转义，例如 `{ "not_chars": "\\]\\^" }`。写汉字/emoji 也直接写，字符集按字符处理。
- **`literal` 由 `regex::escape` 自动转义**：写 `{ "literal": "{" }` 就够了，不必写 `\{`。
- **只有 `regex` 能表达高级语法**：前后瞻、命名分组、内联标志（`(?s)`）都只能走整段正则。
- 一个对象写两个键（`{ "chars": "0-9", "priority": 10 }`）不是"多加个选项"，而是
  `a pattern object must have exactly one key`——`priority` 属于规则，不属于模式。

---

## 4. `grammar`：文法

```json
"grammar": {
  "start": "program",
  "productions": [ { "name": "value", "rhs": { "token": "number" } } ]
}
```

| 键 | 必填 | 含义 |
|---|---|---|
| `start` | 否 | 起始符号；省略时取**第一条**产生式的名字 |
| `productions` | 是 | 产生式数组，至少一条；为空时装配报 `empty grammar` |

一条产生式只有两个键：

| 键 | 含义 |
|---|---|
| `name` | 左部（非终结符名）。可用字母、数字、`_`、`-`（`value-1` 合法），不能为空 |
| `rhs` | 右部，一个结构化表达式（见 §5） |

**同名多次声明是合法的**，等价于多个候选，按声明顺序展开。

---

## 5. `rhs`：右部的三种引用与五个组合子

### 5.1 引用

| 想引用 | 写法 | 校验时机 |
|---|---|---|
| 非终结符（另一条产生式） | `{ "rule": "value" }` | 建文法时，找不到就报"引用了未定义的非终结符" |
| 词法类别（`rules[].name`） | `{ "token": "number" }` | 装配分析器时，找不到就报"未知的词法类别" |
| 字面量终结符 | `{ "literal": "{" }` | 不用声明，建文法时自动派生词法规则 |

三者**互不推断**：`rule` 只找产生式，`token` 只找 `rules` 里的名字。
"我明明定义了 `number`，却不匹配数字"几乎总是因为写成了 `{ "rule": "number" }`。

字面量终结符的几点约定：

- 不需要在 `rules` 里重复声明：`{ "literal": "{" }` 会自动补一条优先级 `1000` 的词法规则，
  所以同长度时关键字不会被标识符规则吞掉；
- 字面量 token 的 `kind` 就是原文（`"{"`、`"<<"`），输出里看到的就是它；
- 多字符字面量（`"<<"`、`"ifelse"`）整体匹配，不用拆成单字符。

### 5.2 组合子

| 形式 | 写法 |
|---|---|
| 序列 | `{ "seq": [ … ] }` |
| 选择（多候选） | `{ "alt": [ … ] }` |
| 零或多次 | `{ "star": … }` |
| 一或多次 | `{ "plus": … }` |
| 零或一次 | `{ "opt": … }` |
| 空（ε） | `{ "seq": [] }` |

**结构是显式的**：没有优先级表、没有结合性要记——`seq` 与 `alt` 谁在里谁在外，看嵌套就知道。
`{ "alt": [] }` 会被拒绝（没有任何候选的分支没有意义）。

**后缀只绑一个符号**：`( word "," )?` 要把 `opt` 套在 `seq` 外面：

```json
{ "opt": { "seq": [ { "rule": "word" }, { "literal": "," } ] } }   // 整组可选
{ "seq": [ { "rule": "word" }, { "opt": { "literal": "," } } ] }   // 只有逗号可选
```

### 5.3 重复在语法树里是平的

`star` / `plus` / `opt` 只是写法糖：语法树里不会多出一层包装节点，`children` 是平的；
`--bnf` 里看到的就是 `*` / `+` / `?`，报错的"期望列表"里也只有你写过的符号。
换句话说，重复怎么实现是库的事，你只需要按上面三种写法表达意图。

---

## 6. 改完配置怎么验

先用最省事的两步确认配置本身成立：

```text
$ bnf-analyzer -c my.json --bnf        # ① 文法编得出来吗？起手看 BNF 视图
$ printf '1 /x { dup }' | bnf-analyzer -c my.json -t --compact   # ② 词法切得对吗？
```

- `--bnf` 会把结构化文法渲染成 BNF 文本（`rule` → 裸名、`token` → `@名字`、`literal` → 引号）。
  它是**只读视图**，用来人读与评审，不能读回配置。想验证文法顺序、重复、ε，看这一份最快。
- `-t` 只跑词法，不做语法检查：括号不配对也能正常输出 token，正好用来单独调规则与优先级。

两步都对，再跑整条流水线：

```text
$ printf '1 /x { dup }' | bnf-analyzer -c my.json --compact
```

命令行只输出语法树，不带动作。要把树变成值（求和、建字典、求值），
得把 crate 当库用，见 `docs/user/api.zh.md`。

---

## 7. 版本与迁移

| `version` | 行为 |
|---|---|
| 省略或 `2` | 正常加载 |
| `1` | 拒绝，并给出逐条迁移提示（见下） |
| 其它 | 拒绝：`unsupported configuration version N (this crate understands version = 2)` |

`version: 1` 的报错长这样（原文是一行，这里为排版折行）：

```text
configuration version 1 is no longer supported: patterns and right-hand sides must be
structured objects. Wrap a regex as { "regex": … }, rewrite BNF text as { "rule": … } /
{ "token": … } / { "seq": [ … ] }, and set version to 2
```

版本号在字段解析**之前**检查，所以旧文件不会先抛一堆"字符串被拒"，而是先告诉你该怎么迁。

---

## 8. 编辑器支持：schema

`schema/config.schema.json` 覆盖整份配置，内置配置用它自己的 `$schema` 指向它
（内置那份在 `examples/` 下，所以写 `../schema/config.schema.json`）：

```json
{
  "$schema": "./schema/config.schema.json",
  "version": 2,
  "rules": [ … ],
  "grammar": { … }
}
```

- VS Code / JetBrains 会据此补全键名、悬停显示说明、拼错画红波浪线；
- 运行时与 schema 的判定一致：`pattern` / `rhs` 都是 `type: object` + `maxProperties: 1`，
  字符串直接被拒；两边都不接受未知键；
- 自己的配置文件建议把 `$schema` 写成相对路径（IDE 也能认），加载时它被忽略但会原样保留。

---

## 9. 报错对照

| 报错 | 原因 | 怎么改 |
|---|---|---|
| `rhs` does not accept a string | 右部还是旧写法 | 拆成 `{ "rule": … }` / `{ "token": … }` / `{ "seq": [ … ] }` |
| `pattern` does not accept a string | 模式还是旧写法 | 正则包成 `{ "regex": "…" }`，固定文本用 `{ "literal": "…" }` |
| a pattern object must have exactly one key | 模式里写了两个运算符 | 一个对象一个键，组合用 `{ "seq": [ … ] }` 嵌套 |
| an expression object must have exactly one key | 右部里写了两个符号 | 多个符号请用 `{ "seq": [ … ] }` |
| an expression must be an object | 右部嵌套里写了字符串 | 改成 `{ "star": { "rule": "value" } }` |
| unknown expression key / unknown pattern key | 键名拼错（`regexp`、`not_char`） | 照 §3 / §5 的键表改 |
| unknown field `…`, expected one of: … | 顶层 / 规则 / grammar 里多了键 | 删掉，或改成表里列出的键 |
| references undefined non-terminal `x` | 用了 `rule`，但没有同名产生式 | 补一条产生式，或改用 `{ "token": "x" }` |
| references undefined token class `x` | `token` 指向的名字不在 `rules` 里 | 在 `rules` 里加同名规则 |
| empty grammar: at least one production is required | `grammar` 段为空或缺失 | 至少写一条产生式 |
| start symbol `x` has no production | `start` 写了个不存在的名字 | 改成实际存在的产生式名，或删掉 `start` |
| 关键字被切成普通标识符（没有报错） | 文法里没写这个字面量，规则优先级也不够 | 文法里写 `{ "literal": "if" }`，或调高该规则的 `priority` |
| failed to compile regex（`chars` 里写 `]`） | `chars` 是正则字符集，需转义 | 写成 `"\\]"` |
| configuration version 1 is no longer supported | 文件还是 `version: 1` | 按提示改成对象写法并升到 `2` |

错误都出现在**读源码之前**（配置问题）或**切词/解析时**（源码问题），两者不会混在一起。
源码类错误会带 `line N, column M` 与插入符，见 `docs/user/cli.zh.md`。

---

## 10. 内置的 PostScript 子集

`examples/postscript.json` 就是本文所有规则的实例，可以直接拿它当模板：

```text
program ::= value* ;
value ::= @number | @boolean | @string | @hexstring | @name | @immediate | @operator
        | procedure | array | dictionary ;
procedure ::= "{" value* "}" ;
array ::= "[" value* "]" ;
dictionary ::= "<<" entry* ">>" ;
entry ::= @name value ;
```

（`bnf-analyzer --bnf` 的输出；第二行原文是一整行，这里按版面折行。）

它覆盖的是一份**够用**的子集：原子（数字、布尔、字符串、十六进制串、名字、直接名、操作符）、
三种容器（过程、数组、字典）、注释与空白。字符串支持一层嵌套括号
（`(a (b) c)` 可以，`(a (b (c)) d)` 不行）——需要更深就把它换成自定义的扫描规则。

想改这门语言（加关键字、加语法），复制一份 `examples/postscript.json` 改，用 `-c` 指过去；
别改内置那份，它同时是测试与文档的基准。
