# Command line: source in, JSON out

The command line is the "configuration plus no actions" route: it reads `postscript.json` (or the configuration
given with `-c`) and analyzes the source into a syntax tree that carries positions. It **does not evaluate
anything** — to turn the tree into values, use the crate as a library, see `docs/user/api.zh.md`.

Writing your own configuration is covered in `docs/user/config.zh.md`; this page only covers how to run `bnf-analyzer`
and what its output looks like.

## 0. Getting the binary

The binary is not in the default features; either route works:

```text
$ cargo install --path . --features cli      # install into ~/.cargo/bin, then just `bnf-analyzer ...`
$ cargo run --features cli -- sample.ps      # no install, run it in place
```

For brevity the rest of this page writes `bnf-analyzer ...`; the first form matches the installed command, and for the
second form replace `bnf-analyzer` with `cargo run --features cli --`.

---

## 1. Usage

```text
usage: bnf-analyzer [options] [file]

Analyze a source file with the lexical rules and grammar from a configuration file
and print the result as JSON (the built-in postscript.json is used by default).

arguments:
  <file>                file to analyze; omit it or pass - to read standard input

options:
  -c, --config <file>   use this configuration file (default: built-in postscript.json)
  -t, --tokens          print the token stream only, without parsing
      --bnf             print the current grammar as BNF (read-only view)
      --compact         print compact JSON (two-space indentation by default)
      --encoding <enc>  how to read the file: utf8 (default), latin1, or auto
  -h, --help            print this help
  -V, --version         print version
```

- Options and the file position are independent: `bnf-analyzer sample.ps -t` is the same as `bnf-analyzer -t sample.ps`.
- `-` means standard input; you can also leave the file argument out entirely.
- `--bnf` needs no source: it only renders the grammar and will not read a file you pass.
- The configuration is **embedded at compile time** by default and does not depend on the working directory; only
  `-c` reads from disk.
- `--encoding` decides how bytes become text; the default only accepts UTF-8, see §3.

---

## 2. Three kinds of output

There are three shapes of output, and the field names follow the same conventions as the keys in a configuration
(one `Value` model; `-t` and the default output are just different projections of the same tree).

### 2.1 Default: the syntax tree with positions

For the input `1 2` the output is (with `span` written on one line to save space; by default it is indented by two
spaces with one field per line):

```json
{
  "kind": "rule",
  "rule": "program",
  "span": { "start": 0, "end": 3, "line": 1, "column": 1 },
  "children": [
    {
      "kind": "rule",
      "rule": "value",
      "span": { "start": 0, "end": 1, "line": 1, "column": 1 },
      "children": [
        { "kind": "number", "text": "1",
          "span": { "start": 0, "end": 1, "line": 1, "column": 1 } }
      ]
    },
    {
      "kind": "rule",
      "rule": "value",
      "span": { "start": 2, "end": 3, "line": 1, "column": 3 },
      "children": [
        { "kind": "number", "text": "2",
          "span": { "start": 2, "end": 3, "line": 1, "column": 3 } }
      ]
    }
  ]
}
```

`kind` tells the two node types apart:

| node | fields | note |
|---|---|---|
| syntax tree node | `kind: "rule"`, `rule`, `span`, `children` | `rule` is the left-hand side of the production |
| leaf token | `kind` (the class), `text` (the source text), `span` | for a literal terminal `kind` is the text itself, e.g. `"{"` |

The four `span` fields:

| field | meaning |
|---|---|
| `start` / `end` | **byte** offsets, half-open, ready for slicing |
| `line` / `column` | start position, **1-based**; `column` counts characters, not bytes |

`end` is not an independent position but the end of the range: with multi-byte characters (Chinese, UTF-8),
`end - start` is not the number of characters, so slice by bytes to stay safe.

### 2.2 `-t`: the token stream only

```text
$ printf '1 2' | bnf-analyzer -t --compact
[{"kind":"number","text":"1","span":{"start":0,"end":1,"line":1,"column":1}},{"kind":"number","text":"2","span":{"start":2,"end":3,"line":1,"column":3}}]
```

The output is an array whose elements have the same shape as the leaves inside the tree. `skip` rules (whitespace,
comments) produce no token, which is why the example has only two elements. This route does no syntax checking:
unbalanced brackets still print tokens just fine.

