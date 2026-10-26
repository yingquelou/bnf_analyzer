//! 两类错误：跑起来才发现的（[`AnalyzeError`]）与装配时就不成立的（[`SetupError`]）。

use std::fmt;

use crate::core::grammar::GrammarError;
use crate::core::parser::ParseError;
use crate::core::rules::RulesError;
use crate::core::tokenizer::{LexError, Span};

/// 分析过程中可能出现的两类错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzeError {
    /// 词法阶段失败。
    Lex(LexError),
    /// 语法阶段失败。
    Parse(ParseError),
}

impl AnalyzeError {
    /// 出错位置，便于上层高亮源码。
    pub fn span(&self) -> Span {
        match self {
            AnalyzeError::Lex(error) => error.span(),
            AnalyzeError::Parse(error) => error.span(),
        }
    }
}

impl fmt::Display for AnalyzeError {
    /// 两层错误都自带位置，直接透传内层的文本。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnalyzeError::Lex(error) => error.fmt(f),
            AnalyzeError::Parse(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AnalyzeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AnalyzeError::Lex(error) => Some(error),
            AnalyzeError::Parse(error) => Some(error),
        }
    }
}

impl From<LexError> for AnalyzeError {
    fn from(error: LexError) -> Self {
        AnalyzeError::Lex(error)
    }
}

impl From<ParseError> for AnalyzeError {
    fn from(error: ParseError) -> Self {
        AnalyzeError::Parse(error)
    }
}

/// 装配阶段的错误：规则或文法本身不成立。
#[derive(Debug)]
pub enum SetupError {
    /// 词法规则不成立。
    Rules(RulesError),
    /// 文法不成立。
    Grammar(GrammarError),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::Rules(error) => error.fmt(f),
            SetupError::Grammar(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SetupError::Rules(error) => Some(error),
            SetupError::Grammar(error) => Some(error),
        }
    }
}

impl From<RulesError> for SetupError {
    fn from(error: RulesError) -> Self {
        SetupError::Rules(error)
    }
}

impl From<GrammarError> for SetupError {
    fn from(error: GrammarError) -> Self {
        SetupError::Grammar(error)
    }
}
