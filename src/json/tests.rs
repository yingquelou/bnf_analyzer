//! JSON 层自己的测试：解析、输出、配置读取，以及"文法就是文档"的对照。

use std::collections::BTreeMap;

use crate::core::pattern::Pattern;
use crate::core::value::Value;
use crate::core::{Analyzer, Document};

use super::grammar;
use super::{parse, to_string, to_string_pretty, Config, ConfigError, JsonError, SUPPORTED_VERSION};

fn text(value: &str) -> Value {
    Value::text(value)
}

fn object(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

#[test]
fn parses_scalars() {
    assert_eq!(parse("null").expect("should be valid"), Value::Null);
    assert_eq!(parse("true").expect("should be valid"), Value::Bool(true));
    assert_eq!(parse("false").expect("should be valid"), Value::Bool(false));
    assert_eq!(parse("-17").expect("should be valid"), Value::Int(-17));
    assert_eq!(parse("0").expect("should be valid"), Value::Int(0));
    assert_eq!(parse("2.5").expect("should be valid"), Value::Real(2.5));
    assert_eq!(parse("1e3").expect("should be valid"), Value::Real(1000.0));
    assert_eq!(parse(r#""hi""#).expect("should be valid"), text("hi"));
}

#[test]
fn parses_containers_with_whitespace() {
    let value = parse(
        "{
            \"a\": [1, 2],
            \"b\": { \"c\": null }
        }",
    )
    .expect("valid JSON");
    assert_eq!(
        value,
        object(&[
            ("a", Value::List(vec![Value::Int(1), Value::Int(2)])),
            ("b", object(&[("c", Value::Null)])),
        ])
    );
}

#[test]
fn parses_empty_containers() {
    assert_eq!(parse("{}").expect("should be valid"), Value::map());
    assert_eq!(parse("[]").expect("should be valid"), Value::List(vec![]));
    assert_eq!(
        parse("[[]]").expect("should be valid"),
        Value::List(vec![Value::List(vec![])])
    );
}

#[test]
fn unescapes_strings_including_surrogate_pairs() {
    let value = parse(r#""a\"b\\c\/d\n\t\u0041\uD83D\uDE00""#).expect("should be valid");
    assert_eq!(value, text("a\"b\\c/d\n\tA😀"));
}

#[test]
fn rejects_bad_escapes_with_position() {
    let error = parse(r#""a\q""#).expect_err("invalid escape");
    assert_eq!((error.line, error.column), (1, 1));
    assert!(error.message.contains("unknown escape"), "{error}");

    let error = parse("\"\\uD83D\"").expect_err("lone surrogate");
    assert!(error.message.contains("high surrogate"), "{error}");
}

#[test]
fn duplicate_keys_keep_the_last_one() {
    assert_eq!(
        parse(r#"{"a": 1, "a": 2}"#).expect("should be valid"),
        object(&[("a", Value::Int(2))])
    );
}

#[test]
fn huge_integers_fall_back_to_real() {
    // 超过 i64 的整数不是错误，退回小数。
    assert_eq!(
        parse("123456789012345678901234567890").expect("should be valid"),
        Value::Real(1.2345678901234568e29)
    );
}

#[test]
fn reports_syntax_errors_with_position() {
    let error = parse("{\n  \"a\": 1,\n}").expect_err("trailing comma");
    assert_eq!(error.line, 3, "{error}");
    assert!(error.message.contains("expected"), "{error}");

    // 位置只报一次：分析器文本里的前缀被摘掉了。
    let rendered = error.to_string();
    assert_eq!(rendered.matches("line 3").count(), 1, "{rendered}");
    assert!(rendered.starts_with("JSON parse failed (line 3, column 1): "), "{rendered}");

    let error = parse("{").expect_err("incomplete");
    assert_eq!((error.line, error.column), (1, 2), "{error}");
}

#[test]
fn error_display_mentions_line_and_column() {
    let error = JsonError::new(2, 5, "example");
    assert_eq!(error.to_string(), "JSON parse failed (line 2, column 5): example");
}

// ---------------------------------------------------------------------------
// 输出
// ---------------------------------------------------------------------------

#[test]
fn writes_compact_json() {
    let value = object(&[
        ("a", Value::List(vec![Value::Int(1), Value::Null, Value::Bool(true)])),
        ("b", text("x\"y\n")),
        ("c", Value::Real(1.0)),
    ]);
    assert_eq!(
        to_string(&value),
        r#"{"a":[1,null,true],"b":"x\"y\n","c":1.0}"#
    );
    assert_eq!(to_string(&Value::map()), "{}");
    assert_eq!(to_string(&Value::List(vec![])), "[]");
}

#[test]
fn writes_pretty_json() {
    let value = object(&[("a", Value::List(vec![Value::Int(1)]))]);
    assert_eq!(
        to_string_pretty(&value),
        "{\n  \"a\": [\n    1\n  ]\n}"
    );
}

#[test]
fn non_finite_reals_become_null() {
    assert_eq!(to_string(&Value::Real(f64::NAN)), "null");
    assert_eq!(to_string(&Value::Real(f64::INFINITY)), "null");
}

#[test]
fn round_trips_through_text() {
    let value = object(&[
        ("a", Value::List(vec![Value::Int(-1), Value::Real(0.5), text("newline\n")])),
        ("b", Value::Null),
    ]);
    assert_eq!(parse(&to_string(&value)).expect("should read back"), value);
    assert_eq!(parse(&to_string_pretty(&value)).expect("should read back"), value);
}

#[test]
fn writes_syntax_trees_in_the_documented_shape() {
    let analyzer = Analyzer::from_config(crate::json::Config::builtin()).expect("the builtin configuration should be consistent");
    let document: Document = analyzer.analyze("1").expect("analysis should succeed");
    let compact = document.to_json();
    assert!(compact.starts_with(r#"{"kind":"rule","rule":"program","span":{"start":0,"end":1"#), "{compact}");
    assert!(compact.contains(r#""children":[{"kind":"rule","rule":"value""#), "{compact}");
    assert!(compact.contains(r#"{"kind":"number","text":"1","span":{"start":0,"end":1,"line":1,"column":1}}"#), "{compact}");

    let pretty = document.to_json_pretty();
    assert!(pretty.starts_with("{\n  \"kind\": \"rule\","), "{pretty}");
    assert_eq!(parse(&pretty).expect("the output is valid JSON itself").as_map().expect("map").len(), 4);
}

#[test]
fn pattern_display_is_its_object_form() {
    assert_eq!(Pattern::regex("\\s+").to_string(), r#"{"regex":"\\s+"}"#);
    assert_eq!(
        Pattern::plus(Pattern::chars("0-9")).to_string(),
        r#"{"plus":{"chars":"0-9"}}"#
    );
}

// ---------------------------------------------------------------------------
// 文法本身
// ---------------------------------------------------------------------------

#[test]
fn grammar_matches_the_documented_bnf() {
    let bnf = grammar::analyzer().grammar().to_bnf();
    for line in [
        "json ::= value ;",
        "value ::= object | array | @string | @number | \"true\" | \"false\" | \"null\" ;",
        "object ::= \"{\" \"}\" | \"{\" members \"}\" ;",
        "members ::= member | members \",\" member ;",
        "member ::= @string \":\" value ;",
        "array ::= \"[\" \"]\" | \"[\" elements \"]\" ;",
        "elements ::= value | elements \",\" value ;",
    ] {
        assert!(bnf.contains(line), "BNF is missing `{line}`:\n{bnf}");
    }
}

#[test]
fn literals_are_derived_not_declared() {
    let analyzer = grammar::analyzer();
    // 只有三条声明过的规则 + 九个自动派生的字面量（true/false/null 与六个标点）。
    assert_eq!(analyzer.rules().rules.len(), 3 + 9);
    assert!(analyzer.rules().rule("true").is_some(), "literals should become rules automatically");
}

#[test]
fn analyzer_can_be_reused_for_many_documents() {
    let analyzer = grammar::analyzer();
    for _ in 0..3 {
        assert_eq!(parse("[1, 2]").expect("should be valid").as_list().expect("list").len(), 2);
    }
    assert!(analyzer.grammar().to_bnf().contains("json ::= value ;"));
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

#[test]
fn builtin_config_is_consistent() {
    let config = Config::builtin();
    assert_eq!(config.version, SUPPORTED_VERSION);
    assert!(!config.rules.is_empty());
    assert!(!config.grammar.productions.is_empty());
}

#[test]
fn rejects_unsupported_version() {
    let error = Config::parse(r#"{"version": 99, "rules": []}"#).expect_err("should fail");
    assert!(matches!(error, ConfigError::UnsupportedVersion(99)));
}

#[test]
fn legacy_version_points_at_the_migration() {
    let error = Config::parse(r#"{"version": 1, "rules": []}"#).expect_err("should fail");
    assert!(matches!(error, ConfigError::LegacyVersion));
    assert!(error.to_string().contains("no longer supported"), "{error}");
}

#[test]
fn rejects_string_pattern() {
    let error = Config::parse(r#"{ "version": 2, "rules": [{ "name": "x", "pattern": "\\d+" }] }"#)
        .expect_err("a string pattern should be rejected");
    assert!(error.to_string().contains("does not accept a string"), "{error}");
}

#[test]
fn rejects_string_rhs() {
    let error = Config::parse(
        r#"{ "version": 2, "rules": [], "grammar": { "productions": [
            { "name": "program", "rhs": "value*" }
        ] } }"#,
    )
    .expect_err("a string right-hand side should be rejected");
    assert!(error.to_string().contains("does not accept a string"), "{error}");
}

#[test]
fn rejects_unknown_keys() {
    let error =
        Config::parse(r#"{ "version": 2, "rules": [], "extra": 1 }"#).expect_err("unknown keys should be rejected");
    assert!(error.to_string().contains("unknown field"), "{error}");
}

#[test]
fn round_trips_config_through_json() {
    let config = Config::builtin();
    let text = config.to_json_pretty();
    let reparsed = Config::parse(&text).expect("should parse again");
    assert_eq!(config, reparsed);
    assert!(text.contains("$schema"), "the $schema hint should be preserved");
    assert!(text.contains(r#""regex""#), "{text}");
}
