//! 分析结果：一颗值树（默认语法树或动作产出），外加两种遍历方式。

use crate::core::value::{RuleNode, Value};

/// 一次完整分析的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// 起始符号对应的值：没有动作时是 [`Value::Rule`]，有动作时是动作返回值。
    pub root: Value,
}

impl Document {
    pub fn new(root: Value) -> Self {
        Self { root }
    }

    /// 根节点（当它仍是语法树节点时）。
    pub fn rule(&self) -> Option<&RuleNode> {
        self.root.as_rule()
    }

    /// 按前序遍历所有值（含语法树节点与列表元素）。
    ///
    /// 这是最省事的"看一眼"接口：不能剪枝、不区分节点类型、回调里拿到的引用也不能
    /// 带出闭包。需要这些能力时用访问者：[`Document::accept`] +
    /// [`Visitor`](crate::core::visit::Visitor)。
    pub fn walk<F: FnMut(&Value)>(&self, mut visit: F) {
        walk_value(&self.root, &mut visit);
    }

    /// 语法树里叶子 token 的数量。
    ///
    /// 内部走的就是访问者（[`TokenCollector`](crate::core::visit::TokenCollector)），
    /// 与 [`Document::tokens`] 口径一致。
    pub fn token_count(&self) -> usize {
        self.tokens().len()
    }
}

fn walk_value<F: FnMut(&Value)>(value: &Value, visit: &mut F) {
    visit(value);
    match value {
        Value::Rule(node) => {
            for child in &node.children {
                walk_value(child, visit);
            }
        }
        Value::List(items) => {
            for item in items {
                walk_value(item, visit);
            }
        }
        _ => {}
    }
}
