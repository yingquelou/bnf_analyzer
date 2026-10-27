# 库 API：把 bnf_analyzer 当依赖用

本文面向**这个库的使用者**：把 `bnf_analyzer` 加进 `Cargo.toml`，在 Rust 代码里描述文法、
挂动作、遍历结果。讲的是入口、动作、`Value` / `Document`、访问者、错误与边界——
都是 `pub` 的东西，不需要读实现。

同一侧的其他手册：命令行见 `docs/user/cli.zh.md`，配置里每个键的含义见 `docs/user/config.zh.md`，
文法在 JSON / Rust 之间怎么对应见 `docs/user/grammar.zh.md`。
要改这个 crate 本身（分层、模块、测试）才需要 `docs/dev/`。

> 本文的每个 Rust 代码块都是 doctest，`cargo test --all-features --doc` 会真的编译并运行它们；
> 这里的 API 名字、返回类型、行为描述都不允许"看着像"。
>
> 分两层：`Analyzer::new` 属于 core（默认 feature）；`Config` 与 `from_config`
> 属于 `json` feature。默认 `cargo add bnf_analyzer` 只拿到 core。

## 0. 路径：crate 根不做 re-export

公共名字都留在自己的模块里，crate 根只有 `core` / `json` / `guide` 三个模块声明。
本文示例一律写全路径，你自己的项目里按需引入或取别名：

```rust
use bnf_analyzer::core as ak;            // 整个 core 换个短名：ak::Analyzer
use bnf_analyzer::core::value::Value;    // 或者只导入要用的那一个
use bnf_analyzer::core::grammar::Expr;   // 名字冲突时还能再取别名

let _ = ak::Document::new(Value::text("x"));
let _ = Expr::Literal("{".to_string());
```

这样安排是故意的：`Grammar`、`Value`、`Rule`、`Config` 这类名字太常见，库不替使用者占位——
根上如果 re-export 一遍，同一个类型在文档里就会挂两个路径，读者永远得先猜"该用哪个"。

---

## 1. 两个入口

| 入口 | 输入 | 动作 | feature | 典型场景 |
|---|---|---|---|---|
| `Analyzer::new(rules, grammar, semantics)` | 规则 + 文法 + 动作表 | 任意 | `core` | 库的主用法 |
| `Analyzer::from_config(config)` | 一份 `Config` | 无 | `json` | 配置驱动，输出默认语法树 |

内置配置不是"另一个分析器"，而是一份配置：`Config::builtin()` 给出编译期内嵌的
`postscript.json`，再交给 `from_config` 装配。少一个入口，就少一处"这份分析器到底用的哪份配置"的歧义。

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::json::Config;

// 内置配置：不读文件，编译期就嵌在库里。
let builtin = Analyzer::from_config(Config::builtin()).expect("the builtin configuration should be consistent");
assert!(builtin.analyze("1 2").is_ok());

// 同一份配置显式走一遍：Config 可以从字符串或文件来。
let config = Config::parse(
    r#"{
        "version": 2,
        "rules": [
            { "name": "space", "pattern": { "plus": { "chars": "\\s" } }, "skip": true },
            { "name": "number", "pattern": { "plus": { "chars": "0-9" } } }
        ],
        "grammar": { "productions": [
            { "name": "program", "rhs": { "star": { "token": "number" } } }
        ] }
    }"#,
)
.expect("the configuration should be valid");
let from_config = Analyzer::from_config(config).expect("the configuration should be consistent");
assert_eq!(from_config.analyze("1 2 3").expect("analysis should succeed").token_count(), 3);
```

`Analyzer::new` 的第一步是自检：文法里引用的词法类别必须存在于 `rules`
（见第 6 节的 `UnknownTokenClass`），文法的字面量终结符会被自动补成词法规则。
`from_config` 只是 `rules` + `grammar` 两步装配之后转调 `new`，所以两条入口的自检完全一致。

---

## 2. 流水线可以拆开

`analyze` 只是 `tokenize` + `parse` 的组合。三步都能单独调用，
`Tokenizer` / `Parser` 本身也是公开类型，可以脱离 `Analyzer` 直接用。

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::tokenizer::Token;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::token("number")))
    .build()
    .expect("the grammar should be valid");

let analyzer = Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent");

// ① 只切词
let tokens: Vec<Token> = analyzer.tokenize("1 2 3").expect("tokenization should succeed");
assert_eq!(tokens.len(), 3);
assert_eq!(tokens[1].text, "2");
assert_eq!(tokens[1].kind, "number");

// ② 只解析（可以接自己产出的 token 流，只要 kind / text / span 对得上）
let value = analyzer.parse(&tokens).expect("parsing should succeed");
assert_eq!(value.rule_name(), Some("program"));

// ③ 一步到底
let document = analyzer.analyze("1 2 3").expect("analysis should succeed");
assert_eq!(document.token_count(), 3);
```

