//! 事后遍历：用访问者模式读取分析结果与默认语法树。
//!
//! 解析产出的 [`Value`] 不一定是树：没挂动作时是 [`Value::Rule`] 节点，
//! 挂了动作则是动作折叠出来的任意结构（`List`、`Map`、标量…）。所以这里不做
//! "只认语法树"的遍历器，而是把**所有值**都交给访问者，由访问者自己决定关心哪些。
//!
//! 分工很明确：**递归由库负责，语义由你负责**。你只需要实现关心的回调，
//! 其余用默认实现——[`Visitor`] 没有任何必需方法，一个空 `struct` 就能用。
//!
//! ```
//! use bnf_analyzer::core::Document;
//! use bnf_analyzer::core::tokenizer::Span;
//! use bnf_analyzer::core::value::{RuleNode, Value};
//! use bnf_analyzer::core::visit::{Flow, Visitor};
//!
//! /// 统计每种产生式出现了多少次。
//! #[derive(Default)]
//! struct Census {
//!     counts: Vec<(String, usize)>,
//! }
//!
//! impl<'ast> Visitor<'ast> for Census {
//!     fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
//!         match self.counts.iter_mut().find(|(name, _)| *name == node.rule) {
//!             Some((_, count)) => *count += 1,
//!             None => self.counts.push((node.rule.clone(), 1)),
//!         }
//!         Flow::Continue
//!     }
//! }
//!
//! // 实际场景里它来自 `analyzer.analyze(source)`；这里手工搭一棵同形状的树。
//! let tree = Value::rule(
//!     "program",
//!     Span::new(0, 9, 1, 1),
//!     vec![Value::rule("procedure", Span::new(4, 9, 1, 5), vec![])],
//! );
//! let document = Document::new(tree);
//!
//! let mut census = Census::default();
//! document.accept(&mut census);
//!
//! let program = census.counts.iter().find(|(name, _)| name == "program");
//! assert_eq!(program.map(|(_, count)| *count), Some(1));
//! ```
//!
//! 三种 [`Flow`] 覆盖了遍历控制的全部分支：继续、剪枝（跳过当前值的子节点）、
//! 中止。`enter_rule` 与 `visit` 都可以返回它；`leave_rule` 没有返回值——
//! 它是"配对收尾"，即使遍历被中止也会照常调用。

use crate::core::tokenizer::Token;
use crate::core::value::{RuleNode, Value};
use crate::core::Document;

/// 访问者对当前位置的处置。
///
/// 由 [`Visitor`] 的回调返回；见模块文档的"三种 Flow"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// 照常继续：会继续下探当前值的子节点。
    Continue,
    /// 跳过当前值的子节点，继续处理同层的下一个值。
    ///
    /// 当前值**自身**的回调仍然会发生（节点会 `enter_rule` + `leave_rule`，
    /// 叶子会 `visit_token`），只是不再往里走。
    Skip,
    /// 立刻结束整趟遍历。
    ///
    /// 已经进入的节点仍会收到 `leave_rule`，因此 `enter_rule` / `leave_rule`
    /// 永远成对出现，可以用栈式的后序处理。
    Stop,
}

impl Flow {
    fn is_stop(self) -> bool {
        matches!(self, Flow::Stop)
    }
}

/// 只读访问者。
///
/// 生命周期参数 `'ast` 是**被遍历的那棵结果**的生命周期：回调拿到的
/// `&'ast Value` / `&'ast RuleNode` / `&'ast Token` 可以直接存进访问者里
/// （见 [`RuleCollector`]），不必在回调内就把结果抄成 `String`。
///
/// 所有方法都有默认实现，按需覆盖即可：
///
/// | 回调 | 时机 | 返回值 |
/// |---|---|---|
/// | `visit` | **每个**值，先于该值的具体回调 | 继续 / 剪枝 / 中止 |
/// | `enter_rule` | 进入 [`Value::Rule`]（`visit` 之后） | 继续 / 剪枝 / 中止 |
/// | `leave_rule` | 离开 [`Value::Rule`]（子节点都处理完之后） | 无 |
/// | `visit_token` | 遇到 [`Value::Token`] 叶子 | 无 |
pub trait Visitor<'ast> {
    /// 每遇到一个值就调用一次，先于 `enter_rule` / `visit_token`。
    ///
    /// 这里是"按值过滤"的地方：返回 [`Flow::Skip`] 就不再下探它的子节点。
    fn visit(&mut self, value: &'ast Value) -> Flow {
        let _ = value;
        Flow::Continue
    }

    /// 进入一个语法树节点。
    fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
        let _ = node;
        Flow::Continue
    }

    /// 离开一个语法树节点；此时它的子节点都已被访问过。
    fn leave_rule(&mut self, node: &'ast RuleNode) {
        let _ = node;
    }

    /// 一个未被打动作改写的叶子 token。
    fn visit_token(&mut self, token: &'ast Token) {
        let _ = token;
    }
}

