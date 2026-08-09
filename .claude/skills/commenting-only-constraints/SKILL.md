---
name: commenting-only-constraints
description: Use when writing or editing code and about to add a comment or doc comment, and when reviewing a diff that adds explanatory prose above working code.
---

# Commenting Only Constraints

## Overview

A comment carries one thing: **a constraint the reader cannot derive from the code in front of them.** It names the external fact and stops.

Everything else a comment could say is already recorded somewhere better — the code says what it does, the types say what it takes, the test names say what it guarantees, `git log` says how it got here.

## When to Use

- Before adding any `//` or `///` to code you are writing.
- When a review diff adds prose above lines that already work.
- When you catch yourself explaining a change rather than the code.

## The Contract

A comment you keep has all three properties:

1. **It states a fact from outside the file** — an ABI rule, a platform quirk, a hardware timing, a spec citation, a linguistic rule, a safety precondition.
2. **The next reader cannot recover it** by reading the code, the types, the tests, or the commit.
3. **It survives the merge** — still true and still useful to someone who never saw the pull request.

Fails any one of the three: delete it.

## Quick Reference

| Comment says | Keep? | Because |
|---|---|---|
| `BOOL is signed char on x86_64, _Bool on arm64` | ✅ | ABI fact; not in the code |
| `GetKeyState reads the calling thread's queue; a hook runs elsewhere` | ✅ | Platform quirk; invisible locally |
| `SAFETY: ptr is non-null and aligned by the caller` | ✅ | Precondition for `unsafe` |
| `SMAppServiceStatus, from <ServiceManagement/SMAppService.h>` | ✅ | Spec citation |
| `An earlier version allocated a CString per call` | ❌ | `git log` |
| `This replaced the hand-written plist` | ❌ | `git log` |
| `Without this, "jira" would compose as "jỉa"` | ❌ | That is a test, not a comment |
| `Counted, not collected — this runs per keystroke` | ❌ | The code says `.count()` |
| `// Reset the buffer` above `self.reset()` | ❌ | Restates the line |

## Example

A rule that earns its comment, next to one that does not:

```rust
// ❌ Argues the change is correct — reviewer-facing, dead on merge
// A vowel must come first, which is what keeps real codas typable: the 'h'
// of "anh" and the 'g' of "ong" follow consonants, so without this guard
// they would be rewritten and "anh" would come out as "anh" + "nh".
if is_vowel(before) { ... }

// ✅ Cites the authority for a table you cannot derive by reading
// OpenKey `_doubleWAllowed` (Vietnamese.cpp:383).
const DOUBLE_W_ALLOWED: [&str; 9] = ["tr", "th", "ch", "nh", "ng", "kh", "gi", "ph", "gh"];
```

The first belongs in a test named `quick_end_consonant_needs_a_vowel_before_it`. The second cannot live anywhere else.

## Common Mistakes

- **Explaining the bug you just fixed.** The regression test is the record. Name the test after the bug.
- **A paragraph on a private function.** If the name needs a paragraph, rename the function.
- **Narrating a rewrite** (`rather than`, `used to`, `an earlier version`, `this replaced`). Those words are the tell.
- **Keeping a comment because deleting feels lossy.** If the fact matters, it belongs in a test, a type, or a doc comment on the public API.

## Real-World Impact

A single feature branch in this repository added 2250 lines, **603 of them comments (27%)**. Nearly all of the comment lines argued that the change was correct. The constraints worth keeping — the `BOOL` width, the hook-thread modifier quirk, the event-tap watchdog — were a couple of dozen lines buried among them.
