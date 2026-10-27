//! 用法二（作为依赖库）：文法写在 Rust 里，挂 token 动作与语义动作。
//!
//! 覆盖：动作取值、默认语法树兜底、动作报错、字典/列表组装、文法校验。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bnf_analyzer::core::{AnalyzeError, Analyzer};
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::parser::ParseError;
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

fn rules() -> Rules {
    Rules::from_rules(vec![
        Rule::skipping_regex("space", r"\s+"),
        Rule::regex("number", r"\d+").with_priority(10),
        Rule::regex("name", r"/[^\s()<>\[\]{}/%]*").with_priority(20),
    ])
    .expect("the rules should be valid")
}

/// `expr ::= number | expr "+" expr`，靠动作做常量折叠。
fn calculator(counter: Option<Arc<AtomicUsize>>) -> Analyzer {
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
        .expect("the grammar should be valid");

    let mut semantics = Semantics::new()
        .token("number", |token| {
            token
                .text
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|error| format!("not an integer: {error}"))
        })
        .rule("program", |values| match values {
            [value] => Ok(value.clone()),
            _ => Err("program produces exactly one value".to_string()),
        })
        .rule("expr", |values| match values {
            [Value::Int(value)] => Ok(Value::Int(*value)),
            [Value::Int(left), Value::Token(op), Value::Int(right)] => match op.text.as_str() {
                "+" => Ok(Value::Int(left + right)),
                other => Err(format!("unknown operator {other}")),
            },
            _ => Err("the expression cannot be evaluated".to_string()),
        });

    if let Some(counter) = counter {
        semantics = semantics.rule("expr", move |values| {
            counter.fetch_add(1, Ordering::Relaxed);
            match values {
                [Value::Int(value)] => Ok(Value::Int(*value)),
                _ => Err("the counter version takes a single value".to_string()),
            }
        });
    }

    Analyzer::new(rules(), grammar, semantics).expect("the configuration should be consistent")
}

#[test]
fn token_action_and_semantic_action_compute_values() {
    let analyzer = calculator(None);
    assert_eq!(analyzer.analyze("1 + 2").expect("analysis should succeed").root, Value::Int(3));
    assert_eq!(
        analyzer.analyze("1 + 2 + 3").expect("analysis should succeed").root,
        Value::Int(6)
    );
}

#[test]
fn actions_are_cached_per_reduction() {
    let counter = Arc::new(AtomicUsize::new(0));
    let analyzer = calculator(Some(Arc::clone(&counter)));
    analyzer.analyze("1").expect("analysis should succeed");
    // `1` 只有一个 expr 归约点，即使分析器内部重复遍历，动作也只执行一次。
    assert_eq!(counter.load(Ordering::Relaxed), 1);
}

#[test]
fn token_actions_run_once_per_token() {
    // 解析器内部会重复遍历同一批项（`while changed` 会重跑集合）。token 动作必须只在
    // 真正插入新项时执行一次，否则计数类动作会翻倍，大输入上也是白白多跑一遍。
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let semantics = Semantics::new().token("number", move |token| {
        counter.fetch_add(1, Ordering::Relaxed);
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| error.to_string())
    });
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::plus(Expr::rule("value")))
        .production("value", Expr::token("number"))
        .build()
        .expect("the grammar should be valid");
    let analyzer =
        Analyzer::new(rules(), grammar, semantics).expect("the configuration should be consistent");

    analyzer.analyze("1 2 3").expect("analysis should succeed");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "each of the three tokens must invoke its token action exactly once"
    );
}

#[test]
fn without_actions_rust_grammar_still_yields_a_tree() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::seq([Expr::token("name"), Expr::literal(";")])))
        .build()
        .expect("the grammar should be valid");
    let analyzer = Analyzer::new(rules(), grammar, Semantics::new()).expect("the configuration should be consistent");

    let document = analyzer.analyze("/a ; /b ;").expect("analysis should succeed");
    let root = document.rule().expect("the default output is a syntax tree");
    assert_eq!(root.rule, "program");
    // 分组与重复都被摊平：两轮 (name ";") 共 4 个 token
    assert_eq!(root.children.len(), 4);
    assert_eq!(document.token_count(), 4);
}

