# 架构与维护

面向**开发这个 crate 的人**：分层怎么切、约束靠什么钉住、改一处要同步哪些地方。
使用这个库的问题（怎么写文法、怎么挂动作、命令行怎么跑）不在本文，
见 `docs/user/` 下的四份手册；文法内部怎么落地见 `docs/dev/internals.zh.md`。

---

## 1. 分层与 feature

三个 feature 一条线叠加，不是三个并列产品：

| feature | 内容 | 依赖 |
|---|---|---|
| `core`（默认） | 词法、文法、Earley、`Value`、访问者、`Analyzer::new` | `regex` |
| `json` | 自举的 JSON 文法（解析 + 输出）、`Config`（含内置配置）、`Analyzer::from_config` | `core` |
| `cli` | `bnf-analyzer` 可执行文件（`src/main.rs`）；`--encoding` 与插入符对齐 | `json` + 可选 `unicode-width` |

```text
cargo add bnf_analyzer                 # 只有 core，不拉 JSON 库，也不产出可执行文件
cargo add bnf_analyzer --features json # 追加配置加载
cargo add bnf_analyzer --features cli  # 追加可执行文件
```

关键性质：**`json` 层不是"引入了一个 JSON 库"，而是"用 core 写的一个应用"**。
JSON 的解析走本库自己的 Earley 流水线（`src/json/grammar.rs`：三条词法规则 +
七条产生式 + 一组动作），输出是手写的递归下降写器（`src/json/write.rs`）。
所以它既是配置加载器，也是"如何用 core 写一门格式"的完整示例。

---

## 2. 模块树

```text
src/
  core/            # core feature：整个引擎，一个环节一个文件
    mod.rs         #   门面：子模块声明 + re-export（core::Analyzer 等）
    analyzer.rs    #   装配 + tokenize / parse / analyze            → core::Analyzer
    document.rs    #   结果树、walk                                → core::Document
    error.rs       #   AnalyzeError / SetupError                  → core::{AnalyzeError, SetupError}
    grammar.rs     #   Grammar / Expr、EBNF 降级、to_bnf            → core::grammar
    parser.rs      #   Earley                                     → core::parser
    pattern.rs     #   词法模式编译                                 → core::pattern
    rules.rs       #   词法规则与优先级                             → core::rules
    semantics.rs   #   token 动作 / 规则动作                       → core::semantics
    tokenizer.rs   #   Token / Span / Tokenizer                    → core::tokenizer
    value.rs       #   Value / RuleNode                            → core::value
    visit.rs       #   Visitor / Flow / 收集器                      → core::visit
    tests.rs       #   单元测试（不对外）
  json/            # json feature：自举的 JSON 层（无外部依赖）
    mod.rs         #   门面 + 文档
    grammar.rs     #   JSON 文法：rules + grammar + semantics（示例在这里）
    parse.rs       #   parse / parse_with
    write.rs       #   Value → JSON 文本、Document::to_json*、Pattern 的 Display
    config.rs      #   Config 模型：from_value / to_value / parse / load
    analyzer.rs    #   impl Analyzer：from_config
    error.rs       #   JsonError（位置 + 说明）
    tests.rs       #   单元测试
  guide.rs         # 文档装配：include_str! 把 docs/ 引进 rustdoc（中英两份 + 切换器）
  lib.rs           # crate 文档 + 模块声明（不 re-export 任何名字）
  main.rs          # cli
docs/
  README.md         # 索引（只给人看，不进 rustdoc）
  intro.{zh,en}.md  # crate 根页正文
  guide.{zh,en}.md  # `guide` 模块页正文
  user/{api,grammar,config,cli}.{zh,en}.md      # 用户手册，两份成对
  dev/{architecture,internals}.{zh,en}.md       # 开发者手册，两份成对
  theme/switcher.html                           # 每页末尾的语言切换器（按钮 + 样式 + 脚本）
tests/
  actions.rs         # 动作：缓存、错误、键
  analyzer.rs        # 两条入口的集成行为
  config_forms.rs    # 配置形态 + schema 自检
  core_isolation.rs  # 依赖 / 自包含 / 命名边界护栏
  docs_layout.rs     # 文档分家：user/ 只讲 pub 面，dev/ 才谈内部
  grammar_forms.rs   # 文法形式逐项对照
  visitor.rs         # 访问者语义
schema/
  config.schema.json # 配置格式的 JSON Schema（Draft 2020-12）
examples/
  postscript.json    # 内置配置（Config::builtin() 读的就是它）
  minimal.json       # 最小可用配置
```

