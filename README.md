# analyzer

可配置的源码分析器：正则词法 + 结构化文法 + Earley 解析。
库本身不认识任何语言——内置的 [`examples/postscript.json`](examples/postscript.json) 只是一份
配置示例（PostScript 子集）；换一份同格式的配置就能分析别的语言。

```text
$ printf '1 /x { dup }' | analyzer --compact          # 带位置信息的语法树 JSON
$ printf '1 /x { dup }' | analyzer -t --compact       # 只看 token 流
$ analyzer --bnf                                      # 当前文法的 BNF 视图（只读）
```

## 分层与 feature

| feature | 内容 | 何时需要 |
|---|---|---|
| `core`（默认） | 词法、文法、Earley、`Value`、访问者、`Analyzer::new` | 任何时候 |
| `json` | JSON 读写、`Config`（含内置配置）、`Analyzer::from_config` | 配置驱动 |
| `cli` | `analyzer` 可执行文件（依赖 `json`） | 命令行使用 |

```text
cargo add analyzer                  # 只有 core，不产出可执行文件
cargo add analyzer --features json  # 追加 JSON 配置加载
cargo add analyzer --features cli   # 追加 analyzer 可执行文件
```

第三方依赖只有 `regex` 一个 crate：`cargo add analyzer` 之后，`Cargo.lock` 里不会多出
JSON 库或错误处理框架。`json` feature 的 JSON 读写也在这一个 crate 里完成。

想给别的格式（YAML 子集、S 表达式、日志、配置文件）写解析器，`analyzer::json::grammar`
就是一份可照抄的完整示例：三条词法规则、十一条产生式、一组动作。

## 文档：先找对自己的那一侧

| 我是谁 | 文档 | 内容 |
|---|---|---|
| **使用**本库：写 Rust | [`docs/user/api.md`](docs/user/api.md) | 两个入口、动作、`Value`/`Document`、访问者、错误、边界 |
| **使用**本库：写文法 | [`docs/user/grammar.md`](docs/user/grammar.md) | JSON / Rust 两种写法对照、三种引用、组合子、`pattern`、动作 |
| **使用**本库：跑命令行 | [`docs/user/cli.md`](docs/user/cli.md) | 参数、三种输出、`span`、错误与退出码 |
| **使用**本库：写配置 | [`docs/user/config.md`](docs/user/config.md) | 键表、优先级、schema、报错对照、版本迁移 |
| **开发**本 crate | [`docs/dev/architecture.md`](docs/dev/architecture.md) | 分层、模块树、硬约束、文档装配、测试矩阵、改动清单 |
| | [`docs/dev/internals.md`](docs/dev/internals.md) | 文法降级、合成符号、`to_bnf` 渲染、Earley 与动作缓存 |

索引在 [`docs/README.md`](docs/README.md)。分界看**读者**：用户文档只写公开面
（`cargo doc` 里点得到的名字），开发者文档才出现 `src/` 路径、`$` 前缀与测试名。

这些 Markdown 同时是 rustdoc 的底本（`src/guide.rs` 用 `include_str!` 挂成
`guide::user::*` 与 `guide::dev::*`），其中的 Rust 代码块是 doctest：
`cargo test --all-features --doc` 会真的执行，文档过时就会红。

## 快速开始

```text
$ cargo run --features cli -- sample.ps          # 输出带位置信息的语法树 JSON
$ cargo run --features cli -- -t sample.ps       # 只看 token 流
$ cargo run --features cli -- --bnf              # 打印当前文法（BNF 文本，只读视图）
$ cargo run --features cli -- -c my.json -       # 用自定义配置，从 stdin 读源码
```

## 两种用法

| | 作为依赖库 | 作为可执行 crate |
|---|---|---|
| feature | `core`（默认） | `cli`（连带 `json`） |
| 文法 | `Grammar::builder()` + `Expr`（Rust 代码） | `postscript.json` 的 `grammar.productions[].rhs`（结构化对象） |
| 动作 | `Semantics`：token 动作 + 语义动作 | 不支持，输出默认语法树 |
| 结果 | 任意 `Value`（求值、组装字典都行） | 带 `span` 的 `Value::Rule` / `Value::Token` 树 |

两条路最终降级成同一份 CFG（`Grammar`），由同一个 Earley 分析器消费。

`pattern` 与 `rhs` **只接受结构化对象**：字符串写法（正则原文、BNF/EBNF 文本）在
`version: 2` 起已废弃——配置里不存在"再解析一段文本"的隐式步骤。
具体键表见 [`docs/user/config.md`](docs/user/config.md)，两侧写法的逐项对照见
[`docs/user/grammar.md`](docs/user/grammar.md)。

## 配置文件与 schema

| 文件 | 是什么 |
|---|---|
| [`examples/postscript.json`](examples/postscript.json) | **内置配置**：PostScript 子集文法。命令行默认用它，代码里用 `Config::builtin()` 取 |
| [`examples/minimal.json`](examples/minimal.json) | 最小可用配置，演示"换一份配置就能分析别的语言" |
| [`schema/config.schema.json`](schema/config.schema.json) | **配置格式的 schema**：任何配置都遵守它（Draft 2020-12），与具体语言无关 |

`examples/postscript.json` 既是被 `include_str!` 内嵌的默认配置，也是最完整的一份配置示例——
它要同时当示例与默认值，所以放在 `examples/` 而不是仓库根。

## 目录

```text
src/core/     # 引擎：analyzer / document / grammar / parser / pattern / rules /
              #       semantics / tokenizer / value / visit（一个环节一个文件）
src/json/     # 自举的 JSON 层：grammar / parse / write / config / analyzer
src/guide.rs  # 文档装配（include_str!）
src/main.rs   # cli
examples/     # 配置示例：postscript.json（内置默认）、minimal.json
schema/       # 配置格式的 schema
tests/        # 集成测试 + 四条硬约束（core 自包含、依赖只有 regex、名字不越层、代码英文）
docs/user/    # 用户文档（使用者）：库 API、文法、命令行、配置
docs/dev/     # 开发者文档（改本 crate）：架构、实现内部
```

crate 根只声明模块，**不 re-export 任何名字**：`Analyzer`、`Grammar`、`Value` 都留在各自模块的
路径下（`analyzer::core::…`、`analyzer::json::…`）。要短名就自己引入或取别名：

```rust
use analyzer::core as ak;          // 之后写 ak::Analyzer
use analyzer::core::value::Value;  // 或者只导入具体名字
```

`Grammar`、`Value`、`Rule` 这类名字太常见，库不替使用者占位，也不在文档里给同一个类型挂两个路径。

## 测试

```text
$ cargo test                    # core：单元测试 + tests/{actions,core_isolation,docs_layout}.rs + core 文档示例
$ cargo test --all-features     # 追加 JSON 配置、可执行文件与其余集成测试
```

默认 `cargo test` 跑不到 JSON 相关的内容（那些测试标了 `required-features = ["json"]`），
所以提交前用 `--all-features`。各测试文件的职责见
[`docs/dev/architecture.md`](docs/dev/architecture.md) §5。
