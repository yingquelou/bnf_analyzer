//! 文档装配：把仓库 `docs/` 下的 Markdown 引进 rustdoc。
//!
//! 分界按**读者**来，不按内容形式（两边都会有 Rust 代码）：
//!
//! | 读者 | rustdoc | 文件 |
//! |---|---|---|
//! | **使用**这个库的人（依赖、命令行、配置） | [`user`] | `docs/user/` |
//! | **开发**这个库的人（读源码、改实现、跑测试） | [`dev`] | `docs/dev/` |
//!
//! 用户文档只写 `pub` 面，读者不需要知道实现；开发者文档才谈模块、降级与测试。
//!
//! 这样做不只是为了少写一份：`cargo test --doc` 会把文档里的 Rust 示例当真测试跑，
//! API 改名、返回类型变化、行为调整都会先让文档变红，而不是让读者先踩坑。
//!
//! 用户文档四份都要读配置文件（`Config`、`--bnf`、`--config`），随 `json` feature 提供；
//! 开发者文档只讲 core，默认 feature 下就有。

/// 用户文档：给**使用**这个库的人。只写公开面。
pub mod user {
    /// 库 API：两个入口、动作、`Value`/`Document`、访问者、错误、边界。
    #[cfg(feature = "json")]
    pub mod api {
        #![doc = include_str!("../docs/user/api.md")]
    }

    /// 文法怎么写：JSON / Rust 两种写法对照、三种引用、组合子、`pattern` 编译、动作。
    #[cfg(feature = "json")]
    pub mod grammar {
        #![doc = include_str!("../docs/user/grammar.md")]
    }

    /// 配置参考：`rules` / `pattern` / `grammar` / `rhs` 的键表、优先级、schema、报错对照。
    #[cfg(feature = "json")]
    pub mod config {
        #![doc = include_str!("../docs/user/config.md")]
    }

    /// 命令行：构建、选项、三种输出、`span`、错误与退出码。
    #[cfg(feature = "json")]
    pub mod cli {
        #![doc = include_str!("../docs/user/cli.md")]
    }
}

/// 开发者文档：给**开发**这个 crate 的人。可以谈内部标识与文件路径。
pub mod dev {
    /// 架构与维护：分层、模块树、硬约束、文档装配、测试矩阵、改动清单。
    pub mod architecture {
        #![doc = include_str!("../docs/dev/architecture.md")]
    }

    /// 实现内部：文法怎么落地（`from_exprs`、糖的降级、合成符号、Earley、动作缓存）。
    pub mod internals {
        #![doc = include_str!("../docs/dev/internals.md")]
    }
}