`analyzer` / `document` / `error` 三个门面文件是**私有**子模块，只以 `core::X` 的形式导出；
引擎模块（`grammar` … `visit`）是 `pub mod`，各管一个环节。所以公共路径只有
`bnf_analyzer::core::…` 一种写法，crate 根上不出现任何名字（§3.3）。

`core` 内部**只有一条从词法到结果的直线**：`Rules` → `Tokenizer` → `Parser`(Earley) →
`Document`/`Value`，`Semantics` 挂在 token 与归约两个点上，`visit` 在结果之上。
没有任何模块反向依赖上层；`Grammar` 不知道 `Rules`，词法类别的存在性在 `Analyzer` 装配时才校验。

---

## 3. 四条硬约束（都有测试）

### 3.1 `core` 自包含

`src/core/` 里所有 `crate::` 引用都必须指向 `crate::core::…`，不回头依赖 `crate::json`、
`crate::guide`。于是 core 能整块切出去单独复用，JSON 层也才配叫"用 core 写的应用"。

`tests/core_isolation.rs::core_only_refers_to_itself` 逐行扫描（跳过注释）钉住这条。

### 3.2 第三方依赖只有 `regex`

错误类型一律手写 `Display` + `std::error::Error`（不用 `thiserror`），JSON 读写自己实现
（不用 `serde`）。两条测试分别查源码里的 `use` 根白名单与 `Cargo.toml` 的 `[dependencies]`：
除 `regex` 之外的依赖必须 `optional = true`，且只有 `cli` 能打开（目前就一条 `unicode-width`）。

### 3.3 名字只留在自己那一层

crate 根**只声明模块，不 re-export**：`Grammar`、`Value`、`Rule`、`Config` 这类名字太常见，
库不替使用者占位，同一个类型也不会在文档里挂两个路径。要短名由使用方自己
`use bnf_analyzer::core as ak;` 或 `use bnf_analyzer::core::value::Value;`。

`core` 门口也只有四个类型（`Analyzer`、`Document`、`AnalyzeError`、`SetupError`）和八个模块；
多一个都要先改测试，等于强制过一次"它到底该住哪层"的讨论。

对应测试：`crate_root_does_not_reexport_core_names`、`core_facade_stays_a_short_list`。

### 3.4 代码英文、注释中文

代码里的每一个字符串都是**产物**——错误消息、CLI 帮助、断言描述——统一写英文；
注释面向维护者，保持中文。产物**不做本地化**：多语言消息要维护两套断言，也不会让谁多得到一点信息。

文档是另一回事：手册中英双份、页内切换（§4），两种语言都由 `cargo test --doc` 当真测试跑。
分界的判据还是「读者是谁」——运行时输出只服务一种形态，手册要照顾两种读者。

判据是「非注释部分不得出现 CJK」：`//`、`//!`、`///`、`/* … */` 之外，代码与字符串
字面量里只要有一个汉字（含全角标点）就红。`tests/code_language.rs` 逐字符扫描
`src/` 与 `tests/` 的 `.rs`，并自带一个反例测试钉住扫描器本身。

测试数据需要非 ASCII 时用别的字符（比如 `/caféé` 验证"列按字符计、偏移按字节计"），
不要拿中文当样本——它会被这条约束拦住。

---

## 4. 文档装配：中英双份 + 页内切换

仓库里的 Markdown 是**唯一底本**，每种语言一份，由 `src/guide.rs` 的 `manual!` 宏挂进 rustdoc：

