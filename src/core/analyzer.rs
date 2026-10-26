//! 装配门面：把规则、文法、动作表绑成一个可复用的分析器。

use crate::core::grammar::Grammar;
use crate::core::parser::{ParseError, Parser};
use crate::core::rules::Rules;
use crate::core::semantics::Semantics;
use crate::core::tokenizer::{LexError, Token, Tokenizer};
use crate::core::value::Value;

use super::document::Document;
use super::error::{AnalyzeError, SetupError};

/// 词法 + 语法的门面，持有规则、文法和动作表。
///
/// `Analyzer::new` 是核心入口：规则、文法、动作都由调用方显式给出。
/// 配置入口 `from_config` 在 `json` feature 下提供（收一份配置文件模型，
/// 内置的 PostScript 配置由 `json::Config::builtin()` 给出）。
#[derive(Debug, Clone)]
pub struct Analyzer {
    tokenizer: Tokenizer,
    grammar: Grammar,
    semantics: Semantics,
    parser: Parser,
}

impl Analyzer {
    /// 使用显式的规则、文法与动作表装配（Rust 内嵌文法用这个入口）。
    pub fn new(
        rules: Rules,
        grammar: Grammar,
        semantics: Semantics,
    ) -> Result<Self, SetupError> {
        grammar.validate(&rules)?;
        // 文法里的字面量终结符自动变成词法规则，优先级高于普通规则。
        let rules = rules.with_literals(&grammar.literals())?;
        Ok(Self {
            tokenizer: Tokenizer::new(rules),
            grammar,
            semantics,
            parser: Parser::new(),
        })
    }

    /// 当前生效的词法规则（已含文法派生的字面量规则）。
    pub fn rules(&self) -> &Rules {
        self.tokenizer.rules()
    }

    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    pub fn semantics(&self) -> &Semantics {
        &self.semantics
    }

    /// 只做词法分析。
    pub fn tokenize(&self, source: &str) -> Result<Vec<Token>, LexError> {
        self.tokenizer.tokenize(source)
    }

    /// 只做语法分析。
    pub fn parse(&self, tokens: &[Token]) -> Result<Value, ParseError> {
        self.parser.parse(&self.grammar, &self.semantics, tokens)
    }

    /// 词法 + 语法，一步得到结果。
    pub fn analyze(&self, source: &str) -> Result<Document, AnalyzeError> {
        let tokens = self.tokenize(source)?;
        Ok(Document::new(self.parse(&tokens)?))
    }
}