拆开用的实际价值：增量场景下可以只重跑解析；调试词法时不必先满足文法；
也可以拿 `analyzer.rules()` / `analyzer.grammar()` 读回当前生效的规则与文法。

---

## 3. 动作：把语法树变成值

两类动作都以字符串为键，返回 `Result<Value, String>`：

| 动作 | 触发时机 | 键 | 注册方法 |
|---|---|---|---|
| token 动作 | 终结符匹配成功后 | 词法类别名，或字面量原文 | `Semantics::token` |
| 语义动作 | 产生式归约时 | 产生式左部 | `Semantics::rule` |

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
    .production("program", Expr::plus(Expr::rule("number")))
    .production("number", Expr::token("number"))
    .build()
    .expect("the grammar should be valid");

let semantics = Semantics::new()
    // token 动作：文本 → 整数
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| format!("not an integer: {error}"))
    })
    // 语义动作：把 `number` 节点的唯一子值提出来
    .rule("number", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("number should produce exactly one value".to_string()),
    })
    // 语义动作：求和
    .rule("program", |values| {
        let mut total = 0;
        for value in values {
            total += value.as_int().ok_or("integers only")?;
        }
        Ok(Value::Int(total))
    });

let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
let document = analyzer.analyze("1 2 3").expect("analysis should succeed");
assert_eq!(document.root, Value::Int(6));
```

要点：

- **动作有缓存**：同一次归约的动作只执行一次，重复遍历不会重复调用。
  因此回调要当成纯函数：不要在里面积累外部状态。
- **挂了动作就不再产出默认节点**。上面的 `program` 有语义动作，所以根是
  `Value::Int(6)` 而不是 `Value::Rule`；`number` 节点也同理。
- **字面量也能挂动作**，键就是原文（`"{"`、`"<<"`），因为字面量 token 的 `kind`
  就是原文。
- 动作返回 `Err(String)` 会变成 `ParseError::TokenAction` / `ParseError::RuleAction`，
  带出错位置；`String` 的内容原样进入错误信息。

---

## 4. `Value` 与 `Document`

`Value` 既是动作的输入输出，也是默认语法树的节点类型。

| 变体 | 含义 | JSON 序列化 |
|---|---|---|
| `Null` | 空 | `null` |
| `Bool(bool)` | 布尔 | `true` / `false` |
| `Int(i64)` | 整数 | 数字 |
| `Real(f64)` | 浮点 | 数字 |
| `Text(String)` | 文本 | 字符串 |
| `List(Vec<Value>)` | 列表 | 数组 |
| `Map(BTreeMap<String, Value>)` | 字典 | 对象（键有序） |
| `Token(Token)` | 未改写的叶子 | `{"kind":…,"text":…,"span":…}` |
| `Rule(RuleNode)` | 默认语法树节点 | `{"kind":"rule","rule":…,"span":…,"children":[…]}` |

访问器一律返回 `Option`：`as_bool`、`as_int`、`as_real`、`as_text`、`as_list`、
`as_map`、`as_token`、`as_rule`；另有 `rule_name()`（节点名或 token 类别）、
`type_name()`（变体名，用于错误提示，如 `integer`）、`children()`、`span()`、`find_rule(name)`。

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::value::Value;

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { dup }").expect("analysis should succeed");

// 根节点是 program，往下能找到 procedure。
let root = document.rule().expect("without actions the root is a rule node");
assert_eq!(root.rule, "program");
assert!(document.root.find_rule("procedure").is_some());

// 前序遍历：节点与 token 都会经过。
let mut rules = 0;
document.walk(|value| {
    if value.as_rule().is_some() {
        rules += 1;
    }
});
assert!(rules >= 3, "program, value and procedure give at least three nodes");

// 直接序列化，字段与命令行输出一致。
let json = document.to_json();
assert!(json.starts_with(r#"{"kind":"rule","rule":"program""#));
```

