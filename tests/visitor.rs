//! 访问者对真实分析结果的行为：顺序、剪枝、中止、后序求值、收集器。
//!
//! 单元测试（`src/visit.rs`）用手搭的 `Value`，这里一律用内置配置装配出的分析器
//! 真跑一遍 PostScript 片段，避免"遍历器对了但树长得不一样"。

use analyzer::core::{Analyzer, Document};
use analyzer::core::tokenizer::Token;
use analyzer::core::value::{RuleNode, Value};
use analyzer::core::visit::{Flow, Visitor};
use analyzer::json::Config;

/// 内置配置装配出来的分析器。
fn builtin() -> Analyzer {
    Analyzer::from_config(Config::builtin()).expect("the builtin configuration should be consistent")
}

/// 节点与 token 的前序记录，用来和 `Document::walk` 对齐口径。
#[derive(Default)]
struct Preorder {
    entries: Vec<String>,
}

impl<'ast> Visitor<'ast> for Preorder {
    fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
        self.entries.push(format!("rule:{}", node.rule));
        Flow::Continue
    }

    fn visit_token(&mut self, token: &'ast Token) {
        self.entries.push(format!("token:{}", token.text));
    }
}

fn preorder(document: &Document) -> Vec<String> {
    let mut visitor = Preorder::default();
    document.accept(&mut visitor);
    visitor.entries
}

fn walked(document: &Document) -> Vec<String> {
    let mut entries = Vec::new();
    document.walk(|value| match value {
        Value::Rule(node) => entries.push(format!("rule:{}", node.rule)),
        Value::Token(token) => entries.push(format!("token:{}", token.text)),
        _ => {}
    });
    entries
}

#[test]
fn accept_matches_walk_on_the_same_tree() {
    let document = builtin()
        .analyze("/f { dup 0 gt } def")
        .expect("analysis should succeed");
    assert_eq!(preorder(&document), walked(&document));
    assert_eq!(
        preorder(&document).first().map(String::as_str),
        Some("rule:program")
    );
}

#[test]
fn enter_and_leave_bracket_every_node() {
    #[derive(Default)]
    struct Balance {
        depth: i32,
        opened: usize,
        closed: usize,
    }

    impl<'ast> Visitor<'ast> for Balance {
        fn enter_rule(&mut self, _node: &'ast RuleNode) -> Flow {
            self.depth += 1;
            self.opened += 1;
            Flow::Continue
        }

        fn leave_rule(&mut self, _node: &'ast RuleNode) {
            self.depth -= 1;
            self.closed += 1;
        }
    }

    let document = builtin()
        .analyze("/f { dup 0 gt { 1 } { 2 } ifelse } bind def")
        .expect("analysis should succeed");
    let mut visitor = Balance::default();
    assert_eq!(document.accept(&mut visitor), Flow::Continue);
    assert_eq!(visitor.depth, 0);
    assert_eq!(visitor.opened, visitor.closed);
    assert!(visitor.opened >= 5, "program/value/procedure should give at least five nodes");
}

#[test]
fn skip_prunes_a_whole_subtree() {
    /// 数 token，但碰到 procedure 就不进去。
    #[derive(Default)]
    struct SkipProcedures {
        tokens: Vec<String>,
    }

    impl<'ast> Visitor<'ast> for SkipProcedures {
        fn visit(&mut self, value: &'ast Value) -> Flow {
            match value.as_rule().map(|node| node.rule.as_str()) {
                Some("procedure") => Flow::Skip,
                _ => Flow::Continue,
            }
        }

        fn visit_token(&mut self, token: &'ast Token) {
            self.tokens.push(token.text.clone());
        }
    }

    let document = builtin()
        .analyze("/f { 1 2 3 } /g 4")
        .expect("analysis should succeed");
    let whole = document.tokens();

    let mut visitor = SkipProcedures::default();
    document.accept(&mut visitor);

    assert!(
        visitor.tokens.len() < whole.len(),
        "skipping procedure yields fewer tokens"
    );
    assert!(!visitor.tokens.iter().any(|text| text == "1"), "the 1 inside braces must not appear");
    assert!(visitor.tokens.iter().any(|text| text == "/g"), "/g on the same level is still visited");
}

#[test]
fn stop_short_circuits_and_reports_partial_work() {
    #[derive(Default)]
    struct StopAtFirstProcedure {
        seen: Vec<String>,
    }

    impl<'ast> Visitor<'ast> for StopAtFirstProcedure {
        fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
            self.seen.push(node.rule.clone());
            if node.rule == "procedure" {
                Flow::Stop
            } else {
                Flow::Continue
            }
        }
    }

    let document = builtin()
        .analyze("/f { 1 } /g { 2 }")
        .expect("analysis should succeed");
    let mut visitor = StopAtFirstProcedure::default();
    assert_eq!(document.accept(&mut visitor), Flow::Stop);
    assert_eq!(visitor.seen.last().map(String::as_str), Some("procedure"));
    assert_eq!(
        visitor.seen.iter().filter(|name| *name == "procedure").count(),
        1,
        "the second procedure must not be visited"
    );
}

