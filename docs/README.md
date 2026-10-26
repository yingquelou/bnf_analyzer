# 文档

两套文档，按**读者是谁**分，不按内容形式分（两边都有 Rust 代码）：

| | 谁读 | 写什么 | 目录 |
|---|---|---|---|
| **用户文档** | **使用**这个库的人：把它加进 `Cargo.toml`、用它的命令行、给它写配置 | 全是 `pub` 面：入口、类型、行为、错误、边界 | [`user/`](user/) |
| **开发者文档** | **开发**这个库的人：读源码、改实现、加特性、跑测试 | 内部分层、模块、约束、糖怎么降级、测试怎么排 | [`dev/`](dev/) |

一句话判据：**用户文档里出现的每个名字，都应该能在 `cargo doc` 里点到；
开发者文档里出现的路径，则直指仓库里的文件。**

## 用户文档

| 文档 | 内容 |
|---|---|
| [`user/api.md`](user/api.md) | 库 API：两个入口、流水线拆用、动作与缓存、`Value`/`Document`、访问者、错误分层、边界 |
| [`user/grammar.md`](user/grammar.md) | 文法怎么写：JSON / Rust 两种写法逐项对照、三种引用、组合子、`pattern` 编译、动作 |
| [`user/config.md`](user/config.md) | 配置参考：`rules` / `pattern` / `grammar` / `rhs` 的键表、优先级裁决、schema、报错对照、版本迁移 |
| [`user/cli.md`](user/cli.md) | 命令行：构建/安装、选项、三种输出（树 / token / BNF）、`span` 字段、错误与退出码 |

## 开发者文档

| 文档 | 内容 |
|---|---|
| [`dev/architecture.md`](dev/architecture.md) | 分层与 feature、模块树、四条硬约束、文档装配、测试矩阵、改动同步清单 |
| [`dev/internals.md`](dev/internals.md) | 文法内部：两条路汇到 `from_exprs`、糖的降级与合成符号、`to_bnf` 渲染、Earley 与动作缓存 |

## 从哪里开始

- 第一次用：仓库根的 `README.md` 有最短路径（装一次、跑一次），然后按上表分流。
- 这些 Markdown 同时是 rustdoc 的底本（`src/guide.rs` 用 `include_str!` 挂成
  `guide::user::*` / `guide::dev::*`），其中的 Rust 代码块是 doctest——
  `cargo test --doc` 会真的执行，文档过时就会红。
- 目录形状由 `tests/docs_layout.rs` 守着：`docs/` 下只有本文件与 `user/`、`dev/`；
  每份手册都挂进了 rustdoc；`docs/user/` 里不出现内部标识。
