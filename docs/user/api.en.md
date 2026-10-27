# Library API: using bnf_analyzer as a dependency

For **users of this library**: add `bnf_analyzer` to `Cargo.toml`, then describe a grammar, attach actions, and walk the
result from Rust. This page covers entry points, actions, `Value` / `Document`, visitors, errors, and the edges —
all of it `pub`, with no need to read the implementation.

Other manuals on the same side: the command line is in `docs/user/cli.zh.md`, the meaning of each configuration key
is in `docs/user/config.zh.md`, and how a grammar maps between JSON and Rust is in `docs/user/grammar.zh.md`.
You only need `docs/dev/` if you are changing this crate itself (layers, modules, tests).

> Every Rust block on this page is a doctest, and `cargo test --all-features --doc` really compiles and runs them;
> the API names, return types, and described behavior here are not allowed to merely "look about right".
>
> Two layers: `Analyzer::new` belongs to core (the default feature); `Config` and `from_config` belong to the
> `json` feature. A plain `cargo add bnf_analyzer` gives you core only.

## 0. Paths: the crate root re-exports nothing

Public names stay in their own modules; the crate root only declares `core` / `json` / `guide`. The examples here
always spell the full path; in your own project, import what you need or alias it:

```rust
use bnf_analyzer::core as ak;            // give the whole of core a short name: ak::Analyzer
use bnf_analyzer::core::value::Value;    // or import just the one name
use bnf_analyzer::core::grammar::Expr;   // and alias again when names collide

let _ = ak::Document::new(Value::text("x"));
let _ = Expr::Literal("{".to_string());
```

This is deliberate: names like `Grammar`, `Value`, `Rule`, and `Config` are far too common for a library to claim
at the root — if the root re-exported them, the same type would hang under two paths in the docs, and a reader
would always have to guess which one to use.

---

## 1. Two entry points

| entry point | input | actions | feature | typical use |
|---|---|---|---|---|
| `Analyzer::new(rules, grammar, semantics)` | rules + grammar + action table | any | `core` | the main library route |
| `Analyzer::from_config(config)` | one `Config` | none | `json` | configuration driven, prints the default syntax tree |

The builtin configuration is not "another analyzer", just a configuration: `Config::builtin()` returns the
`postscript.json` embedded at compile time, which is then handed to `from_config`. One entry point less means one
less place where "which configuration is this analyzer actually using" can be ambiguous.

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::json::Config;

// the builtin configuration: no file read, embedded into the library at compile time
let builtin = Analyzer::from_config(Config::builtin()).expect("the builtin configuration should be consistent");
assert!(builtin.analyze("1 2").is_ok());

// the same configuration spelled out: a Config can come from a string or a file
let config = Config::parse(
    r#"{
        "version": 2,
        "rules": [
            { "name": "space", "pattern": { "plus": { "chars": "\\s" } }, "skip": true },
            { "name": "number", "pattern": { "plus": { "chars": "0-9" } } }
        ],
        "grammar": { "productions": [
            { "name": "program", "rhs": { "star": { "token": "number" } } }
        ] }
    }"#,
)
.expect("the configuration should be valid");
let from_config = Analyzer::from_config(config).expect("the configuration should be consistent");
assert_eq!(from_config.analyze("1 2 3").expect("analysis should succeed").token_count(), 3);
```

The first thing `Analyzer::new` does is check itself: every lexical class referenced by the grammar must exist in
`rules` (see `UnknownTokenClass` in section 6), and literal terminals in the grammar get their lexical rules added
automatically. `from_config` just assembles `rules` + `grammar` and then calls `new`, so both entry points perform
exactly the same checks.

---

## 2. The pipeline can be taken apart

`analyze` is only `tokenize` + `parse`. All three steps can be called separately, and `Tokenizer` / `Parser` are
public types that work without an `Analyzer`.

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::tokenizer::Token;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::token("number")))
    .build()
    .expect("the grammar should be valid");

let analyzer = Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent");

// 1 tokenize only
let tokens: Vec<Token> = analyzer.tokenize("1 2 3").expect("tokenization should succeed");
assert_eq!(tokens.len(), 3);
assert_eq!(tokens[1].text, "2");
assert_eq!(tokens[1].kind, "number");

// 2 parse only (your own token stream works too, as long as kind / text / span line up)
let value = analyzer.parse(&tokens).expect("parsing should succeed");
assert_eq!(value.rule_name(), Some("program"));

// 3 all the way in one step
let document = analyzer.analyze("1 2 3").expect("analysis should succeed");
assert_eq!(document.token_count(), 3);
```

