//! 词法规则：把 JSON 声明的 token 规则编译成可直接用于扫描的匹配器。
//!
//! `pattern` 只接受结构化对象（见 [`Pattern`]），整段正则要显式包成 `regex`：
//!
//! ```json
//! { "rules": [{ "name": "number", "pattern": { "plus": { "chars": "0-9" } }, "priority": 10 }] }
//! { "name": "lbrace", "pattern": { "literal": "{" } }
//! { "name": "ident",  "pattern": { "seq": [
//!     { "chars": "a-zA-Z_" },
//!     { "star": { "chars": "a-zA-Z0-9_" } }
//! ] } }
//! ```
//!
//! - 每条规则的正则都会被加上 `\A` 锚定，因此只会从扫描位置起匹配；
//! - `priority` 用于同长度竞争时决定胜负（越大越优先）；
//! - `skip` 为真时只消费源码、不产出 token（空白、注释等）。
//!
//! 分隔符、关键字这类**字面量终结符**不在这里声明，而是由文法（`postscript.json` 的
//! `grammar.productions[].rhs`）里的 `{ "literal": "{" }` 给出，
//! 运行时由 [`Rules::with_literals`] 自动补成优先级更高的规则。

use std::collections::HashSet;
use std::fmt;

use regex::Regex;

use crate::core::pattern::Pattern;
use crate::core::value::Value;

/// 自动派生的字面量终结符的优先级：同长度时压过普通规则。
pub const LITERAL_PRIORITY: i32 = 1000;

/// 配置里一条规则允许的键。
pub const RULE_KEYS: &[&str] = &["name", "pattern", "priority", "skip"];

/// 一条匹配规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 规则名，会作为 token 的 kind 输出。
    pub name: String,
    /// 匹配模式：结构化的 [`Pattern`] 对象。
    pub pattern: Pattern,
    /// 同长度竞争时的优先级，越大越优先。
    pub priority: i32,
    /// 为真时只消费源码、不产出 token（空白、注释等）。
    pub skip: bool,
}

impl Rule {
    /// 用显式的 [`Pattern`] 构造。
    pub fn new(name: impl Into<String>, pattern: Pattern) -> Self {
        Self {
            name: name.into(),
            pattern,
            priority: 0,
            skip: false,
        }
    }

    /// 用整段正则构造，等价于 `Rule::regex(name, Pattern::regex(…))`。
    pub fn regex(name: impl Into<String>, regex: impl Into<String>) -> Self {
        Self::new(name, Pattern::regex(regex))
    }

    /// 用字面量构造，等价于 `Rule::regex(name, Pattern::literal(…))`。
    pub fn literal(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(name, Pattern::literal(text))
    }

    /// 只消费、不产出的规则。
    pub fn skipping(name: impl Into<String>, pattern: Pattern) -> Self {
        Self {
            skip: true,
            ..Self::new(name, pattern)
        }
    }

    /// 只消费、不产出的正则规则。
    pub fn skipping_regex(name: impl Into<String>, regex: impl Into<String>) -> Self {
        Self::skipping(name, Pattern::regex(regex))
    }