| 读者 | rustdoc 路径 | 文件 | feature |
|---|---|---|---|
| 使用者 | crate 根 | `docs/intro.{zh,en}.md` | 无 |
| 使用者 | `guide` | `docs/guide.{zh,en}.md` | 无 |
| 使用者 | `guide::user::api` | `docs/user/api.{zh,en}.md` | `json` |
| 使用者 | `guide::user::grammar` | `docs/user/grammar.{zh,en}.md` | `json` |
| 使用者 | `guide::user::cli` | `docs/user/cli.{zh,en}.md` | `json` |
| 使用者 | `guide::user::config` | `docs/user/config.{zh,en}.md` | `json` |
| 开发者 | `guide::dev::architecture` | `docs/dev/architecture.{zh,en}.md` | 无 |
| 开发者 | `guide::dev::internals` | `docs/dev/internals.{zh,en}.md` | 无 |

装法是固定三件套，`manual!("user/api")` 展开成这样：

```text
concat!(
    "<div class=\"lang\" data-lang=\"zh\">\n\n",
    include_str!("../docs/user/api.zh.md"),
    "\n</div>\n\n<div class=\"lang\" data-lang=\"en\">\n\n",
    include_str!("../docs/user/api.en.md"),
    "\n</div>\n\n",
    include_str!("../docs/theme/switcher.html"),
)
```

两份正文在**同一个页面**里，`docs/theme/switcher.html` 只做三件事：

1. 右下角一个固定的 `中文` / `English` 按钮组；
2. 两条 CSS 决定显示哪份：`html:not([data-lang="en"]) [data-lang="en"] { display: none }` 与
   `html[data-lang="en"] [data-lang="zh"] { display: none }`——还没写 `data-lang` 时按中文处理，不会先闪一下英文；
3. 一段脚本：把 localStorage 里记住的选择写回 `<html data-lang>`，并把侧边栏目录（`#rustdoc-toc`）里
   另一种语言的条目打上 `hidden`，否则两种语言的标题会在目录里各列一遍。

`switcher.html` 里 `<style>` 与 `<script>` 内部**不留空行**：CommonMark 的空行会截断 HTML 块，
剩下的部分按 Markdown 段落处理——CSS 里的 `--` 会被替换成 en dash，样式直接失效（`cargo doc` 会给出
`nested Markdown paragraph in HTML style tag` 警告）。

为什么不跳页而是页内切换：`cargo doc` 只产出一棵树，两套文档就意味着两棵树、两套搜索索引、两套相对链接；
页内切换不跳页、不改地址、锚点还在原页，代价只是同一页 HTML 里放两份正文。

crate 根（`src/lib.rs`）与 `guide` 页（`src/guide.rs`）各自手写一遍同样的 `concat!`：
`#![doc = …]` 是内层属性，必须排在 `macro_rules!` 之前，那里用不了 `manual!`。

**覆盖范围**：挂了 `manual!` 的 8 个页面整页双语；条目级 `///`（`Value::text` 这类）仍是中文，
那些页面没有切换器，也就不会出现"按了没反应"的按钮。

**doctest 是双份的**：两份正文里的 Rust 代码块都会被 `cargo test --doc` 编译并运行，所以改 API 要同时改两种语言；
`tests/docs_layout.rs` 还会比对两份的代码块数量，漏译一个示例就红。

这样做不只是少写一份：文档里的 Rust 代码块是 **doctest**，`cargo test --doc` 会真的编译并运行。
API 改名、返回类型变化、行为调整都会先让文档变红，而不是让读者先踩坑。

rustdoc 本身也分界：crate 根与各模块开头的 `//!` / `///` 面向**使用者**——只讲这层给你什么、
怎么导入、行为如何，不写仓库路径、测试名与设计动机（"为什么不 re-export"这类话在 §3.3）。
`tests/docs_layout.rs::rustdoc_never_points_at_repository_files` 扫 `src/` 的每行文档注释，
出现 `src/`、`tests/`、`Cargo.toml` 就红。要把实现理由写给人看，写在这里。

分界线的判据是**读者是谁**，不是内容形式（两边都有 Rust）：

