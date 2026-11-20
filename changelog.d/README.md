# Changes not yet released

One file per change a user notices (ADR 0065), named after the change (`bluetooth-keyboard.md`), in this form:

```markdown
Fixed:

- What changed, as a user sees it, with a link to the page that tells more
  ([First Steps](wiki/First-Steps.md)), and "(unit)" when it also ran on a scope.
```

The first line is one of `Added:`, `Interface:`, `Removed:`, `Changed:`, `Fixed:`. `tools/changelog.py` prints the
next release's section with these files in it, `tools/changelog.py --check` (layer `text`) checks their form, and
the pull request of a release runs `tools/changelog.py --release X.Y.Z`, which writes them into
[CHANGELOG.md](../CHANGELOG.md) and removes them. Links are written from the top of the tree, as in CHANGELOG.md.