What taking it apart buys you: in an incremental setting you can rerun only the parse; when debugging the tokenizer
you do not have to satisfy the grammar first; and `analyzer.rules()` / `analyzer.grammar()` hand back the rules and
grammar currently in effect.

---

## 3. Actions: turning the syntax tree into values

Both kinds of action are keyed by a string and return `Result<Value, String>`:

| action | when it runs | key | how to register |
|---|---|---|---|
| token action | after a terminal matches | the lexical class name, or the literal text | `Semantics::token` |
| rule action | when a production is reduced | the left-hand side of the production | `Semantics::rule` |

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("number")))
    .production("number", Expr::token("number"))
    .build()
    .expect("the grammar should be valid");

let semantics = Semantics::new()
    // token action: text to integer
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| format!("not an integer: {error}"))
    })
    // rule action: pull the single child value out of the `number` node
    .rule("number", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("number should produce exactly one value".to_string()),
    })
    // rule action: sum them up
    .rule("program", |values| {
        let mut total = 0;
        for value in values {
            total += value.as_int().ok_or("integers only")?;
        }
        Ok(Value::Int(total))
    });

let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
let document = analyzer.analyze("1 2 3").expect("analysis should succeed");
assert_eq!(document.root, Value::Int(6));
```

The key points:

- **actions are cached**: the action for one reduction runs once, so repeated traversal does not call it again.
  Treat callbacks as pure functions: do not accumulate external state inside them.
- **an action replaces the default node**. The `program` above has a rule action, so the root is `Value::Int(6)`
  rather than a `Value::Rule`; the same goes for the `number` node.
- **literals can take actions too**, keyed by their text (`"{"`, `"<<"`), because the `kind` of a literal token is
  that text.
- an action returning `Err(String)` becomes `ParseError::TokenAction` / `ParseError::RuleAction` with the failing
  position; the `String` goes into the error message verbatim.

---

## 4. `Value` and `Document`

`Value` is both the input and output of actions and the node type of the default syntax tree.

| variant | meaning | JSON serialization |
|---|---|---|
| `Null` | nothing | `null` |
| `Bool(bool)` | boolean | `true` / `false` |
| `Int(i64)` | integer | a number |
| `Real(f64)` | float | a number |
| `Text(String)` | text | a string |
| `List(Vec<Value>)` | list | an array |
| `Map(BTreeMap<String, Value>)` | dictionary | an object (keys in order) |
| `Token(Token)` | a leaf that was not rewritten | `{"kind":…,"text":…,"span":…}` |
| `Rule(RuleNode)` | a default syntax tree node | `{"kind":"rule","rule":…,"span":…,"children":[…]}` |

The accessors all return `Option`: `as_bool`, `as_int`, `as_real`, `as_text`, `as_list`, `as_map`, `as_token`,
`as_rule`; there are also `rule_name()` (the node name, or the token class), `type_name()` (the variant name used in
error messages, such as `integer`), `children()`, `span()`, and `find_rule(name)`.

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::value::Value;

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { dup }").expect("analysis should succeed");

// the root node is program, and a procedure can be found below it
let root = document.rule().expect("without actions the root is a rule node");
assert_eq!(root.rule, "program");
assert!(document.root.find_rule("procedure").is_some());

// pre-order: both nodes and tokens pass through
let mut rules = 0;
document.walk(|value| {
    if value.as_rule().is_some() {
        rules += 1;
    }
});
assert!(rules >= 3, "program, value and procedure give at least three nodes");

// serialize directly; the fields match the command line output
let json = document.to_json();
assert!(json.starts_with(r#"{"kind":"rule","rule":"program""#));
```

`Document` is what `analyze` returns; it has a single field `root` and everything else is a convenience method:
`rule()`, `walk()`, `token_count()`, `accept()`, `rules_named()`, `tokens()` (core), plus `to_json()` and
`to_json_pretty()` (needs the `json` feature, returns `String`).

