//! 语法分析：按文法骨架做 Earley 分析，边识别边执行动作。
//!
//! 分析器只依赖文法（[`Grammar`]）与动作表（[`Semantics`]），不认识任何具体语言：
//!
//! - 终结符匹配后先跑 token 动作，没有动作就产出 [`Value::Token`]；
//! - 产生式归约时先跑语义动作，没有动作就产出 [`Value::Rule`]（默认语法树节点）；
//! - `*`/`+`/`?`/分组降级出的辅助产生式不建节点，直接把子值摊平给上层。
//!
//! Earley 的好处是不用改写文法：左递归、ε 产生式、歧义都能直接吃。同一输入有多种推导时
//! 取最先完成的那条。

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::core::grammar::{Grammar, Item, Terminal};
use crate::core::semantics::Semantics;
use crate::core::tokenizer::{Span, Token};
use crate::core::value::{RuleNode, Value};

/// 语法分析器：无状态，可自由复用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Parser;

impl Parser {
    pub const fn new() -> Self {
        Self
    }

    /// 分析一段 token 流，返回起始符号对应的值。
    pub fn parse(
        &self,
        grammar: &Grammar,
        semantics: &Semantics,
        tokens: &[Token],
    ) -> Result<Value, ParseError> {
        let len = tokens.len();
        let mut chart: Vec<Vec<State>> = vec![Vec::new(); len + 1];
        let mut seen: Vec<HashSet<Key>> = vec![HashSet::new(); len + 1];
        // 已归约项的产出缓存：动作只执行一次，重复遍历不会产生副作用。
        let mut yields: HashMap<(usize, usize, usize, usize), Vec<Value>> = HashMap::new();

        for &production in grammar.start_productions() {
            let state = State {
                production,
                dot: 0,
                origin: 0,
                slots: Vec::new(),
            };
            insert(&mut chart[0], &mut seen[0], state);
        }

        for pos in 0..=len {
            if chart[pos].is_empty() {
                continue;
            }

            // 同一集合里的项会互相触发（预测 / 归约 / 空产生式），
            // 直到不再新增项为止；去重表保证收敛。
            let mut changed = true;
            while changed {
                changed = false;
                let mut index = 0;
                while index < chart[pos].len() {
                    let state = chart[pos][index].clone();
                    index += 1;
                    let production = grammar.production(state.production);

                    match production.items.get(state.dot) {
                        Some(Item::Terminal(terminal)) => {
                            let Some(token) = tokens.get(pos) else {
                                continue;
                            };
                            if !terminal.matches(token) {
                                continue;
                            }
                            let value = match semantics.token_action(terminal.label()) {
                                Some(action) => {
                                    action(token).map_err(|message| ParseError::TokenAction {
                                        kind: terminal.label().to_string(),
                                        span: token.span,
                                        message,
                                    })?
                                }
                                None => Value::token(token.clone()),
                            };
                            let mut next = state.clone();
                            next.dot += 1;
                            next.slots.push(vec![value]);
                            insert(&mut chart[pos + 1], &mut seen[pos + 1], next);
                        }
                        Some(Item::NonTerminal(name)) => {
                            for &production in grammar.productions_of(name) {
                                let predicted = State {
                                    production,
                                    dot: 0,
                                    origin: pos,
                                    slots: Vec::new(),
                                };
                                if insert(&mut chart[pos], &mut seen[pos], predicted) {
                                    changed = true;
                                }
                            }
                        }
                        None => {
                            // 归约：先算出这个非终结符的"产出值"（0 个或多个）。
                            // 缓存键必须含结束位置：同一产生式在不同位置结束是两个不同的归约。
                            let key = (state.production, state.dot, state.origin, pos);
                            let yield_values = match yields.get(&key) {
                                Some(cached) => cached.clone(),
                                None => {
                                    let args: Vec<Value> =
                                        state.slots.iter().flatten().cloned().collect();
                                    let computed = if production.synthetic {
                                        args
                                    } else {
                                        let span = span_of(&state.slots, pos, tokens);
                                        let value = match semantics.rule_action(&production.name) {
                                            Some(action) => action(&args).map_err(|message| {
                                                ParseError::RuleAction {
                                                    rule: production.name.clone(),
                                                    span,
                                                    message,
                                                }
                                            })?,
                                            None => Value::Rule(RuleNode {
                                                rule: production.name.clone(),
                                                span,
                                                children: args,
                                            }),
                                        };
                                        vec![value]
                                    };
                                    yields.insert(key, computed.clone());
                                    computed
                                }
                            };

                            let waiting: Vec<State> = chart[state.origin]
                                .iter()
                                .filter(|candidate| {
                                    matches!(
                                        grammar.production(candidate.production).items.get(candidate.dot),
                                        Some(Item::NonTerminal(name)) if *name == production.name
                                    )
                                })
                                .cloned()
                                .collect();

                            for mut parent in waiting {
                                parent.dot += 1;
                                parent.slots.push(yield_values.clone());
                                if insert(&mut chart[pos], &mut seen[pos], parent) {
                                    changed = true;
                                }
                            }

                            if pos == len && state.origin == 0 && production.name == grammar.start()
                            {
                                let mut values = yield_values;
                                return Ok(match values.len() {
                                    1 => values.pop().expect("length is one"),
                                    _ => Value::list(values),
                                });
                            }
                        }
                    }
                }
            }
        }

        Err(failure(&chart, tokens, grammar))
    }
}

