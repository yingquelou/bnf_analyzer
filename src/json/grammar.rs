//! JSON 文法：三块积木（词法规则、产生式、动作）全部写在 Rust 里。
//!
//! 这份文件是本库的**活示例**：它没有为 JSON 开任何后门，用的就是
//! `Rules` + `Grammar::builder()` + `Semantics` 这三个公开接口。
//!
//! # 文法（`analyzer().grammar().to_bnf()` 的输出）
//!
//! ```text
//! json ::= value ;
//! value ::= object | array | @string | @number | "true" | "false" | "null" ;
//! object ::= "{" "}" | "{" member ( "," member )* "}" ;
//! member ::= @string ":" value ;
//! array ::= "[" "]" | "[" value ( "," value )* "]" ;
//! ```
//!
//! （同名多条产生式在 BNF 视图里合并成一行 `|`，源码里是分开声明的。）
//!
//! 几点说明：
//!
//! - **只有三条词法规则**：空白、字符串、数字。`true`/`false`/`null` 与
//!   `{ } [ ] , :` 六个标点都是字面量终结符，由 [`Grammar::literals`] 在装配时
//!   自动派生成词法规则，不必在 `rules` 里重复声明。
//! - **重复写在容器自己的产生式里**（`"{" member ( "," member )* "}"`），不另立一条
//!   `members` 规则。单独成规则也能解析，但那条规则在每个前缀位置都会完成一次，
//!   容器每多一个成员就要重建一次已累积的列表，规模一上去就是平方；写在容器里，
//!   容器只在 `}` / `]` 处完成一次。`*` 展平出来的参数里夹着逗号，动作按下标隔一个取。
//! - **空对象/空数组单独一条产生式**：`"{" "}"` 与非空形态的形状不同，
//!   动作只要看参数个数就能分辨，不需要额外哨兵。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::core::Analyzer;
use crate::core::grammar::{Expr, Grammar};
use crate::core::pattern::Pattern;
use crate::core::rules::{Rule, Rules};
use crate::core::semantics::Semantics;
use crate::core::value::Value;

/// JSON 的词法规则：空白跳过，字符串与数字各一条。
///
/// 字符串用结构化 [`Pattern`] 拼出来（引号 + 若干个"普通字符或转义" + 引号），
/// 数字用一整段正则——两种写法都在配置里合法，这里各示范一次。
pub fn rules() -> Rules {
    // `[^"\\]`：除了引号与反斜杠以外的任意字符；
    // `\\.`：反斜杠加任意字符（具体是不是合法转义交给 token 动作判断）。
    let plain = Pattern::NotChars("\"\\\\".to_string());
    let escaped = Pattern::Seq(vec![
        Pattern::literal("\\"),
        Pattern::regex("."),
    ]);
    let string = Pattern::Seq(vec![
        Pattern::literal("\""),
        Pattern::star(Pattern::Alt(vec![plain, escaped])),
        Pattern::literal("\""),
    ]);

    Rules::from_rules(vec![
        Rule::skipping_regex("space", "[ \t\r\n]+"),
        Rule::new("string", string),
        Rule::regex(
            "number",
            r"-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?",
        ),
    ])
    .expect("the JSON lexical rules must be consistent")
}

/// JSON 的产生式：`value` 七选一，对象与数组各两条（空 / 非空）。
pub fn grammar() -> Grammar {
    Grammar::builder()
        .start("json")
        .production("json", Expr::rule("value"))
        .production(
            "value",
            Expr::alt([
                Expr::rule("object"),
                Expr::rule("array"),
                Expr::token("string"),
                Expr::token("number"),
                Expr::literal("true"),
                Expr::literal("false"),
                Expr::literal("null"),
            ]),
        )
        // 对象：空 与 非空
        .production(
            "object",
            Expr::seq([Expr::literal("{"), Expr::literal("}")]),
        )
        // 重复写在容器自己的产生式里：`"{" member ("," member)* "}"`。
        // 单独开一条 `members` 规则也能解析，但那条规则会在**每个前缀位置**完成一次
        // （`member` 一结束它就成立），容器每多一个成员就要重建一次已累积的列表，
        // 代价是 O(n²)。写在容器里，容器只在 `}` 处完成一次。
        .production(
            "object",
            Expr::seq([
                Expr::literal("{"),
                Expr::rule("member"),
                Expr::star(Expr::seq([Expr::literal(","), Expr::rule("member")])),
                Expr::literal("}"),
            ]),
        )
        .production(
            "member",
            Expr::seq([
                Expr::token("string"),
                Expr::literal(":"),
                Expr::rule("value"),
            ]),
        )
        // 数组：空 与 非空，重复同样写在容器产生式里
        .production(
            "array",
            Expr::seq([Expr::literal("["), Expr::literal("]")]),
        )
        .production(
            "array",
            Expr::seq([
                Expr::literal("["),
                Expr::rule("value"),
                Expr::star(Expr::seq([Expr::literal(","), Expr::rule("value")])),
                Expr::literal("]"),
            ]),
        )
        .build()
        .expect("the JSON grammar must be consistent")
}

/// JSON 的动作表：token 动作负责"文本 → 值"，语义动作负责"组装容器"。
///
/// 只有 `value`、`object`、`array` 这些外层结构需要动作；没有动作的产生式
/// 会自动产出默认语法树节点，所以这里**每个**需要值的产生式都挂了动作。
pub fn semantics() -> Semantics {
    Semantics::new()
        // ---- token 动作：把原始文本变成值 ----
        .token("string", |token| unescape(&token.text))
        .token("number", |token| number(&token.text))
        .token("true", |_| Ok(Value::Bool(true)))
        .token("false", |_| Ok(Value::Bool(false)))
        .token("null", |_| Ok(Value::Null))
        // ---- 语义动作：透传与组装 ----
        .rule("json", pass_through)
        .rule("value", pass_through)
        .rule("object", object)
        .rule("member", member)
        .rule("array", array)
}

