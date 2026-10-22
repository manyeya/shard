<!--
Thanks for the pull request. Please open it against `dev`, not `main`.
See CONTRIBUTING.md for the details behind each point below.
-->

## Summary

<!-- What does this change, in a sentence or two? Link the issue it fixes: "Fixes #123". -->

## Why

<!-- The problem it solves, or the reason for the approach when it isn't obvious. -->

## How it was tested

<!-- What you ran and what you tried by hand: the window, the tray, the extension, a real download. -->

Tested on:

- [ ] macOS
- [ ] Windows
- [ ] Linux (distribution and desktop: )
- [ ] With the browser extension (browser: )

## Checklist

- [ ] It's against `dev`.
- [ ] `cargo test --locked` passes.
- [ ] `cargo clippy --all-targets --locked` adds no new warnings.
- [ ] Formatting matches the surrounding code, and no whole files were reformatted.
- [ ] New behaviour has tests next to the code, where it can be tested.
- [ ] User-visible changes have a line under "Unreleased" in `CHANGELOG.md`.
- [ ] The README, `docs/` or `site/` are updated where they describe what changed.
- [ ] `Cargo.lock` is committed if dependencies changed.