impl Value {
    /// 用访问者遍历这个值，返回遍历的最终状态。
    ///
    /// 返回值是 [`Flow::Stop`] 表示访问者中途叫停了；否则是 [`Flow::Continue`]。
    ///
    /// ```
    /// use bnf_analyzer::core::tokenizer::Span;
    /// use bnf_analyzer::core::value::{RuleNode, Value};
    /// use bnf_analyzer::core::visit::{Flow, Visitor};
    ///
    /// struct StopAtProcedure {
    ///     seen: Vec<String>,
    /// }
    ///
    /// impl<'ast> Visitor<'ast> for StopAtProcedure {
    ///     fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
    ///         self.seen.push(node.rule.clone());
    ///         if node.rule == "procedure" {
    ///             Flow::Stop
    ///         } else {
    ///             Flow::Continue
    ///         }
    ///     }
    /// }
    ///
    /// let tree = Value::rule(
    ///     "program",
    ///     Span::new(0, 9, 1, 1),
    ///     vec![Value::rule("procedure", Span::new(4, 9, 1, 5), vec![])],
    /// );
    ///
    /// let mut visitor = StopAtProcedure { seen: Vec::new() };
    /// assert_eq!(tree.accept(&mut visitor), Flow::Stop);
    /// assert_eq!(visitor.seen.last().map(String::as_str), Some("procedure"));
    /// ```
    pub fn accept<'ast, V>(&'ast self, visitor: &mut V) -> Flow
    where
        V: Visitor<'ast> + ?Sized,
    {
        accept_value(self, visitor)
    }
}

impl Document {
    /// 用访问者遍历整份结果（从 `root` 开始）。
    ///
    /// ```
    /// use bnf_analyzer::core::Document;
    /// use bnf_analyzer::core::tokenizer::{Span, Token};
    /// use bnf_analyzer::core::value::Value;
    /// use bnf_analyzer::core::visit::{Flow, Visitor};
    ///
    /// #[derive(Default)]
    /// struct Texts(Vec<String>);
    ///
    /// impl<'ast> Visitor<'ast> for Texts {
    ///     fn visit_token(&mut self, token: &'ast Token) {
    ///         self.0.push(token.text.clone());
    ///     }
    /// }
    ///
    /// let tree = Value::rule(
    ///     "program",
    ///     Span::new(0, 9, 1, 1),
    ///     vec![Value::rule(
    ///         "procedure",
    ///         Span::new(4, 9, 1, 5),
    ///         vec![Value::Token(Token::new("name", "/f", Span::new(4, 6, 1, 5)))],
    ///     )],
    /// );
    /// let document = Document::new(tree);
    ///
    /// let mut texts = Texts::default();
    /// assert_eq!(document.accept(&mut texts), Flow::Continue);
    /// assert_eq!(texts.0, ["/f"]);
    /// ```
    pub fn accept<'ast, V>(&'ast self, visitor: &mut V) -> Flow
    where
        V: Visitor<'ast> + ?Sized,
    {
        self.root.accept(visitor)
    }

    /// 指定名字的全部语法树节点，按文档顺序（深度优先、先序）。
    ///
    /// 这是 `find_rule` 的"全部版"：`find_rule` 只给第一个，且要求根也是节点。
    ///
    /// ```
    /// use bnf_analyzer::core::Document;
    /// use bnf_analyzer::core::tokenizer::Span;
    /// use bnf_analyzer::core::value::Value;
    ///
    /// let tree = Value::rule(
    ///     "program",
    ///     Span::new(0, 9, 1, 1),
    ///     vec![
    ///         Value::rule("procedure", Span::new(0, 4, 1, 1), vec![]),
    ///         Value::rule("procedure", Span::new(5, 9, 1, 6), vec![]),
    ///     ],
    /// );
    /// assert_eq!(Document::new(tree).rules_named("procedure").len(), 2);
    /// ```
    pub fn rules_named(&self, name: &str) -> Vec<&RuleNode> {
        let mut collector = RuleCollector::new(name);
        self.accept(&mut collector);
        collector.into_nodes()
    }

    /// 全部叶子 token，按文档顺序。等价于 `tokenize` 之后的引用视图。
    pub fn tokens(&self) -> Vec<&Token> {
        let mut collector = TokenCollector::new();
        self.accept(&mut collector);
        collector.into_tokens()
    }
}