/// 装好的 JSON 分析器：`analyze` 一次，得到一个 [`Value`] 树。
///
/// 每次调用都会重新编译词法与文法；需要"一份装配、反复使用"时自己保存返回值，
/// 或用 [`super::parse`]（它内部用的是单例）。
pub fn analyzer() -> Analyzer {
    Analyzer::new(rules(), grammar(), semantics()).expect("the JSON grammar must be consistent")
}

/// `json::parse` 用的单例。
///
/// [`Analyzer`] 是 `Send + Sync`（动作表用 `Arc`），所以能放进 `OnceLock`。
/// 没有这一步，每解析一段 JSON 都要重新编译整份词法与文法——小输入上这笔开销
/// 比解析本身还大。
pub(crate) fn shared() -> &'static Analyzer {
    static ANALYZER: OnceLock<Analyzer> = OnceLock::new();
    ANALYZER.get_or_init(analyzer)
}

// ---------------------------------------------------------------------------
// 语义动作
// ---------------------------------------------------------------------------

/// `json ::= value` 与 `value ::= …`：唯一的孩子就是结果。
fn pass_through(values: &[Value]) -> Result<Value, String> {
    match values {
        [value] => Ok(value.clone()),
        _ => Err(format!("internal error: forwarding production received {} values", values.len())),
    }
}

/// `object`：空对象得到空字典，非空对象把成员对塞进字典。
///
/// 非空形态的参数是 `["{", pair, ",", pair, …, "}"]`——重复部分被摊平进来，
/// 所以按下标隔一个取一个。
fn object(values: &[Value]) -> Result<Value, String> {
    match values {
        // "{" "}"
        [_, _] => Ok(Value::map()),
        // "{" member ("," member)* "}"
        [_, pairs @ .., _] => {
            let mut map = BTreeMap::new();
            for pair in pairs.iter().step_by(2) {
                let [Value::Text(key), value] = pair
                    .as_list()
                    .ok_or_else(|| "internal error: a member is not a [key, value] pair".to_string())?
                else {
                    return Err("internal error: a member is not a [key, value] pair".to_string());
                };
                // 重复键按 JSON 的惯例处理：后者覆盖前者。
                map.insert(key.clone(), value.clone());
            }
            Ok(Value::Map(map))
        }
        _ => Err("internal error: unexpected shape for the right-hand side of `object`".to_string()),
    }
}

/// `member ::= @string ":" value` → `[key, value]`。
fn member(values: &[Value]) -> Result<Value, String> {
    match values {
        [Value::Text(key), _, value] => Ok(Value::List(vec![Value::text(key), value.clone()])),
        _ => Err("internal error: unexpected shape for the right-hand side of `member`".to_string()),
    }
}

/// `array`：空数组得到空列表，非空数组收下元素。
fn array(values: &[Value]) -> Result<Value, String> {
    match values {
        // "[" "]"
        [_, _] => Ok(Value::List(Vec::new())),
        // "[" value ("," value)* "]"
        [_, items @ .., _] => Ok(Value::List(items.iter().step_by(2).cloned().collect())),
        _ => Err("internal error: unexpected shape for the right-hand side of `array`".to_string()),
    }
}

// ---------------------------------------------------------------------------
// token 动作
// ---------------------------------------------------------------------------

/// 去掉引号并处理转义，包括 `\uXXXX` 与代理对。
fn unescape(text: &str) -> Result<Value, String> {
    let inner = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| format!("string token is missing its closing quote: {text:?}"))?;

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let mut code = take_hex(&mut chars)?;
                // 高位代理必须跟一个低位代理，合成一个增补平面码点。
                if (0xD800..=0xDBFF).contains(&code) {
                    let rest: String = chars.clone().take(2).collect();
                    if rest != "\\u" {
                        return Err(format!(
                            "\\u{code:04X} is a high surrogate, it must be followed by a \\uDC00–\\uDFFF"
                        ));
                    }
                    chars.next();
                    chars.next();
                    let low = take_hex(&mut chars)?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(format!("low surrogate out of the \\uDC00–\\uDFFF range: \\u{low:04X}"));
                    }
                    code = 0x1_0000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                }
                let ch = char::from_u32(code)
                    .ok_or_else(|| format!("\\u{code:04X} is not a valid Unicode scalar value"))?;
                out.push(ch);
            }
            Some(other) => return Err(format!("unknown escape sequence: \\{other}")),
            None => return Err("string ends with a lone backslash".to_string()),
        }
    }
    Ok(Value::Text(out))
}

/// 读 4 位十六进制。
fn take_hex(chars: &mut std::str::Chars<'_>) -> Result<u32, String> {
    let mut code = 0;
    for _ in 0..4 {
        let digit = chars
            .next()
            .ok_or_else(|| "`\\u` needs four hexadecimal digits".to_string())?;
        code = code * 16
            + digit
                .to_digit(16)
                .ok_or_else(|| format!("`\\u` is not followed by a hex digit: {digit:?}"))?;
    }
    Ok(code)
}

/// 数字：带小数点或指数就当小数，否则整数；超出 `i64` 的整数退回小数。
fn number(text: &str) -> Result<Value, String> {
    let as_real = || {
        text.parse::<f64>()
            .map(Value::Real)
            .map_err(|error| format!("not a valid JSON number: {text} ({error})"))
    };
    if text.contains(['.', 'e', 'E']) {
        return as_real();
    }
    match text.parse::<i64>() {
        Ok(value) => Ok(Value::Int(value)),
        Err(_) => as_real(),
    }
}
