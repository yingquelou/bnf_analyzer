//! 核心库：词法、文法、Earley 解析、值树、访问者。默认 feature，写文法、挂动作、
//! 遍历结果都在这一层完成。
//!
//! 入口是 [`Analyzer`]：`Analyzer::new(rules, grammar, semantics)` 装配一次，之后
//! `tokenize` / `parse` / `analyze` 反复调用。`rules` 管词法（正则、字面量、字符类……），
//! `grammar` 管文法（产生式与组合子），`semantics` 是动作表——留空就输出带位置的默认语法树。
//! 配置文件（`postscript.json` 那种）不在这层：那是 `json` feature 的事，不用配置文件也完全可用。
//!
//! 公共路径统一是 `bnf_analyzer::core::…`：`core` 根上只放 [`Analyzer`]、[`Document`]、
//! [`AnalyzeError`]、[`SetupError`] 四个类型，其余按环节分模块。
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`grammar`] | [`Grammar`](grammar::Grammar) / [`Expr`](grammar::Expr)：产生式、EBNF 组合子、`to_bnf` |
//! | [`rules`] | [`Rules`](rules::Rules) / [`Rule`](rules::Rule)：词法规则与优先级 |
//! | [`pattern`] | [`Pattern`](pattern::Pattern)：一条词法模式怎么写、怎么编译 |
//! | [`tokenizer`] | [`Token`](tokenizer::Token) / [`Span`](tokenizer::Span)：切分结果与位置 |
//! | [`semantics`] | [`Semantics`](semantics::Semantics)：token 动作与规则动作 |
//! | [`value`] | [`Value`](value::Value) / [`RuleNode`](value::RuleNode)：值树 |
//! | [`visit`] | [`Visitor`](visit::Visitor) / [`Flow`](visit::Flow) / 两个收集器：事后遍历 |
//! | [`parser`] | Earley 分析器；由 `Analyzer` 驱动，通常不用直接碰 |
//!
//! 想写短名就自己起：`use bnf_analyzer::core as ak;` 之后写 `ak::Analyzer`。
//!
//! ```
//! use bnf_analyzer::core::Analyzer;
//! use bnf_analyzer::core::grammar::{Expr, Grammar};
//! use bnf_analyzer::core::rules::{Rule, Rules};
//! use bnf_analyzer::core::semantics::Semantics;
//!
//! let rules = Rules::from_rules(vec![
//!     Rule::skipping_regex("space", r"\s+"),
//!     Rule::regex("number", r"\d+"),
//! ])
//! .expect("规则合法");
//!
//! let grammar = Grammar::builder()
//!     .start("program")
//!     .production("program", Expr::plus(Expr::rule("number")))
//!     .production("number", Expr::token("number"))
//!     .build()
//!     .expect("文法合法");
//!
//! // 动作表可空：此时输出带位置的默认语法树。
//! let analyzer = Analyzer::new(rules, grammar, Semantics::new()).expect("配置自洽");
//! let document = analyzer.analyze("1 2").expect("分析成功");
//! assert_eq!(document.rule().expect("默认产出语法树").rule, "program");
//! assert_eq!(document.token_count(), 2);
//! ```
//!
//! 要改这一层的实现（模块划分、糖怎么降级、约束靠什么钉住）看
//! [`guide::dev::architecture`](crate::guide::dev::architecture) 与
//! [`guide::dev::internals`](crate::guide::dev::internals)。

pub mod grammar;
pub mod parser;
pub mod pattern;
pub mod rules;
pub mod semantics;
pub mod tokenizer;
pub mod value;
pub mod visit;

mod analyzer;
mod document;
mod error;

// 子模块用 `self::` 前缀：`analyzer` / `document` / `error` 都只在本模块内可见。
pub use self::analyzer::Analyzer;
pub use self::document::Document;
pub use self::error::{AnalyzeError, SetupError};

#[cfg(test)]
mod tests;
