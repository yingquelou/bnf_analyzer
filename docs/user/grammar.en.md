# Writing a grammar: JSON and Rust

For people who **use this library to define a language**. The same grammar has two spellings, and they produce
exactly the same result:

| form | used for | entry point | feature |
|---|---|---|---|
| **JSON objects** | `pattern` and `rhs` inside a configuration file | `Config` → `Analyzer::from_config` | `json` |
| **Rust code** | using the crate as a dependency | `Grammar::builder()` → `Analyzer::new` | `core` (default) |
| BNF text | a **read-only view**: the output of `Grammar::to_bnf()` / `bnf-analyzer --bnf` | not an input format | — |

This page compares the two sides item by item and explains how to read the BNF view. For the full meaning of every
key, how lexical priorities are decided, the schema, and the error reference, see `docs/user/config.zh.md`; for the
rest of the library API (actions, `Value`, visitors, errors) see `docs/user/api.zh.md`. You only need `docs/dev/`
if you are changing this crate itself.

> Everything JSON-related (`Config`, `from_config`, the `--bnf` output) lives behind the `json` feature;
> a plain `cargo add bnf_analyzer` gives you core only. The Rust snippets on this page use core alone, so
> `cargo test --doc` runs them with the default features too.

---

## 1. Keys and Rust constructors

In JSON you deal with key names; in Rust you deal with constructors — both are the same shape:

| JSON key | Rust (`Pattern` / `Expr`) | used for |
|---|---|---|
| `regex` | `Pattern::regex("…")` | only the lexical layer has raw regexes |
| `literal` | `Pattern::literal("{")` / `Expr::literal("{")` | escaped automatically on the lexical side; derives a lexical rule on the grammar side |
| `chars` / `not_chars` | `Pattern::chars("0-9")` / `Pattern::not_chars("\\s")` | character-class fragments |
| `seq` / `alt` | `Pattern::seq(..)`, `Expr::seq([..])` / `alt` | concatenation / alternation |
| `star` / `plus` / `opt` | `Pattern::star(..)`, `Expr::star(..)`, etc. | zero or more / one or more / zero or one |
| `rule` / `token` | `Expr::rule("value")` / `Expr::token("number")` | only the grammar layer can reference other symbols |

Both sides can be rendered back to JSON (`to_value()`), and `Expr` can also be rendered to BNF (that is the route
`to_bnf()` takes).

---

## 2. Three-way comparison: `digit`

### JSON

```json
{
  "name": "digit",
  "rhs": { "alt": [
    { "literal": "0" }, { "literal": "1" }, { "literal": "2" },
    { "literal": "3" }, { "literal": "4" }, { "literal": "5" },
    { "literal": "6" }, { "literal": "7" }, { "literal": "8" },
    { "literal": "9" }
  ] }
}
```

### Rust

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};

let grammar = Grammar::builder()
    .start("digit")
    .production(
        "digit",
        // ten alternatives -> Expr::alt; every "0" -> Expr::literal
        Expr::alt(('0'..='9').map(|digit| Expr::literal(digit.to_string()))),
    )
    .build()
    .expect("the grammar should be valid");

assert_eq!(grammar.productions_of("digit").len(), 10);
```

### BNF view (the output of `to_bnf()`, read-only)

```text
digit ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
```

### Piece by piece

| piece in the BNF view | JSON object | Rust |
|---|---|---|
| the left-hand side `digit` | `"name": "digit"` | the first argument of `.production("digit", …)` |
| `::=` | the `"rhs":` key | the second argument of `.production` |
| `\|` (ten alternatives) | `{ "alt": [ … ] }` | `Expr::alt([ … ])` |
| `"0"` (a literal) | `{ "literal": "0" }` | `Expr::literal("0")` |
| the trailing `;` | one production is one entry, no semicolon needed | one `.production` call |

### Plugging it into a sequence: `digit+`

```json
"grammar": {
  "start": "program",
  "productions": [
    { "name": "program", "rhs": { "plus": { "rule": "digit" } } },
    { "name": "digit",   "rhs": { "alt": [
        { "literal": "0" }, { "literal": "1" }, { "literal": "2" },
        { "literal": "3" }, { "literal": "4" }, { "literal": "5" },
        { "literal": "6" }, { "literal": "7" }, { "literal": "8" },
        { "literal": "9" }
    ] } }
  ]
}
```

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};

let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::plus(Expr::rule("digit")))
    .production(
        "digit",
        Expr::alt(('0'..='9').map(|digit| Expr::literal(digit.to_string()))),
    )
    .build()
    .expect("the grammar should be valid");

assert_eq!(grammar.to_bnf().lines().next(), Some("program ::= digit+ ;"));
```

> **Ten alternatives can also live in the tokenizer.** Writing 10 productions for a single-character enumeration
> like `digit` is verbose; the equivalent is to add `{ "name": "digit", "pattern": { "chars": "0-9" } }` to `rules`
> and write `{ "token": "digit" }` in the grammar (in Rust, `Expr::token("digit")`). The only difference is in the
> syntax tree: ten alternatives give a `digit` node wrapping a literal token, while a lexical class gives one
> `digit` token directly.