    /// 指定优先级。
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// 从 [`Value`] 树读取一条规则：`{ "name": …, "pattern": …, "priority": …, "skip": … }`。
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let map = value.as_map().ok_or_else(|| {
            format!(
                "a rule must be an object (keys: {}), found {}",
                RULE_KEYS.join(", "),
                value.type_name()
            )
        })?;
        for key in map.keys() {
            if !RULE_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "unknown field `{key}` in rule, expected one of: {}",
                    RULE_KEYS.join(", ")
                ));
            }
        }
        let name = map
            .get("name")
            .ok_or_else(|| "rule is missing `name`".to_string())?
            .as_text()
            .ok_or_else(|| "`name` must be a string".to_string())?
            .to_string();
        let pattern = map
            .get("pattern")
            .ok_or_else(|| format!("rule `{name}` is missing `pattern`"))?;
        let priority = match map.get("priority") {
            None => 0,
            Some(value) => value
                .as_int()
                .and_then(|number| i32::try_from(number).ok())
                .ok_or_else(|| {
                    format!(
                        "`priority` of rule `{name}` must be a 32-bit integer, found {}",
                        value.type_name()
                    )
                })?,
        };
        let skip = match map.get("skip") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                format!(
                    "`skip` of rule `{name}` must be a boolean, found {}",
                    value.type_name()
                )
            })?,
        };
        let pattern = Pattern::from_value(pattern)
            .map_err(|error| format!("pattern of rule `{name}`: {error}"))?;
        Ok(Self {
            name,
            pattern,
            priority,
            skip,
        })
    }

    /// 转回 [`Value`] 树；`priority` / `skip` 是默认值时省略。
    pub fn to_value(&self) -> Value {
        let mut map = std::collections::BTreeMap::new();
        map.insert("name".to_string(), Value::text(&self.name));
        map.insert("pattern".to_string(), self.pattern.to_value());
        if self.priority != 0 {
            map.insert("priority".to_string(), Value::Int(i64::from(self.priority)));
        }
        if self.skip {
            map.insert("skip".to_string(), Value::Bool(true));
        }
        Value::Map(map)
    }
}

/// 编译后每个正则槽位的语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// 只消费、不产出 token。
    Skip,
    /// 产出 `kind` 为该名字的 token。
    Emit(String),
}

/// 一条已编译的规则。
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub(crate) slot: Slot,
    pub(crate) regex: Regex,
    pub(crate) priority: i32,
    /// 声明顺序，用于同优先级时的稳定裁决。
    pub(crate) order: usize,
    /// 原始 pattern，便于错误提示。
    pub(crate) source: String,
}

/// 校验并编译后的规则集合。
#[derive(Debug, Clone)]
pub struct Rules {
    /// 原始规则声明，保留下来便于回写配置或输出诊断信息。
    pub rules: Vec<Rule>,
    compiled: Vec<CompiledRule>,
}

impl Rules {
    /// 从规则列表编译，期间完成全部校验。
    pub fn from_rules(rules: Vec<Rule>) -> Result<Self, RulesError> {
        let mut compiled = Vec::new();
        let mut names: HashSet<&str> = HashSet::new();

        for (order, rule) in rules.iter().enumerate() {
            if rule.name.trim().is_empty() {
                return Err(RulesError::EmptyName);
            }
            if !names.insert(rule.name.as_str()) {
                return Err(RulesError::DuplicateName(rule.name.clone()));
            }
            let source = rule.pattern.to_regex();
            if source.is_empty() {
                return Err(RulesError::EmptyPattern(rule.name.clone()));
            }
            compiled.push(CompiledRule {
                slot: if rule.skip {
                    Slot::Skip
                } else {
                    Slot::Emit(rule.name.clone())
                },
                regex: compile_regex(&rule.name, &source)?,
                priority: rule.priority,
                order,
                source,
            });
        }

        Ok(Self { rules, compiled })
    }

    /// 把一组字面量终结符补进规则集，重复的会跳过。
    ///
    /// 文法里的 `"{"` 之类字面量由这里转成规则，规则名就是字面量本身，
    /// 因此 token 的 kind 会直接是 `{`。
    pub fn with_literals(&self, literals: &[String]) -> Result<Self, RulesError> {
        let mut rules = self.rules.clone();
        for literal in literals {
            if literal.is_empty() {
                return Err(RulesError::EmptyPattern("<literal>".to_string()));
            }
            if rules.iter().any(|rule| rule.name == *literal) {
                continue;
            }
            rules.push(Rule {
                name: literal.clone(),
                // 用结构化的字面量模式：转义交给 Pattern 处理，语义也更清楚。
                pattern: Pattern::Literal(literal.clone()),
                priority: LITERAL_PRIORITY,
                skip: false,
            });
        }
        Self::from_rules(rules)
    }

    /// 已编译的槽位列表。
    pub fn entries(&self) -> &[CompiledRule] {
        &self.compiled
    }