#[test]
fn post_order_can_recompute_a_value_from_children() {
    /// 后序统计每个节点子树里的 token 数：进入时压栈，离开时把总数交给父节点。
    #[derive(Default)]
    struct Tally {
        stack: Vec<usize>,
        root_total: Option<usize>,
    }

    impl<'ast> Visitor<'ast> for Tally {
        fn enter_rule(&mut self, _node: &'ast RuleNode) -> Flow {
            self.stack.push(0);
            Flow::Continue
        }

        fn leave_rule(&mut self, _node: &'ast RuleNode) {
            let own = self.stack.pop().expect("enter and leave must pair up");
            match self.stack.last_mut() {
                Some(parent) => *parent += own,
                None => self.root_total = Some(own),
            }
        }

        fn visit_token(&mut self, _token: &'ast Token) {
            if let Some(top) = self.stack.last_mut() {
                *top += 1;
            }
        }
    }

    let document = builtin()
        .analyze("/f { dup 0 gt } def")
        .expect("analysis should succeed");
    let mut tally = Tally::default();
    document.accept(&mut tally);
    assert_eq!(tally.root_total, Some(document.token_count()));
    assert!(tally.stack.is_empty());
}

#[test]
fn collectors_agree_with_walk() {
    let document = builtin()
        .analyze("/a { 1 } /b { 2 } /c { 3 }")
        .expect("analysis should succeed");

    assert_eq!(document.rules_named("procedure").len(), 3);
    assert_eq!(document.rules_named("a missing name").len(), 0);
    assert!(document.rules_named("a missing name").is_empty());

    let from_collector: Vec<&str> = document
        .tokens()
        .iter()
        .map(|token| token.text.as_str())
        .collect();
    // `walk` 的闭包是 HRTB，引用带不出来，只能抄成 String；
    // 访问者的 `&'ast Token` 可以直接存（这正是 `Document::tokens` 的实现方式）。
    let from_walk: Vec<String> = {
        let mut texts = Vec::new();
        document.walk(|value| {
            if let Some(token) = value.as_token() {
                texts.push(token.text.clone());
            }
        });
        texts
    };
    assert_eq!(
        from_collector,
        from_walk.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(from_collector.len(), document.token_count());
}

#[test]
fn visits_action_results_not_only_syntax_trees() {
    use analyzer::core::grammar::{Expr, Grammar};
use analyzer::core::rules::{Rule, Rules};
use analyzer::core::semantics::Semantics;

    #[derive(Default)]
    struct Numbers {
        ints: Vec<i64>,
        rules: Vec<String>,
    }

    impl<'ast> Visitor<'ast> for Numbers {
        fn visit(&mut self, value: &'ast Value) -> Flow {
            if let Some(number) = value.as_int() {
                self.ints.push(number);
            }
            Flow::Continue
        }

        fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
            self.rules.push(node.rule.clone());
            Flow::Continue
        }
    }

    // 带动作的分析器：根是 Int(6)，整棵树都被动作折叠掉了。
    let rules = Rules::from_rules(vec![
        Rule::skipping_regex("space", r"\s+"),
        Rule::regex("number", r"\d+"),
    ])
    .expect("the rules should be valid");
    let grammar = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("number")))
        .production("number", Expr::token("number"))
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
        .rule("number", |values| match values {
            [value] => Ok(value.clone()),
            _ => Err("number should produce exactly one value".to_string()),
        })
        .rule("program", |values| {
            let mut total = 0;
            for value in values {
                total += value.as_int().ok_or("integers only")?;
            }
            Ok(Value::Int(total))
        });
    let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
    let document = analyzer.analyze("1 2 3").expect("analysis should succeed");
    assert_eq!(document.root, Value::Int(6), "the action folds the tree into a scalar");

    let mut visitor = Numbers::default();
    assert_eq!(document.accept(&mut visitor), Flow::Continue);
    assert_eq!(visitor.ints, [6], "the Int at the root is visited too");
    assert!(visitor.rules.is_empty(), "with actions attached there is no Rule node");

    // 同一个访问者直接作用在值上也一样。
    let mut visitor = Numbers::default();
    Value::list(vec![Value::Int(1), Value::Int(2)]).accept(&mut visitor);
    assert_eq!(visitor.ints, [1, 2]);
}

#[test]
fn visitors_can_borrow_nodes_out_of_the_document() {
    // 访问者能把 &'ast RuleNode 存下来，说明它不要求回调内就把数据抄走。
    struct Keep<'ast> {
        spans: Vec<analyzer::core::tokenizer::Span>,
        first: Option<&'ast RuleNode>,
    }

    impl<'ast> Visitor<'ast> for Keep<'ast> {
        fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
            if self.first.is_none() {
                self.first = Some(node);
            }
            self.spans.push(node.span);
            Flow::Continue
        }
    }

    let document = builtin()
        .analyze("/f { dup }")
        .expect("analysis should succeed");
    let mut keep = Keep {
        spans: Vec::new(),
        first: None,
    };
    document.accept(&mut keep);

    let first = keep.first.expect("there should be at least one node");
    assert_eq!(first.rule, "program");
    assert_eq!(first.span.start, 0);
    assert_eq!(keep.spans.len(), document.rules_named("value").len() + 2);
}

#[test]
fn empty_input_still_yields_one_visit() {
    let document = builtin().analyze("").expect("empty input is valid");
    let mut visitor = Preorder::default();
    assert_eq!(document.accept(&mut visitor), Flow::Continue);
    assert_eq!(visitor.entries, ["rule:program"]);
    assert_eq!(document.token_count(), 0);
}
