A configurable source analyzer: one set of lexical rules plus one grammar, turning source text into tokens and then into a value tree that carries positions.

The library knows nothing about any particular language. The bundled `examples/postscript.json` is only a **sample configuration**
(a subset of PostScript); swap in your own `rules` and `grammar` and it analyzes your language instead.

The package is called `bnf_analyzer` (`use bnf_analyzer::…` in Rust); the binary is called `bnf-analyzer`.

# Which features to enable

| feature | what you get | when to enable it |
|---|---|---|
| `core` (default) | tokenizer, grammar, Earley, `Value`, visitors; `Analyzer::new` | `cargo add bnf_analyzer` gives you this |
| `json` | self-hosted JSON reading and writing, `Config`, `Analyzer::from_config` | you want to describe grammars in a configuration file |
| `cli` | the `bnf-analyzer` binary (`--bnf`, `-t`, default syntax tree) | you want to run it straight from the command line |

`cargo add bnf_analyzer` gives you core only, and no binary; add `--features json` for configuration files,
then stack `--features cli` for the command line.

# One: write Rust, grammar in code, actions allowed

```
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+").with_priority(10),
])
.expect("the rules are valid");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::rule("expr"))
    .production(
        "expr",
        Expr::alt([
            Expr::token("number"),
            Expr::seq([Expr::rule("expr"), Expr::literal("+"), Expr::rule("expr")]),
        ]),
    )
    .build()
    .expect("the grammar is valid");

// token action: turn the number text into an integer
// rule action: fold `a + b` into its result
let semantics = Semantics::new()
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| error.to_string())
    })
    .rule("program", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("program produces exactly one value".to_string()),
    })
    .rule("expr", |values| match values {
        [Value::Int(value)] => Ok(Value::Int(*value)),
        [Value::Int(left), Value::Token(op), Value::Int(right)] if op.text == "+" => {
            Ok(Value::Int(left + right))
        }
        _ => Err("cannot evaluate this expression".to_string()),
    });

let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration is consistent");
assert_eq!(analyzer.analyze("1 + 2").expect("the analysis succeeds").root, Value::Int(3));
```

The action table may stay empty: the output is then the default syntax tree with positions, and you never define a value yourself.

# Two: run the command line, grammar in JSON, no actions, default syntax tree

This needs both the `json` (configuration) and `cli` (binary) features:

```text
$ cargo run --features cli -- --bnf        # show the current grammar
$ cargo run --features cli -- sample.ps    # print the syntax tree JSON with positions
$ cargo run --features cli -- -t sample.ps # print the token stream only
```

`pattern` and `rhs` in a configuration accept structured objects only (`{ "regex": … }`, `{ "seq": [ … ] }`);
there is no "parse another piece of text" form. The keys map one to one onto the variants of
[`Pattern`](crate::core::pattern::Pattern) and [`Expr`](crate::core::grammar::Expr). What each key means,
how priorities are decided, and how to read an error are covered in `guide::user::config`.

Both routes end up as the same [`Grammar`](crate::core::grammar::Grammar),
consumed by the same Earley parser.

# Import paths

Every name stays in its own module; the crate root only declares modules:

| you want | path |
|---|---|
| analyzer and result | `bnf_analyzer::core::{Analyzer, Document}` |
| value tree | `bnf_analyzer::core::value::{Value, RuleNode}` |
| visitors | `bnf_analyzer::core::visit::{Visitor, Flow}` |
| grammar | `bnf_analyzer::core::grammar::{Grammar, Expr}` |
| lexical rules | `bnf_analyzer::core::rules::{Rules, Rule}`, `bnf_analyzer::core::pattern::Pattern` |
| JSON reading, writing, configuration | `bnf_analyzer::json::{parse, to_string, Config}` |

Pick your own aliases to keep it short:

```
use bnf_analyzer::core as ak; // alias: write ak::Analyzer from here on
use bnf_analyzer::core::value::Value; // concrete names: import only what you need

let document = ak::Document::new(Value::text("x"));
assert_eq!(document.token_count(), 0);
```

# What to do with the result

[`Document`](crate::core::Document) is the analysis product: `root` is the value tree, `walk` runs a closure over it.
When you need pruning, or "enter and leave are always paired" callbacks, use [`Visitor`](crate::core::visit::Visitor):
`accept` drives the recursion, and `Flow` decides whether to continue, skip a subtree, or stop.

# Documentation map

The manuals are split by reader, both under [`guide`] (the sources live in the repository `docs/`):

| who am I | read | contents |
|---|---|---|
| **user** (depend on this library, or drive the CLI) | `guide::user::api` | library API: entry points, actions, `Value`, visitors, errors |
| | `guide::user::grammar` | how to write a grammar: JSON and Rust forms, BNF view, actions |
| | `guide::user::cli`, `guide::user::config` | command line and output formats, configuration reference |
| **developer of this library** (changing this crate) | [`guide::dev::architecture`] | layers, module tree, hard constraints, test matrix, change checklist |
| | [`guide::dev::internals`] | grammar lowering, synthetic productions, when validation happens |

The four user manuals all read configuration files, so they ship with the `json` feature; the developer manuals only cover core, which is available by default.

## License

MIT OR Apache-2.0, at your option.
