# Implementation internals: how a grammar is built

For **people changing this crate**. Users only need "the two spellings are equivalent, and here is what comes out",
which is in `docs/user/grammar.zh.md`; this file covers how the library produces it: how sugar is expanded,
how synthetic symbols are hidden, at which step validation happens, and where the tests are.

Read `docs/dev/architecture.zh.md` next (layers, modules, constraints, test matrix).

---

## 1. Two enums, one shape

`Pattern` (lexical) and `Expr` (grammar) are two enums with the same shape, and the JSON keys are the variant names:

| JSON key | `Pattern` | `Expr` |
|---|---|---|
| `regex` | `Regex` | — |
| `literal` | `Literal` | `Literal` |
| `chars` / `not_chars` | `Chars` / `NotChars` | — |
| `seq` / `alt` | `Seq` / `Alt` | `Seq` / `Alt` |
| `star` / `plus` / `opt` | `Star` / `Plus` / `Opt` | `Star` / `Plus` / `Opt` |
| `rule` / `token` | — | `Rule` / `Token` |

The key whitelists are the constants `PATTERN_KEYS` / `EXPR_KEYS` (`src/core/pattern.rs`, `src/core/grammar.rs`),
and `tests/config_forms.rs` keeps them aligned with `schema/config.schema.json`: adding a key means changing four
places at once — the enum, `from_value`, the constant, and the schema.

---

## 2. Assembly: two routes meeting at `Grammar::from_exprs`

```text
Rust:  Grammar::builder().start(..).production(..).build()
JSON:  Config.grammar.parse_grammar()  ── GrammarSpec::from_spec
                    │
                    └──► Grammar::from_exprs(start, Vec<(name, Expr)>)
                              │  1 empty production check (Empty)
                              │  2 non-empty names (EmptyName)
                              │  3 resolve the start symbol: default productions[0].0, else UnknownStart
                              │  4 expand sugar one by one, appending $starN/$plusN/$optN/$groupN
                              │  5 collect Expr::Literal (for Rules::with_literals)
                              │  6 check that nonterminals referenced by Expr::Rule exist (UnknownSymbol)
                              │  7 check every nonterminal can derive a terminal string (NonProductive)
                              └──► Grammar
```

**Lexical classes are not validated here**: `Expr::Token(name)` only requires a non-empty name; the real existence
check happens in `Analyzer::new` while the token table is assembled (`UnknownTokenClass`). The reason is that
`Grammar` is a pure syntactic structure and building one should not depend on the token table — which is why
`GrammarSpec::parse_grammar()` can succeed on its own.

---

## 3. Lowering the sugar

| spelling | expansion |
|---|---|
| `star(a)` | `$starN ::= $starN a` + `$starN ::= ε` (left-recursive) |
| `plus(a)` | `$plusN ::= a` + `$plusN ::= $plusN a` (left-recursive) |
| `opt(a)` | `$optN ::= a` + `$optN ::= ε` |
| grouping (`seq`/`alt` as a sub-item) | one `$groupN` (which may hold several alternatives) |
| `seq([])` | a single production with no items (ε), no helper symbol |

Repetition **must** be left-recursive: the right-recursive shape (`$starN ::= a $starN`) is the worst case for
Earley — every reduction copies "all remaining child values" up to the parent, giving O(n³) time and O(n²) memory.
Left recursion only links one more node onto the yield chain per step (see §6), and the flattened order is
unchanged.

Grouping does **not** distribute either: a `seq` holding n two-way alternatives would expand to 2ⁿ productions;
now each sub-item lowers to its own `$groupN`, so the size stays proportional to the expression.
`nested_alternatives_stay_linear` guards this.

A synthetic production (`synthetic = true`) has to be hidden in **three places**, otherwise the user sees
implementation details:

1. **AST**: children are flattened on reduction, no `Value::Rule` node is produced, and `children` stays flat;
2. **`to_bnf()`**: skipped, so the user only ever sees `*` / `+` / `?`;
3. **the expected list of an error**: skipped, otherwise `$star1` would leak out.

The test is always `Grammar::is_synthetic(name)`: a name counts as synthetic only when **every** production with
that name is synthetic. So when someone writes a production with the same name by hand, that name shows up in the
view and in errors as usual — which is exactly the intended behavior.

Numbering increases globally regardless of kind (`$opt2` may follow `$star1`), and no code may depend on the
actual numbers. `src/core/parser.rs::hides_synthetic_productions_from_expectations` is the regression test for
this convention.

---

## 4. Rendering rules for `to_bnf()`

| `Expr` | rendering |
|---|---|
| `Rule(name)` | the bare name `value` |
| `Token(name)` | `@name` |
| `Literal(text)` | the quoted text `"{"` |
| `Seq` / `Alt` / suffix | `a b` / `a \| b` / `a*`, `a+`, `a?` |

