# 实现内部：文法怎么落地

面向**改这个 crate 的人**。用户在 `docs/user/grammar.md` 只需要知道"两种写法等价、结果是什么样"，
这里讲"库里怎么把它做出来"：糖怎么展开、合成符号怎么藏、检查在哪一步发生、测试在哪。

读完这份再看 `docs/dev/architecture.md`（分层、模块、约束、测试矩阵）。

---

## 1. 两个枚举，一套形状

`Pattern`（词法）与 `Expr`（文法）是同一套形状的两个枚举，JSON 的键就是变体名：

| JSON 键 | `Pattern` | `Expr` |
|---|---|---|
| `regex` | `Regex` | — |
| `literal` | `Literal` | `Literal` |
| `chars` / `not_chars` | `Chars` / `NotChars` | — |
| `seq` / `alt` | `Seq` / `Alt` | `Seq` / `Alt` |
| `star` / `plus` / `opt` | `Star` / `Plus` / `Opt` | `Star` / `Plus` / `Opt` |
| `rule` / `token` | — | `Rule` / `Token` |

键名白名单常量是 `PATTERN_KEYS` / `EXPR_KEYS`（`src/core/pattern.rs`、`src/core/grammar.rs`），
`tests/config_forms.rs` 会拿它们和 `schema/config.schema.json` 对齐——加一个键要同时改四处：
枚举、`from_value`、常量、schema。

---

## 2. 装配：两条路汇到 `Grammar::from_exprs`

```text
Rust:  Grammar::builder().start(..).production(..).build()
JSON:  Config.grammar.parse_grammar()  ── GrammarSpec::from_spec
                    │
                    └──► Grammar::from_exprs(start, Vec<(name, Expr)>)
                              │  ① 空产生式检查（Empty）
                              │  ② 名字非空（EmptyName）
                              │  ③ 起点解析：缺省取 productions[0].0，不存在报 UnknownStart
                              │  ④ 逐条展开糖 → 追加 $starN/$plusN/$optN/$groupN
                              │  ⑤ 收集 Expr::Literal（供 Rules::with_literals 用）
                              │  ⑥ 校验 Expr::Rule 引用的非终结符存在（UnknownSymbol）
                              └──► Grammar
```

**词法类别不在这里校验**：`Expr::Token(name)` 只要求名字非空，真正的存在性检查在
`Analyzer::new` 装配词法表时做（`UnknownTokenClass`）。原因是 `Grammar` 是纯语法结构，
建文法时不该依赖词法表——`GrammarSpec::parse_grammar()` 因此能单独成功。

---

## 3. 糖的降级

| 写法 | 展开 |
|---|---|
| `star(a)` | `$starN ::= a $starN` + `$starN ::= ε` |
| `plus(a)` | `$plusN ::= a $starM`（复用 `star` 的那个 `$starM`） |
| `opt(a)` | `$optN ::= a` + `$optN ::= ε` |
| 分组（`seq`/`alt` 作为子项） | 一个 `$groupN`（内部可能有多条候选） |
| `seq([])` | 一条空 items 的产生式（ε），不生成辅助符号 |

合成产生式（`synthetic = true`）必须**三处都藏住**，否则用户会看到实现细节：

1. **AST**：归约时把子值摊平，不产出 `Value::Rule` 节点，`children` 是平的；
2. **`to_bnf()`**：跳过，用户看到的只有 `*` / `+` / `?`；
3. **错误提示的 expected 列表**：跳过，否则会冒出 `$star1`。

判定统一走 `Grammar::is_synthetic(name)`：**同名的所有产生式都是合成的**才算合成。
所以手工写了一个同名产生式时，这个名字会正常出现在视图与错误里——这正好是想要的行为。

编号全局递增、与种类无关（`$star1` 之后可能是 `$opt2`），任何代码都不得依赖具体编号。
`src/core/parser.rs::hides_synthetic_productions_from_expectations` 是这个约定的回归测试。

---

## 4. `to_bnf()` 的渲染约定

