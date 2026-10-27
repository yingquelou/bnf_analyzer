// 本页也是「中文块 + 英文块 + 切换器」三件套；形状与 `src/lib.rs` 的 crate 根、下面的 `manual!` 一致。
#![doc = concat!(
    "<div class=\"lang\" data-lang=\"zh\">\n\n",
    include_str!("../docs/guide.zh.md"),
    "\n</div>\n\n<div class=\"lang\" data-lang=\"en\">\n\n",
    include_str!("../docs/guide.en.md"),
    "\n</div>\n\n",
    include_str!("../docs/theme/switcher.html"),
)]

// 手册页的装法：`docs/<stem>.zh.md` 与 `docs/<stem>.en.md` 一前一后，末尾接切换器。
// 两份正文结构相同（标题、表格、示例一一对应），`tests/docs_layout.rs` 会检查这一点。
macro_rules! manual {
    ($path:literal) => {
        concat!(
            "<div class=\"lang\" data-lang=\"zh\">\n\n",
            include_str!(concat!("../docs/", $path, ".zh.md")),
            "\n</div>\n\n<div class=\"lang\" data-lang=\"en\">\n\n",
            include_str!(concat!("../docs/", $path, ".en.md")),
            "\n</div>\n\n",
            include_str!("../docs/theme/switcher.html"),
        )
    };
}

/// 用户文档：给**使用**这个库的人。只写公开面。
pub mod user {
    /// 库 API：两个入口、动作、`Value`/`Document`、访问者、错误、边界。
    #[cfg(feature = "json")]
    pub mod api {
        #![doc = manual!("user/api")]
    }

    /// 文法怎么写：JSON / Rust 两种写法对照、三种引用、组合子、`pattern` 编译、动作。
    #[cfg(feature = "json")]
    pub mod grammar {
        #![doc = manual!("user/grammar")]
    }

    /// 配置参考：`rules` / `pattern` / `grammar` / `rhs` 的键表、优先级、schema、报错对照。
    #[cfg(feature = "json")]
    pub mod config {
        #![doc = manual!("user/config")]
    }

    /// 命令行：构建、选项、三种输出、`span`、错误与退出码。
    #[cfg(feature = "json")]
    pub mod cli {
        #![doc = manual!("user/cli")]
    }
}

/// 开发者文档：给**开发**这个 crate 的人。可以谈内部标识与文件路径。
pub mod dev {
    /// 架构与维护：分层、模块树、硬约束、文档装配、测试矩阵、改动清单。
    pub mod architecture {
        #![doc = manual!("dev/architecture")]
    }

    /// 实现内部：文法怎么落地（`from_exprs`、糖的降级、合成符号、Earley、动作缓存）。
    pub mod internals {
        #![doc = manual!("dev/internals")]
    }
}