`Document` 是 `analyze` 的返回值，只有 `root` 一个字段，其余都是便捷方法：
`rule()`、`walk()`、`token_count()`、`accept()`、`rules_named()`、`tokens()`（core），
以及 `to_json()`、`to_json_pretty()`（需要 `json` feature，返回 `String`）。

`walk` 是最省事的一层：闭包里拿到 `&Value`，看完就走。它的三个局限——
不能剪枝、不区分节点 / token、引用带不出闭包——正是下一节访问者要解决的问题。

---

## 5. 事后遍历：访问者

`Document::accept(&mut visitor)`（或 `Value::accept`）把递归交给库，把"看到什么算什么"
交给你。`Visitor` 的四个回调都**没有必填项**，空 `struct` 也能实现：

| 回调 | 时机 | 返回值 |
|---|---|---|
| `visit(&Value)` | **每个**值，先于该值的具体回调 | `Flow` |
| `enter_rule(&RuleNode)` | 进入 `Value::Rule`（`visit` 之后） | `Flow` |
| `leave_rule(&RuleNode)` | 离开 `Value::Rule`，子节点都处理完了 | 无 |
| `visit_token(&Token)` | 遇到 `Value::Token` 叶子 | 无 |

返回值 [`Flow`](crate::core::visit::Flow) 决定要不要继续往下走：

| `Flow` | 含义 |
|---|---|
| `Continue` | 照常下探子节点 |
| `Skip` | 跳过这个值的**子节点**；这个值自身的回调仍然发生 |
| `Stop` | 立刻结束整趟遍历；已进入的节点仍会收到 `leave_rule` |

`enter_rule` / `leave_rule` 永远成对（包括被 `Skip` 跳过的节点、以及 `Stop` 之后
栈上的节点），所以可以在 `leave_rule` 里做后序计算。

### 5.1 剪枝：只看 `procedure` 外面的 token

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::tokenizer::Token;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::core::visit::{Flow, Visitor};

#[derive(Default)]
struct OutsideProcedures(Vec<String>);

impl<'ast> Visitor<'ast> for OutsideProcedures {
    // `visit` 先于进入节点，所以这里返回 Skip 就不会下探 procedure 的 children。
    fn visit(&mut self, value: &'ast Value) -> Flow {
        match value.as_rule().map(|node| node.rule.as_str()) {
            Some("procedure") => Flow::Skip,
            _ => Flow::Continue,
        }
    }

    fn visit_token(&mut self, token: &'ast Token) {
        self.0.push(token.text.clone());
    }
}

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { 1 2 } /g").expect("analysis should succeed");
let mut visitor = OutsideProcedures::default();
assert_eq!(document.accept(&mut visitor), Flow::Continue);

assert!(visitor.0.contains(&"/f".to_string()), "/f before procedure is still visited");
assert!(visitor.0.contains(&"/g".to_string()), "/g on the same level is still visited");
assert!(!visitor.0.contains(&"1".to_string()), "tokens inside the braces are pruned");
```

### 5.2 后序：离开节点时再算

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::value::RuleNode;
use bnf_analyzer::core::visit::{Flow, Visitor};

/// 找出 procedure 的最大嵌套层数：进入时压栈，离开时把子树的成绩交给父节点。
#[derive(Default)]
struct Deepest {
    stack: Vec<usize>,
    best: usize,
}

impl<'ast> Visitor<'ast> for Deepest {
    fn enter_rule(&mut self, _node: &'ast RuleNode) -> Flow {
        self.stack.push(0);
        Flow::Continue
    }

    fn leave_rule(&mut self, node: &'ast RuleNode) {
        let mut count = self.stack.pop().expect("enter and leave must pair up");
        if node.rule == "procedure" {
            count += 1;
        }
        self.best = self.best.max(count);
        if let Some(parent) = self.stack.last_mut() {
            *parent += count;
        }
    }
}

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { { } }").expect("analysis should succeed");
let mut visitor = Deepest::default();
document.accept(&mut visitor);
assert_eq!(visitor.best, 2, "procedure nested inside procedure");
assert!(visitor.stack.is_empty(), "the root is left as well");
```

### 5.3 常用查询直接用收集器

遍历 `&'ast Value` 的引用可以直接存进访问者（`Visitor<'ast>` 的 `'ast` 就是这棵结果的
生命周期），`RuleCollector` / `TokenCollector` 就是这么实现的，也已暴露成便捷方法：

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::visit::{RuleCollector, TokenCollector};

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/a { 1 } /b { 2 }").expect("analysis should succeed");

