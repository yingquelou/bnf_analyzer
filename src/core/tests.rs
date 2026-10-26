//! core 的单元测试：装配是否把字面量、规则、文法拼对，模型是否给得出位置。

use super::{AnalyzeError, Analyzer, Document};
use crate::core::grammar::{Expr, Grammar};
use crate::core::rules::{Rule, Rules};
use crate::core::semantics::Semantics;
use crate::core::tokenizer::Span;
use crate::core::value::Value;

fn analyzer() -> Analyzer {
    let rules = Rules::from_rules(vec![
        Rule::skipping_regex("space", r"\s+"),
        Rule::regex("number", r"\d+"),
        Rule::regex("name", r"/[a-z]+"),
    ])
    .expect("the rules should be valid");
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("value")))
        .production(
            "value",
            Expr::alt([Expr::token("number"), Expr::token("name")]),
        )
        .build()
        .expect("the grammar should be valid");
    Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent")
}

#[test]
fn counts_tokens_across_nested_values() {
    let document = analyzer().analyze("/a 1 2").expect("should succeed");
    assert_eq!(document.rule().expect("should be a rule node").rule, "program");
    assert_eq!(document.token_count(), 3);
}

#[test]
fn error_exposes_span() {
    let error = analyzer().analyze("1 !").expect_err("should fail");
    assert_eq!(error.span().line, 1);
    assert!(matches!(error, AnalyzeError::Lex(_)));
}

#[test]
fn document_new_wraps_value_without_analyzer() {
    let span = Span::new(0, 0, 1, 1);
    let document = Document::new(Value::rule("program", span, Vec::new()));
    assert_eq!(document.token_count(), 0);
    assert!(document.rule().is_some());
}