/// 一个 Earley 项：产生式下标 + 已匹配到的位置 + 起点。
///
/// `slots` 与已匹配的右部符号一一对应；每个槽可以容纳多个值，
/// 这是为了让 `*`/`+` 之类辅助产生式把子值摊平给上层。
#[derive(Debug, Clone)]
struct State {
    production: usize,
    dot: usize,
    origin: usize,
    slots: Vec<Vec<Value>>,
}

type Key = (usize, usize, usize);

fn insert(set: &mut Vec<State>, seen: &mut HashSet<Key>, state: State) -> bool {
    let key = (state.production, state.dot, state.origin);
    if seen.insert(key) {
        set.push(state);
        true
    } else {
        false
    }
}

/// 归约后的区间：取子值的首尾；空产生式退化为一个零宽区间。
fn span_of(slots: &[Vec<Value>], pos: usize, tokens: &[Token]) -> Span {
    let mut span: Option<Span> = None;
    for value in slots.iter().flatten() {
        if let Some(value_span) = value.span() {
            span = Some(match span {
                Some(existing) => Span::merge(existing, value_span),
                None => value_span,
            });
        }
    }
    span.unwrap_or_else(|| empty_span(pos, tokens))
}

fn empty_span(pos: usize, tokens: &[Token]) -> Span {
    if let Some(token) = tokens.get(pos) {
        return Span::new(
            token.span.start,
            token.span.start,
            token.span.line,
            token.span.column,
        );
    }
    match tokens.last() {
        Some(last) => {
            let tail = last.text.rsplit('\n').next().unwrap_or("");
            let (line, column) = if last.text.contains('\n') {
                (
                    last.span.line + last.text.matches('\n').count(),
                    tail.chars().count() + 1,
                )
            } else {
                (last.span.line, last.span.column + last.text.chars().count())
            };
            Span::new(last.span.end, last.span.end, line, column)
        }
        None => Span::new(0, 0, 1, 1),
    }
}

/// 分析失败时，找出最远到达的位置并列出此时期望的符号。
fn failure(chart: &[Vec<State>], tokens: &[Token], grammar: &Grammar) -> ParseError {
    let furthest = chart
        .iter()
        .rposition(|set| !set.is_empty())
        .unwrap_or(0);

    let mut expected: Vec<String> = chart[furthest]
        .iter()
        .filter_map(|state| {
            grammar
                .production(state.production)
                .items
                .get(state.dot)
                .map(|item| match item {
                    Item::Terminal(Terminal::Literal(text)) => format!("{text:?}"),
                    other => other.label().to_string(),
                })
        })
        // 辅助产生式是内部实现，不该出现在用户可见的"期望"里。
        .filter(|label| !grammar.is_synthetic(label))
        .collect();
    expected.sort();
    expected.dedup();

    match tokens.get(furthest) {
        Some(token) => ParseError::UnexpectedToken {
            text: token.text.clone(),
            span: token.span,
            expected,
        },
        None => ParseError::UnexpectedEof {
            span: empty_span(tokens.len(), tokens),
            expected,
        },
    }
}

/// 语法分析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// 输入流里出现了不该出现的 token。
    UnexpectedToken {
        text: String,
        span: Span,
        expected: Vec<String>,
    },
    /// 输入提前结束。
    UnexpectedEof { span: Span, expected: Vec<String> },
    /// token 动作失败。
    TokenAction {
        kind: String,
        span: Span,
        message: String,
    },
    /// 语义动作失败。
    RuleAction {
        rule: String,
        span: Span,
        message: String,
    },
}

impl ParseError {
    /// 出错位置，便于调用方高亮源码。
    pub fn span(&self) -> Span {
        match self {
            ParseError::UnexpectedToken { span, .. }
            | ParseError::UnexpectedEof { span, .. }
            | ParseError::TokenAction { span, .. }
            | ParseError::RuleAction { span, .. } => *span,
        }
    }

