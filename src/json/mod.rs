//! JSON 层（`json` feature）：**用本库自己的文法读写 JSON**，不依赖任何外部 JSON 库。
//!
//! | 想用 | 入口 |
//! |---|---|
//! | 文本 → 值树 | [`parse`](crate::json::parse) / [`parse_with`](crate::json::parse_with) |
//! | 值树 → 文本 | [`to_string`](crate::json::to_string) / [`to_string_pretty`](crate::json::to_string_pretty) |
//! | 分析结果 → JSON | `Document::to_json` / `Document::to_json_pretty` |
//! | 配置文件 | [`Config`](crate::json::Config)：`builtin()` / `parse()` / `load()`，交给 `Analyzer::from_config` |
//!
//! ```
//! use bnf_analyzer::json;
//!
//! let value = json::parse(r#"{ "a": [1, 2.5, true, null, "x\u0041"] }"#).expect("合法 JSON");
//! assert_eq!(
//!     json::to_string(&value),
//!     r#"{"a":[1,2.5,true,null,"xA"]}"#
//! );
//! ```
//!
//! 这一层还有第二个用处：它是一份**完整的用法示例**。JSON 的文法、词法规则与动作都是拿
//! [`Analyzer`](crate::core::Analyzer) 自己的 `Rules` + `Grammar` + `Semantics` 写的，
//! 跑的是和别的语言完全相同的 Earley 流水线；想给别的格式（YAML 子集、S 表达式、日志……）
//! 写解析器时，照 [`grammar`](crate::json::grammar) 模块抄三段即可。
//!
//! 词法只有三条规则（空白、字符串、数字）：`true`/`false`/`null` 与六个标点都是文法里的
//! 字面量终结符，由 [`Grammar::literals`](crate::core::grammar::Grammar::literals) 自动派生成
//! 词法规则——这也是"字面量不必在 rules 里重复声明"的一个活例子。

pub mod grammar;
mod analyzer;
mod config;
mod error;
mod parse;
mod write;

pub use config::{Config, ConfigError, CONFIG_KEYS, DEFAULT_CONFIG, SUPPORTED_VERSION};
pub use error::JsonError;
pub use parse::{parse, parse_with};
pub use write::{to_string, to_string_pretty};

#[cfg(test)]
mod tests;