`walk` is the laziest layer: the closure gets a `&Value` and you move on. Its three limits — no pruning, no
distinction between nodes and tokens, and references that cannot leave the closure — are exactly what the visitor
in the next section solves.

---

## 5. Walking afterwards: visitors

`Document::accept(&mut visitor)` (or `Value::accept`) hands the recursion to the library and leaves "what to do with
what you see" to you. None of the four `Visitor` callbacks is **required**, so an empty `struct` implements it:

| callback | when | returns |
|---|---|---|
| `visit(&Value)` | **every** value, before that value's specific callback | `Flow` |
| `enter_rule(&RuleNode)` | entering a `Value::Rule` (after `visit`) | `Flow` |
| `leave_rule(&RuleNode)` | leaving a `Value::Rule`, all children done | nothing |
| `visit_token(&Token)` | meeting a `Value::Token` leaf | nothing |

The return value [`Flow`](crate::core::visit::Flow) decides whether to go deeper:

| `Flow` | meaning |
|---|---|
| `Continue` | descend into the children as usual |
| `Skip` | skip the **children** of this value; the callbacks for this value still happen |
| `Stop` | end the whole walk immediately; nodes already entered still receive `leave_rule` |

`enter_rule` / `leave_rule` always come in pairs (including nodes skipped by `Skip`, and nodes still on the stack
when `Stop` happens), so post-order work can live in `leave_rule`.

### 5.1 Pruning: only look at tokens outside `procedure`

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::tokenizer::Token;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::core::visit::{Flow, Visitor};

#[derive(Default)]
struct OutsideProcedures(Vec<String>);

impl<'ast> Visitor<'ast> for OutsideProcedures {
    // `visit` runs before entering a node, so returning Skip here keeps us out of procedure's children.
    fn visit(&mut self, value: &'ast Value) -> Flow {
        match value.as_rule().map(|node| node.rule.as_str()) {
            Some("procedure") => Flow::Skip,
            _ => Flow::Continue,
        }
    }

    fn visit_token(&mut self, token: &'ast Token) {
        self.0.push(token.text.clone());
    }
}

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { 1 2 } /g").expect("analysis should succeed");
let mut visitor = OutsideProcedures::default();
assert_eq!(document.accept(&mut visitor), Flow::Continue);

assert!(visitor.0.contains(&"/f".to_string()), "/f before procedure is still visited");
assert!(visitor.0.contains(&"/g".to_string()), "/g on the same level is still visited");
assert!(!visitor.0.contains(&"1".to_string()), "tokens inside the braces are pruned");
```

### 5.2 Post-order: compute when leaving a node

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::value::RuleNode;
use bnf_analyzer::core::visit::{Flow, Visitor};

/// Find the deepest procedure nesting: push on entry, hand the subtree's score to the parent on exit.
#[derive(Default)]
struct Deepest {
    stack: Vec<usize>,
    best: usize,
}

impl<'ast> Visitor<'ast> for Deepest {
    fn enter_rule(&mut self, _node: &'ast RuleNode) -> Flow {
        self.stack.push(0);
        Flow::Continue
    }

    fn leave_rule(&mut self, node: &'ast RuleNode) {
        let mut count = self.stack.pop().expect("enter and leave must pair up");
        if node.rule == "procedure" {
            count += 1;
        }
        self.best = self.best.max(count);
        if let Some(parent) = self.stack.last_mut() {
            *parent += count;
        }
    }
}

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/f { { } }").expect("analysis should succeed");
let mut visitor = Deepest::default();
document.accept(&mut visitor);
assert_eq!(visitor.best, 2, "procedure nested inside procedure");
assert!(visitor.stack.is_empty(), "the root is left as well");
```

### 5.3 The collectors cover the common queries

A `&'ast Value` borrowed from the result can be stored directly inside the visitor (`'ast` in `Visitor<'ast>` is
the lifetime of that result), which is how `RuleCollector` / `TokenCollector` are built; they are also exposed as
convenience methods:

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::visit::{RuleCollector, TokenCollector};

let analyzer = Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent");
let document = analyzer.analyze("/a { 1 } /b { 2 }").expect("analysis should succeed");

// convenience methods
assert_eq!(document.rules_named("procedure").len(), 2);
assert_eq!(document.tokens().len(), document.token_count());

