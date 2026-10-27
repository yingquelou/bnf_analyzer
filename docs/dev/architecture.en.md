# Architecture and maintenance

For **people developing this crate**: where the layers are cut, what pins the constraints down, and what has to be
changed together. Questions about *using* the library (writing a grammar, attaching actions, running the command
line) are not here — see the four manuals under `docs/user/`; how a grammar is built internally is in
`docs/dev/internals.zh.md`.

---

## 1. Layers and features

Three features stack along one line; they are not three separate products:

| feature | contents | depends on |
|---|---|---|
| `core` (default) | tokenizer, grammar, Earley, `Value`, visitors, `Analyzer::new` | `regex` |
| `json` | the self-hosted JSON grammar (parse + write), `Config` (including the builtin configuration), `Analyzer::from_config` | `core` |
| `cli` | the `bnf-analyzer` binary (`src/main.rs`) | `json` |

```text
cargo add bnf_analyzer                 # core only: no JSON library pulled in, no binary produced
cargo add bnf_analyzer --features json # add configuration loading
cargo add bnf_analyzer --features cli  # add the binary
```

The key property: **the `json` layer is not "a JSON library was added", it is "an application written with core"**.
JSON is parsed by this library's own Earley pipeline (`src/json/grammar.rs`: three lexical rules, eleven
productions, one set of actions), and written by a hand-written recursive writer (`src/json/write.rs`). So it is
both a configuration loader and a complete example of "how to write a format with core".

---

## 2. Module tree

```text
src/
  core/            # core feature: the whole engine, one stage per file
    mod.rs         #   facade: submodule declarations + re-exports (core::Analyzer, ...)
    analyzer.rs    #   assembly + tokenize / parse / analyze            -> core::Analyzer
    document.rs    #   result tree, walk                               -> core::Document
    error.rs       #   AnalyzeError / SetupError                       -> core::{AnalyzeError, SetupError}
    grammar.rs     #   Grammar / Expr, EBNF lowering, to_bnf            -> core::grammar
    parser.rs      #   Earley                                          -> core::parser
    pattern.rs     #   lexical pattern compilation                      -> core::pattern
    rules.rs       #   lexical rules and priorities                     -> core::rules
    semantics.rs   #   token actions / rule actions                     -> core::semantics
    tokenizer.rs   #   Token / Span / Tokenizer                         -> core::tokenizer
    value.rs       #   Value / RuleNode                                 -> core::value
    visit.rs       #   Visitor / Flow / collectors                      -> core::visit
    tests.rs       #   unit tests (not public)
  json/            # json feature: the self-hosted JSON layer (no external dependency)
    mod.rs         #   facade + docs
    grammar.rs     #   the JSON grammar: rules + grammar + semantics (the example lives here)
    parse.rs       #   parse / parse_with
    write.rs       #   Value -> JSON text, Document::to_json*, Display for Pattern
    config.rs      #   the Config model: from_value / to_value / parse / load
    analyzer.rs    #   impl Analyzer: from_config
    error.rs       #   JsonError (position + message)
    tests.rs       #   unit tests
  guide.rs         # doc assembly: include_str! pulls docs/ into rustdoc (both languages + switcher)
  lib.rs           # crate docs + module declarations (re-exports nothing)
  main.rs          # cli
docs/
  README.md         # index (for humans only, not part of rustdoc)
  intro.{zh,en}.md  # body of the crate root page
  guide.{zh,en}.md  # body of the `guide` module page
  user/{api,grammar,config,cli}.{zh,en}.md      # user manuals, in pairs
  dev/{architecture,internals}.{zh,en}.md       # developer manuals, in pairs
  theme/switcher.html                           # the language switcher at the end of every page (buttons + css + js)
tests/
  actions.rs         # actions: cache, errors, keys
  analyzer.rs        # integrated behavior of the two entry points
  config_forms.rs    # configuration shapes + schema self-check
  core_isolation.rs  # dependency / self-containment / naming boundaries
  docs_layout.rs     # docs split: user/ covers the pub surface only, dev/ talks internals
  grammar_forms.rs   # grammar forms compared item by item
  visitor.rs         # visitor semantics
schema/
  config.schema.json # JSON Schema of the configuration format (Draft 2020-12)
examples/
  postscript.json    # the builtin configuration (what Config::builtin() reads)
  minimal.json       # the smallest usable configuration
```

`analyzer` / `document` / `error` are **private** submodules, exported only as `core::X`; the engine modules
(`grammar` … `visit`) are `pub mod`, one stage each. So the only public path spellings look like
`bnf_analyzer::core::…`, and no name appears at the crate root (§3.3).