- **`docs/user/` 面向使用者**：只写 `pub` 面——公开入口、公开类型、公开行为、错误与边界。
  读者不需要知道实现，所以这里不出现 `src/` 路径、`tests/` 名字、`$starN` / `is_synthetic` /
  `from_exprs` 这类内部标识。写配置的人要的键表在 `docs/user/config.zh.md`，
  写 Rust 的人要的 API 在 `docs/user/api.zh.md`。
- **`docs/dev/` 面向本 crate 的开发者**：分层、模块、约束、糖怎么降级、测试怎么排、
  改一处要同步哪些文件。这里可以并且应该谈内部标识。

`tests/docs_layout.rs` 钉住这个形状：`docs/` 顶层只允许 `README.md` + `intro.*.md` + `guide.*.md` + `user/` +
`dev/` + `theme/`；八份底本成对（`X.zh.md` 必有 `X.en.md`，反之亦然，中文那份必须含 CJK、英文那份不许含），
两份代码块数量一致；每份底本都能在 `src/lib.rs` / `src/guide.rs` 里找到装配入口；`docs/user/` 里不许出现内部标识；
仓库里也不许再出现上一版的路径（平铺手册、`dev/` 下的 api/grammar 位置，或没有语言后缀的 `docs/user/<名字>.md`）。

---

## 5. 测试矩阵

```text
$ cargo test                          # core：56 个单元测试 + actions + code_language + core_isolation + docs_layout + 12 项文档示例（共 96 项）
$ cargo test --all-features           # 追加 json / cli 的全部内容（共 222 项）
$ cargo test --all-features --doc     # 只跑文档里的示例（50 项，中英各一份）
$ cargo test --no-default-features    # 确认 core 不靠 json 也能活（与默认等价）
```

提交前用 `--all-features`：默认 `cargo test` 跑不到标了 `required-features = ["json"]` 的测试。

| 测试文件 | 管什么 |
|---|---|
| `src/core/tests.rs`、`src/json/tests.rs` | 单元级：词法裁决、Earley 归约、值树形状、序列化 |
| `tests/actions.rs` | 动作的键、缓存、错误透传 |
| `tests/analyzer.rs` | `Analyzer::new` 与 `from_config` 两条入口的行为一致性 |
| `tests/grammar_forms.rs` | `docs/user/grammar.zh.md` 每一行对照的可执行版本 |
| `tests/config_forms.rs` | schema 与代码常量 `PATTERN_KEYS` / `EXPR_KEYS` 一致、示例配置合规 |
| `tests/visitor.rs` | `Visitor` 的 `Flow` 语义、`enter`/`leave` 配对、收集器 |
| `tests/core_isolation.rs` | 3.1–3.3：core 自包含、依赖只有 `regex`（其余必须 optional 且归 `cli`）、名字不越层 |
| `tests/code_language.rs` | 3.4：代码英文、注释中文 |
| `tests/docs_layout.rs` | 文档分家、中英成对与代码块对称、rustdoc 装配、rustdoc 不指仓库路径 |

还有一个非测试的检查：`cargo doc --all-features` 必须 0 警告。文档之间的 intra-doc link
（比如 `crate::core::visit::Flow`）断了，这里会先红。

---

## 6. 改一处要同步哪些地方

下表提到的每份手册都是成对的：`X.zh.md` 与 `X.en.md` **一起改**（只改一份，`tests/docs_layout.rs` 的代码块
数量比对与"英文那份不许出现中文"两条会先红）。下表只写出中文那份，英文那份是同一节的同一张表。