// 便捷方法
assert_eq!(document.rules_named("procedure").len(), 2);
assert_eq!(document.tokens().len(), document.token_count());

// 等价的手写收集器，可以再叠加自己的回调
let mut rules = RuleCollector::new("procedure");
document.accept(&mut rules);
let mut tokens = TokenCollector::new();
document.accept(&mut tokens);
assert_eq!(rules.len(), 2);
assert_eq!(tokens.len(), document.token_count());
```

### 5.4 动作产出的结果也能访问

访问者不假设"结果一定是树"：`Value::List` / `Map` / 标量都会经过 `visit`，
只是不会触发 `enter_rule` / `visit_token`。所以同一个访问者既能扫默认语法树，
也能扫动作折叠出的值。

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::core::visit::{Flow, Visitor};

#[derive(Default)]
struct Sums(Vec<i64>);

impl<'ast> Visitor<'ast> for Sums {
    fn visit(&mut self, value: &'ast Value) -> Flow {
        if let Some(number) = value.as_int() {
            self.0.push(number);
        }
        Flow::Continue
    }
}

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("number")))
    .production("number", Expr::token("number"))
    .build()
    .expect("the grammar should be valid");
let semantics = Semantics::new()
    .token("number", |token| {
        token.text.parse::<i64>().map(Value::Int).map_err(|e| e.to_string())
    })
    .rule("number", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("number produces exactly one value".to_string()),
    })
    .rule("program", |values| {
        let mut total = 0;
        for value in values {
            total += value.as_int().ok_or("integers only")?;
        }
        Ok(Value::Int(total))
    });

let document = Analyzer::new(rules, grammar, semantics)
    .expect("the configuration should be consistent")
    .analyze("1 2 3")
    .expect("analysis should succeed");
assert_eq!(document.root, Value::Int(6));

let mut sums = Sums::default();
document.accept(&mut sums);
assert_eq!(sums.0, [6], "the Int at the root is visited too");
```

### 5.5 两遍式分析：结构归解析，条件归复查