Inside `core` there is **one straight line from text to result**: `Rules` → `Tokenizer` → `Parser` (Earley) →
`Document`/`Value`, with `Semantics` attached at two points (tokens and reductions) and `visit` on top of the
result. No module depends on a layer above it; `Grammar` knows nothing about `Rules`, and the existence of a
lexical class is only checked when `Analyzer` is assembled.

---

## 3. Four hard constraints (all covered by tests)

### 3.1 `core` is self-contained

Every `crate::` reference inside `src/core/` must point at `crate::core::…` and never back at `crate::json` or
`crate::guide`. That way core can be lifted out and reused as one block, and the JSON layer earns the name "an
application written with core".

`tests/core_isolation.rs::core_only_refers_to_itself` pins this down by scanning line by line (skipping comments).

### 3.2 The only third-party dependency is `regex`

Error types hand-write `Display` + `std::error::Error` (no `thiserror`), and JSON reading and writing are
implemented here (no `serde`). Two tests check the `use` root whitelist in the source and the `[dependencies]`
section of `Cargo.toml`.

### 3.3 Names stay in their own layer

The crate root **declares modules and re-exports nothing**: names like `Grammar`, `Value`, `Rule`, and `Config` are
far too common for a library to claim, and the same type never hangs under two paths in the docs. If you want a
short name, write `use bnf_analyzer::core as ak;` or `use bnf_analyzer::core::value::Value;` yourself.

The `core` facade holds four types (`Analyzer`, `Document`, `AnalyzeError`, `SetupError`) and eight modules; adding
one more means changing the test first, which forces the "which layer does this belong to" discussion exactly once.

The tests: `crate_root_does_not_reexport_core_names`, `core_facade_stays_a_short_list`.

### 3.4 Code in English, comments in Chinese

Every string in the code is a **product** — error messages, CLI help, assertion descriptions — and all of them are
written in English; comments address maintainers and stay in Chinese. The products are **not localized**: several
sets of messages would mean several sets of assertions, and nobody would learn anything more from them.

Documentation is a different matter: the manuals come in Chinese and English pairs with an in-page switch (§4), and
`cargo test --doc` runs both languages as real tests. The criterion is still "who is the reader" — runtime output
serves one shape only, while the manuals serve two kinds of readers.

The test is "no CJK outside comments": outside `//`, `//!`, `///`, `/* … */`, a single Chinese character (including
full-width punctuation) in code or in a string literal turns it red. `tests/code_language.rs` scans `src/` and the
`.rs` files under `tests/` character by character, and carries a counter-example test that pins the scanner itself.

