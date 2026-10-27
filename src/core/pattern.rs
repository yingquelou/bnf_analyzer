//! 词法模式：`pattern` 只接受结构化对象，字符串形式的"整段正则"已废弃。
//!
//! ```json
//! { "name": "space",  "pattern": { "regex": "\\s+" }, "skip": true }
//! { "name": "lbrace", "pattern": { "literal": "{" } }
//! { "name": "digit",  "pattern": { "chars": "0-9" } }
//! { "name": "ident",  "pattern": { "seq": [
//!     { "chars": "a-zA-Z_" },
//!     { "star": { "chars": "a-zA-Z0-9_" } }
//! ] } }
//! ```
//!
//! 整段正则仍然能写，但必须显式包成 `{ "regex": "…" }`——配置里不存在"这个字符串
//! 到底是正则还是别的什么"的猜测。字面量与字符集合会自动转义，不必再和反斜杠搏斗。
//!
//! 可用的键：`regex`、`literal`、`chars`、`not_chars`、`seq`、`alt`、`star`、`plus`、`opt`。

use crate::core::value::Value;

/// 一条词法规则的模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// 整段正则，需要显式写成 `{ "regex": "…" }`。
    Regex(String),
    /// 精确匹配一段文本；正则元字符会被转义。
    Literal(String),
    /// 字符集合，编译成 `[…]`；内容按正则语法写，如 `0-9a-f`、`a-zA-Z_`。
    Chars(String),
    /// 排除型字符集合，编译成 `[^…]`。
    NotChars(String),
    /// 依次匹配。
    Seq(Vec<Pattern>),
    /// 任选其一。
    Alt(Vec<Pattern>),
    /// 零或多次。
    Star(Box<Pattern>),
    /// 一或多次。
    Plus(Box<Pattern>),
    /// 零或一次。
    Opt(Box<Pattern>),
}

/// 模式对象允许的键：写别的会被当作未知键拒绝，并在提示里列出这几项。
pub const PATTERN_KEYS: &[&str] = &[
    "regex", "literal", "chars", "not_chars", "seq", "alt", "star", "plus", "opt",
];

/// 同上，拼成一句话用于错误提示。
fn keys_hint() -> String {
    PATTERN_KEYS.join(", ")
}

impl Pattern {
    /// 便捷构造：整段正则。
    pub fn regex(text: impl Into<String>) -> Self {
        Pattern::Regex(text.into())
    }

    /// 便捷构造：字面量（元字符自动转义）。
    pub fn literal(text: impl Into<String>) -> Self {
        Pattern::Literal(text.into())
    }

    /// 便捷构造：字符集合。
    pub fn chars(set: impl Into<String>) -> Self {
        Pattern::Chars(set.into())
    }

    /// 便捷构造：一或多次。
    pub fn plus(inner: Pattern) -> Self {
        Pattern::Plus(Box::new(inner))
    }

    /// 便捷构造：零或多次。
    pub fn star(inner: Pattern) -> Self {
        Pattern::Star(Box::new(inner))
    }

    /// 便捷构造：零或一次。
    pub fn opt(inner: Pattern) -> Self {
        Pattern::Opt(Box::new(inner))
    }

    /// 编译成等价的正则源码。
    pub fn to_regex(&self) -> String {
        match self {
            Pattern::Regex(text) => text.clone(),
            Pattern::Literal(text) => regex::escape(text),
            Pattern::Chars(set) => format!("[{set}]"),
            Pattern::NotChars(set) => format!("[^{set}]"),
            Pattern::Seq(items) => items.iter().map(Pattern::atom_regex).collect(),
            Pattern::Alt(items) => items
                .iter()
                .map(Pattern::atom_regex)
                .collect::<Vec<_>>()
                .join("|"),
            Pattern::Star(inner) => format!("(?:{})*", inner.to_regex()),
            Pattern::Plus(inner) => format!("(?:{})+", inner.to_regex()),
            Pattern::Opt(inner) => format!("(?:{})?", inner.to_regex()),
        }
    }