---

## 3. The left-hand side: production names

| you want to express | JSON | Rust |
|---|---|---|
| define a nonterminal | `{ "name": "digit", "rhs": … }` | `.production("digit", …)` |
| declare the start symbol | `"start": "program"` (omitted means the first one) | `.start("program")` |
| several alternatives for one name | two `productions` with the same name | two `.production("a", …)` calls |
| an empty production (ε) | `"rhs": { "seq": [] }` | `Expr::seq([])` |

- Declaring the same name several times means several alternatives, kept in declaration order; `to_bnf()` merges
  them into `a ::= x | y ;`.
- Letters, digits, `_`, and `-` are allowed in a name (`value-1` is fine).
- When the start symbol is omitted the first production is used; naming something that does not exist is an error
  (`GrammarError::UnknownStart`), and having no production at all is `GrammarError::Empty`.

---

## 4. Three references: `rule` / `token` / `literal`

| meaning | JSON key | Rust | when it is checked |
|---|---|---|---|
| a nonterminal (another production) | `{ "rule": "digit" }` | `Expr::rule("digit")` | when the grammar is built (`UnknownSymbol`) |
| a lexical class (`rules[].name`) | `{ "token": "digit" }` | `Expr::token("digit")` | when the analyzer is assembled (`UnknownTokenClass`) |
| a literal terminal | `{ "literal": "{" }` | `Expr::literal("{")` | no declaration needed; the lexical rule is derived |

The three **never infer one another**: `rule` does not fall back to a lexical class, and `token` does not look for a
production. So a problem like "I defined `number` but digits do not match" is almost always a `rule` written where
`token` was meant.

```rust
use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::core::rules::{Rule, Rules};
use bnf_analyzer::core::semantics::Semantics;

let rules = Rules::from_rules(vec![
    Rule::skipping_regex("space", r"\s+"),
    Rule::regex("number", r"\d+"),
])
.expect("the rules should be valid");

// `number` is both a production name and a lexical class name; each reference stays on its own side.
let grammar = Grammar::builder()
    .start("program")
    .production("program", Expr::token("number"))   // lexical class: accepts 42
    .production("number", Expr::literal("n"))       // production: accepts n
    .build()
    .expect("the grammar should be valid");

let analyzer = Analyzer::new(rules, grammar, Semantics::new()).expect("the configuration should be consistent");
assert!(analyzer.analyze("42").is_ok());
assert!(analyzer.analyze("n").is_err());
```

The conventions for **literals**:

- no need to declare them in `rules`: `Grammar::literals()` hands them to `Rules::with_literals`, which adds the
  rules for you with the fixed `LITERAL_PRIORITY = 1000`, so a keyword is not swallowed by an identifier rule at
  the same length.
- the `kind` of a literal token is the text itself (`"{"`, `"<<"`), and an action can be registered under that text
  too.
- multi-character literals (`"<<"`, `"ifelse"`) match as a whole and need no splitting.

---

## 5. Combinators

| form | JSON | Rust |
|---|---|---|
| sequence | `{ "seq": [ … ] }` | `Expr::seq([a, b])` |
| alternation | `{ "alt": [ … ] }` | `Expr::alt([a, b])` |
| zero or more | `{ "star": … }` | `Expr::star(a)` |
| one or more | `{ "plus": … }` | `Expr::plus(a)` |
| zero or one | `{ "opt": … }` | `Expr::opt(a)` |
| grouping | nest directly: `{ "seq": [ { "alt": [ … ] }, c ] }` | `Expr::seq([Expr::alt([a, b]), c])` |
| empty (ε) | `{ "seq": [] }` | `Expr::seq([])` |

**The structure is explicit**: no precedence table, no associativity to remember — which of `seq` and `alt` is
inside which is visible from the nesting. `{ "alt": [] }` is rejected.

**A suffix binds one symbol only**: `( word "," )?` needs `opt` around the `seq`;
`{ "seq": [{ "rule": "word" }, { "opt": { "literal": "," } }] }` means something else (only the comma is
optional).

**Repetition is flat in the syntax tree**: `star` / `plus` / `opt` are only sugar, and children are flattened on
reduction, so `children` holds the elements themselves with no extra wrapper node; `to_bnf()` shows `*` / `+` / `?`
as well. (How they expand into ordinary productions internally is covered in `docs/dev/internals.zh.md`.)

---

## 6. `pattern`: which regex a key compiles to

`rules[].pattern` is an object with **exactly one key**, and `Pattern::to_regex()` is the only exit:

