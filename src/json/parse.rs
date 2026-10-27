//! JSON 文本 → [`Value`]。
//!
//! 入口只有两个：[`parse`] 用内置的 JSON 分析器，[`parse_with`] 允许传入别的分析器
//! （比如你自己改造过的、更严格的 JSON 文法）。

use crate::core::value::Value;
use crate::core::Analyzer;

use super::error::JsonError;
use super::grammar;

/// 用内置 JSON 文法解析一段文本。
///
/// ```
/// use bnf_analyzer::json;
///
/// let value = json::parse(r#"{"n": 42, "xs": [true, null]}"#).expect("合法 JSON");
/// assert_eq!(value.as_map().expect("对象").len(), 2);
/// ```
pub fn parse(text: &str) -> Result<Value, JsonError> {
    parse_with(&grammar::analyzer(), text)
}

/// 用指定的分析器解析：文法是外部给的，本函数只负责把错误翻成 [`JsonError`]。
pub fn parse_with(analyzer: &Analyzer, text: &str) -> Result<Value, JsonError> {
    analyzer
        .analyze(text)
        .map(|document| document.root)
        .map_err(JsonError::from)
}
