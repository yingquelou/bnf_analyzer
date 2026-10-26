//! JSON 配置的**唯一**形态：结构化的 `pattern` 与 `rhs`，以及
//! `schema/config.schema.json` 与实现的一致性。
//!
//! 字符串写法（正则原文、BNF/EBNF 文本）已废弃，这里逐个断言它们会被拒绝，
//! 并检查错误信息给出了改法。
//!
//! 这里不引入 JSON Schema 校验器（那会为一个测试拉进上百个 crate），而是用一个
//! 只认识本 schema 用到的关键字的自检器：`type`、`const`、`required`、`properties`、
//! `additionalProperties`、`items`、`minItems`、`minLength`、`minProperties`、
//! `maxProperties`、`$ref`。它保证「schema 忘声明某个键」这类漂移会被发现。
//!
//! 连读取 JSON 本身也用的是本 crate 的 `json` 模块——测试里没有第三方 JSON 依赖。

use std::fs;

use analyzer::json;
use analyzer::core::Analyzer;
use analyzer::core::grammar::EXPR_KEYS;
use analyzer::core::pattern::{Pattern, PATTERN_KEYS};
use analyzer::core::rules::Rules;
use analyzer::core::tokenizer::Tokenizer;
use analyzer::core::value::Value;
use analyzer::json::{Config, ConfigError};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn json_text(text: &str) -> Value {
    json::parse(text).unwrap_or_else(|error| panic!("{text} is not valid JSON: {error}"))
}

fn read_json(path: &str) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    json::parse(&text).unwrap_or_else(|error| panic!("{path} is not valid JSON: {error}"))
}

fn schema() -> Value {
    read_json(&format!("{ROOT}/schema/config.schema.json"))
}

// ---------------------------------------------------------------------------
// 取值小工具：只覆盖 schema 自检用到的几种形态
// ---------------------------------------------------------------------------

fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_map().and_then(|map| map.get(key))
}

fn get_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    get(value, key).and_then(Value::as_text)
}

fn get_int(value: &Value, key: &str) -> Option<i64> {
    get(value, key).and_then(Value::as_int)
}

fn pointer<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut node = value;
    for raw in path.split('/') {
        if raw.is_empty() || raw == "#" {
            continue;
        }
        let key = raw.replace("~1", "/").replace("~0", "~");
        node = get(node, &key)?;
    }
    Some(node)
}

// ---------------------------------------------------------------------------
// 结构自检器
// ---------------------------------------------------------------------------

fn resolve<'a>(schema: &'a Value, node: &'a Value) -> &'a Value {
    match get_str(node, "$ref") {
        Some(reference) => {
            let path = reference.strip_prefix('#').unwrap_or(reference);
            pointer(schema, path)
                .unwrap_or_else(|| panic!("$ref points to a missing definition: {reference}"))
        }
        None => node,
    }
}

fn check(schema: &Value, node: &Value, instance: &Value, path: &str, errors: &mut Vec<String>) {
    let node = resolve(schema, node);

    if let Some(expected) = get(node, "const")
        && expected != instance
    {
        errors.push(format!(
            "{path}: expected {}, found {}",
            json::to_string(expected),
            json::to_string(instance)
        ));
    }

    match get_str(node, "type") {
        Some("object") => {
            let Some(map) = instance.as_map() else {
                errors.push(format!("{path}: expected an object"));
                return;
            };
            let properties = get(node, "properties").and_then(Value::as_map);
            let reject_unknown = get(node, "additionalProperties") == Some(&Value::Bool(false));

            for key in get(node, "required")
                .and_then(Value::as_list)
                .unwrap_or(&[])
                .iter()
            {
                let key = key.as_text().unwrap_or_default();
                if !map.contains_key(key) {
                    errors.push(format!("{path}: missing required field {key}"));
                }
            }
            if let Some(limit) = get_int(node, "minProperties")
                && (map.len() as i64) < limit
            {
                errors.push(format!("{path}: needs at least {limit} keys, found {}", map.len()));
            }
            if let Some(limit) = get_int(node, "maxProperties")
                && (map.len() as i64) > limit
            {
                errors.push(format!("{path}: allows at most {limit} keys, found {}", map.len()));
            }
            for (key, value) in map {
                match properties.and_then(|properties| properties.get(key)) {
                    Some(child) => check(schema, child, value, &format!("{path}.{key}"), errors),
                    None if reject_unknown => {
                        errors.push(format!("{path}.{key}: the schema does not declare this key"))
                    }
                    None => {}
                }
            }
        }
        Some("array") => {
            let Some(items) = instance.as_list() else {
                errors.push(format!("{path}: expected an array"));
                return;
            };
            if let Some(limit) = get_int(node, "minItems")
                && (items.len() as i64) < limit
            {
                errors.push(format!("{path}: needs at least {limit} elements, found {}", items.len()));
            }
            if let Some(item_schema) = get(node, "items") {
                for (index, item) in items.iter().enumerate() {
                    check(schema, item_schema, item, &format!("{path}[{index}]"), errors);
                }
            }
        }
        Some("string") => match instance.as_text() {
            Some(text) => {
                if let Some(limit) = get_int(node, "minLength")
                    && (text.chars().count() as i64) < limit
                {
                    errors.push(format!("{path}: length must be at least {limit}"));
                }
            }
            None => errors.push(format!("{path}: expected a string")),
        },
        Some("integer") if instance.as_int().is_none() => {
            errors.push(format!("{path}: expected an integer"))
        }
        Some("boolean") if instance.as_bool().is_none() => {
            errors.push(format!("{path}: expected a boolean"))
        }
        _ => {}
    }
}