| you want to match | JSON | Rust | compiled to |
|---|---|---|---|
| a piece of raw regex | `{ "regex": "\\s+" }` | `Pattern::regex(r"\s+")` | `\s+` |
| a left brace | `{ "literal": "{" }` | `Pattern::literal("{")` | `\{` |
| decimal digits | `{ "chars": "0-9" }` | `Pattern::chars("0-9")` | `[0-9]` |
| anything but whitespace | `{ "not_chars": "\\s" }` | `Pattern::not_chars(r"\s")` | `[^\s]` |
| an identifier | `{ "seq": [{ "chars": "a-zA-Z_" }, { "star": { "chars": "a-zA-Z0-9_" } }] }` | `Pattern::seq([...])` | `[a-zA-Z_](?:[a-zA-Z0-9_])*` |
| one of two keywords | `{ "alt": [{ "literal": "if" }, { "literal": "else" }] }` | `Pattern::alt([...])` | `if\|else` |

```rust
use bnf_analyzer::core::pattern::Pattern;
use bnf_analyzer::core::rules::Rule;

// `{ "name": …, "pattern": … }` in JSON deserializes into `Rule` (needs the json feature).
// Here the same object is built in Rust to check what the pattern keys compile to.
let number = Rule::new("number", Pattern::plus(Pattern::chars("0-9")));
assert_eq!(number.pattern.to_regex(), "(?:[0-9])+");

// `{ "literal": "{" }`: metacharacters go through regex::escape, no backslashes to write yourself.
let brace = Rule::literal("brace", "{");
assert_eq!(brace.pattern, Pattern::literal("{"));
```

- the contents of `chars` / `not_chars` are **regex character-class fragments**, so `]`, `^`, and `\` need escaping
  by hand; `literal` goes through `regex::escape`, so metacharacters are free.
- advanced syntax (look-around, named groups, the inline flag `(?s)`) can only be written as a raw `regex`.
- every combinator wraps its parts in a non-capturing group `(?:…)`, so precedence cannot be swallowed by an outer
  operator.

---

## 7. Actions: only on the Rust route

The JSON route (the binary) carries no actions; it prints the default syntax tree. To compute values during
reduction, use the crate as a library (for the caching semantics and the error types see `docs/user/api.zh.md`):

| action | attached to which kind of symbol | key | example |
|---|---|---|---|
| token action | `{ "token": "digit" }` | the class name | `.token("digit", …)` |
| token action | `{ "literal": "{" }` | the literal text | `.token("{", …)` |
| rule action | any production | the production name | `.rule("program", …)` |

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
    .production("program", Expr::seq([Expr::literal("("), Expr::token("number"), Expr::literal(")")]))
    .build()
    .expect("the grammar should be valid");

let semantics = Semantics::new()
    .token("number", |token| {
        token
            .text
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|error| error.to_string())
    })
    // a literal can take an action too; the key is its text
    .token("(", |_| Ok(Value::text("open")))
    .token(")", |_| Ok(Value::text("close")))
    .rule("program", |values| Ok(Value::list(values.to_vec())));

let analyzer = Analyzer::new(rules, grammar, semantics).expect("the configuration should be consistent");
let document = analyzer.analyze("(7)").expect("the source should be valid");
assert_eq!(
    document.root,
    Value::list(vec![Value::text("open"), Value::Int(7), Value::text("close")])
);
```

Worth knowing:

- actions are cached per **reduction point**: a token action runs once per matched token, a rule action once per
  reduction; callbacks should be free of side effects (do not accumulate external state inside them).
- a production with a rule action no longer produces a `Value::Rule` node; `children` receives whatever the action
  returned.
- an action returning `Err(String)` becomes `ParseError::RuleAction` / `TokenAction` with the failing position.

---

## 8. Write JSON first, move it into code later

`Config` → `Grammar` produces exactly the same CFG as `Grammar::builder()`, so you can prototype a grammar in JSON
and move it into Rust once it settles (or the other way around).

```rust
use bnf_analyzer::core::grammar::{Expr, Grammar};
use bnf_analyzer::json::Config;

let from_json = Config::parse(
    r#"{
        "version": 2,
        "rules": [],
        "grammar": { "productions": [
            { "name": "program", "rhs": { "seq": [
                { "literal": "{" },
                { "star": { "rule": "value" } },
                { "literal": "}" }
            ] } },
            { "name": "value", "rhs": { "alt": [
                { "token": "number" },
                { "rule": "value" }
            ] } }
        ] }
    }"#,
)
.expect("the configuration should be valid")
.grammar
.parse_grammar()
.expect("the grammar is consistent, token classes are checked at setup");

let from_code = Grammar::builder()
    .start("program")
    .production(
        "program",
        Expr::seq([
            Expr::literal("{"),
            Expr::star(Expr::rule("value")),
            Expr::literal("}"),
        ]),
    )
    .production("value", Expr::alt([Expr::token("number"), Expr::rule("value")]))
    .build()
    .expect("the grammar should be valid");

assert_eq!(from_json.to_bnf(), from_code.to_bnf());
```

One reminder: `Grammar::to_bnf()` is a **read-only view** (`rule` to a bare name, `token` to `@name`), meant for
humans and review; it cannot be read back as a configuration, because a configuration only accepts structured
objects.