#[test]
fn semantic_action_can_build_structured_values() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("pair")))
        .production("pair", Expr::seq([Expr::token("name"), Expr::token("number")]))
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
        .rule("program", |values| Ok(Value::list(values.to_vec())))
        .rule("pair", |values| match values {
            [Value::Token(name), Value::Int(number)] => {
                let mut map = BTreeMap::new();
                map.insert(name.text.trim_start_matches('/').to_string(), Value::Int(*number));
                Ok(Value::Map(map))
            }
            _ => Err("a pair looks like /key 1".to_string()),
        });

    let analyzer = Analyzer::new(rules(), grammar, semantics).expect("the configuration should be consistent");
    let document = analyzer.analyze("/a 1 /b 2").expect("analysis should succeed");

    match &document.root {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(
                items[0].as_map().expect("should be a map").get("a"),
                Some(&Value::Int(1))
            );
            assert_eq!(
                items[1].as_map().expect("should be a map").get("b"),
                Some(&Value::Int(2))
            );
        }
        other => panic!("expected a list, found {other:?}"),
    }
}

#[test]
fn token_action_errors_are_reported_with_position() {
    let analyzer = calculator(None);
    let error = analyzer.analyze("1 + 99999999999999999999").expect_err("should fail");
    match error {
        AnalyzeError::Parse(ParseError::TokenAction { kind, span, .. }) => {
            assert_eq!(kind, "number");
            assert_eq!(span.column, 5);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn semantic_action_errors_are_reported() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::token("number"))
        .build()
        .expect("the grammar should be valid");
    let semantics = Semantics::new()
        .rule("program", |_| Err("deliberate failure".to_string()));
    let analyzer = Analyzer::new(rules(), grammar, semantics).expect("the configuration should be consistent");

    let error = analyzer.analyze("7").expect_err("should fail");
    assert!(matches!(
        error,
        AnalyzeError::Parse(ParseError::RuleAction { ref rule, .. }) if rule == "program"
    ));
    assert!(error.to_string().contains("deliberate failure"));
}

#[test]
fn rejects_grammar_referencing_missing_nonterminal() {
    let error = Grammar::builder()
        .production("program", Expr::rule("missing"))
        .build()
        .expect_err("should fail");
    assert!(error.to_string().contains("missing"));
}

#[test]
fn rejects_actions_for_missing_token_class() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::token("number"))
        .build()
        .expect("the grammar should be valid");
    let analyzer = Analyzer::new(
        Rules::from_rules(vec![Rule::skipping_regex("space", r"\s+")]).expect("the rules should be valid"),
        grammar,
        Semantics::new(),
    );
    assert!(analyzer.is_err(), "number is not declared in rules, setup should fail");
}

#[test]
fn literals_are_auto_registered_with_high_priority() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::literal("then")))
        .build()
        .expect("the grammar should be valid");
    let analyzer = Analyzer::new(
        Rules::from_rules(vec![
            Rule::skipping_regex("space", r"\s+"),
            Rule::regex("word", "[a-z]+"),
        ])
        .expect("the rules should be valid"),
        grammar,
        Semantics::new(),
    )
    .expect("the configuration should be consistent");

    // `then` 与 `word` 同长度，字面量规则优先级更高，因此被切成字面量 token。
    let tokens = analyzer.tokenize("then").expect("tokenization should succeed");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, "then");
}

#[test]
fn grammar_exposes_a_bnf_view() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("pair")))
        .production("pair", Expr::seq([Expr::literal("("), Expr::token("number"), Expr::literal(")")]))
        .build()
        .expect("the grammar should be valid");
    let bnf = grammar.to_bnf();
    assert!(bnf.contains("program ::= pair* ;"), "{bnf}");
    // `Expr::token` 是显式的词法类别，回渲染成 `@number`，与 JSON 右部写法一致。
    assert!(bnf.contains("pair ::= \"(\" @number \")\" ;"), "{bnf}");
    assert_eq!(grammar.literals(), vec!["(", ")"]);
}

#[test]
fn rule_reference_requires_an_existing_production() {
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::rule("pair"))
        .production("pair", Expr::seq([Expr::literal("("), Expr::token("number"), Expr::literal(")")]))
        .build()
        .expect("the grammar should be valid");
    assert_eq!(grammar.productions_of("program").len(), 1);
}