| 你改了 | 必须一起改 |
|---|---|
| 任何一篇手册的正文 | 同一篇的另一种语言（`X.zh.md` ↔ `X.en.md`），标题层级、表格行列、代码块一一对应 |
| 文法 / `Expr` 的键 | `schema/config.schema.json`、`EXPR_KEYS`、`docs/user/grammar.zh.md` 的对照表、`docs/user/config.zh.md` 的键表、`docs/dev/internals.zh.md` 的降级表、`tests/grammar_forms.rs` |
| 词法 / `Pattern` 的键 | `schema/config.schema.json`、`PATTERN_KEYS`、`docs/user/grammar.zh.md` §6、`docs/user/config.zh.md` §3 |
| 糖的展开 / 合成符号 | `docs/dev/internals.zh.md` §3、`docs/user/grammar.zh.md` §5 的"语法树里是平的"、`src/core/parser.rs` 的 expected 过滤测试 |
| 配置字段 | `CONFIG_KEYS` / `RULE_KEYS` / `GRAMMAR_KEYS` / `PRODUCTION_KEYS`、schema、`docs/user/config.zh.md` §1–§4 |
| 默认配置 | `examples/postscript.json`、`docs/user/config.zh.md` §10 的 BNF 输出、`docs/user/cli.zh.md` 里 `--bnf` 的样例 |
| 公开 API | `docs/user/api.zh.md` 的示例与表格（否则 doctest 红）、`src/lib.rs` 的路径表 |
| 错误消息 / CLI 输出 | `docs/user/cli.zh.md` §1 的帮助与 §3 的错误样例、`docs/user/config.zh.md` §7/§9 的文案与对照表、断言里 `contains("…")` 检查的片段 |
| 输出格式 | `src/json/write.rs`、`docs/user/cli.zh.md` §2 的样例、`tests/analyzer.rs` |
| 模块结构 | 本文 §2 的树、`src/core/mod.rs` 的分文件说明、`tests/core_isolation.rs` 的清单 |
| 发布相关（版本号、标签、CI 检查项） | `Cargo.toml` 的 `version`、`[package.metadata.docs.rs]`、`.github/workflows/{ci,release}.yml`、本文 §8 |

判断标准很简单：**能跑绿的测试 + 能编译的文档**，缺一条都算没改完。

---

## 7. 本地验证

```text
$ cargo test --all-features
$ cargo test --all-features --doc
$ cargo doc --all-features --no-deps
$ cargo clippy --all-targets --all-features
```

Windows 上增量编译偶尔会报 `os error 5`（删不掉旧的 `.lock`）：加 `CARGO_INCREMENTAL=0`
重跑即可，不是代码问题。

## 8. 发布与 CI


README 面向使用者，不写这些；本节是发布流程的唯一底本。

**首次发布手动**：`cargo login` → `cargo publish` → `git tag v0.2.0 && git push origin v0.2.0`。
之后每次发版只打标签，交给 `.github/workflows/release.yml`：

1. 改 `Cargo.toml` 的 `version`，`cargo test --all-features` 通过后提交；
2. `git tag v0.1.1 && git push origin v0.1.1`；
3. 流水线校验「标签 = `Cargo.toml` 版本」→ 跑测试 → `cargo doc` 零警告 → `cargo publish` →
   建 GitHub Release 并附上 `.crate`。

发布作业是**可重跑**的：`cargo publish` 遇到 "already uploaded"（给老版本补 Release、重跑流水线）
输出 warning 并按成功退出；建 Release 前先 `gh release view` 查重。不要用 `cargo info` 预检
"是否已发布"——它优先解析本地工作区，永远返回"已经存在"，会把发布全挡住。

一次性配置：仓库 Settings → Secrets and variables → Actions 新建 `CARGO_REGISTRY_TOKEN`
（crates.io 的 API token，权限只勾 `publish-new` 与 `publish-update`）。也可以改用 crates.io 的
Trusted Publishing（OIDC，无需存 token）：把 publish 步骤换成 `rust-lang/crates-io-auth-action`
取 token，并给作业加 `id-token: write` 权限。

`.github/workflows/ci.yml` 在 PR 与 `master` 推送上跑三种 feature 组合
（默认 / `--all-features` / `--no-default-features`）、clippy 与 rustdoc。本仓库不是 rustfmt
默认风格，所以 CI 不设 `cargo fmt --check` 这道闸。docs.rs 那边由
`[package.metadata.docs.rs] all-features = true` 保证 json / cli 的手册页也构建出来。