fn lint(instance: &Value) -> Vec<String> {
    let schema = schema();
    let mut errors = Vec::new();
    check(&schema, &schema, instance, "$", &mut errors);
    errors
}

// ---------------------------------------------------------------------------
// pattern：只接受对象
// ---------------------------------------------------------------------------

fn config_with_rules(rules: &str) -> String {
    format!(r#"{{ "version": 2, {rules} }}"#)
}

fn tokenize(config: &str, source: &str) -> Vec<(String, String)> {
    let config = Config::parse(config).expect("the configuration should be valid");
    let rules = Rules::from_rules(config.rules).expect("the rules should be valid");
    Tokenizer::new(rules)
        .tokenize(source)
        .expect("the source should tokenize")
        .into_iter()
        .map(|token| (token.kind, token.text))
        .collect()
}

#[test]
fn pattern_compiles_to_the_expected_regex() {
    let config = Config::parse(
        r#"{ "version": 2, "rules": [
            { "name": "space", "pattern": { "regex": "\\s+" }, "skip": true },
            { "name": "number", "pattern": { "plus": { "chars": "0-9" } } },
            { "name": "brace", "pattern": { "literal": "{" } },
            { "name": "ident", "pattern": { "seq": [
                { "chars": "a-zA-Z_" },
                { "star": { "chars": "a-zA-Z0-9_" } }
            ] } }
        ] }"#,
    )
    .expect("the configuration should be valid");

    assert_eq!(config.rules[0].pattern, Pattern::regex("\\s+"));
    assert_eq!(config.rules[1].pattern.to_regex(), "(?:[0-9])+");
    assert_eq!(config.rules[2].pattern, Pattern::literal("{"));
    assert_eq!(config.rules[2].pattern.to_regex(), r"\{", "literals are escaped automatically");
    assert_eq!(
        config.rules[3].pattern.to_regex(),
        "[a-zA-Z_](?:[a-zA-Z0-9_])*"
    );

    let tokens = tokenize(
        &config_with_rules(
            r#""rules": [
                { "name": "space", "pattern": { "regex": "\\s+" }, "skip": true },
                { "name": "ident", "pattern": { "seq": [
                    { "chars": "a-zA-Z_" },
                    { "star": { "chars": "a-zA-Z0-9_" } }
                ] } }
            ]"#,
        ),
        "abc_1 _x9",
    );
    assert_eq!(
        tokens,
        vec![
            ("ident".to_string(), "abc_1".to_string()),
            ("ident".to_string(), "_x9".to_string())
        ]
    );
}

#[test]
fn string_pattern_is_rejected() {
    let error = Config::parse(&config_with_rules(
        r#""rules": [{ "name": "space", "pattern": "\\s+" }]"#,
    ))
    .expect_err("a string pattern should be rejected");

    let message = error.to_string();
    assert!(message.contains("does not accept a string"), "{message}");
    assert!(message.contains("regex"), "the error should suggest a fix: {message}");
}

#[test]
fn pattern_errors_point_at_the_key() {
    let error = Config::parse(&config_with_rules(
        r#""rules": [{ "name": "x", "pattern": { "literal": "a", "chars": "b" } }]"#,
    ))
    .expect_err("two keys should be rejected");
    assert!(error.to_string().contains("exactly one key"), "{error}");

    let error = Config::parse(&config_with_rules(
        r#""rules": [{ "name": "x", "pattern": { "regexp": "a" } }]"#,
    ))
    .expect_err("unknown keys should be rejected");
    assert!(error.to_string().contains("unknown pattern key"), "{error}");

    let error = Config::parse(&config_with_rules(
        r#""rules": [{ "name": "x", "pattern": { "star": "a" } }]"#,
    ))
    .expect_err("nested values must be objects");
    assert!(error.to_string().contains("does not accept a string"), "{error}");
}

