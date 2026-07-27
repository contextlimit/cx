# CX Read-Like Rewrites

CX supports a small set of familiar file-reading commands by routing clear
single-file shapes into `cx read` behavior. These are compatibility rewrites, not
complete reimplementations of Unix utilities.

The supported read-like command names are:

```sh
cx cat <file>
cx head <file>
cx tail <file>
cx sed -n 'START,ENDp' <file>
cx nl <file>
```

The same clear shapes can be used through auto mode:

```sh
cx -- cat <file>
cx -- head -n 40 <file>
cx -- tail -n 40 <file>
cx -- sed -n '120,180p' <file>
cx -- nl -ba <file>
```

Parser-risky, multi-file, pipe-heavy, or unsupported utility shapes fall back to
passthrough when enabled.

## Cat

Clear single-file cat:

```sh
cx -- cat src/lib.rs
```

conceptually converts to:

```sh
cx read src/lib.rs
```

Line-number cat:

```sh
cx -- cat -n src/lib.rs
```

conceptually converts to:

```sh
cx read src/lib.rs --line-numbers
```

CX does not promise to emulate every `cat` flag.

## Head

Default head:

```sh
cx -- head src/lib.rs
```

conceptually converts to a first-lines read window.

Explicit count:

```sh
cx -- head -n 80 src/lib.rs
cx -- head -80 src/lib.rs
```

conceptually converts to:

```sh
cx read src/lib.rs --range 1:80
```

## Tail

Default tail:

```sh
cx -- tail src/lib.rs
```

conceptually converts to a last-lines read window.

Explicit count:

```sh
cx -- tail -n 80 src/lib.rs
cx -- tail -80 src/lib.rs
```

conceptually converts to a read range for the final 80 lines after CX counts the
file lines.

Start-from-line form:

```sh
cx -- tail -n +120 src/lib.rs
```

conceptually converts to a read range starting at line 120.

## Sed Print Ranges

The clear sed range shape:

```sh
cx -- sed -n '120,180p' src/lib.rs
```

conceptually converts to:

```sh
cx read src/lib.rs --range 120:180
```

## Nl

Clear `nl` shapes are intentionally pipe-safe. CX recognizes `nl <file>`,
`nl -ba <file>`, and `nl -b a <file>`, then emits the full file with stable line
numbers instead of applying read compaction. This preserves downstream shell
filters such as:

```sh
cx -- nl -ba src/lib.rs | sed -n '880,920p'
```

For large files where bounded output is preferred, use an explicit range command
instead:

```sh
cx -- sed -n '880,920p' src/lib.rs
cx read src/lib.rs --range 880:920 --line-numbers
```

End-of-file forms such as `120,$p` are accepted for simple print ranges.

CX does not implement arbitrary sed programs, substitutions, address logic, or
editing behavior. Use `cx sh` or passthrough when real sed semantics are needed.

## Nl

Line-number listing:

```sh
cx -- nl -ba src/lib.rs
cx -- nl -b a src/lib.rs
```

conceptually converts to:

```sh
cx read src/lib.rs --line-numbers
```

## Output Contract

Read-like rewrites inherit the `cx read` output contract. They may compact or
truncate according to read settings unless the selected read behavior says
otherwise.

Clear `head`, `tail`, and `sed -n` rewrites use exact range selection. When no
generated/blob guard or line-number transform changes the content, they preserve
the native selected bytes, including CRLF and a missing final newline.

Exact `sed -n` ranges preserve long structured source literals, including Rust
raw strings used as JSON or command templates. Generated/blob tokens remain
bounded by the shared long-line guard. Read-like commands must not force an
agent to switch languages or editing strategies merely to recover a complete
human-authored source line.

The rewrite must not change the target file. It only changes how CX reads and
renders the file.

## Insights Labels

When insights are enabled, these commands are useful at two levels:

- process: `cat`, `head`, `tail`, `sed`, or `nl`
- command family: read-like command family or rewritten read behavior

Future conversion metadata should record the original program and the converted
read window so usage analysis can show how often agents rely on familiar Unix
forms.

## Command Selection Guide

Use direct `cx read` when you need exact CX mode control.

Use read-like commands when you are translating a familiar shell habit into CX.

Use `cx sh` for pipelines, substitutions, command substitutions, or any sed
program more complex than a clear print range.