Several productions with the same name merge into one line `a ::= x | y ;`; an empty right-hand side renders as
`x ::= ;` (ε). It is a **read-only view**: the `Rhs` text parser was removed in version 2, so do not expect to
parse it back.

---

## 5. How the lexical side is built

- Every `Pattern` goes through `to_regex()`, which anchors with `\A`, so it only matches at the current scan
  position.
- `Rules` decides by **longest match, then priority, then declaration order**; a `skip` rule consumes input but
  produces no token.
- Literals in the grammar are handed by `Grammar::literals()` to `Rules::with_literals`, which adds rules with the
  fixed `LITERAL_PRIORITY = 1000` so that keywords are not swallowed by an identifier rule.
- Compile-time failures are `RulesError` (`Regex` / `DuplicateName` / `EmptyPattern` / `EmptyName`), with
  `regex::Error` passed through as `source()`.

---

## 6. At run time: Earley and the action cache

- The parser is Earley: left recursion, ε productions and ambiguity all work without rewriting the grammar.
  Worst case is O(n³); with an ambiguity it takes the **first completed derivation** and does not enumerate every
  parse.
- **Recognition does not materialize semantic values**: what a matched symbol yields is a lazily flattened
  concatenation tree (the `Run` type in `parser.rs`: `Arc` nodes, either a leaf or a concatenation). Concatenation
  is O(1), each node caches the span it covers, and the tree is only flattened into `&[Value]` when something
  actually needs it.
- Before reducing, the parser checks whether any parent in `chart[origin]` is waiting for this nonterminal;
  **if nobody is waiting and this is not the accepting position, no value is built**. A start symbol such as
  `program ::= value*` completes at every prefix position, and without this check each completion would flatten the
  whole prefix — pure O(n²) waste.
- Result with the built-in PostScript configuration: 1001 tokens went from 5.5 s to 14 ms, 64001 tokens take about
  1 s (linear). `src/core/parser.rs::parses_long_repetitions` and
  `src/json/tests.rs::parses_long_arrays_and_objects` guard it.
- The concatenation tree is as deep as the input is long, so `Node::drop` releases it with an explicit work stack —
  the default recursive drop overflows the stack after a few thousand tokens.
- Reduction actions are cached by "reduction point + end position", so one reduction runs its action once, and a
  reduction no derivation adopts runs no action at all. Callbacks must therefore be free of side effects.
- Lexical classes and grammar symbols **never infer one another**: `Rule` only looks in the production table,
  `Token` only in the token table.
- `Span.start/end` are byte offsets, half-open; `line/column` count characters and are 1-based. The EOF span is a
  zero-width range.

---

## 7. Index of the comparison tests

```text
$ cargo test --test grammar_forms    # grammar forms
$ cargo test --test config_forms     # configuration shapes + schema
```

| behavior | test |
|---|---|
| one-of-ten / the two spellings of `digit` | `digit_example_matches_both_spellings` |
| the two spellings of a nonterminal / lexical class | `nonterminal_has_two_spellings` |
| `rule` and `token` do not infer each other | `rule_and_token_are_not_interchangeable` |
| literal terminals derive lexical rules | `literal_terminals_are_derived_from_the_grammar` |
| sequence and alternation | `sequence_and_alternation_match_rust_expr` |
| the only difference between `*` and `+` | `star_and_plus_differ_only_at_zero` |
| `?` | `opt_makes_a_piece_optional` |
| grouping, grouping plus suffix | `group_carries_inner_alternatives_and_suffix`, `group_with_star_repeats_the_whole_sequence` |
| empty production ε | `empty_seq_is_epsilon`, `empty_seq_as_an_alternative_is_optional` |
| an empty `alt` | `empty_alt_is_rejected` |
| the same name declared twice | `repeated_declarations_merge_into_alternatives` |
| a suffix binds one symbol only | `suffix_binds_to_one_piece_not_the_whole_sequence` |
| a string right-hand side / pattern is rejected | `string_rhs_is_rejected`, `string_pattern_is_rejected` |
| errors from the object form | `unknown_keys_are_rejected`, `pattern_errors_point_at_the_key` |
| structured right-hand side to grammar | `structured_rhs_builds_the_expected_grammar` |
| errors from a structured right-hand side | `structured_rhs_errors_are_actionable` |
| action keys | `actions_are_keyed_by_symbol_label` |
| the compiled `pattern` | `pattern_compiles_to_the_expected_regex` |
| migration hint for an old version | `legacy_version_points_at_the_migration` |
| reversible serialization | `config_round_trips_through_json` |
| schema agrees with the code | `schema_declares_exactly_the_supported_keys`, `schema_refs_all_resolve` |
| builtin / example configuration matches the schema | `builtin_config_matches_schema`, `example_config_matches_schema_and_parses` |
| schema agrees with the runtime on what is invalid | `schema_agrees_with_runtime_on_what_is_invalid` |
| helper productions stay out of error hints | `src/core/parser.rs::hides_synthetic_productions_from_expectations` |