// the same thing with a hand-written collector, which you can then extend with your own callbacks
let mut rules = RuleCollector::new("procedure");
document.accept(&mut rules);
let mut tokens = TokenCollector::new();
document.accept(&mut tokens);
assert_eq!(rules.len(), 2);
assert_eq!(tokens.len(), document.token_count());
```

### 5.4 Values produced by actions are visitable too

The visitor makes no assumption that "the result is a tree": `Value::List` / `Map` / scalars all pass through
`visit`, they just never trigger `enter_rule` / `visit_token`. So the same visitor walks the default syntax tree and
a value folded by actions.

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::core::visit::{Flow, Visitor};

#[derive(Default)]
struct Sums(Vec<i64>);

impl<'ast> Visitor<'ast> for Sums {
    fn visit(&mut self, value: &'ast Value) -> Flow {
        if let Some(number) = value.as_int() {
            self.0.push(number);
        }
        Flow::Continue
    }
}

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("number")))
    .production("number", Expr::token("number"))
    .build()
    .expect("the grammar should be valid");
let semantics = Semantics::new()
    .token("number", |token| {
        token.text.parse::<i64>().map(Value::Int).map_err(|e| e.to_string())
    })
    .rule("number", |values| match values {
        [value] => Ok(value.clone()),
        _ => Err("number produces exactly one value".to_string()),
    })
    .rule("program", |values| {
        let mut total = 0;
        for value in values {
            total += value.as_int().ok_or("integers only")?;
        }
        Ok(Value::Int(total))
    });

let document = Analyzer::new(rules, grammar, semantics)
    .expect("the configuration should be consistent")
    .analyze("1 2 3")
    .expect("analysis should succeed");
assert_eq!(document.root, Value::Int(6));

let mut sums = Sums::default();
document.accept(&mut sums);
assert_eq!(sums.0, [6], "the Int at the root is visited too");
```

### 5.5 Two-pass analysis: structure to the parser, conditions to a second pass