// ---------------------------------------------------------------------------
// rhs：只接受对象
// ---------------------------------------------------------------------------

const GRAMMAR: &str = r#"{ "version": 2,
    "rules": [
        { "name": "space", "pattern": { "regex": "\\s+" }, "skip": true },
        { "name": "number", "pattern": { "plus": { "chars": "0-9" } } },
        { "name": "name", "pattern": { "seq": [
            { "literal": "/" },
            { "star": { "chars": "a-zA-Z0-9" } }
        ] } }
    ],
    "grammar": {
        "start": "program",
        "productions": [
            { "name": "program", "rhs": { "star": { "rule": "value" } } },
            { "name": "value", "rhs": { "alt": [
                { "token": "number" },
                { "token": "name" },
                { "rule": "procedure" }
            ] } },
            { "name": "procedure", "rhs": { "seq": [
                { "literal": "{" },
                { "star": { "rule": "value" } },
                { "literal": "}" }
            ] } }
        ]
    }
}"#;

#[test]
fn structured_rhs_builds_the_expected_grammar() {
    let analyzer = Analyzer::from_config(Config::parse(GRAMMAR).expect("the configuration should be valid"))
        .expect("the configuration should be consistent");

    assert_eq!(
        analyzer.grammar().to_bnf(),
        "program ::= value* ;\nvalue ::= @number | @name | procedure ;\n\
         procedure ::= \"{\" value* \"}\" ;\n"
    );

    let document = analyzer.analyze("{ 1 /x }").expect("the source should be valid");
    assert_eq!(document.token_count(), 4, "`{{ 1 /x }}` yields 4 tokens");
    assert!(document.root.find_rule("procedure").is_some());
}

#[test]
fn string_rhs_is_rejected() {
    let error = Config::parse(
        r#"{ "version": 2, "rules": [], "grammar": { "productions": [
            { "name": "program", "rhs": "value*" }
        ] } }"#,
    )
    .expect_err("a string right-hand side should be rejected");

    let message = error.to_string();
    assert!(message.contains("does not accept a string"), "{message}");
    assert!(message.contains("structured object"), "the error should suggest a fix: {message}");
}

#[test]
fn structured_rhs_errors_are_actionable() {
    let error = Config::parse(
        r#"{ "version": 2, "rules": [], "grammar": { "productions": [
            { "name": "program", "rhs": { "regexp": "value" } }
        ] } }"#,
    )
    .expect_err("unknown keys should be rejected");
    assert!(error.to_string().contains("unknown expression key"), "{error}");

    let error = Config::parse(
        r#"{ "version": 2, "rules": [], "grammar": { "productions": [
            { "name": "program", "rhs": { "star": "value" } }
        ] } }"#,
    )
    .expect_err("nested values must be objects");
    assert!(error.to_string().contains("does not accept a string"), "{error}");
}