When test data needs to be non-ASCII, use other characters (for example `/caféé` to verify "columns count characters,
offsets count bytes"), not Chinese — the rule would stop it.

---

## 4. Doc assembly: two languages per page, switched in place

The Markdown in the repository is the **single source**, one file per language, pulled into rustdoc by the
`manual!` macro in `src/guide.rs`:

| reader | rustdoc path | files | feature |
|---|---|---|---|
| user | crate root | `docs/intro.{zh,en}.md` | none |
| user | `guide` | `docs/guide.{zh,en}.md` | none |
| user | `guide::user::api` | `docs/user/api.{zh,en}.md` | `json` |
| user | `guide::user::grammar` | `docs/user/grammar.{zh,en}.md` | `json` |
| user | `guide::user::cli` | `docs/user/cli.{zh,en}.md` | `json` |
| user | `guide::user::config` | `docs/user/config.{zh,en}.md` | `json` |
| developer | `guide::dev::architecture` | `docs/dev/architecture.{zh,en}.md` | none |
| developer | `guide::dev::internals` | `docs/dev/internals.{zh,en}.md` | none |

The assembly is a fixed three-piece shape; `manual!("user/api")` expands to:

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

Both bodies live in the **same page**, and `docs/theme/switcher.html` does three things:

1. a fixed Chinese / English button group in the bottom right corner;
2. two CSS rules decide which body shows: `html:not([data-lang="en"]) [data-lang="en"] { display: none }` and
   `html[data-lang="en"] [data-lang="zh"] { display: none }` — while `data-lang` is still unset the page counts as
   Chinese, so English never flashes first;
3. a script that writes the choice remembered in localStorage back onto `<html data-lang>`, and marks the sidebar
   table of contents (`#rustdoc-toc`) entries of the other language as `hidden`, so the headings are not listed
   twice.

Inside `switcher.html` there are **no blank lines** within `<style>` or `<script>`: a blank line ends the HTML block
in CommonMark and the rest is parsed as Markdown paragraphs — the `--` in CSS turns into an en dash and the styles
stop working (`cargo doc` warns with `nested Markdown paragraph in HTML style tag`).

Why switch in place instead of navigating: `cargo doc` produces one tree, and two sets of documents would mean two
trees, two search indexes, and two sets of relative links. Switching in place needs no navigation and no address
change, anchors stay on the page; the price is two bodies inside one HTML page.

The crate root (`src/lib.rs`) and the `guide` page (`src/guide.rs`) each spell the same `concat!` by hand:
`#![doc = …]` is an inner attribute and has to come before any `macro_rules!`, so `manual!` cannot be used there.

**Coverage**: the eight pages wired with `manual!` are bilingual end to end; item-level `///` docs (things like
`Value::text`) stay Chinese, and those pages carry no switcher, so no button ever does nothing.

**Doctests are doubled**: the Rust blocks in both bodies are compiled and run by `cargo test --doc`, so an API change
means changing both languages; `tests/docs_layout.rs` also compares the number of code blocks in each pair, so
dropping one example from a translation turns it red.

This is not just about writing one copy less: the Rust blocks in the manuals are **doctests**, and
`cargo test --doc` really compiles and runs them. A renamed API, a changed return type, or a different behavior
turns the docs red before it trips up a reader.

rustdoc has its own boundary as well: the crate root and the `//!` / `///` heads of each module address **users** —
what this layer gives you, how to import it, how it behaves — and never mention repository paths, test names, or
design rationale (why nothing is re-exported lives in §3.3). `tests/docs_layout.rs::rustdoc_never_points_at_repository_files`
scans every doc comment line in `src/` and turns red on `src/`, `tests/`, or `Cargo.toml`. If you want to explain an
implementation decision to a human, write it here.

The dividing line is **who the reader is**, not the form of the content (both sides contain Rust):

- **`docs/user/` addresses users**: the `pub` surface only — public entry points, types, behavior, errors, and edges.
  A reader does not need to know the implementation, so no `src/` paths, `tests/` names, or internal identifiers such
  as `$starN` / `is_synthetic` / `from_exprs` appear here. The key tables a configuration author needs are in
  `docs/user/config.zh.md`, and the API a Rust author needs is in `docs/user/api.zh.md`.
- **`docs/dev/` addresses developers of this crate**: layers, modules, constraints, how the sugar is lowered, how the
  tests are arranged, and what has to be changed together. Internal identifiers belong here, and should be used.

`tests/docs_layout.rs` pins the shape down: the top level of `docs/` holds only `README.md` + `intro.*.md` +
`guide.*.md` + `user/` + `dev/` + `theme/`; the eight sources come in pairs (`X.zh.md` always has `X.en.md` and the
other way round; the Chinese one must contain CJK and the English one must not), the two sides have the same number of
code blocks, every source is wired in `src/lib.rs` / `src/guide.rs`, `docs/user/` contains no internal identifiers,
and no previous-version path survives anywhere (flat manuals, the old api/grammar positions under `dev/`, or a
language-less `docs/user/<name>.md`).

---

## 5. Test matrix

```text
$ cargo test                          # core: 49 unit tests + actions + code_language + core_isolation + docs_layout + 12 doc examples (88 total)
$ cargo test --all-features           # plus everything from json / cli (205 total)
$ cargo test --all-features --doc     # only the examples in the docs (50, one set per language)
$ cargo test --no-default-features    # confirms core lives without json (same as the default)
```

Use `--all-features` before committing: a plain `cargo test` never runs the tests marked
`required-features = ["json"]`.

| test file | what it guards |
|---|---|
| `src/core/tests.rs`, `src/json/tests.rs` | unit level: lexical decisions, Earley reductions, value tree shapes, serialization |
| `tests/actions.rs` | action keys, caching, error pass-through |
| `tests/analyzer.rs` | behavior equality of the `Analyzer::new` and `from_config` entry points |
| `tests/grammar_forms.rs` | an executable version of every row in `docs/user/grammar.zh.md` |
| `tests/config_forms.rs` | the schema matches the `PATTERN_KEYS` / `EXPR_KEYS` constants, and the example configurations conform |
| `tests/visitor.rs` | `Flow` semantics, `enter`/`leave` pairing, the collectors |
| `tests/core_isolation.rs` | 3.1–3.3: core self-containment, `regex` as the only dependency, names staying in their layer |
| `tests/code_language.rs` | 3.4: code in English, comments in Chinese |
| `tests/docs_layout.rs` | the docs split, the zh/en pairs and their symmetry, rustdoc assembly, rustdoc not pointing at repository paths |

One non-test check as well: `cargo doc --all-features` must have zero warnings. When an intra-doc link between
documents (say `crate::core::visit::Flow`) breaks, this goes red first.

---

## 6. What to change together

Every manual mentioned below is a pair: change `X.zh.md` and `X.en.md` **together** (changing one turns red the
code-block comparison and the "the English one must not contain Chinese" rule in `tests/docs_layout.rs`). The table
names the Chinese file only; the English one has the same section and the same table.

| you changed | change these too |
|---|---|
| the body of any manual | the other language of the same manual (`X.zh.md` ↔ `X.en.md`): same heading levels, same table rows, same code blocks |
| a grammar / `Expr` key | `schema/config.schema.json`, `EXPR_KEYS`, the comparison table in `docs/user/grammar.zh.md`, the key tables in `docs/user/config.zh.md`, the lowering table in `docs/dev/internals.zh.md`, `tests/grammar_forms.rs` |
| a lexical / `Pattern` key | `schema/config.schema.json`, `PATTERN_KEYS`, `docs/user/grammar.zh.md` §6, `docs/user/config.zh.md` §3 |
| sugar expansion / synthetic symbols | `docs/dev/internals.zh.md` §3, "repetition is flat" in `docs/user/grammar.zh.md` §5, the expected-filter test in `src/core/parser.rs` |
| a configuration field | `CONFIG_KEYS` / `RULE_KEYS` / `GRAMMAR_KEYS` / `PRODUCTION_KEYS`, the schema, `docs/user/config.zh.md` §1–§4 |
| the default configuration | `examples/postscript.json`, the BNF output in `docs/user/config.zh.md` §10, the `--bnf` sample in `docs/user/cli.zh.md` |
| the public API | the examples and tables in `docs/user/api.zh.md` (otherwise the doctests go red), the path table in `src/lib.rs` |
| error messages / CLI output | the help in `docs/user/cli.zh.md` §1 and the error samples in §3, the wording and reference table in `docs/user/config.zh.md` §7/§9, the fragments asserted with `contains("…")` |
| the output format | `src/json/write.rs`, the samples in `docs/user/cli.zh.md` §2, `tests/analyzer.rs` |
| the module structure | the tree in §2 here, the per-file notes in `src/core/mod.rs`, the list in `tests/core_isolation.rs` |
| anything release-related (version, tags, CI checks) | `version` in `Cargo.toml`, `[package.metadata.docs.rs]`, `.github/workflows/{ci,release}.yml`, §8 here |

The criterion is simple: **tests that run green + docs that compile**. Missing either one means the change is not
finished.

---

## 7. Local verification

```text
$ cargo test --all-features
$ cargo test --all-features --doc
$ cargo doc --all-features --no-deps
$ cargo clippy --all-targets --all-features
```

On Windows, incremental compilation occasionally reports `os error 5` (it cannot delete an old `.lock`): rerun with
`CARGO_INCREMENTAL=0`, it is not a code problem.

## 8. Release and CI


The README is written for users and does not cover any of this; this section is the single source of
truth for the release process.

**The first release is manual**: `cargo login` → `cargo publish` → `git tag v0.1.0 && git push origin v0.1.0`.
After that, a release is just a tag, handled by `.github/workflows/release.yml`:

1. bump `version` in `Cargo.toml`, make sure `cargo test --all-features` passes, commit;
2. `git tag v0.1.1 && git push origin v0.1.1`;
3. the pipeline checks that the tag equals the `Cargo.toml` version → runs the tests → builds rustdoc
   warning-free → `cargo publish` → creates a GitHub Release with the `.crate` attached.

The publish job is **re-runnable**: when `cargo publish` reports "already uploaded" (backfilling a
Release for an old version, re-running the pipeline), it prints a warning and exits successfully; and the
GitHub release step checks `gh release view` first. Do not pre-check "is this version published" with
`cargo info` — it resolves the local workspace first and always reports "already exists", blocking every
release.

One-time setup: repo Settings → Secrets and variables → Actions, create `CARGO_REGISTRY_TOKEN`
(a crates.io API token with only `publish-new` and `publish-update` scopes). Alternatively switch to
crates.io Trusted Publishing (OIDC, no stored token): replace the publish step with
`rust-lang/crates-io-auth-action` and add `id-token: write` to the job.

`.github/workflows/ci.yml` runs on PRs and pushes to `master`: the three feature combinations
(default / `--all-features` / `--no-default-features`), clippy, and rustdoc. This repository does not
follow rustfmt's default style, so CI deliberately has no `cargo fmt --check` gate. On docs.rs,
`[package.metadata.docs.rs] all-features = true` makes sure the json / cli manual pages get built too.