| `Expr` | 渲染 |
|---|---|
| `Rule(name)` | 裸名 `value` |
| `Token(name)` | `@name` |
| `Literal(text)` | 带引号原文 `"{"` |
| `Seq` / `Alt` / 后缀 | `a b` / `a \| b` / `a*`、`a+`、`a?` |

同名多条产生式合并成一行 `a ::= x | y ;`；空右部渲染成 `x ::= ;`（ε）。
它是**只读视图**：`Rhs` 文本解析器在 version 2 已经删除，别指望读回来。

---

## 5. 词法侧的落地

- 每条 `Pattern` 经 `to_regex()` 编译，统一加 `\A` 锚定，所以只从当前扫描位置起匹配。
- `Rules` 按 **最长匹配 → priority → 声明顺序** 裁决；`skip` 规则只消费不产 token。
- 文法里的字面量由 `Grammar::literals()` 交给 `Rules::with_literals` 补规则，
  优先级固定 `LITERAL_PRIORITY = 1000`，保证关键字不被标识符规则吞掉。
- 编译期错误是 `RulesError`（`Regex` / `DuplicateName` / `EmptyPattern` / `EmptyName`），
  `regex::Error` 通过 `source()` 透传。

---

## 6. 运行时：Earley 与动作缓存

- 解析器是 Earley：左递归、ε 产生式、歧义都能直接吃（不改写文法）。最坏 O(n³)，
  歧义时只取**最先完成的推导**，不枚举全部解析。
- 归约动作按"归约点 + 结束位置"缓存，所以一次归约只执行一次；回调必须无副作用。
- 词法类别与文法符号**互不推断**：`Rule` 只查产生式表，`Token` 只查词法表。
- `Span.start/end` 按字节、左闭右开；`line/column` 按字符、1 基。EOF 的 span 是零宽区间。

---

## 7. 对照测试索引

```text
$ cargo test --test grammar_forms    # 文法形式
$ cargo test --test config_forms     # 配置形态 + schema
```

| 行为 | 测试 |
|---|---|
| 十选一 / `digit` 两种写法 | `digit_example_matches_both_spellings` |
| 非终结符 / 词法类别的两种写法 | `nonterminal_has_two_spellings` |
| `rule` 与 `token` 不互相推断 | `rule_and_token_are_not_interchangeable` |
| 字面量自动派生词法规则 | `literal_terminals_are_derived_from_the_grammar` |
| 序列与选择 | `sequence_and_alternation_match_rust_expr` |
| `*` 与 `+` 的唯一差别 | `star_and_plus_differ_only_at_zero` |
| `?` | `opt_makes_a_piece_optional` |
| 分组、分组 + 后缀 | `group_carries_inner_alternatives_and_suffix`、`group_with_star_repeats_the_whole_sequence` |
| 空产生式 ε | `empty_seq_is_epsilon`、`empty_seq_as_an_alternative_is_optional` |
| 空的 `alt` | `empty_alt_is_rejected` |
| 同名多次声明 | `repeated_declarations_merge_into_alternatives` |
| 后缀只绑一个符号 | `suffix_binds_to_one_piece_not_the_whole_sequence` |
| 字符串右部 / pattern 被拒 | `string_rhs_is_rejected`、`string_pattern_is_rejected` |
| 对象写法的报错 | `unknown_keys_are_rejected`、`pattern_errors_point_at_the_key` |
| 结构化右部 → 文法 | `structured_rhs_builds_the_expected_grammar` |
| 结构化右部的报错 | `structured_rhs_errors_are_actionable` |
| 动作的键 | `actions_are_keyed_by_symbol_label` |
| `pattern` 编译结果 | `pattern_compiles_to_the_expected_regex` |
| 旧版本号的迁移提示 | `legacy_version_points_at_the_migration` |
| 可逆序列化 | `config_round_trips_through_json` |
| schema 与代码一致 | `schema_declares_exactly_the_supported_keys`、`schema_refs_all_resolve` |
| 内置配置 / 示例符合 schema | `builtin_config_matches_schema`、`example_config_matches_schema_and_parses` |
| schema 与运行时判定一致 | `schema_agrees_with_runtime_on_what_is_invalid` |
| 辅助产生式不进错误提示 | `src/core/parser.rs::hides_synthetic_productions_from_expectations` |
