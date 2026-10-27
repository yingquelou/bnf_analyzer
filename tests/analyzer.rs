//! 用法一（可执行 crate）：从 JSON 加载词法规则 + 文法，输出默认语法树。
//!
//! 这里只通过公开 API 使用，覆盖内置配置、自定义配置、错误定位与 JSON 输出。

use bnf_analyzer::core::{AnalyzeError, Analyzer};
use bnf_analyzer::core::parser::ParseError;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::json::Config;

/// 内置 PostScript 配置装配出来的分析器（配置是配置文件的事，不是分析器的入口）。
fn builtin() -> Analyzer {
    Analyzer::from_config(Config::builtin()).expect("the builtin configuration should be consistent")
}

#[test]
fn parses_procedure_tree() {
    let document = builtin()
        .analyze("/f { dup 0 gt { 1 } { 2 } ifelse } bind")
        .expect("the source should be valid");

    let root = document.rule().expect("the root should be the program node");
    assert_eq!(root.rule, "program");
    assert_eq!(root.children.len(), 3, "program ::= value* should flatten to three values");

    let procedure = root
        .children
        .iter()
        .find_map(|value| value.find_rule("procedure"))
        .expect("should find a procedure node");
    // 子值 = `{` + 6 个 body 值 + `}`
    assert_eq!(procedure.children.len(), 8);
    assert_eq!(document.token_count(), 14);
}

#[test]
fn parses_dictionary_entries() {
    let document = builtin()
        .analyze("<< /Type /Page /Kids [ 1 0 R ] >>")
        .expect("the source should be valid");

    let dictionary = document
        .root
        .find_rule("dictionary")
        .expect("should find a dictionary node");
    let entries: Vec<_> = dictionary
        .children
        .iter()
        .filter_map(|value| value.find_rule("entry"))
        .collect();
    assert_eq!(entries.len(), 2, "the dictionary has two entries");
    assert!(
        dictionary
            .children
            .iter()
            .any(|value| value.find_rule("array").is_some())
    );
}

#[test]
fn keeps_grammar_nodes_for_every_value() {
    let document = builtin().analyze("true 42 /x").expect("the source should be valid");
    let root = document.rule().expect("the program node");

    let kinds: Vec<_> = root
        .children
        .iter()
        .map(|value| {
            let value_node = value.as_rule().expect("a value node");
            assert_eq!(value_node.rule, "value");
            value_node.children[0]
                .as_token()
                .expect("leaves are tokens")
                .kind
                .clone()
        })
        .collect();
    assert_eq!(kinds, vec!["boolean", "number", "name"]);
}

#[test]
fn reports_unclosed_procedure() {
    let error = builtin().analyze("/a 1\n{ 2 3").expect_err("should fail");
    match error {
        AnalyzeError::Parse(ParseError::UnexpectedEof { span, expected }) => {
            assert_eq!(span.line, 2);
            assert!(
                expected.iter().any(|item| item.contains('}')),
                "{expected:?}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn reports_unexpected_close() {
    let error = builtin().analyze("1 ]").expect_err("should fail");
    match error {
        AnalyzeError::Parse(error) => {
            assert_eq!(error.span().column, 3);
            assert!(error.to_string().contains(']'));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn reports_lexical_error_position() {
    let error = builtin().analyze("(unterminated").expect_err("should fail");
    assert!(matches!(error, AnalyzeError::Lex(_)));
    assert_eq!(error.span().column, 1);
}

#[test]
fn custom_json_config_drives_the_pipeline() {
    let config = Config::parse(
        r#"{
            "version": 2,
            "rules": [
                { "name": "space", "pattern": { "regex": "\\s+" }, "skip": true },
                { "name": "number", "pattern": { "regex": "\\d+" }, "priority": 10 }
            ],
            "grammar": {
                "start": "list",
                "productions": [{ "name": "list", "rhs": { "seq": [
                    { "literal": "(" },
                    { "star": { "token": "number" } },
                    { "literal": ")" }
                ] } }]
            }
        }"#,
    )
    .expect("the configuration should be valid");
    let analyzer = Analyzer::from_config(config).expect("the configuration should be consistent");

    let document = analyzer.analyze("(1 2 3)").expect("the source should be valid");
    let list = document.rule().expect("the root node");
    assert_eq!(list.rule, "list");
    // 子值 = `(` + 3 个数字 + `)`
    assert_eq!(list.children.len(), 5);
    let numbers: Vec<_> = list
        .children
        .iter()
        .filter_map(|value| value.as_token())
        .filter(|token| token.kind == "number")
        .map(|token| token.text.as_str())
        .collect();
    assert_eq!(numbers, vec!["1", "2", "3"]);

    // `number*` 只认数字，嵌套括号会在这里暴露出来
    let error = analyzer.analyze("(1 (2))").expect_err("should fail");
    assert!(matches!(error, AnalyzeError::Parse(_)));
}

#[test]
fn rejects_config_referencing_unknown_token_class() {
    let config = Config::parse(
        r#"{
            "version": 2,
            "rules": [{ "name": "number", "pattern": { "regex": "\\d+" } }],
            "grammar": { "productions": [
                { "name": "program", "rhs": { "star": { "token": "mystery" } } }
            ] }
        }"#,
    )
    .expect("the configuration should parse");
    let error = Analyzer::from_config(config).expect_err("grammar validation should fail");
    assert!(error.to_string().contains("mystery"), "{error}");
}

#[test]
fn rejects_unsupported_config_version() {
    assert!(Config::parse(r#"{ "version": 9 }"#).is_err());
}

#[test]
fn renders_grammar_back_to_bnf() {
    let bnf = builtin().grammar().to_bnf();
    assert!(bnf.contains("program ::= value* ;"), "{bnf}");
    assert!(bnf.contains("procedure ::= \"{\" value* \"}\" ;"), "{bnf}");
    assert!(bnf.contains("dictionary ::= \"<<\" entry* \">>\" ;"), "{bnf}");
}

#[test]
fn serializes_tree_with_positions() {
    let document = builtin().analyze("/a [1]").expect("the source should be valid");
    let json = document.to_json_pretty();

    assert!(json.contains("\"kind\": \"rule\""));
    assert!(json.contains("\"rule\": \"program\""));
    assert!(json.contains("\"span\""));
    assert!(json.contains("\"kind\": \"[\""));
}

#[test]
fn spans_count_characters_not_bytes() {
    // 两个非 ASCII 字符让"字符列"和"字节偏移"拉开差距：`é` 占 1 列、2 字节。
    let tokens = builtin().tokenize("/caféé 1").expect("the source should be valid");
    assert_eq!(tokens[0].text, "/caféé");
    assert_eq!(tokens[1].span.column, 8, "columns count characters");
    assert_eq!(tokens[1].span.start, 9, "offsets count bytes");
}

#[test]
fn empty_source_yields_empty_program() {
    let document = builtin().analyze("   \n% comment\n").expect("the source should be valid");
    let root = document.rule().expect("the program node");
    assert!(root.children.is_empty());
    assert_eq!(document.token_count(), 0);
}

#[test]
fn root_is_a_rule_value() {
    let document = builtin().analyze("1").expect("the source should be valid");
    assert!(matches!(document.root, Value::Rule(_)));
    assert!(!document.to_json().is_empty());
}