    /// 此时期望的符号（仅"语法不匹配"类错误有）。
    pub fn expected(&self) -> &[String] {
        match self {
            ParseError::UnexpectedToken { expected, .. } | ParseError::UnexpectedEof { expected, .. } => {
                expected
            }
            _ => &[],
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken {
                text,
                span,
                expected,
            } => write!(f, "{span}: unexpected {text:?}{}", Expectations(expected)),
            ParseError::UnexpectedEof { span, expected } => {
                write!(f, "{span}: unexpected end of input{}", Expectations(expected))
            }
            ParseError::TokenAction {
                kind,
                span,
                message,
            } => write!(f, "{span}: token action {kind} failed: {message}"),
            ParseError::RuleAction {
                rule,
                span,
                message,
            } => write!(f, "{span}: semantic action {rule} failed: {message}"),
        }
    }
}

impl std::error::Error for ParseError {}

struct Expectations<'a>(&'a [String]);

impl fmt::Display for Expectations<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return Ok(());
        }
        let shown = self.0.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if self.0.len() > 8 {
            write!(f, ", expected one of: {shown} and {} more", self.0.len().saturating_sub(8))
        } else {
            write!(f, ", expected one of: {shown}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::grammar::Expr;
    use crate::core::rules::{Rule, Rules};

    fn rules() -> Rules {
        Rules::from_rules(vec![
            Rule::skipping_regex("space", r"\s+"),
            Rule::regex("number", r"\d+").with_priority(10),
        ])
        .expect("the rules should be valid")
    }

    fn grammar() -> Grammar {
        Grammar::builder()
            .start("program")
            .production("program", Expr::star(Expr::rule("expr")))
            .production(
                "expr",
                Expr::alt([
                    Expr::token("number"),
                    Expr::seq([
                        Expr::rule("expr"),
                        Expr::literal("+"),
                        Expr::rule("expr"),
                    ]),
                ]),
            )
            .build()
            .expect("the grammar should be valid")
    }

    #[test]
    fn builds_default_tree_without_actions() {
        let tokenizer = crate::core::tokenizer::Tokenizer::new(
            rules()
                .with_literals(&grammar().literals())
                .expect("the literals should be valid"),
        );
        let tokens = tokenizer.tokenize("1 + 2").expect("tokenization should succeed");
        let value = Parser::new()
            .parse(&grammar(), &Semantics::new(), &tokens)
            .expect("parsing should succeed");

        let root = value.as_rule().expect("the root should be a rule node");
        assert_eq!(root.rule, "program");
        assert_eq!(root.children.len(), 1, "helper productions should be flattened");
    }

    #[test]
    fn reports_unexpected_token() {
        let grammar = Grammar::builder()
            .start("program")
            .production("program", Expr::seq([Expr::token("number"), Expr::literal(";")]))
            .build()
            .expect("the grammar should be valid");
        let tokenizer = crate::core::tokenizer::Tokenizer::new(
            rules().with_literals(&grammar.literals()).expect("should be valid"),
        );
        let tokens = tokenizer.tokenize("1 2").expect("tokenization should succeed");
        let error = Parser::new()
            .parse(&grammar, &Semantics::new(), &tokens)
            .expect_err("should fail");
        assert!(matches!(error, ParseError::UnexpectedToken { .. }));
        assert!(error.expected().iter().any(|item| item.contains(';')));
    }

    #[test]
    fn hides_synthetic_productions_from_expectations() {
        let rules = Rules::from_rules(vec![
            Rule::skipping_regex("space", r"\s+"),
            Rule::regex("number", r"\d+"),
            Rule::regex("word", "[a-z]+"),
        ])
        .expect("the rules should be valid");
        let grammar = Grammar::builder()
            .start("program")
            .production("program", Expr::plus(Expr::rule("value")))
            .production("value", Expr::token("number"))
            .build()
            .expect("the grammar should be valid");
        let tokenizer = crate::core::tokenizer::Tokenizer::new(
            rules.with_literals(&grammar.literals()).expect("should be valid"),
        );
        // `word` 能通过词法，但不是合法的 value：出错点落在 `$plus1 ::= value . $star1`。
        let tokens = tokenizer.tokenize("1 x").expect("tokenization should succeed");
        let error = Parser::new()
            .parse(&grammar, &Semantics::new(), &tokens)
            .expect_err("`x` is not a value, parsing should fail");
        let expected = error.expected();
        assert!(!expected.is_empty(), "the expected list should not be empty");
        assert!(
            expected.iter().all(|label| !label.starts_with('$')),
            "helper productions must not appear in the expected list: {expected:?}"
        );
        assert!(
            !error.to_string().contains('$'),
            "the error must not mention helper production names: {error}"
        );
    }
}
