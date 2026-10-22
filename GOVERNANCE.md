# Governance

Shard is a small project with a single maintainer, [@manyeya](https://github.com/manyeya). This
page says how decisions get made today and how that can change.

## The maintainer

The maintainer:

- decides what goes in, reviews and merges pull requests, and fast-forwards `main` to `dev`;
- makes releases and holds the release secrets, including the update signing key;
- triages issues, answers security reports, and enforces the [code of conduct](CODE_OF_CONDUCT.md).

Code owners are listed in [.github/CODEOWNERS](.github/CODEOWNERS).

## How decisions are made

- Most decisions happen in the open, in issues, discussions and pull request reviews. If you
  disagree with one, say so there, with your reasons.
- The maintainer makes the final call. The questions behind it are what Shard is for (downloading
  files fast and reliably, without fuss), whether the change can be kept working on macOS,
  Windows and Linux, and whether it can be maintained by one person.
- A "no" isn't permanent. If circumstances change, or a better approach comes along, bring it
  back.
- Security reports and conduct reports are handled in private, as [SECURITY.md](SECURITY.md) and
  the code of conduct describe.

## Becoming a maintainer

There's no formal process yet. People who keep making good contributions over time (code,
reviews, triage, answers in Discussions, testing on platforms the maintainer doesn't use) may be
asked to help maintain an area, starting with triage rights and growing from there. If you'd like
that, say so in a discussion.

When there is more than one maintainer, this file will say how they decide together and who can
release.

## Changes to this document

Changes to governance are made by pull request, like any other change, and decided by the
maintainer.