Some conditions cannot be expressed by any single local shape: "a `use` must reference a name that
was already `def`-ined", "no duplicate definitions inside one procedure" — the decision needs
information from the whole document (a symbol table, definition order, duplicates). Such conditions
belong neither in `priority` (that decides same-length competition) nor in an action (a reduction
only sees its own arguments). The recommended pattern is **two passes**: parse into a tree and
collect the definitions, then check every use against what was collected, aggregating violations
with their positions into one diagnostic list.

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;
use bnf_analyzer::core::Analyzer;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("name", r"[a-z]+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("statement")))
    .production("statement", Expr::alt([Expr::rule("def"), Expr::rule("use")]))
    .production("def", Expr::seq([
        Expr::literal("def"),
        Expr::token("name"),
        Expr::token("number"),
    ]))
    .production("use", Expr::seq([Expr::literal("use"), Expr::token("name")]))
    .build()
    .expect("the grammar should be valid");
let document = Analyzer::new(rules, grammar, Semantics::new())
    .expect("the configuration should be consistent")
    .analyze("def width 10 use width use height")
    .expect("analysis should succeed");

// Pass one: collect every defined name.
let mut defined = std::collections::HashSet::new();
for node in document.rules_named("def") {
    if let Some(token) = node.children[1].as_token() {
        defined.insert(token.text.as_str());
    }
}

// Pass two: re-check every use against the definition table.
let mut undefined = Vec::new();
for node in document.rules_named("use") {
    let Some(token) = node.children[1].as_token() else { continue };
    if !defined.contains(token.text.as_str()) {
        undefined.push(token.text.as_str());
    }
}
assert_eq!(undefined, ["height"], "only height is never defined");
```

The two passes do not depend on order: a `use` may well appear before its `def`; once the collection
pass is done, the check covers every use. Tokens carry a `Span`, so collecting `&Token` instead of
names gives you `(name, line, column)` — exactly what an IDE diagnostics panel or a batch report
wants, and friendlier than an action `Err`, which stops at the first violation.

Note the division of labor: **the parser only answers "is the structure valid"** (`use height` is a
valid statement even if `height` is never defined); "is the condition satisfied" is answered by your
second pass. This mirrors how the Rust compiler is layered — parsing is a pure CFG, name resolution
comes afterwards. A semantic predicate that would "pick a different parse when the condition fails"
(ANTLR's `{...}?`) is deliberately not provided: predicates depending on semantic values break
Earley's complexity and determinism assumptions. For the overwhelming majority of cases, "parse plus
a second pass" is the standard solution.

**Which one to pick**: for "just look at every value" use `walk`; for pruning, post-order work, carrying node
references out, or telling nodes from leaves, use `accept` + `Visitor`.

---

## 6. Errors: layers and positions

| type | stage | main variants |
|---|---|---|
| `JsonError` | parsing JSON text | `line` / `column` / `message` (a struct, not an enum) |
| `ConfigError` | reading a configuration | `Io`, `Json` (wrapping a `JsonError`), `Invalid`, `UnsupportedVersion`, `LegacyVersion` |
| `RulesError` | compiling lexical rules | `Regex`, `DuplicateName`, `EmptyPattern`, `EmptyName` |
| `GrammarError` | building / checking a grammar | `UnknownStart`, `UnknownSymbol`, `UnknownTokenClass`, `Empty` |
| `SetupError` | `Analyzer::new` | passes through `RulesError` / `GrammarError` |
| `LexError` | tokenizing | `Unexpected { ch, span }`, `EmptyMatch` |
| `ParseError` | parsing | `UnexpectedToken`, `UnexpectedEof`, `TokenAction`, `RuleAction` |
| `AnalyzeError` | what `analyze` returns | `Lex` / `Parse` |

The two configuration stages are clearly separated: `JsonError` means "this text is not valid JSON" (produced by
the JSON grammar in the `json` module), while `ConfigError::Invalid` means "valid JSON, wrong fields" (checked field
by field in `from_value`, with a path such as `rules[0]: …` in the message).

Positions are always available through `span()`: `LexError`, `ParseError`, and `AnalyzeError` all have it, returning
`Span { start, end, line, column }` (byte offsets plus 1-based line and column). Syntax errors can also tell you
what was expected at that point:

```rust
use bnf_analyzer::core::AnalyzeError;

let error = analyzer_error();
fn analyzer_error() -> AnalyzeError {
    bnf_analyzer::core::Analyzer::from_config(bnf_analyzer::json::Config::builtin()).expect("the builtin configuration should be consistent")
        .analyze("/f { dup 1")
        .expect_err("unbalanced parentheses should fail")
}

assert_eq!(error.span().line, 1);
let expected = match &error {
    AnalyzeError::Parse(parse) => parse.expected(),
    _ => &[],
};
assert!(expected.iter().any(|label| label == "array"), "the expected list only shows symbols you can write");
```

`SetupError` has no span — it describes "the rules or grammar do not hold", which has nothing to do with the source.

All these error types implement `Display` and `std::error::Error`: `source()` on `RulesError` / `ConfigError`
returns the underlying `std::io::Error` or `regex::Error`, and the pass-through wrappers (`AnalyzeError`,
`SetupError`, `ConfigError::Json`) only forward.

---

## 7. Customizing the lexical layer

```rust
use bnf_analyzer::core::rules::{Rule, Rules, LITERAL_PRIORITY};

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::skipping_regex("comment", r"%[^\n]*"),
    Rule::literal("lbrace", "{"),                 // metacharacters escaped for you
    Rule::regex("number", r"\d+").with_priority(10),
])
.expect("the rules should be valid");

assert_eq!(rules.entries().len(), 4);
assert_eq!(rules.rule("lbrace").expect("that rule should exist").priority, 0);
assert!(LITERAL_PRIORITY > 10, "literal rules derived from the grammar outrank ordinary rules");
```

The decision order is: **longest match**, then larger `priority`, then earlier declaration. Literal terminals in
the grammar (`"{"`, `"ifelse"`) are turned into rules by `Rules::with_literals` with the fixed
`LITERAL_PRIORITY = 1000`, so keywords are not swallowed by an identifier rule.

---

## 8. Implementing a format with the library itself: the `json` module

The `bnf_analyzer::json::grammar` module is a complete example — to write a parser for another format (a YAML subset,
S-expressions, logs, configuration files…), copy its three pieces:

| piece | code | API |
|---|---|---|
| lexical | `json::grammar::rules()` | `Rules::from_rules` + `Rule` / `Pattern` |
| grammar | `json::grammar::grammar()` | `Grammar::builder()` + `Expr` |
| semantics | `json::grammar::semantics()` | `Semantics::token` / `Semantics::rule` |

```rust
use bnf_analyzer::json;

