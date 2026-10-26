//! [`Value`] → JSON 文本。
//!
//! 输出形状与 `docs/user/cli.md` 一致：语法树节点写成 `{ "kind": "rule", … }`，
//! token 写成 `{ "kind": 类别, "text": 原文, "span": … }`，三种容器（字典、数组、
//! span）之外没有别的特殊结构。
//!
//! 这里同时补上 `core` 里无法实现的两件事：`Document::to_json*` 与 `Pattern` 的
//! JSON 形态 `Display`——它们都需要"写 JSON"，而这是 `json` 层的职责。

use std::collections::BTreeMap;
use std::fmt;

use crate::core::pattern::Pattern;
use crate::core::tokenizer::{Span, Token};
use crate::core::value::{RuleNode, Value};
use crate::core::Document;

/// 紧凑 JSON：没有多余空白。
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0);
    out
}

/// 缩进两格的 JSON。
pub fn to_string_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(2), 0);
    out
}

impl Document {
    /// 整棵结果树写成紧凑 JSON。
    pub fn to_json(&self) -> String {
        to_string(&self.root)
    }

    /// 整棵结果树写成缩进两格的 JSON。
    pub fn to_json_pretty(&self) -> String {
        to_string_pretty(&self.root)
    }
}

impl fmt::Display for Pattern {
    /// 渲染成对象 JSON：配置里只有这一种形态，输出也保持同一形态。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_string(&self.to_value()))
    }
}

// ---------------------------------------------------------------------------
// 递归下降地写
// ---------------------------------------------------------------------------

fn write_value(out: &mut String, value: &Value, indent: Option<usize>, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Int(number) => out.push_str(&number.to_string()),
        Value::Real(number) => write_real(out, *number),
        Value::Text(text) => write_text(out, text),
        Value::List(items) => write_list(out, items, indent, level),
        Value::Map(map) => write_map(out, map, indent, level),
        Value::Token(token) => write_token(out, token, indent, level),
        Value::Rule(node) => write_rule(out, node, indent, level),
    }
}

fn write_list(out: &mut String, items: &[Value], indent: Option<usize>, level: usize) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        newline(out, indent, level + 1);
        write_value(out, item, indent, level + 1);
    }
    newline(out, indent, level);
    out.push(']');
}

fn write_map(
    out: &mut String,
    map: &BTreeMap<String, Value>,
    indent: Option<usize>,
    level: usize,
) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push('{');
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        newline(out, indent, level + 1);
        write_text(out, key);
        out.push(':');
        space(out, indent);
        write_value(out, value, indent, level + 1);
    }
    newline(out, indent, level);
    out.push('}');
}

fn write_span(out: &mut String, span: &Span, indent: Option<usize>, level: usize) {
    out.push('{');
    let mut first = true;
    for (name, number) in [
        ("start", span.start),
        ("end", span.end),
        ("line", span.line),
        ("column", span.column),
    ] {
        begin_field(out, &mut first, indent, level + 1);
        write_text(out, name);
        out.push(':');
        space(out, indent);
        out.push_str(&number.to_string());
    }
    newline(out, indent, level);
    out.push('}');
}

fn write_token(out: &mut String, token: &Token, indent: Option<usize>, level: usize) {
    out.push('{');
    let mut first = true;
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"kind\":");
    space(out, indent);
    write_text(out, &token.kind);
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"text\":");
    space(out, indent);
    write_text(out, &token.text);
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"span\":");
    space(out, indent);
    write_span(out, &token.span, indent, level + 1);
    newline(out, indent, level);
    out.push('}');
}

fn write_rule(out: &mut String, node: &RuleNode, indent: Option<usize>, level: usize) {
    out.push('{');
    let mut first = true;
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"kind\":");
    space(out, indent);
    write_text(out, "rule");
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"rule\":");
    space(out, indent);
    write_text(out, &node.rule);
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"span\":");
    space(out, indent);
    write_span(out, &node.span, indent, level + 1);
    begin_field(out, &mut first, indent, level + 1);
    out.push_str("\"children\":");
    space(out, indent);
    write_list(out, &node.children, indent, level + 1);
    newline(out, indent, level);
    out.push('}');
}

// ---------------------------------------------------------------------------
// 标量与空白
// ---------------------------------------------------------------------------

fn write_real(out: &mut String, number: f64) {
    if !number.is_finite() {
        // JSON 没有 NaN / Infinity，退化成 null。
        out.push_str("null");
        return;
    }
    // `{:?}` 是"最短可回读"的写法：1.0、0.1、1e300 都能被原样读回来。
    out.push_str(&format!("{number:?}"));
}

fn write_text(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// 换行 + 缩进；紧凑模式下什么也不做。
fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..width * level {
            out.push(' ');
        }
    }
}

/// `key:` 后面的空格，只在缩进模式下出现。
fn space(out: &mut String, indent: Option<usize>) {
    if indent.is_some() {
        out.push(' ');
    }
}

/// 对象里每个字段前的逗号与缩进。
fn begin_field(out: &mut String, first: &mut bool, indent: Option<usize>, level: usize) {
    if *first {
        *first = false;
    } else {
        out.push(',');
    }
    newline(out, indent, level);
}