    /// 作为组合子的一部分使用时的渲染：可能改变结合性的部分加 `(?:…)`。
    fn atom_regex(&self) -> String {
        match self {
            // 本身就是一个原子，或者已经自带分组
            Pattern::Literal(_)
            | Pattern::Chars(_)
            | Pattern::NotChars(_)
            | Pattern::Star(_)
            | Pattern::Plus(_)
            | Pattern::Opt(_) => self.to_regex(),
            // 序列、选择、以及裸正则需要包一层，才能安全地参与拼接
            Pattern::Seq(_) | Pattern::Alt(_) | Pattern::Regex(_) => {
                format!("(?:{})", self.to_regex())
            }
        }
    }

    /// 编译出的正则源码是否为空（只有 `{ "regex": "" }` 这种退化写法才会）。
    ///
    /// 注意它**不**判断"是否会匹配空串"：`a*` 的源码不为空，却能匹配空串。
    /// 后者在装配 [`Rules`](crate::core::rules::Rules) 时用编译好的正则试匹配空串来拒绝。
    pub fn is_empty(&self) -> bool {
        self.to_regex().is_empty()
    }

    /// 从 [`Value`] 树读取（就是 `{ "regex": … }` 那种形态），错误信息面向配置作者。
    ///
    /// 字符串一律拒绝：整段正则必须显式写成 `{ "regex": "…" }`。
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let error_for_string = |text: &str| {
            format!(
                "`pattern` does not accept a string ({text:?}): use a structured object such as \
                 {{ \"regex\": {text:?} }}, {{ \"literal\": \"{{\" }} or {{ \"chars\": \"0-9\" }}; keys: {}",
                keys_hint()
            )
        };
        if let Some(text) = value.as_text() {
            return Err(error_for_string(text));
        }
        let map = value
            .as_map()
            .ok_or_else(|| format!("`pattern` must be an object (one of {}), found {}", keys_hint(), value.type_name()))?;
        if map.len() != 1 {
            return Err(format!(
                "a pattern object must have exactly one key (one of {}), found {}; combine patterns with {{ \"seq\": [ … ] }}",
                keys_hint(),
                map.len()
            ));
        }
        let (key, value) = map.iter().next().expect("length is one");
        match key.as_str() {
            "regex" => Ok(Pattern::Regex(string_of(key, value)?)),
            "literal" => Ok(Pattern::Literal(string_of(key, value)?)),
            "chars" => Ok(Pattern::Chars(string_of(key, value)?)),
            "not_chars" => Ok(Pattern::NotChars(string_of(key, value)?)),
            "seq" => Ok(Pattern::Seq(patterns_of(key, value)?)),
            "alt" => Ok(Pattern::Alt(patterns_of(key, value)?)),
            "star" => Ok(Pattern::Star(Box::new(Pattern::from_value(value)?))),
            "plus" => Ok(Pattern::Plus(Box::new(Pattern::from_value(value)?))),
            "opt" => Ok(Pattern::Opt(Box::new(Pattern::from_value(value)?))),
            other => Err(format!(
                "unknown pattern key `{other}`, expected one of: {}",
                keys_hint()
            )),
        }
    }

    /// 转回 [`Value`] 树：每个变体写成同名单键对象，与配置里的写法完全一致。
    pub fn to_value(&self) -> Value {
        let (key, value) = match self {
            Pattern::Regex(text) => ("regex", Value::text(text)),
            Pattern::Literal(text) => ("literal", Value::text(text)),
            Pattern::Chars(set) => ("chars", Value::text(set)),
            Pattern::NotChars(set) => ("not_chars", Value::text(set)),
            Pattern::Seq(items) => ("seq", Value::List(items.iter().map(Pattern::to_value).collect())),
            Pattern::Alt(items) => ("alt", Value::List(items.iter().map(Pattern::to_value).collect())),
            Pattern::Star(inner) => ("star", inner.to_value()),
            Pattern::Plus(inner) => ("plus", inner.to_value()),
            Pattern::Opt(inner) => ("opt", inner.to_value()),
        };
        let mut map = std::collections::BTreeMap::new();
        map.insert(key.to_string(), value);
        Value::Map(map)
    }
}

