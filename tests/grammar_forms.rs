//! `docs/user/grammar.zh.md` 里每一行「文法形式 ↔ 代码」的可执行版本。
//!
//! 文档只描述这里被断言过的行为：改文法模型时，先让这些测试重新变绿，再同步文档。
//! 每个测试名都能在文档的「对应测试」列里找到。
//!
//! 注意：右部只接受结构化对象。这里的 JSON 对象写成 JSON 文本（由本 crate 自己的
//! JSON 文法解析，见 `src/json/grammar.rs`）或直接构造 [`Value`]，再交给
//! `GrammarSpec::from_value`——和 `postscript.json` 的加载路径完全一致。

use std::collections::BTreeMap;

use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar, GrammarSpec, Item};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

/// 一小段 JSON 文本 → [`Value`]，顺便验一遍本库的 JSON 文法。
fn json_text(text: &str) -> Value {
    bnf_analyzer::json::parse(text).unwrap_or_else(|error| panic!("{text} is not valid JSON: {error}"))
}

/// 手搭一个对象值，供需要动态生成的用例使用。
fn obj(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// 用 JSON 声明文法，起点取第一条产生式。
fn from_json(productions: &[(&str, Value)]) -> Grammar {
    let productions = Value::List(
        productions
            .iter()
            .map(|(name, rhs)| obj(&[("name", Value::text(*name)), ("rhs", rhs.clone())]))
            .collect(),
    );
    let spec =
        GrammarSpec::from_value(&obj(&[("productions", productions)])).expect("the JSON grammar should be valid");
    spec.parse_grammar().expect("the grammar should be consistent")
}

/// 只含空白与单词的最小词法环境。
fn words() -> Rules {
    Rules::from_rules(vec![
        Rule::skipping_regex("space", r"\s+"),
        Rule::regex("word", "[a-z]+"),
    ])
    .expect("the rules should be valid")
}

fn analyzer(rules: Rules, grammar: Grammar) -> Analyzer {
    Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent")
}

/// 把根节点摊平成子值的标签，便于断言"重复/分组是否被摊平"。
fn children(analyzer: &Analyzer, source: &str) -> Vec<String> {
    let document = analyzer.analyze(source).expect("the source should be valid");
    document
        .root
        .children()
        .iter()
        .map(|value| {
            value
                .as_token()
                .map(|token| token.text.clone())
                .or_else(|| value.rule_name().map(str::to_string))
                .unwrap_or_default()
        })
        .collect()
}

/// 降级后的结构：`N:` 是非终结符，`T:` 是终结符（类别名或字面量文本）。
fn shape(grammar: &Grammar) -> Vec<(String, Vec<String>, bool)> {
    grammar
        .productions()
        .iter()
        .map(|production| {
            let items = production
                .items
                .iter()
                .map(|item| match item {
                    Item::NonTerminal(name) => format!("N:{name}"),
                    Item::Terminal(terminal) => format!("T:{}", terminal.label()),
                })
                .collect();
            (production.name.clone(), items, production.synthetic)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 符号引用：非终结符 / 词法类别 / 字面量
// ---------------------------------------------------------------------------

#[test]
fn nonterminal_has_two_spellings() {
    // JSON 对象与 Rust 构造子一一对应，降级成同一份 CFG。
    let json = from_json(&[
        ("program", json_text(r#"{ "star": { "rule": "pair" } }"#)),
        ("pair", json_text(r#"{ "token": "number" }"#)),
    ]);

    let built = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("pair")))
        .production("pair", Expr::token("number"))
        .build()
        .expect("the grammar should be valid");

    assert_eq!(json.to_bnf(), built.to_bnf());
    assert_eq!(shape(&json), shape(&built));
}

#[test]
fn rule_and_token_are_not_interchangeable() {
    // `number` 同时是产生式名与词法类别名：`rule` 与 `token` 各走各的。
    let rules = || {
        Rules::from_rules(vec![
            Rule::skipping_regex("space", r"\s+"),
            Rule::regex("number", r"\d+"),
        ])
        .expect("the rules should be valid")
    };

    // token：强制词法类别，接受数字。
    let by_class = analyzer(
        rules(),
        from_json(&[
            ("program", json_text(r#"{ "token": "number" }"#)),
            ("number", json_text(r#"{ "literal": "n" }"#)),
        ]),
    );
    assert!(by_class.analyze("42").is_ok(), "a token should resolve to a token class");
    assert!(by_class.analyze("n").is_err(), "a token must not resolve to a production");

    // rule：强制非终结符，因此接受 `n`，不接受数字。
    let by_rule = analyzer(
        rules(),
        from_json(&[
            ("program", json_text(r#"{ "rule": "number" }"#)),
            ("number", json_text(r#"{ "literal": "n" }"#)),
        ]),
    );
    assert!(by_rule.analyze("n").is_ok(), "a rule should resolve to a production");
    assert!(by_rule.analyze("42").is_err(), "a rule must not resolve to a token class");

    // `rule` 指向不存在的产生式：建文法时就报错，而不是留到运行时。
    let error = Grammar::builder()
        .production("program", Expr::rule("missing"))
        .build()
        .expect_err("should fail");
    assert!(error.to_string().contains("undefined non-terminal"), "{error}");
}

#[test]
fn digit_example_matches_both_spellings() {
    // 文档开篇的例子：十选一。
    let digits: Vec<Value> = ('0'..='9')
        .map(|digit| obj(&[("literal", Value::text(digit.to_string()))]))
        .collect();
    let json = from_json(&[("digit", obj(&[("alt", Value::List(digits))]))]);

    let built = Grammar::builder()
        .start("digit")
        .production(
            "digit",
            Expr::alt(('0'..='9').map(|digit| Expr::literal(digit.to_string()))),
        )
        .build()
        .expect("the grammar should be valid");

    assert_eq!(shape(&json), shape(&built));
    assert_eq!(json.productions_of("digit").len(), 10, "ten alternatives give ten productions");
    assert_eq!(json.literals().len(), 10);

    let rules = Rules::from_rules(vec![Rule::skipping_regex("space", r"\s+")]).expect("the rules should be valid");
    let digits = analyzer(rules, json);
    assert_eq!(children(&digits, "7"), vec!["7"]);
    assert!(digits.analyze("x").is_err());
}

#[test]
fn literal_terminals_are_derived_from_the_grammar() {
    // 多字符字面量（关键字、`<<` 等）整体匹配，不需要在 rules 里声明。
    let grammar = from_json(&[(
        "program",
        json_text(r#"{ "seq": [
            { "literal": "<<" },
            { "literal": "end" }
        ] }"#),
    )]);
    assert_eq!(grammar.literals(), vec!["<<", "end"]);

    let rules = Rules::from_rules(vec![Rule::skipping_regex("space", r"\s+")]).expect("the rules should be valid");
    let analyzer = analyzer(rules, grammar);
    let tokens = analyzer.tokenize("<< end").expect("tokenization should succeed");
    assert_eq!(tokens[0].kind, "<<", "a literal token keeps its text as the kind");
    assert_eq!(tokens[1].kind, "end");
}

// ---------------------------------------------------------------------------
// 组合子：序列 / 选择 / 重复 / 分组 / 空
// ---------------------------------------------------------------------------

#[test]
fn sequence_and_alternation_match_rust_expr() {
    let json = from_json(&[(
        "entry",
        json_text(r#"{ "alt": [
            { "seq": [{ "token": "word" }, { "token": "word" }] },
            { "token": "word" }
        ] }"#),
    )]);
    let built = Grammar::builder()
        .start("entry")
        .production(
            "entry",
            Expr::alt([
                Expr::seq([Expr::token("word"), Expr::token("word")]),
                Expr::token("word"),
            ]),
        )
        .build()
        .expect("the grammar should be valid");

    assert_eq!(json.to_bnf(), built.to_bnf());
    assert_eq!(json.productions_of("entry").len(), 2, "two alternatives give two productions");
}

#[test]
fn star_and_plus_differ_only_at_zero() {
    let star = analyzer(
        words(),
        from_json(&[("program", json_text(r#"{ "star": { "token": "word" } }"#))]),
    );
    assert!(star.analyze("").is_ok(), "`*` allows zero occurrences");
    assert_eq!(children(&star, "a b"), vec!["a", "b"], "repetitions are flattened");

    let plus = analyzer(
        words(),
        from_json(&[("program", json_text(r#"{ "plus": { "token": "word" } }"#))]),
    );
    assert!(plus.analyze("").is_err(), "`+` requires at least one occurrence");
    assert_eq!(children(&plus, "a b"), vec!["a", "b"]);

    // 辅助产生式不出现在语法树里，也不出现在错误提示的期望列表里。
    let lit_plus = analyzer(
        words(),
        from_json(&[("program", json_text(r#"{ "plus": { "literal": "a" } }"#))]),
    );
    let error = lit_plus.analyze("b").expect_err("b does not match a");
    let message = error.to_string();
    assert!(message.contains("\"a\""), "{message}");
    assert!(!message.contains("$star"), "the expected list must not expose helper productions: {message}");
}

#[test]
fn opt_makes_a_piece_optional() {
    let grammar = from_json(&[("program", json_text(r#"{ "opt": { "literal": "a" } }"#))]);
    let optional = analyzer(words(), grammar);
    assert!(optional.analyze("").is_ok());
    assert_eq!(children(&optional, "a"), vec!["a"]);
    assert!(optional.analyze("a a").is_err(), "`?` allows zero or one occurrence");
}

#[test]
fn group_carries_inner_alternatives_and_suffix() {
    let grammar = from_json(&[(
        "program",
        json_text(r#"{ "seq": [
            { "alt": [{ "literal": "a" }, { "literal": "b" }] },
            { "literal": "+" }
        ] }"#),
    )]);
    let grouped = analyzer(words(), grammar);

    assert_eq!(children(&grouped, "a+"), vec!["a", "+"]);
    assert_eq!(children(&grouped, "b+"), vec!["b", "+"]);
    assert!(grouped.analyze("c+").is_err());
}

#[test]
fn group_with_star_repeats_the_whole_sequence() {
    // `( word "," )*`：分组让 `*` 作用在整段序列上。
    let grammar = from_json(&[(
        "program",
        json_text(r#"{ "star": { "seq": [{ "token": "word" }, { "literal": "," }] } }"#),
    )]);
    let repeated = analyzer(words(), grammar);
    assert!(repeated.analyze("").is_ok());
    assert_eq!(children(&repeated, "a,b,"), vec!["a", ",", "b", ","]);
    assert!(repeated.analyze("a").is_err(), "a group requires `,` and the word together");
}

#[test]
fn empty_seq_is_epsilon() {
    let grammar = from_json(&[("program", json_text(r#"{ "seq": [] }"#))]);
    assert_eq!(grammar.productions_of("program").len(), 1);
    assert!(grammar.production(grammar.productions_of("program")[0]).is_empty());
    assert_eq!(grammar.to_bnf(), "program ::= ;\n");

    let rules = Rules::from_rules(vec![Rule::skipping_regex("space", r"\s+")]).expect("the rules should be valid");
    let epsilon = analyzer(rules, grammar);
    let document = epsilon.analyze("   ").expect("empty input should be valid");
    assert!(document.root.children().is_empty());
}

#[test]
fn empty_seq_as_an_alternative_is_optional() {
    let grammar = from_json(&[(
        "program",
        json_text(r#"{ "alt": [{ "token": "word" }, { "seq": [] }] }"#),
    )]);
    assert_eq!(grammar.productions_of("program").len(), 2);
    let optional = analyzer(words(), grammar);
    assert!(optional.analyze("").is_ok());
    assert_eq!(children(&optional, "a"), vec!["a"]);
}

#[test]
fn empty_alt_is_rejected() {
    let error = GrammarSpec::from_value(&json_text(r#"{
        "productions": [{ "name": "program", "rhs": { "alt": [] } }]
    }"#))
    .expect_err("an empty `alt` should be rejected");
    assert!(error.to_string().contains("needs at least one alternative"), "{error}");
}

// ---------------------------------------------------------------------------
// 声明层面的细节
// ---------------------------------------------------------------------------

#[test]
fn repeated_declarations_merge_into_alternatives() {
    let grammar = from_json(&[
        ("value", json_text(r#"{ "token": "word" }"#)),
        ("value", json_text(r#"{ "literal": "x" }"#)),
    ]);
    assert_eq!(grammar.productions_of("value").len(), 2);
    assert_eq!(
        grammar.to_bnf(),
        "value ::= @word | \"x\" ;\n",
        "same-name declarations merge into one BNF line"
    );

    let built = Grammar::builder()
        .start("value")
        .production("value", Expr::token("word"))
        .production("value", Expr::literal("x"))
        .build()
        .expect("calling the same name twice should be allowed");
    assert_eq!(built.to_bnf(), grammar.to_bnf());
    assert!(analyzer(words(), built).analyze("x").is_ok());
}

#[test]
fn suffix_binds_to_one_piece_not_the_whole_sequence() {
    // `word ","?` 与 `( word "," )?` 结构不同：前者 `?` 只作用在逗号上。
    let piece = from_json(&[(
        "program",
        json_text(r#"{ "seq": [{ "token": "word" }, { "opt": { "literal": "," } }] }"#),
    )]);
    assert_eq!(piece.to_bnf(), "program ::= @word \",\"? ;\n");

    let whole = from_json(&[(
        "program",
        json_text(r#"{ "opt": { "seq": [{ "token": "word" }, { "literal": "," }] } }"#),
    )]);
    assert_eq!(whole.to_bnf(), "program ::= ( @word \",\" )? ;\n");

    // 行为差别：前者允许 `a`，后者要求逗号与词一起出现或一起缺席。
    assert!(analyzer(words(), piece).analyze("a").is_ok());
    assert!(analyzer(words(), whole).analyze("a").is_err());
}

// ---------------------------------------------------------------------------
// 右部只能是对象
// ---------------------------------------------------------------------------

#[test]
fn string_rhs_is_rejected() {
    let error = GrammarSpec::from_value(&json_text(r#"{
        "productions": [{ "name": "program", "rhs": "value*" }]
    }"#))
    .expect_err("a string right-hand side should be rejected");
    let message = error.to_string();
    assert!(message.contains("does not accept a string"), "{message}");
    assert!(message.contains("structured object"), "the error should suggest a fix: {message}");
}

#[test]
fn unknown_keys_are_rejected() {
    let error = GrammarSpec::from_value(&json_text(r#"{
        "productions": [{ "name": "program", "rhs": { "regexp": "value" } }]
    }"#))
    .expect_err("unknown keys should be rejected");
    assert!(error.to_string().contains("unknown expression key"), "{error}");

    let error = GrammarSpec::from_value(&json_text(r#"{
        "productions": [{ "name": "program", "rhs": { "literal": "a", "token": "b" } }]
    }"#))
    .expect_err("multiple keys should be rejected");
    assert!(error.to_string().contains("exactly one key"), "{error}");

    let error = GrammarSpec::from_value(&json_text(r#"{
        "productions": [{ "name": "program", "rhs": { "rule": "value" }, "note": 1 }]
    }"#))
    .expect_err("unknown keys on a production should be rejected");
    assert!(error.to_string().contains("unknown field"), "{error}");
}

// ---------------------------------------------------------------------------
// 声明形态与代码形态等价
// ---------------------------------------------------------------------------

#[test]
fn json_text_and_builder_agree() {
    // core 的单测只用手搭的 `Value` 树（core 不依赖 JSON 层），
    // 这里补上"同一份文法从 JSON 文本读进来"这条路径，两者结果必须一致。
    let parsed = GrammarSpec::from_value(&json_text(
        r#"{
            "productions": [
                { "name": "program", "rhs": { "star": { "rule": "value" } } },
                { "name": "value", "rhs": { "alt": [
                    { "token": "number" }, { "rule": "procedure" }
                ] } },
                { "name": "procedure", "rhs": { "seq": [
                    { "literal": "{" },
                    { "star": { "rule": "value" } },
                    { "literal": "}" }
                ] } }
            ]
        }"#,
    ))
    .expect("the JSON declaration should read as a GrammarSpec")
    .parse_grammar()
    .expect("the grammar should be valid");

    let built = Grammar::builder()
        .start("program")
        .production("program", Expr::star(Expr::rule("value")))
        .production(
            "value",
            Expr::alt([Expr::token("number"), Expr::rule("procedure")]),
        )
        .production(
            "procedure",
            Expr::seq([
                Expr::literal("{"),
                Expr::star(Expr::rule("value")),
                Expr::literal("}"),
            ]),
        )
        .build()
        .expect("the grammar should be valid");

    assert_eq!(built.to_bnf(), parsed.to_bnf());
    assert_eq!(built.literals(), parsed.literals());
    assert_eq!(built.productions().len(), parsed.productions().len());
}

// ---------------------------------------------------------------------------
// 动作的键
// ---------------------------------------------------------------------------

#[test]
fn actions_are_keyed_by_symbol_label() {
    let rules = Rules::from_rules(vec![
        Rule::skipping_regex("space", r"\s+"),
        Rule::regex("number", r"\d+"),
    ])
    .expect("the rules should be valid");
    let grammar = Grammar::builder()
        .start("program")
        .production(
            "program",
            Expr::seq([
                Expr::literal("("),
                Expr::token("number"),
                Expr::literal(")"),
            ]),
        )
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
        // 字面量终结符也能挂 token 动作，键就是字面量文本。
        .token("(", |_| Ok(Value::text("open")))
        .token(")", |_| Ok(Value::text("close")))
        .rule("program", |values| Ok(Value::list(values.to_vec())));

    let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
    let document = analyzer.analyze("(7)").expect("the source should be valid");
    assert_eq!(
        document.root,
        Value::list(vec![Value::text("open"), Value::Int(7), Value::text("close")])
    );
}
