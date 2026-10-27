# 文法怎么写：JSON 与 Rust 两种写法

面向**使用本库定义一门语言**的人。同一份文法有两种写法，写法不同、结果完全一样：

| 形态 | 用在哪 | 入口 | feature |
|---|---|---|---|
| **JSON 对象** | 配置文件里的 `pattern` 与 `rhs` | `Config` → `Analyzer::from_config` | `json` |
| **Rust 代码** | 作为依赖库 | `Grammar::builder()` → `Analyzer::new` | `core`（默认） |
| BNF 文本 | **只读视图**：`Grammar::to_bnf()` / `bnf-analyzer --bnf` 的输出 | 不是输入格式 | — |

本文逐项对照这两侧，并说明 BNF 视图怎么读。配置里每个键的完整语义、词法优先级裁决、
schema 与报错对照在 `docs/user/config.zh.md`；库 API 的其他部分（动作、`Value`、访问者、错误）
在 `docs/user/api.zh.md`。要改这个 crate 本身才需要 `docs/dev/`。

> JSON 相关能力（`Config`、`from_config`、`--bnf` 输出）都在 `json` feature 下；
> 默认 `cargo add bnf_analyzer` 只有 core。本文的 Rust 代码块只用 core，
> `cargo test --doc` 在默认 feature 下也能跑。

---

## 1. 键与 Rust 构造器的对应

写 JSON 时你面对的是键名，写 Rust 时面对的是构造器——它们是同一套形状：

| JSON 键 | Rust（`Pattern` / `Expr`） | 用在哪 |
|---|---|---|
| `regex` | `Pattern::regex("…")` | 只有词法层有整段正则 |
| `literal` | `Pattern::literal("{")` / `Expr::literal("{")` | 词法侧自动转义；文法侧派生词法规则 |
| `chars` / `not_chars` | `Pattern::chars("0-9")` / `Pattern::not_chars("\\s")` | 字符集片段 |
| `seq` / `alt` | `Pattern::seq(..)`、`Expr::seq([..])` / `alt` | 连接 / 选择 |
| `star` / `plus` / `opt` | `Pattern::star(..)`、`Expr::star(..)` 等 | 零或多次 / 一或多次 / 零或一次 |
| `rule` / `token` | `Expr::rule("value")` / `Expr::token("number")` | 只有文法层能引用别的符号 |

两侧都能再渲染回 JSON（`to_value()`），`Expr` 还能渲染成 BNF（`to_bnf()` 走的就是这条路）。

---

## 2. 三态对照：`digit`

### JSON 写法

```json
{
  "name": "digit",
  "rhs": { "alt": [
    { "literal": "0" }, { "literal": "1" }, { "literal": "2" },
    { "literal": "3" }, { "literal": "4" }, { "literal": "5" },
    { "literal": "6" }, { "literal": "7" }, { "literal": "8" },
    { "literal": "9" }
  ] }
}
```

### Rust 写法

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};

let grammar = Grammar::builder()
    .start("digit")
    .production(
        "digit",
        // 十个候选 → Expr::alt；每个 "0" → Expr::literal
        Expr::alt(('0'..='9').map(|digit| Expr::literal(digit.to_string()))),
    )
    .build()
    .expect("the grammar should be valid");

assert_eq!(grammar.productions_of("digit").len(), 10);
```

### BNF 视图（`to_bnf()` 的输出，只读）

```text
digit ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
```

### 逐段对应

| BNF 视图里的片段 | JSON 对象 | Rust |
|---|---|---|
| 左部 `digit` | `"name": "digit"` | `.production("digit", …)` 的第一个参数 |
| `::=` | `"rhs":` 这个键 | `.production` 的第二个参数 |
| `\|`（十选一） | `{ "alt": [ … ] }` | `Expr::alt([ … ])` |
| `"0"`（字面量） | `{ "literal": "0" }` | `Expr::literal("0")` |
| 结尾 `;` | 一条产生式就是一项，不需要分号 | 一次 `.production` 调用 |

### 把它接到序列里：`digit+`

```json
"grammar": {
  "start": "program",
  "productions": [
    { "name": "program", "rhs": { "plus": { "rule": "digit" } } },
    { "name": "digit",   "rhs": { "alt": [
        { "literal": "0" }, { "literal": "1" }, { "literal": "2" },
        { "literal": "3" }, { "literal": "4" }, { "literal": "5" },
        { "literal": "6" }, { "literal": "7" }, { "literal": "8" },
        { "literal": "9" }
    ] } }
  ]
}
```

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("digit")))
    .production(
        "digit",
        Expr::alt(('0'..='9').map(|digit| Expr::literal(digit.to_string()))),
    )
    .build()
    .expect("the grammar should be valid");

assert_eq!(grammar.to_bnf().lines().next(), Some("program ::= digit+ ;"));
```