fn accept_value<'ast, V>(value: &'ast Value, visitor: &mut V) -> Flow
where
    V: Visitor<'ast> + ?Sized,
{
    // 先问"这个值整体要不要下探"。
    let mut descend = true;
    match visitor.visit(value) {
        Flow::Stop => return Flow::Stop,
        Flow::Skip => descend = false,
        Flow::Continue => {}
    }

    match value {
        Value::Rule(node) => {
            let mut flow = Flow::Continue;
            match visitor.enter_rule(node) {
                Flow::Stop => {
                    flow = Flow::Stop;
                    descend = false;
                }
                Flow::Skip => descend = false,
                Flow::Continue => {}
            }
            if descend {
                for child in &node.children {
                    if accept_value(child, visitor).is_stop() {
                        flow = Flow::Stop;
                        break;
                    }
                }
            }
            // 无论是否被跳过、是否中止，都收尾，保证与 enter_rule 配对。
            visitor.leave_rule(node);
            flow
        }
        Value::Token(token) => {
            // 叶子没有子节点，`Skip` 对它的唯一影响是"不再往里走"（本来也没有）。
            visitor.visit_token(token);
            Flow::Continue
        }
        Value::List(items) => {
            if descend {
                for item in items {
                    if accept_value(item, visitor).is_stop() {
                        return Flow::Stop;
                    }
                }
            }
            Flow::Continue
        }
        Value::Map(entries) => {
            if descend {
                for item in entries.values() {
                    if accept_value(item, visitor).is_stop() {
                        return Flow::Stop;
                    }
                }
            }
            Flow::Continue
        }
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Real(_) | Value::Text(_) => {
            Flow::Continue
        }
    }
}

/// 收集指定产生式名的全部节点。
///
/// 与 `Document::rules_named` 的区别是可以自己再叠加别的回调，
/// 或者挂在别的 [`Value`] 上（例如动作产出的子树）：
///
/// ```
/// use bnf_analyzer::core::Document;
/// use bnf_analyzer::core::tokenizer::Span;
/// use bnf_analyzer::core::value::Value;
/// use bnf_analyzer::core::visit::RuleCollector;
///
/// let tree = Value::rule(
///     "program",
///     Span::new(0, 4, 1, 1),
///     vec![Value::rule("procedure", Span::new(0, 4, 1, 1), vec![])],
/// );
/// let document = Document::new(tree);
///
/// let mut collector = RuleCollector::new("procedure");
/// document.accept(&mut collector);
/// assert_eq!(collector.nodes().len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct RuleCollector<'ast> {
    name: String,
    nodes: Vec<&'ast RuleNode>,
}

impl<'ast> RuleCollector<'ast> {
    /// 按左部名字建一个收集器。
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nodes: Vec::new(),
        }
    }

    /// 已收集节点（文档顺序）。
    pub fn nodes(&self) -> &[&'ast RuleNode] {
        &self.nodes
    }

    /// 取出收集结果。
    pub fn into_nodes(self) -> Vec<&'ast RuleNode> {
        self.nodes
    }

    /// 收集到的节点数量。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否一个都没收到。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl<'ast> Visitor<'ast> for RuleCollector<'ast> {
    fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
        if node.rule == self.name {
            self.nodes.push(node);
        }
        Flow::Continue
    }
}