#[test]
fn legacy_version_points_at_the_migration() {
    let error = Config::parse(r#"{ "version": 1, "rules": [{ "name": "x", "pattern": "\\d+" }] }"#)
        .expect_err("version 1 should be rejected");
    assert!(matches!(error, ConfigError::LegacyVersion), "{error}");
    assert!(error.to_string().contains("no longer supported"), "{error}");
}

// ---------------------------------------------------------------------------
// 序列化：读进来什么样，写出去还是什么样
// ---------------------------------------------------------------------------

#[test]
fn config_round_trips_through_json() {
    let config = Config::parse(GRAMMAR).expect("the configuration should be valid");
    let text = config.to_json_pretty();
    let back = Config::parse(&text).expect("should parse again");
    assert_eq!(config, back, "serialization should round-trip");

    // 正则回写成对象，而不是退回字符串。
    assert!(text.contains(r#""regex""#), "{text}");
    assert!(text.contains(r#""star""#), "{text}");
}

// ---------------------------------------------------------------------------
// schema 与实现的一致性
// ---------------------------------------------------------------------------

fn keys_of(value: &Value, path: &str) -> Vec<String> {
    let mut keys: Vec<String> = pointer(value, path)
        .and_then(Value::as_map)
        .unwrap_or_else(|| panic!("the schema is missing {path}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn sorted(keys: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
    keys.sort();
    keys
}

#[test]
fn schema_declares_exactly_the_supported_keys() {
    let schema = schema();
    assert_eq!(
        keys_of(&schema, "/$defs/pattern/properties"),
        sorted(PATTERN_KEYS),
        "schema pattern keys do not match Pattern"
    );
    assert_eq!(
        keys_of(&schema, "/$defs/expression/properties"),
        sorted(EXPR_KEYS),
        "schema expression keys do not match Expr"
    );

    for path in ["/$defs/pattern", "/$defs/expression"] {
        let node = pointer(&schema, path).expect("should exist");
        assert_eq!(
            get_str(node, "type"),
            Some("object"),
            "{path} must accept objects only"
        );
        assert_eq!(get_int(node, "maxProperties"), Some(1), "{path} must allow exactly one key");
        assert_eq!(
            get(node, "additionalProperties"),
            Some(&Value::Bool(false)),
            "{path} must reject unknown keys"
        );
    }
}

#[test]
fn schema_refs_all_resolve() {
    let schema = schema();
    let mut refs = Vec::new();
    collect_refs(&schema, &mut refs);
    assert!(!refs.is_empty(), "the schema should use $ref");
    for reference in refs {
        let path = reference.strip_prefix('#').unwrap_or(&reference).to_string();
        assert!(
            pointer(&schema, &path).is_some(),
            "$ref points to a missing definition: {reference}"
        );
    }
}

fn collect_refs(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Map(map) => {
            for (key, value) in map {
                if key == "$ref" {
                    if let Some(reference) = value.as_text() {
                        out.push(reference.to_string());
                    }
                } else {
                    collect_refs(value, out);
                }
            }
        }
        Value::List(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

#[test]
fn builtin_config_matches_schema() {
    let instance = read_json(&format!("{ROOT}/examples/postscript.json"));
    let errors = lint(&instance);
    assert!(errors.is_empty(), "postscript.json does not match the schema: {errors:#?}");
    Config::parse(&json::to_string(&instance)).expect("postscript.json should parse");
}

#[test]
fn example_config_matches_schema_and_parses() {
    let instance = read_json(&format!("{ROOT}/examples/minimal.json"));
    let errors = lint(&instance);
    assert!(
        errors.is_empty(),
        "examples/minimal.json does not match the schema: {errors:#?}"
    );

    let config = Config::parse(&json::to_string(&instance)).expect("the example configuration should parse");
    let analyzer = Analyzer::from_config(config).expect("the example configuration should be consistent");
    let document = analyzer.analyze("{ 1 /x }").expect("the example source should be valid");
    assert_eq!(document.token_count(), 4, "`{{ 1 /x }}` yields 4 tokens");
}

#[test]
fn schema_agrees_with_runtime_on_what_is_invalid() {
    // 字符串 pattern：schema 要求 object，运行时也拒绝。
    let string_pattern = json_text(
        r#"{
            "version": 2,
            "rules": [{ "name": "space", "pattern": "\\s+" }]
        }"#,
    );
    assert!(!lint(&string_pattern).is_empty(), "the schema should reject strings");
    assert!(Config::parse(&json::to_string(&string_pattern)).is_err());

    // 字符串 rhs：同上。
    let string_rhs = json_text(
        r#"{
            "version": 2,
            "rules": [],
            "grammar": { "productions": [{ "name": "program", "rhs": "value*" }] }
        }"#,
    );
    assert!(!lint(&string_rhs).is_empty(), "the schema should reject strings");
    assert!(Config::parse(&json::to_string(&string_rhs)).is_err());

    // 未知的表达式键：schema 与运行时都拒绝。
    let bad_expr = json_text(
        r#"{
            "version": 2,
            "rules": [],
            "grammar": { "productions": [{ "name": "program", "rhs": { "regexp": "value" } }] }
        }"#,
    );
    assert!(!lint(&bad_expr).is_empty(), "the schema should reject unknown keys");
    assert!(Config::parse(&json::to_string(&bad_expr)).is_err());

    // 两个键的对象：schema 用 maxProperties 拦住，运行时用"只有一个键"拦住。
    let bad_pattern = json_text(
        r#"{
            "version": 2,
            "rules": [{ "name": "x", "pattern": { "literal": "a", "chars": "b" } }]
        }"#,
    );
    assert!(!lint(&bad_pattern).is_empty(), "the schema should reject multiple keys");
    assert!(Config::parse(&json::to_string(&bad_pattern)).is_err());

    // 正常配置两边都放行。
    let good = json_text(
        r#"{
            "version": 2,
            "rules": [{ "name": "x", "pattern": { "literal": "a" } }],
            "grammar": { "productions": [{ "name": "program", "rhs": { "token": "x" } }] }
        }"#,
    );
    assert!(lint(&good).is_empty(), "{:#?}", lint(&good));
    assert!(Config::parse(&json::to_string(&good)).is_ok());
}