> **十选一也可以交给词法层。** `digit` 这种"单字符枚举"写 10 条产生式很啰嗦，等价的写法是
> 在 `rules` 里加 `{ "name": "digit", "pattern": { "chars": "0-9" } }`，文法里写
> `{ "token": "digit" }`（Rust 用 `Expr::token("digit")`）。区别只在语法树：十选一得到
> `digit` 节点包着一个字面量 token，词法类别直接得到一个 `digit` token。

---

## 3. 左部：产生式名

| 你想表达 | JSON | Rust |
|---|---|---|
| 定义非终结符 | `{ "name": "digit", "rhs": … }` | `.production("digit", …)` |
| 声明起点 | `"start": "program"`（省略则取第一条） | `.start("program")` |
| 一个名字多个候选 | 两条同名 `productions` | 两次 `.production("a", …)` |
| 空产生式（ε） | `"rhs": { "seq": [] }` | `Expr::seq([])` |

- 同名多次声明就是多个候选，保持声明顺序；`to_bnf()` 会合并成 `a ::= x | y ;`。
- 名字里可以用字母、数字、`_`、`-`（`value-1` 合法）。
- 起点省略时取第一条产生式；显式写了不存在的名字会报错（`GrammarError::UnknownStart`），
  一条产生式都没有则报 `GrammarError::Empty`。

---

## 4. 三种引用：`rule` / `token` / `literal`

| 含义 | JSON 键 | Rust | 什么时候检查 |
|---|---|---|---|
| 非终结符（另一条产生式） | `{ "rule": "digit" }` | `Expr::rule("digit")` | 建文法时（`UnknownSymbol`） |
| 词法类别（`rules[].name`） | `{ "token": "digit" }` | `Expr::token("digit")` | 装配分析器时（`UnknownTokenClass`） |
| 字面量终结符 | `{ "literal": "{" }` | `Expr::literal("{")` | 不用声明，自动派生词法规则 |

三种引用**互不推断**：`rule` 不会退化成词法类别，`token` 也不会去找产生式。
所以"我明明定义了 `number`，却不匹配数字"这类问题，几乎都是把 `token` 写成了 `rule`。

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

// `number` 同时是产生式名与词法类别名，两种引用各走各的。
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::token("number"))   // 词法类别：接受 42
    .production("number", Expr::literal("n"))       // 产生式：接受 n
    .build()
    .expect("the grammar should be valid");