有的条件**不是任何单点的形状**能表达的："`use` 引用的名字必须已经 `def` 过"、
"同一个过程里不许重复定义"——判断依据是整份文档的信息（符号表、定义顺序、重复性）。
这类条件既不该塞给 `priority`（那是同长度竞争的胜负手），也不适合硬放进动作
（归约时动作只看到自己的参数）。推荐的模式是**两遍**：第一遍解析出树并收集定义，
第二遍带着收集到的信息复查使用，把违规点连同位置聚成一份诊断列表。

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::Analyzer;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("name", r"[a-z]+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("statement")))
    .production("statement", Expr::alt([Expr::rule("def"), Expr::rule("use")]))
    .production("def", Expr::seq([
        Expr::literal("def"),
        Expr::token("name"),
        Expr::token("number"),
    ]))
    .production("use", Expr::seq([Expr::literal("use"), Expr::token("name")]))
    .build()
    .expect("the grammar should be valid");
let document = Analyzer::new(rules, grammar, Semantics::new())
    .expect("the configuration should be consistent")
    .analyze("def width 10 use width use height")
    .expect("analysis should succeed");

// 第一遍：收集全部定义过的名字。
let mut defined = std::collections::HashSet::new();
for node in document.rules_named("def") {
    if let Some(token) = node.children[1].as_token() {
        defined.insert(token.text.as_str());
    }
}

// 第二遍：带着定义表复查每个使用。
let mut undefined = Vec::new();
for node in document.rules_named("use") {
    let Some(token) = node.children[1].as_token() else { continue };
    if !defined.contains(token.text.as_str()) {
        undefined.push(token.text.as_str());
    }
}
assert_eq!(undefined, ["height"], "only height is never defined");
```

两遍不依赖先后：`use` 完全可以出现在 `def` 之前，收集完成后复查自然覆盖全部使用。
token 上带着 `Span`，把名字换成 `&Token` 就能得到 `(名字, 行, 列)`，正适合 IDE 的
诊断面板或批量报告——比动作 `Err` 的"第一个错误即中断"友好得多。

注意分工：**解析器只回答"结构对不对"**（`use height` 是合法语句，哪怕 `height`
从未定义）；"条件对不对"由你的复查代码回答。这与 Rust 编译器的分层一致——解析是
纯 CFG，名字解析在后面。"条件不满足就换一种解析"的语义谓词（ANTLR 的 `{...}?`）
是刻意不提供的：谓词依赖语义值会破坏 Earley 的复杂度与确定性假设；绝大多数场景下，
"解析 + 两遍复查"就是标准解法。

**怎么选**：只想"看一眼每个值"用 `walk`；要剪枝、要后序、要把节点引用带出来、
要区分节点与叶子，用 `accept` + `Visitor`。

---

## 6. 错误：分层与定位

| 类型 | 出现阶段 | 主要变体 |
|---|---|---|
| `JsonError` | 解析 JSON 文本 | `line` / `column` / `message`（一个结构体，不是枚举） |
| `ConfigError` | 读配置 | `Io`、`Json`（内含 `JsonError`）、`Invalid`、`UnsupportedVersion`、`LegacyVersion` |
| `RulesError` | 编译词法规则 | `Regex`、`DuplicateName`、`EmptyPattern`、`EmptyName` |
| `GrammarError` | 建文法 / 自检 | `UnknownStart`、`UnknownSymbol`、`UnknownTokenClass`、`Empty` |
| `SetupError` | `Analyzer::new` | 透传 `RulesError` / `GrammarError` |
| `LexError` | 切词 | `Unexpected { ch, span }`、`EmptyMatch` |
| `ParseError` | 解析 | `UnexpectedToken`、`UnexpectedEof`、`TokenAction`、`RuleAction` |
| `AnalyzeError` | `analyze` 汇总 | `Lex` / `Parse` 二选一 |

配置错误的两个阶段分得很清楚：`JsonError` 是"这段文本不是合法 JSON"
（由 `json` 模块的 JSON 文法给出），`ConfigError::Invalid` 是"JSON 合法但字段不对"
（由 `from_value` 逐字段检查，消息里带 `rules[0]: …` 这样的路径）。

定位统一用 `span()`：`LexError`、`ParseError`、`AnalyzeError` 都有这个方法，
返回 `Span { start, end, line, column }`（字节偏移 + 1 基行列）。
语法类错误还能问"此时期望什么"：

```rust
use bnf_analyzer::core::AnalyzeError;

let error = analyzer_error();
fn analyzer_error() -> AnalyzeError {
    bnf_analyzer::core::Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent")
        .analyze("/f { dup 1")
        .expect_err("unbalanced parentheses should fail")
}

assert_eq!(error.span().line, 1);
let expected = match &error {
    AnalyzeError::Parse(parse) => parse.expected(),
    _ => &[],
};
assert!(expected.iter().any(|label| label == "array"), "the expected list only shows symbols you can write");
```

`SetupError` 没有 span——它描述的是"规则/文法本身不成立"，与源码无关。

这些错误类型都实现了 `Display` 与 `std::error::Error`：`RulesError` / `ConfigError`
的 `source()` 会给出底层的 `std::io::Error`、`regex::Error`，透传型外层（`AnalyzeError`、
`SetupError`、`ConfigError::Json`）只做转发。

---

## 7. 定制词法层

```rust
use bnf_analyzer::core::rules::{Rule, Rules, LITERAL_PRIORITY};

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::skipping_regex("comment", r"%[^\n]*"),
    Rule::literal("lbrace", "{"),                 // 元字符自动转义
    Rule::regex("number", r"\d+").with_priority(10),
])
.expect("the rules should be valid");

assert_eq!(rules.entries().len(), 4);
assert_eq!(rules.rule("lbrace").expect("that rule should exist").priority, 0);
assert!(LITERAL_PRIORITY > 10, "literal rules derived from the grammar outrank ordinary rules");
```

裁决顺序：**最长匹配** → `priority` 大的 → 声明顺序靠前的。
文法里的字面量终结符（`"{"`、`"ifelse"`）由 `Rules::with_literals` 补成规则，
优先级固定为 `LITERAL_PRIORITY = 1000`，所以关键字不会被标识符规则吞掉。

---

## 8. 用库自己实现一门格式：`json` 模块

`bnf_analyzer::json::grammar` 模块是一份完整的示例——想给别的格式（YAML 子集、S 表达式、
日志、配置文件……）写解析器时，照抄它的三段即可：

| 段落 | 代码 | 对应 API |
|---|---|---|
| 词法 | `json::grammar::rules()` | `Rules::from_rules` + `Rule` / `Pattern` |
| 文法 | `json::grammar::grammar()` | `Grammar::builder()` + `Expr` |
| 语义 | `json::grammar::semantics()` | `Semantics::token` / `Semantics::rule` |

```rust
use bnf_analyzer::json;

