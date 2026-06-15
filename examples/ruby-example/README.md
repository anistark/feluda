# Ruby Example Project for Feluda

This is an example Ruby project designed to test Feluda's license analysis
capabilities for Ruby/Bundler projects.

## Dependencies

This project declares a small Sinatra web app whose dependencies pull in several
transitive gems:

- **sinatra** — web framework (brings in `mustermann`, `rack`, `rack-protection`, `rack-session`, `tilt`, `logger`, `base64`)
- **puma** — web server (brings in `nio4r`)
- **nokogiri** — HTML/XML parsing (brings in `mini_portile2`, `racc`)
- **rake** — build tool

## Project Files

- `Gemfile` — direct dependency declarations
- `Gemfile.lock` — resolved lockfile with the full transitive set at exact versions
- `app.rb` — sample code using the dependencies

Feluda prefers `Gemfile.lock` when present: it already contains every resolved
gem (direct and transitive) with exact versions, so no registry resolution is
needed. A project with only a `Gemfile` is parsed best-effort.

## Testing with Feluda

```sh
# From the repository root
feluda --path examples/ruby-example

# Verbose output with OSI status
feluda --path examples/ruby-example --verbose

# JSON output
feluda --path examples/ruby-example --json

# License compatibility against your project license
feluda --path examples/ruby-example --project-license MIT
```

## Expected Output

Feluda resolves each gem's license from RubyGems and flags any that are
restrictive or incompatible with your project license.