    /// 按名字查找原始规则。
    pub fn rule(&self, name: &str) -> Option<&Rule> {
        self.rules.iter().find(|rule| rule.name == name)
    }
}

fn compile_regex(owner: &str, pattern: &str) -> Result<Regex, RulesError> {
    let anchored = format!(r"\A(?:{pattern})");
    Regex::new(&anchored).map_err(|source| RulesError::Regex {
        owner: owner.to_string(),
        pattern: pattern.to_string(),
        source,
    })
}

/// 规则构建过程中的错误。
#[derive(Debug)]
pub enum RulesError {
    /// 读文件失败。
    Io {
        /// 文件路径。
        path: String,
        /// 底层错误。
        source: std::io::Error,
    },
    /// 正则编译失败。
    Regex {
        /// 规则名或字面量原文。
        owner: String,
        /// 编译失败的正则。
        pattern: String,
        /// 底层错误。
        source: regex::Error,
    },
    /// 规则名为空。
    EmptyName,
    /// 规则名重复。
    DuplicateName(String),
    /// 正则能匹配空串。
    EmptyPattern(String),
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::Io { path, source } => {
                write!(f, "failed to read rules file {path}: {source}")
            }
            RulesError::Regex {
                owner,
                pattern,
                source,
            } => write!(f, "failed to compile regex ({owner}: {pattern}): {source}"),
            RulesError::EmptyName => f.write_str("rule name must not be empty"),
            RulesError::DuplicateName(name) => write!(f, "duplicate rule name: {name}"),
            RulesError::EmptyPattern(name) => write!(f, "empty regex (rule {name})"),
        }
    }
}

impl std::error::Error for RulesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RulesError::Io { source, .. } => Some(source),
            RulesError::Regex { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::from_rules(vec![
            Rule::skipping_regex("space", r"\s+"),
            Rule::regex("word", "[a-z]+"),
        ])
        .expect("the rules should be valid")
    }

    #[test]
    fn compiles_rules_in_declaration_order() {
        let rules = rules();
        assert_eq!(rules.entries().len(), 2);
        assert_eq!(rules.entries()[0].slot, Slot::Skip);
        assert_eq!(rules.entries()[1].slot, Slot::Emit("word".to_string()));
    }

    #[test]
    fn rejects_duplicate_rule_names() {
        let error = Rules::from_rules(vec![Rule::regex("a", "a"), Rule::regex("a", "b")])
            .expect_err("should fail");
        assert!(matches!(error, RulesError::DuplicateName(name) if name == "a"));
    }

    #[test]
    fn rejects_bad_regex() {
        let error = Rules::from_rules(vec![Rule::regex("bad", "(unclosed")]).expect_err("should fail");
        assert!(matches!(error, RulesError::Regex { .. }));
    }

    #[test]
    fn rejects_empty_pattern() {
        let error = Rules::from_rules(vec![Rule::regex("empty", "")]).expect_err("should fail");
        assert!(matches!(error, RulesError::EmptyPattern(name) if name == "empty"));
    }

    #[test]
    fn literals_are_escaped_and_prioritised() {
        let extended = rules()
            .with_literals(&["{".to_string(), "}".to_string(), "<<".to_string()])
            .expect("the literals should be valid");

        let entry = extended
            .rules
            .iter()
            .find(|rule| rule.name == "<<")
            .expect("should add a rule for `<<`");
        assert_eq!(entry.pattern, Pattern::Literal("<<".to_string()));
        assert_eq!(entry.pattern.to_regex(), "<<");
        assert_eq!(entry.priority, LITERAL_PRIORITY);

        let brace = extended
            .rules
            .iter()
            .find(|rule| rule.name == "{")
            .expect("should add a rule for `{`");
        assert_eq!(brace.pattern.to_regex(), r"\{", "regex metacharacters must be escaped");
    }

    #[test]
    fn with_literals_is_idempotent() {
        let once = rules().with_literals(&["{".to_string()]).expect("should be valid");
        let twice = once.with_literals(&["{".to_string()]).expect("should be valid");
        assert_eq!(once.rules.len(), twice.rules.len());
    }
}
