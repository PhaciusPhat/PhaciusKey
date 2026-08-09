# PhaciusKey

## Changing code

Invoke `commenting-only-constraints` and `rust-best-practices` before writing or
editing code, not as a review pass afterwards. Read the skill rather than
working from a memory of it.

## Comments

Do not document code. A comment states a constraint the reader cannot derive
from the code in front of them — an ABI rule, a platform quirk, a spec
citation, a safety precondition — and stops.

Never comment to explain the bug you fixed, to argue a change is correct, or to
say what the code used to do. The regression test is the record of the bug;
`git log` is the record of the change.

Full rule and examples: `.claude/skills/commenting-only-constraints/SKILL.md`.

## Rust

Follow `.claude/skills/rust-best-practices/` (Apollo GraphQL's handbook, vendored
with its nine reference chapters).

`Engine::process`, `Engine::backspace` and everything they reach run inside a
keyboard-hook callback on every keystroke. macOS disables an event tap whose
callback overruns; Windows removes a hook past `LowLevelHooksTimeout`. Do not
clone or allocate on that path.

Lint levels are set in the workspace `[lints]` table, so `cargo clippy` locally
enforces what CI does. `unwrap`/`expect` are denied outside `cfg(test)`.

## Checks

```sh
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
```
