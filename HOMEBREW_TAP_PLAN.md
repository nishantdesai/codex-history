# HOMEBREW_TAP_PLAN.md

## Objective

Distribute `codex-history` through the Homebrew tap hosted at:
- `github.com/nishantdesai/homebrew-tap`

The tap should install release binaries produced by the main `codex-history` repository.

## Recommended model

Use a separate main repo for the tool and a separate tap repo for the Homebrew formula.

### Main repo responsibilities
- build release archives
- publish GitHub Releases
- publish SHA256 checksums
- document install and upgrade steps

### Tap repo responsibilities
- contain `Formula/codex-history.rb`
- reference release archive URLs
- embed release SHA256 values
- include a minimal `test do` block

## Expected release artifacts

Per release, publish at least:
- macOS arm64 archive
- macOS x86_64 archive if supported
- checksums file

Optional later:
- Linux tarballs

## Naming recommendation

Archive naming pattern example:

```text
codex-history-v0.1.0-aarch64-apple-darwin.tar.gz
codex-history-v0.1.0-x86_64-apple-darwin.tar.gz
```

Each archive should contain:
- `codex-history` binary
- optionally `README.md`, `LICENSE`

## Homebrew formula expectations

Create formula:
- `Formula/codex-history.rb`

Suggested shape:
- `desc`
- `homepage`
- `url`
- `sha256`
- `license "Apache-2.0"`
- `def install`
- `test do`

### Expected install block
The formula should install the `codex-history` binary into `bin`.

### Expected test block
Use a lightweight test such as:

```ruby
system "#{bin}/codex-history", "--version"
```

## Release workflow expectations in main repo

The main repo should have CI or release automation that:
- builds release binaries
- archives them
- computes SHA256 checksums
- publishes GitHub Release assets

## Manual tap update flow

For each new release:
1. publish release in main repo
2. copy release archive URL(s)
3. compute or copy SHA256
4. update `Formula/codex-history.rb` in `homebrew-tap`
5. commit and push tap repo changes
6. validate fresh install locally with `brew install codex-history`

## Suggested formula skeleton

```ruby
class CodexHistory < Formula
  desc "Read-only CLI for locally accessible Codex session history"
  homepage "https://github.com/nishantdesai/codex-history"
  url "https://github.com/nishantdesai/codex-history/releases/download/v0.1.0/codex-history-v0.1.0-aarch64-apple-darwin.tar.gz"
  sha256 "REPLACE_ME"
  license "Apache-2.0"

  def install
    bin.install "codex-history"
  end

  test do
    system "#{bin}/codex-history", "--version"
  end
end
```

## Notes for Codex when building release packaging

- keep binary name exactly `codex-history`
- ensure release archives unpack cleanly
- avoid nested archive layouts that complicate `bin.install`
- keep version output stable for `test do`
- include macOS release target first because Homebrew tap is the immediate target

## Acceptance criteria

The tap plan is complete when:
- main repo can produce release archives
- checksums are available
- `homebrew-tap` formula installs binary successfully
- `brew test codex-history` passes locally
