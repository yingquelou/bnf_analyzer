可配置的源码分析器：一份词法规则 + 一份文法，把源码切成 token、再解析成带位置的值树。

库本身不认识任何语言。内置的 `examples/postscript.json` 只是一份**配置示例**
（PostScript 的一个子集）；换成你自己的 `rules` 与 `grammar`，它就能分析你的语言。

包名是 `bnf_analyzer`（Rust 里 `use bnf_analyzer::…`），可执行文件名是 `bnf-analyzer`。

# 开哪个 feature

| feature | 给你什么 | 什么时候开 |
|---|---|---|
| `core`（默认） | 词法、文法、Earley、`Value`、访问者；`Analyzer::new` | `cargo add bnf_analyzer` 就有 |
| `json` | 自举的 JSON 读写、`Config`、`Analyzer::from_config` | 想用配置文件描述文法 |
| `cli` | `bnf-analyzer` 可执行文件（`--bnf`、`-t`、默认语法树） | 想直接从命令行跑 |

`cargo add bnf_analyzer` 只拿到 core，也不会产出可执行文件；要用配置文件再加 `--features json`，
要用命令行再叠 `--features cli`。

# 一、写 Rust：文法在代码里，可以挂动作

```
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+").with_priority(10),
])
.expect("规则合法");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::rule("expr"))
    .production(
        "expr",
        Expr::alt([
            Expr::token("number"),
            Expr::seq([Expr::rule("expr"), Expr::literal("+"), Expr::rule("expr")]),
        ]),
    )
    .build()
    .expect("文法合法");

// token 动作：把数字文本解析成整数
// 语义动作：把 `a + b` 折叠成结果
let semantics = Semantics::new()
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| error.to_string())
    })
    .rule("program", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("program 只能产出一个值".to_string()),
    })
    .rule("expr", |values| match values {
        [Value::Int(value)] => Ok(Value::Int(*value)),
        [Value::Int(left), Value::Token(op), Value::Int(right)] if op.text == "+" => {
            Ok(Value::Int(left + right))
        }
        _ => Err("无法求值".to_string()),
    });

let analyzer = Analyzer::new(rules, grammar, semantics).expect("配置自洽");
assert_eq!(analyzer.analyze("1 + 2").expect("分析成功").root, Value::Int(3));
```

动作表可以留空：那时输出的是带位置的默认语法树，不需要自己定义值。

# 二、跑命令行：文法在 JSON 里，不带动作，输出默认语法树

需要 `json`（配置）与 `cli`（可执行文件）两个 feature：

```text
$ cargo run --features cli -- --bnf        # 查看当前文法
$ cargo run --features cli -- sample.ps    # 输出带位置信息的语法树 JSON
$ cargo run --features cli -- -t sample.ps # 只输出 token 流
```

配置里的 `pattern` 与 `rhs` 只接受结构化对象（`{ "regex": … }`、`{ "seq": [ … ] }`），
没有"再解析一段文本"的写法：键名与 [`Pattern`](crate::core::pattern::Pattern) /
[`Expr`](crate::core::grammar::Expr) 的变体一一对应。每个键的含义、优先级怎么裁决、
报错怎么读，见 `guide::user::config`。

两条路最后都降级成同一份 [`Grammar`](crate::core::grammar::Grammar)，
由同一个 Earley 分析器消费。

# 导入路径

名字都留在自己的模块里，crate 根只有模块声明：

| 想用 | 路径 |
|---|---|
| 分析器与结果 | `bnf_analyzer::core::{Analyzer, Document}` |
| 值树 | `bnf_analyzer::core::value::{Value, RuleNode}` |
| 访问者 | `bnf_analyzer::core::visit::{Visitor, Flow}` |
| 文法 | `bnf_analyzer::core::grammar::{Grammar, Expr}` |
| 词法 | `bnf_analyzer::core::rules::{Rules, Rule}`、`bnf_analyzer::core::pattern::Pattern` |
| JSON 读写与配置 | `bnf_analyzer::json::{parse, to_string, Config}` |

想写短就自己起别名：

```
use bnf_analyzer::core as ak; // 别名：之后写 ak::Analyzer
use bnf_analyzer::core::value::Value; // 具体名字：只导入需要的

let document = ak::Document::new(Value::text("x"));
assert_eq!(document.token_count(), 0);
```

# 结果怎么用

[`Document`](crate::core::Document) 是分析产物：`root` 是值树，`walk` 跑一遍闭包；
需要剪枝、或者要"进入/离开成对回调"时用 [`Visitor`](crate::core::visit::Visitor)——
`accept` 负责递归，`Flow` 决定继续、跳过子树还是中止。

# 文档地图

手册按读者分两套，都挂在 [`guide`] 下（源文件在仓库 `docs/`）：

| 我是谁 | 读 | 内容 |
|---|---|---|
| **使用者**（把本库当依赖、或只用命令行） | `guide::user::api` | 库 API：入口、动作、`Value`、访问者、错误 |
| | `guide::user::grammar` | 文法怎么写：JSON / Rust 两种写法、BNF 视图、动作 |
| | `guide::user::cli`、`guide::user::config` | 命令行与输出格式、配置文件参考 |
| **本库的开发者**（改这个 crate） | [`guide::dev::architecture`] | 分层、模块树、硬约束、测试矩阵、改动清单 |
| | [`guide::dev::internals`] | 文法降级、合成产生式、校验时机等实现细节 |

用户文档四份都要读配置文件，随 `json` feature 提供；开发者文档只讲 core，默认就有。

## 许可证

MIT OR Apache-2.0，任选其一。
