//! 动作表：token 动作与语义动作。
//!
//! 两种动作都是可选的：
//!
//! - **token 动作**在终结符匹配成功后运行，把 [`Token`] 变成 [`Value`]
//!   （例如把数字文本解析成整数、把字符串反转义）；
//! - **语义动作**在产生式归约时运行，把右部各符号的值折叠成一个 [`Value`]
//!   （例如做常量折叠、构造 AST、组装字典）。
//!
//! 没有动作时，解析器自动产出默认语法树（[`Value::Token`] / [`Value::Rule`]）。
//! 动作以"规则名 / 产生式名 / 字面量文本"为键，用 `Arc` 共享（要求 `Send + Sync`），
//! 可随 [`Analyzer`](crate::core::Analyzer) 复用，也可以跨线程共享同一份装配结果。
//!
//! ```
//! use bnf_analyzer::core::semantics::Semantics;
//! use bnf_analyzer::core::value::Value;
//!
//! let semantics = Semantics::new()
//!     .token("number", |token| {
//!         token
//!             .text
//!             .parse::<i64>()
//!             .map(Value::Int)
//!             .map_err(|error| format!("不是整数：{error}"))
//!     })
//!     .rule("expr", |values| match values {
//!         [Value::Int(value)] => Ok(Value::Int(*value)),
//!         _ => Err("无法求值".to_string()),
//!     });
//! assert_eq!(semantics.rule_names().count(), 1);
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::core::tokenizer::Token;
use crate::core::value::Value;

/// token 动作：把匹配到的 token 折叠成一个值。
pub type TokenAction = Arc<dyn Fn(&Token) -> Result<Value, String> + Send + Sync>;

/// 语义动作：把产生式右部各符号的值折叠成一个值。
pub type RuleAction = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

/// 动作集合。
#[derive(Clone, Default)]
pub struct Semantics {
    tokens: HashMap<String, TokenAction>,
    rules: HashMap<String, RuleAction>,
}

impl Semantics {
    /// 空动作表：全部使用默认语法树行为。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册 token 动作。键可以是词法类别名（`number`），也可以是字面量文本（`"{"`）。
    ///
    /// 动作要 `Send + Sync`——这样 [`Analyzer`](crate::core::Analyzer) 才能跨线程共享、
    /// 放进 `static`/`OnceLock` 里复用一份装配结果。要往动作里带可变状态，用
    /// `Arc<AtomicUsize>`、`Arc<Mutex<…>>` 这类线程安全容器。
    pub fn token<F>(mut self, name: impl Into<String>, action: F) -> Self
    where
        F: Fn(&Token) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.tokens.insert(name.into(), Arc::new(action));
        self
    }

    /// 注册语义动作，键为产生式左部。同样要求 `Send + Sync`。
    pub fn rule<F>(mut self, name: impl Into<String>, action: F) -> Self
    where
        F: Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.rules.insert(name.into(), Arc::new(action));
        self
    }

    /// 按 token 类别或字面量原文查动作。
    pub fn token_action(&self, name: &str) -> Option<&TokenAction> {
        self.tokens.get(name)
    }

    /// 按产生式左部查动作。
    pub fn rule_action(&self, name: &str) -> Option<&RuleAction> {
        self.rules.get(name)
    }

    /// 没有任何动作时，解析结果就是纯语法树。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.rules.is_empty()
    }

    /// 已注册的 token 动作名。
    pub fn token_names(&self) -> impl Iterator<Item = &String> {
        self.tokens.keys()
    }

    /// 已注册的语义动作名。
    pub fn rule_names(&self) -> impl Iterator<Item = &String> {
        self.rules.keys()
    }
}

impl fmt::Debug for Semantics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tokens: Vec<&str> = self.tokens.keys().map(String::as_str).collect();
        let mut rules: Vec<&str> = self.rules.keys().map(String::as_str).collect();
        tokens.sort_unstable();
        rules.sort_unstable();
        f.debug_struct("Semantics")
            .field("tokens", &tokens)
            .field("rules", &rules)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::Span;

    fn token(text: &str) -> Token {
        Token::new("number", text, Span::new(0, text.len(), 1, 1))
    }

    #[test]
    fn runs_registered_actions() {
        let semantics = Semantics::new().token("number", |token| {
            token
                .text
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|error| error.to_string())
        });
        let action = semantics.token_action("number").expect("the token action should be reached");
        assert_eq!(action(&token("42")).expect("should succeed"), Value::Int(42));
        assert!(semantics.token_action("name").is_none());
        assert!(!semantics.is_empty());
    }

    #[test]
    fn defaults_to_empty_table() {
        let semantics = Semantics::new();
        assert!(semantics.is_empty());
        assert_eq!(format!("{semantics:?}"), "Semantics { tokens: [], rules: [] }");
    }
}