fn string_of(key: &str, value: &Value) -> Result<String, String> {
    value
        .as_text()
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` must be a string, found {}", value.type_name()))
}

fn patterns_of(key: &str, value: &Value) -> Result<Vec<Pattern>, String> {
    let items = value
        .as_list()
        .ok_or_else(|| format!("`{key}` must be an array, found {}", value.type_name()))?;
    if items.is_empty() {
        return Err(format!("`{key}` needs at least one element"));
    }
    items.iter().map(Pattern::from_value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn object(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn literal_escapes_regex_metacharacters() {
        assert_eq!(Pattern::literal("{").to_regex(), r"\{");
        assert_eq!(Pattern::literal("<<").to_regex(), "<<");
        assert_eq!(Pattern::literal("a.b").to_regex(), r"a\.b");
    }

    #[test]
    fn sequence_wraps_alternatives_to_keep_precedence() {
        let pattern = Pattern::Seq(vec![
            Pattern::Alt(vec![Pattern::literal("a"), Pattern::literal("b")]),
            Pattern::plus(Pattern::chars("0-9")),
        ]);
        assert_eq!(pattern.to_regex(), "(?:a|b)(?:[0-9])+");
    }

    #[test]
    fn nested_regex_is_grouped() {
        let pattern = Pattern::Seq(vec![Pattern::regex("a|b")]);
        assert_eq!(pattern.to_regex(), "(?:a|b)");
        // 顶层正则保持原样
        assert_eq!(Pattern::regex("a|b").to_regex(), "a|b");
    }

    #[test]
    fn accepts_only_objects() {
        let value = object(&[("literal", Value::text("{"))]);
        assert_eq!(
            Pattern::from_value(&value).expect("should be valid"),
            Pattern::Literal("{".into())
        );

        let error = Pattern::from_value(&Value::text("\\s+")).expect_err("a string should be rejected");
        assert!(error.contains("does not accept a string"), "{error}");
        assert!(error.contains("regex"), "the error should suggest a fix: {error}");
    }

    #[test]
    fn reports_actionable_errors() {
        let two_keys = object(&[("literal", Value::text("a")), ("chars", Value::text("b"))]);
        let error = Pattern::from_value(&two_keys).expect_err("should fail");
        assert!(error.contains("exactly one key"), "{error}");

        let unknown = object(&[("regexp", Value::text("a"))]);
        let error = Pattern::from_value(&unknown).expect_err("should fail");
        assert!(error.contains("unknown pattern key"), "{error}");

        let error = Pattern::from_value(&Value::Int(42)).expect_err("should fail");
        assert!(error.contains("must be an object"), "{error}");
        assert!(error.contains("integer"), "the error should name the actual type: {error}");
    }

    #[test]
    fn value_round_trips_and_keeps_the_object_shape() {
        let pattern = Pattern::Seq(vec![
            Pattern::Literal("{".to_string()),
            Pattern::Star(Box::new(Pattern::Chars("0-9".to_string()))),
            Pattern::Opt(Box::new(Pattern::NotChars("\\n".to_string()))),
        ]);
        let value = pattern.to_value();
        assert_eq!(Pattern::from_value(&value).expect("should read back"), pattern);

        let map = value.as_map().expect("the top level is an object");
        assert!(map.contains_key("seq"), "a combinator is a single-key object: {value}");
        assert!(
            matches!(Pattern::regex("\\s+").to_value(), Value::Map(_)),
            "a regex is an object, not a bare string"
        );
    }
}