let analyzer = Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent");
assert!(analyzer.analyze("42").is_ok());
assert!(analyzer.analyze("n").is_err());
```

**字面量**的约定：

- 不需要在 `rules` 里声明：`Grammar::literals()` 会把它交给 `Rules::with_literals` 自动补规则，
  优先级固定 `LITERAL_PRIORITY = 1000`，所以同长度时关键字不会被标识符规则吞掉。
- 字面量 token 的 `kind` 就是原文（`"{"`、`"<<"`），动作也可以按原文注册。
- 多字符字面量（`"<<"`、`"ifelse"`）整体匹配，不需要拆。

---

## 5. 组合子

| 形式 | JSON | Rust |
|---|---|---|
| 序列 | `{ "seq": [ … ] }` | `Expr::seq([a, b])` |
| 选择 | `{ "alt": [ … ] }` | `Expr::alt([a, b])` |
| 零或多次 | `{ "star": … }` | `Expr::star(a)` |
| 一或多次 | `{ "plus": … }` | `Expr::plus(a)` |
| 零或一次 | `{ "opt": … }` | `Expr::opt(a)` |
| 分组 | 直接嵌套：`{ "seq": [ { "alt": [ … ] }, c ] }` | `Expr::seq([Expr::alt([a, b]), c])` |
| 空（ε） | `{ "seq": [] }` | `Expr::seq([])` |

**结构是显式的**：没有优先级表、没有结合性要记——`seq` 与 `alt` 谁在里谁在外，看嵌套就知道。
`{ "alt": [] }` 会被拒绝。

**后缀只绑一个符号**：`( word "," )?` 要把 `opt` 套在 `seq` 外面；
写成 `{ "seq": [{ "rule": "word" }, { "opt": { "literal": "," } }] }` 是另一个意思（只有逗号可选）。

**重复在语法树里是平的**：`star` / `plus` / `opt` 只是写法糖，归约时把子值摊平，
所以 `children` 里看到的是一个个元素，不会多出一层包装节点；`to_bnf()` 里也只看到 `*` / `+` / `?`。
（内部怎么展开成等价的普通产生式，见 `docs/dev/internals.zh.md`。）

---

## 6. `pattern`：会说成什么正则

`rules[].pattern` 是对象，**只写一个键**；`Pattern::to_regex()` 是唯一出口：

| 想匹配 | JSON | Rust | 编译成 |
|---|---|---|---|
| 整段正则 | `{ "regex": "\\s+" }` | `Pattern::regex(r"\s+")` | `\s+` |
| 左花括号 | `{ "literal": "{" }` | `Pattern::literal("{")` | `\{` |
| 十进制数字 | `{ "chars": "0-9" }` | `Pattern::chars("0-9")` | `[0-9]` |
| 非空白 | `{ "not_chars": "\\s" }` | `Pattern::not_chars(r"\s")` | `[^\s]` |
| 标识符 | `{ "seq": [{ "chars": "a-zA-Z_" }, { "star": { "chars": "a-zA-Z0-9_" } }] }` | `Pattern::seq([...])` | `[a-zA-Z_](?:[a-zA-Z0-9_])*` |
| 关键字二选一 | `{ "alt": [{ "literal": "if" }, { "literal": "else" }] }` | `Pattern::alt([...])` | `if\|else` |

```rust
use bnf_analyzer::core::pattern::Pattern;
use bnf_analyzer::core::rules::Rule;

// JSON 里的 `{ "name": …, "pattern": … }` 读出来就是 `Rule`（需要 json feature）。
// 这里直接用 Rust 构造同样的对象，验证 pattern 键编译出来的正则。
let number = Rule::new("number", Pattern::plus(Pattern::chars("0-9")));
assert_eq!(number.pattern.to_regex(), "(?:[0-9])+");

// `{ "literal": "{" }`：元字符由 regex::escape 处理，不用自己写反斜杠。
let brace = Rule::literal("brace", "{");
assert_eq!(brace.pattern, Pattern::literal("{"));
```

- `chars` / `not_chars` 的内容是**正则字符集片段**，`]`、`^`、`\` 要自己转义；
  `literal` 走 `regex::escape`，不用管元字符。
- 高级语法（前后瞻、命名分组、内联标志 `(?s)`）只能写整段 `regex`。
- 组合子一律加非捕获分组 `(?:…)`，避免优先级被外层吞掉。

---

## 7. 动作：只有 Rust 路径有

JSON 路径（可执行 crate）不带动作，输出默认语法树。要在归约时算值，就把 crate 当库用
（动作的缓存语义与错误类型见 `docs/user/api.zh.md`）：

| 动作 | 挂在哪种符号上 | 键 | 例子 |
|---|---|---|---|
| token 动作 | `{ "token": "digit" }` | 类别名 | `.token("digit", …)` |
| token 动作 | `{ "literal": "{" }` | 字面量原文 | `.token("{", …)` |
| 语义动作 | 任意产生式 | 产生式名 | `.rule("program", …)` |

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::seq([Expr::literal("("), Expr::token("number"), Expr::literal(")")]))
    .build()
    .expect("the grammar should be valid");

let semantics = Semantics::new()
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| error.to_string())
    })
    // 字面量也能挂动作，键就是它的原文
    .token("(", |_| Ok(Value::text("open")))
    .token(")", |_| Ok(Value::text("close")))
    .rule("program", |values| Ok(Value::list(values.to_vec())));

let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
let document = analyzer.analyze("(7)").expect("the source should be valid");
assert_eq!(
    document.root,
    Value::list(vec![Value::text("open"), Value::Int(7), Value::text("close")])
);
```

注意：

- 动作按**归约点**缓存：token 动作每个 token 匹配一次，语义动作每个归约点一次；
  回调应当无副作用（别在里面累加外部状态）。
- 挂了语义动作的产生式不再产出 `Value::Rule` 节点，`children` 里拿到的是动作的返回值。
- 动作返回 `Err(String)` 会变成 `ParseError::RuleAction` / `TokenAction`，带出错位置。

---

## 8. 先写 JSON，再搬进代码

`Config` → `Grammar` 与 `Grammar::builder()` 产出的 CFG 完全一致，所以可以先用 JSON 快速试文法，
稳定后再搬成 Rust 代码（或反过来）。

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::json::Config;

let from_json = Config::parse(
    r#"{
        "version": 2,
        "rules": [],
        "grammar": { "productions": [
            { "name": "program", "rhs": { "seq": [
                { "literal": "{" },
                { "star": { "rule": "value" } },
                { "literal": "}" }
            ] } },
            { "name": "value", "rhs": { "alt": [
                { "token": "number" },
                { "rule": "value" }
            ] } }
        ] }
    }"#,
)
.expect("the configuration should be valid")
.grammar
.parse_grammar()
.expect("the grammar is consistent, token classes are checked at setup");

let from_code = Grammar::builder()
    .start("program")
    .production(
        "program",
        Expr::seq([
            Expr::literal("{"),
            Expr::star(Expr::rule("value")),
            Expr::literal("}"),
        ]),
    )
    .production("value", Expr::alt([Expr::token("number"), Expr::rule("value")]))
    .build()
    .expect("the grammar should be valid");

assert_eq!(from_json.to_bnf(), from_code.to_bnf());
```

小提醒：`Grammar::to_bnf()` 是**只读视图**（`rule` → 裸名、`token` → `@名字`），
用于人读与评审；它不能读回配置，配置里只有结构化对象。
