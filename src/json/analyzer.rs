//! `Config` → `Analyzer`：配置驱动的装配入口。
//!
//! 与 core 的差别只有动作：配置里不能写动作，所以这条路径产出的一定是带位置的
//! 默认语法树（[`Value::Rule`](crate::core::value::Value::Rule) /
//! [`Value::Token`](crate::core::value::Value::Token)）。
//! 需要 token 动作或语义动作时，用 [`Analyzer::new`]。
//!
//! 这里**不提供**"内置分析器"这种便捷入口：内置配置是
//! [`Config::builtin`](crate::json::Config::builtin) 的事，拿到 `Config` 之后照常走
//! [`Analyzer::from_config`]。少一个入口，就少一处"到底用的是哪份配置"的歧义。

use crate::core::rules::Rules;
use crate::core::semantics::Semantics;
use crate::core::{Analyzer, SetupError};

use super::config::Config;

impl Analyzer {
    /// 从配置装配，不带任何动作（配置驱动与可执行 crate 用这个入口）。
    ///
    /// 两步自检：先把 `rules` 编译成词法规则（`RulesError`），再把 `grammar` 建成 CFG
    /// 并与词法类别对齐（`GrammarError`）；任一步失败都返回 [`SetupError`]。
    pub fn from_config(config: Config) -> Result<Self, SetupError> {
        let rules = Rules::from_rules(config.rules)?;
        let grammar = config.grammar.parse_grammar()?;
        Self::new(rules, grammar, Semantics::new())
    }
}
