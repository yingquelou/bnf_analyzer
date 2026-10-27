//! 语义值：动作（action）的输入输出，也是默认语法树的节点类型。
//!
//! 动作是动态的：token 动作把 token 变成 [`Value`]，语义动作把子值折叠成一个 [`Value`]。
//! 没有动作时，解析器自动生成 [`Value::Rule`] / [`Value::Token`]，即一棵带位置的语法树，
//! 这样"JSON 加载文法（无动作）"和"Rust 内嵌文法（带动作）"跑的是同一条流水线。

use std::collections::BTreeMap;
use std::fmt;

use crate::core::tokenizer::{Span, Token};

/// 一个语义值。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 空值。
    Null,
    /// 布尔值。
    Bool(bool),
    /// 整数。
    Int(i64),
    /// 浮点数。
    Real(f64),
    /// 字符串。
    Text(String),
    /// 列表。
    List(Vec<Value>),
    /// 字典；键有序，比较与输出都稳定。
    Map(BTreeMap<String, Value>),
    /// 叶子：原始 token（未被 token 动作改写时）。
    Token(Token),
    /// 默认语法树节点：一个产生式及其子值（该产生式没有语义动作时）。
    Rule(RuleNode),
}

/// 默认语法树节点。
#[derive(Debug, Clone, PartialEq)]
pub struct RuleNode {
    /// 产生式左部。
    pub rule: String,
    /// 覆盖的源码区间（与 `children` 的首尾一致）。
    pub span: Span,
    /// 右部各符号对应的值；`star` / `plus` / `opt` 写法的重复层已被摊平，这里是平的。
    pub children: Vec<Value>,
}

impl Value {
    /// 由字符串构造。
    pub fn text(value: impl Into<String>) -> Self {
        Value::Text(value.into())
    }

    /// 由整数构造。
    pub fn int(value: i64) -> Self {
        Value::Int(value)
    }

    /// 由浮点数构造。
    pub fn real(value: f64) -> Self {
        Value::Real(value)
    }

    /// 由布尔值构造。
    pub fn boolean(value: bool) -> Self {
        Value::Bool(value)
    }

    /// 由列表构造。
    pub fn list(values: Vec<Value>) -> Self {
        Value::List(values)
    }

    /// 一个空字典值。
    pub fn map() -> Self {
        Value::Map(BTreeMap::new())
    }

    /// 由原始 token 构造。
    pub fn token(token: Token) -> Self {
        Value::Token(token)
    }

    /// 由产生式名、区间与子值构造默认语法树节点。
    pub fn rule(rule: impl Into<String>, span: Span, children: Vec<Value>) -> Self {
        Value::Rule(RuleNode {
            rule: rule.into(),
            span,
            children,
        })
    }

    /// 是否为空值。
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// 取出布尔值。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// 取出整数；浮点数不会被截断成整数。
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// 取出浮点数；整数会提升成 `f64`。
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(value) => Some(*value),
            Value::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    /// 取出字符串。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(value) => Some(value),
            _ => None,
        }
    }

    /// 取出列表。
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(values) => Some(values),
            _ => None,
        }
    }

    /// 取出字典。
    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(values) => Some(values),
            _ => None,
        }
    }

    /// 取出原始 token。
    pub fn as_token(&self) -> Option<&Token> {
        match self {
            Value::Token(token) => Some(token),
            _ => None,
        }
    }

    /// 取出默认语法树节点。
    pub fn as_rule(&self) -> Option<&RuleNode> {
        match self {
            Value::Rule(node) => Some(node),
            _ => None,
        }
    }

    /// 该值对应的产生式名（叶子为 token 的 kind）。
    pub fn rule_name(&self) -> Option<&str> {
        match self {
            Value::Rule(node) => Some(&node.rule),
            Value::Token(token) => Some(&token.kind),
            _ => None,
        }
    }

    /// 变体的名字，用于"expected X, found Y"这类提示。
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "integer",
            Value::Real(_) => "real",
            Value::Text(_) => "string",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Token(_) => "token",
            Value::Rule(_) => "rule",
        }
    }

    /// 子值：语法树节点或列表的内容，其余为空。
    pub fn children(&self) -> &[Value] {
        match self {
            Value::Rule(node) => &node.children,
            Value::List(values) => values,
            Value::Token(_) => &[],
            _ => &[],
        }
    }

    /// 源码区间（只有 token 与语法树节点有）。
    pub fn span(&self) -> Option<Span> {
        match self {
            Value::Token(token) => Some(token.span),
            Value::Rule(node) => Some(node.span),
            _ => None,
        }
    }

    /// 在语法树里找第一个指定名字的节点（深度优先）。
    pub fn find_rule(&self, rule: &str) -> Option<&RuleNode> {
        let node = self.as_rule()?;
        if node.rule == rule {
            return Some(node);
        }
        node.children.iter().find_map(|child| child.find_rule(rule))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Int(value) => write!(f, "{value}"),
            Value::Real(value) => write!(f, "{value}"),
            Value::Text(value) => write!(f, "{value:?}"),
            Value::List(values) => {
                f.write_str("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{value}")?;
                }
                f.write_str("]")
            }
            Value::Map(values) => {
                f.write_str("{")?;
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                f.write_str("}")
            }
            Value::Token(token) => write!(f, "{}", token.text),
            Value::Rule(node) => write!(f, "{}[{}]", node.rule, node.children.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witnesses_typed_accessors() {
        let mut map = BTreeMap::new();
        map.insert("k".to_string(), Value::Int(1));
        let value = Value::Map(map);
        assert_eq!(value.as_map().expect("should be a map").len(), 1);
        assert!(value.as_list().is_none());
    }

    #[test]
    fn finds_nested_rule() {
        let inner = Value::rule("inner", Span::new(0, 1, 1, 1), vec![Value::Int(1)]);
        let outer = Value::rule("outer", Span::new(0, 1, 1, 1), vec![inner]);
        assert!(outer.find_rule("inner").is_some());
        assert!(outer.find_rule("missing").is_none());
    }
}