### 2.3 `--bnf`: a read-only view of the current grammar

```text
$ bnf-analyzer --bnf
program ::= value* ;
value ::= @number | @boolean | @string | @hexstring | @name | @immediate | @operator | procedure | array | dictionary ;
procedure ::= "{" value* "}" ;
array ::= "[" value* "]" ;
dictionary ::= "<<" entry* ">>" ;
entry ::= @name value ;
```

Rendering rules:

- a `rule` reference becomes the bare name (`value`);
- a `token` reference becomes `@class` (`@number`);
- a `literal` becomes the quoted text (`"{"`);
- several productions with the same name merge into one line with `|`; an empty right-hand side renders as
  `x ::= ;` (ε);
- helper symbols used inside the grammar **do not appear**; only the names you can actually write in a configuration
  are left.

This text is for humans and review only; it cannot be read back as a configuration, because a configuration only
accepts structured objects.

---

## 3. When something goes wrong

Errors always go to stderr, the exit code is 1, and the offending line is printed with a caret:

```text
$ printf '/f { dup 1' | bnf-analyzer
line 1, column 11: unexpected end of input, expected one of: "<<", "[", "{", "}", array, boolean,
dictionary, hexstring and 7 more
  /f { dup 1
            ^
```

- A lexical error names the exact character: `line 1, column 1: no rule matches character '('`;
- the expected list shows at most 8 items, then `and N more`;
- only symbols the user can see appear in that list — helper productions from inside the grammar are filtered out;
- the caret is aligned by **terminal column**: a tab advances to the next multiple of 8, and wide characters such as
  CJK count as two columns, so indented PostScript snippets still point at the right place;
- PostScript is a byte language and its files are not necessarily UTF-8. The default decodes UTF-8 only, and a
  failure reports the byte offset:

```text
$ bnf-analyzer latin1.ps
failed to read latin1.ps: invalid UTF-8 at byte 4
  pass --encoding latin1 to read it as Latin-1, or --encoding auto to detect
```

  `--encoding latin1` treats each byte as one code point (ISO-8859-1 lines up with the first 256 Unicode code
  points); `--encoding auto` tries UTF-8 first and falls back to Latin-1. Note that the byte offsets in `span` are
  counted on the **decoded text**: a 0x80–0xFF byte becomes two UTF-8 bytes under Latin-1, so offsets differ from
  the positions in the file. Line and column numbers are unaffected.
- configuration errors happen before the source is read, so there is no caret:

```text
$ bnf-analyzer -c old.json -
configuration version 1 is no longer supported: patterns and right-hand sides must be structured
objects. Wrap a regex as { "regex": … }, rewrite BNF text as { "rule": … } / { "token": … } /
{ "seq": [ … ] }, and set version to 2
```

---

## 4. Exit codes

| code | meaning |
|---|---|
| 0 | success (including `-h` / `-V` / `--bnf`) |
| 1 | the file could not be read, the configuration is invalid, a lexical or syntax error, or JSON serialization failed |

There is no partial success: either one complete JSON document is printed, or nothing is.

---

## 5. Common recipes

```text
# analyze a file
bnf-analyzer sample.ps

# read from a pipe, compact output, then hand it to jq
cat sample.ps | bnf-analyzer - --compact | jq '.children | length'

# try a grammar with a different configuration
bnf-analyzer -c examples/minimal.json --bnf
printf '{ 1 /x }' | bnf-analyzer -c examples/minimal.json --compact

# look at the tokenizer only, to check longest match and priorities
printf '1 /a true' | bnf-analyzer -t --compact
```

When you need "analyze and evaluate / build an AST", neither `-t` nor the default output is enough: the command
line route runs no actions. Use the crate as a library and register a `Semantics` as described in
`docs/user/api.zh.md`; it reads the same `Config`.

---

## 6. Where to read next

| you want to | read |
|---|---|
| write your own configuration (another language, more syntax, different priorities) | `docs/user/config.zh.md` |
| turn the tree into values (sum, build a dictionary, evaluate) | `docs/user/api.zh.md` |
| find out which grammar node a key maps to | `docs/user/grammar.zh.md` |
| change this crate itself | `docs/dev/architecture.zh.md` |