/// 收集全部叶子 token。
#[derive(Debug, Clone, Default)]
pub struct TokenCollector<'ast> {
    tokens: Vec<&'ast Token>,
}

impl<'ast> TokenCollector<'ast> {
    /// 空收集器。
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    /// 已收集的 token。
    pub fn tokens(&self) -> &[&'ast Token] {
        &self.tokens
    }

    /// 取走收集结果。
    pub fn into_tokens(self) -> Vec<&'ast Token> {
        self.tokens
    }

    /// 收集到的 token 数量。
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// 是否一个都没收到。
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

impl<'ast> Visitor<'ast> for TokenCollector<'ast> {
    fn visit_token(&mut self, token: &'ast Token) {
        self.tokens.push(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tokenizer::Span;

    fn leaf(text: &str) -> Value {
        Value::Token(Token::new("word", text, Span::new(0, 0, 1, 1)))
    }

    /// 用缩进记录 enter/leave 的嵌套关系。
    #[derive(Default)]
    struct Structure {
        depth: usize,
        max_depth: usize,
        trace: Vec<String>,
        unbalanced: bool,
    }

    impl<'ast> Visitor<'ast> for Structure {
        fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
            self.depth += 1;
            self.max_depth = self.max_depth.max(self.depth);
            self.trace.push(format!("{}>{}", " ".repeat(self.depth), node.rule));
            Flow::Continue
        }

        fn leave_rule(&mut self, node: &'ast RuleNode) {
            self.trace.push(format!("{}<{}", " ".repeat(self.depth), node.rule));
            if self.depth == 0 {
                self.unbalanced = true;
            }
            self.depth = self.depth.saturating_sub(1);
        }
    }

    #[test]
    fn enter_and_leave_are_paired_and_nested() {
        let tree = Value::rule(
            "outer",
            Span::new(0, 4, 1, 1),
            vec![
                Value::rule("left", Span::new(0, 1, 1, 1), vec![leaf("a")]),
                Value::rule("right", Span::new(2, 3, 1, 3), vec![]),
            ],
        );

        let mut visitor = Structure::default();
        assert_eq!(tree.accept(&mut visitor), Flow::Continue);
        assert_eq!(visitor.depth, 0, "enter and leave must balance");
        assert!(!visitor.unbalanced);
        assert_eq!(visitor.max_depth, 2);
        assert_eq!(
            visitor.trace,
            [" >outer", "  >left", "  <left", "  >right", "  <right", " <outer"]
        );
    }

    #[test]
    fn skip_prunes_children_but_still_visits_the_node() {
        #[derive(Default)]
        struct SkipInner {
            rules: Vec<String>,
            leaves: Vec<String>,
        }

        impl<'ast> Visitor<'ast> for SkipInner {
            fn visit(&mut self, value: &'ast Value) -> Flow {
                if value.as_rule().map(|node| node.rule.as_str()) == Some("inner") {
                    Flow::Skip
                } else {
                    Flow::Continue
                }
            }

            fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
                self.rules.push(node.rule.clone());
                Flow::Continue
            }

            fn visit_token(&mut self, token: &'ast Token) {
                self.leaves.push(token.text.clone());
            }
        }

        let tree = Value::rule(
            "outer",
            Span::new(0, 4, 1, 1),
            vec![
                Value::rule("inner", Span::new(1, 3, 1, 2), vec![leaf("hidden")]),
                leaf("shown"),
            ],
        );

        let mut visitor = SkipInner::default();
        assert_eq!(tree.accept(&mut visitor), Flow::Continue);
        assert_eq!(visitor.rules, ["outer", "inner"], "a pruned node is still visible itself");
        assert_eq!(visitor.leaves, ["shown"], "tokens inside inner must not appear");
    }

    #[test]
    fn stop_aborts_but_leaves_still_pair() {
        #[derive(Default)]
        struct StopAtB {
            events: Vec<String>,
        }

        impl<'ast> Visitor<'ast> for StopAtB {
            fn enter_rule(&mut self, node: &'ast RuleNode) -> Flow {
                self.events.push(format!(">{}", node.rule));
                if node.rule == "b" {
                    Flow::Stop
                } else {
                    Flow::Continue
                }
            }

            fn leave_rule(&mut self, node: &'ast RuleNode) {
                self.events.push(format!("<{}", node.rule));
            }
        }

        let tree = Value::rule(
            "root",
            Span::new(0, 4, 1, 1),
            vec![
                Value::rule("a", Span::new(0, 1, 1, 1), vec![]),
                Value::rule("b", Span::new(1, 2, 1, 2), vec![]),
                Value::rule("c", Span::new(2, 3, 1, 3), vec![]),
            ],
        );

        let mut visitor = StopAtB::default();
        assert_eq!(tree.accept(&mut visitor), Flow::Stop);
        assert_eq!(
            visitor.events,
            [">root", ">a", "<a", ">b", "<b", "<root"],
            "c is no longer visited, but b and root must still be left"
        );
    }

    #[test]
    fn visit_sees_lists_maps_and_scalars() {
        #[derive(Default)]
        struct Shapes {
            kinds: Vec<&'static str>,
        }

        impl<'ast> Visitor<'ast> for Shapes {
            fn visit(&mut self, value: &'ast Value) -> Flow {
                let kind = match value {
                    Value::Null => "null",
                    Value::Bool(_) => "bool",
                    Value::Int(_) => "int",
                    Value::Real(_) => "real",
                    Value::Text(_) => "text",
                    Value::List(_) => "list",
                    Value::Map(_) => "map",
                    Value::Token(_) => "token",
                    Value::Rule(_) => "rule",
                };
                self.kinds.push(kind);
                Flow::Continue
            }
        }

        let mut map = std::collections::BTreeMap::new();
        map.insert("k".to_string(), Value::Int(1));
        let value = Value::List(vec![Value::text("a"), Value::Map(map)]);

        let mut visitor = Shapes::default();
        assert_eq!(value.accept(&mut visitor), Flow::Continue);
        assert_eq!(visitor.kinds, ["list", "text", "map", "int"]);
    }

    #[test]
    fn default_trait_needs_no_methods() {
        struct Silent;
        impl<'ast> Visitor<'ast> for Silent {}

        let tree = Value::rule("r", Span::new(0, 1, 1, 1), vec![leaf("x")]);
        let mut visitor = Silent;
        assert_eq!(tree.accept(&mut visitor), Flow::Continue);
    }

    #[test]
    fn visit_runs_before_enter_rule() {
        #[derive(Default)]
        struct Order(Vec<&'static str>);

        impl<'ast> Visitor<'ast> for Order {
            fn visit(&mut self, value: &'ast Value) -> Flow {
                if value.as_rule().is_some() {
                    self.0.push("visit");
                }
                Flow::Continue
            }

            fn enter_rule(&mut self, _node: &'ast RuleNode) -> Flow {
                self.0.push("enter");
                Flow::Continue
            }
        }

        let tree = Value::rule("r", Span::new(0, 1, 1, 1), vec![]);
        let mut visitor = Order::default();
        tree.accept(&mut visitor);
        assert_eq!(visitor.0, ["visit", "enter"]);
    }

    #[test]
    fn collectors_follow_document_order() {
        let tree = Value::rule(
            "root",
            Span::new(0, 4, 1, 1),
            vec![
                Value::rule("target", Span::new(0, 1, 1, 1), vec![leaf("a")]),
                Value::rule("other", Span::new(1, 2, 1, 2), vec![leaf("b")]),
                Value::rule("target", Span::new(2, 3, 1, 3), vec![leaf("c")]),
            ],
        );

        let mut rules = RuleCollector::new("target");
        tree.accept(&mut rules);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules.nodes()[0].span.start, 0);
        assert_eq!(rules.nodes()[1].span.start, 2);

        let mut tokens = TokenCollector::new();
        tree.accept(&mut tokens);
        assert_eq!(
            tokens
                .tokens()
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }
}