// 1. parse: JSON text to Value
let value = json::parse(r#"{"n": 42, "xs": [true, null]}"#).expect("valid JSON");
let map = value.as_map().expect("map");
assert_eq!(map["n"].as_int(), Some(42));

// 2. write: Value to JSON text (compact / two-space indentation)
assert_eq!(json::to_string(&value), r#"{"n":42,"xs":[true,null]}"#);
assert!(json::to_string_pretty(&value).starts_with("{\n  \"n\": 42,"));

// 3. the grammar itself is public: read it, or take it and change it
let bnf = json::grammar::analyzer().grammar().to_bnf();
assert!(bnf.contains(r#"value ::= object | array | @string | @number"#), "{bnf}");

// 4. want stricter or looser JSON? bring your own analyzer
let mine = json::grammar::analyzer();
assert_eq!(json::parse_with(&mine, "[1, 2]").expect("should be valid").as_list().unwrap().len(), 2);
```

That grammar looks like this (the output of `to_bnf()`, productions with the same name merged onto one line):

```text
json ::= value ;
value ::= object | array | @string | @number | "true" | "false" | "null" ;
object ::= "{" "}" | "{" members "}" ;
members ::= member | members "," member ;
member ::= @string ":" value ;
array ::= "[" "]" | "[" elements "]" ;
elements ::= value | elements "," value ;
```

A few things worth copying:

- **only three lexical rules**: whitespace (`skip`), strings, numbers. `true`/`false`/`null` and `{ } [ ] , :` are
  all literal terminals, and `Grammar::literals()` derives their lexical rules during assembly.
- **left recursion accumulates the list**: `members ::= member | members "," member` makes the action easier to
  write than `star` does — each reduction only gets "the list so far + one element", with no need to guess offsets
  in a flattened argument sequence.
- **an empty container gets its own production**: `"{" "}"` and `"{" members "}"` have different shapes, so the
  action can tell them apart by the number of arguments.
- **positions for free**: when a token action returns `Err(String)`, the parser attaches that token's span, and
  `json::parse` turns it into `JsonError { line, column, message }`.

`Config` reads through exactly this path: `Config::parse` is `json::parse` plus reading fields out of the `Value`,
so syntax errors and configuration errors in `postscript.json` share one set of positions.

---

## 9. Edges and trade-offs

- **Earley, without rewriting the grammar**: left recursion, ε productions, and ambiguity all work directly. The
  price is a worst case of O(n³), and that **with an ambiguity only the first completed derivation is returned**,
  never an enumeration of every parse.
- **no error recovery**: a syntax error returns the first failure point; there is no "skip a token and carry on".
  If you need multiple errors, analyze line by line or block by block on the caller's side.
- **actions must be pure**: the reduction cache assumes the action for one reduction point runs once and its result
  can be reused.
- **the EOF span is a zero-width range** positioned at the end of the last token; empty input gives `(0,0,1,1)`.
- **strings in the default configuration allow one level of nesting**: `(a (b) c)` works, `(a (b (c)) d)` fails in
  the tokenizer. That is exactly where a "token action" earns its keep — do the bracket matching inside the action,
  or replace `string` with a scanner hook that handles arbitrary depth.
- **`Analyzer` is reusable**: rules and grammar are immutable once assembled, `analyze` shares no mutable state, and
  one instance can analyze many sources in a row; `Parser` itself is a zero-sized, stateless type.
- **visitors are read-only**: a `Visitor` gets `&'ast`. To rewrite the tree, collect `(&Span, …)` during the walk
  and rebuild it yourself, or use `Clone` on `Value` and recurse by hand.
- **traversal is depth-first, in document order**: `Rule` and `List` follow element order; `Map` is a `BTreeMap`,
  so it follows the key order, not insertion order.
- **the dependency surface is tiny**: only one extra crate (`regex`) shows up in `Cargo.lock`, with no JSON library
  and no error-handling framework.