// 1. 解析：JSON 文本 → Value
let value = json::parse(r#"{"n": 42, "xs": [true, null]}"#).expect("valid JSON");
let map = value.as_map().expect("map");
assert_eq!(map["n"].as_int(), Some(42));

// 2. 输出：Value → JSON 文本（紧凑 / 缩进两格）
assert_eq!(json::to_string(&value), r#"{"n":42,"xs":[true,null]}"#);
assert!(json::to_string_pretty(&value).starts_with("{\n  \"n\": 42,"));

// 3. 文法本身是公开的：可以直接看，也可以拿来改
let bnf = json::grammar::analyzer().grammar().to_bnf();
assert!(bnf.contains(r#"value ::= object | array | @string | @number"#), "{bnf}");

// 4. 想要更严/更松的 JSON？换成自己的分析器
let mine = json::grammar::analyzer();
assert_eq!(json::parse_with(&mine, "[1, 2]").expect("should be valid").as_list().unwrap().len(), 2);
```

那份文法长这样（`to_bnf()` 的输出，同名产生式合并成一行）：

```text
json ::= value ;
value ::= object | array | @string | @number | "true" | "false" | "null" ;
object ::= "{" "}" | "{" members "}" ;
members ::= member | members "," member ;
member ::= @string ":" value ;
array ::= "[" "]" | "[" elements "]" ;
elements ::= value | elements "," value ;
```

几个值得抄的点：

- **词法只有三条规则**：空白（`skip`）、字符串、数字。`true`/`false`/`null` 与
  `{ } [ ] , :` 都是字面量终结符，`Grammar::literals()` 在装配时自动派生成词法规则。
- **左递归累积列表**：`members ::= member | members "," member` 比 `star` 更好写动作——
  每次归约只拿"已累积的列表 + 一个元素"，不必和摊平后的参数序列猜位置。
- **空容器单独一条产生式**：`"{" "}"` 与 `"{" members "}"` 形状不同，动作靠参数个数就能分辨。
- **错误位置免费**：token 动作返回 `Err(String)` 时，解析器附上该 token 的 span，
  `json::parse` 再把它搬成 `JsonError { line, column, message }`。

`Config` 读的就是这条路径：`Config::parse` = `json::parse` + 逐字段读 `Value`，
所以 `postscript.json` 的语法错误与配置语义错误共用同一套位置信息。

---

## 9. 边界与取舍

- **Earley，不改写文法**：左递归、ε 产生式、歧义都能直接吃。
  代价是最坏 O(n³)，以及**歧义时只返回最先完成的推导**，不枚举全部解析。
- **没有错误恢复**：语法错误直接返回第一个失败点，不做"跳过 token 继续"。
  需要多错误报告时，应在调用方按行/按块分段分析。
- **动作必须纯**：归约缓存假设同一归约点的动作只执行一次、结果可复用。
- **EOF 的 span 是零宽区间**，位置取最后一个 token 的末尾；空输入取 `(0,0,1,1)`。
- **默认配置的字符串只支持一层嵌套**：`(a (b) c)` 可以，`(a (b (c)) d)` 会词法失败。
  这正好是"token 动作"的用武之地——在动作里自己做括号配对，或者把 `string` 换成
  一个能处理任意深度的扫描器钩子。
- **`Analyzer` 可复用**：文法与规则在装配后不可变，`analyze` 不共享可变状态，
  同一个实例可以连续分析多份源码；`Parser` 本身是零大小类型、无状态。
- **访问者是只读的**：`Visitor` 拿到的是 `&'ast`。要改写树，就在遍历里收集
  `(&Span, …)` 再自己重建，或者用 `Value` 的 `Clone` + 手工递归。
- **遍历顺序是深度优先、文档顺序**：`Rule` 与 `List` 按元素顺序；
  `Map` 是 `BTreeMap`，因此按 key 的字典序，而不是插入顺序。
- **依赖面很小**：`Cargo.lock` 里只会多出 `regex` 一个 crate，不会冒出 JSON 库或
  错误处理框架。
