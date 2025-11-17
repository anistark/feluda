# Variables
CRATE_NAME := "feluda"
VERSION := `cargo pkgid | cut -d# -f2 | cut -d: -f2`
GITHUB_REPO := "anistark/feluda"

# Setup development environment
setup:
    @echo "🔧 Setting up development environment..."
    @echo ""
    @echo "📝 Making hooks executable..."
    chmod +x .githooks/*
    @echo "✅ Hooks are now executable"
    @echo ""
    @echo "⚙️  Configuring git hooks path..."
    git config core.hooksPath .githooks
    @echo "✅ Git configured to use .githooks"
    @echo ""
    @echo "🎉 Setup complete!"
    @echo ""
    @echo "You can now:"
    @echo "  • Commit code (pre-commit checks will run automatically)"
    @echo "  • Run 'just test-ci' anytime to check before committing"
    @echo ""

# Build the crate
build: format lint test
    @echo "🚀 Building release version..."
    cargo build --release

# Create the crate package (to validate before publishing)
package:
    @echo "📦 Creating package for validation..."
    cargo package

# Test the release build
test-release:
    @echo "🧪 Testing the release build..."
    cargo test --release

# Create a release on GitHub
gh-release:
    @echo "📢 Creating GitHub release for version v{{VERSION}}"
    gh release create v{{VERSION}}

# Release the crate to Homebrew
homebrew-release:
    @echo "🍺 Releasing {{CRATE_NAME}} to Homebrew..."
    brew tap-new {{GITHUB_REPO}}
    brew create --tap {{GITHUB_REPO}} https://github.com/{{GITHUB_REPO}}/archive/refs/tags/{{VERSION}}.tar.gz
    brew install --build-from-source {{GITHUB_REPO}}/{{CRATE_NAME}} --formula

# Release the crate to Debian APT
debian-release:
    @echo "📦 Releasing {{CRATE_NAME}} to Debian APT..."
    debmake -b -u {{VERSION}} -n {{CRATE_NAME}}
    dpkg-buildpackage -us -uc
    dput ppa:your-ppa-name ../{{CRATE_NAME}}_{{VERSION}}_source.changes

# Publish the crate to crates.io
publish RELEASE_TYPE="": build test-release package
    cargo publish
    @if [ -z "{{RELEASE_TYPE}}" ]; then git tag v{{VERSION}}; else git tag v{{VERSION}}-{{RELEASE_TYPE}}; fi
    @if [ -z "{{RELEASE_TYPE}}" ]; then git push origin v{{VERSION}}; else git push origin v{{VERSION}}-{{RELEASE_TYPE}}; fi

# Clean up the build artifacts
clean:
    @echo "🧹 Cleaning up build artifacts..."
    cargo clean

# Login to crates.io
login:
    @echo "🔑 Logging in to crates.io..."
    cargo login

# Run unit tests
test:
    @echo "🧪 Running unit tests..."
    cargo test

# Format code and check for lint issues
format:
    @echo "🎨 Formatting code with rustfmt..."
    cargo fmt --all
    @echo "✅ Format complete!"

# Check for lint issues without making changes
lint:
    @echo "🧹 Cleaning build artifacts to mimic CI..."
    cargo clean
    @echo "🔍 Checking code style with rustfmt..."
    cargo fmt --all -- --check
    @echo "🔬 Running clippy lints..."
    cargo clippy --all-targets --all-features -- -D warnings

# Run all checks before submitting code
check-all: format lint test
    @echo "🎉 All checks passed! Code is ready for submission."

# Run benchmarks
bench:
    @echo "⏱️ Running benchmarks..."
    cargo bench

# Run example projects for testing
examples:
    @echo "🧪 Running example projects for testing..."
    @echo "\n📦 Rust Example:"
    cargo run --example rust-example
    @echo "\n📦 Node.js Example:"
    @echo "Run: feluda --path examples/node-example"
    @echo "\n📦 Go Example:"
    @echo "Run: feluda --path examples/go-example"
    @echo "\n📦 Python Example:"
    @echo "Run: feluda --path examples/python-example"
    @echo "\n📦 C Example:"
    @echo "Run: feluda --path examples/c-example"
    @echo "\n📦 C++ Example:"
    @echo "Run: feluda --path examples/cpp-example"

# Test Feluda on all example projects
test-examples:
    @echo "🧪 Testing Feluda on all example projects..."
    @echo "\n📦 Testing Rust Example:"
    ./target/debug/feluda --path examples/rust-example || cargo run -- --path examples/rust-example
    @echo "\n📦 Testing Node.js Example:"
    ./target/debug/feluda --path examples/node-example || cargo run -- --path examples/node-example
    @echo "\n📦 Testing Go Example:"
    ./target/debug/feluda --path examples/go-example || cargo run -- --path examples/go-example
    @echo "\n📦 Testing Python Example:"
    ./target/debug/feluda --path examples/python-example || cargo run -- --path examples/python-example
    @echo "\n📦 Testing C Example:"
    ./target/debug/feluda --path examples/c-example || cargo run -- --path examples/c-example
    @echo "\n📦 Testing C++ Example:"
    ./target/debug/feluda --path examples/cpp-example || cargo run -- --path examples/cpp-example

# Mimic CI checks exactly as they run on GitHub Actions
test-ci:
    @echo "🔍 Running CI checks locally (format, lint, test)..."
    @echo "\n📋 1️⃣ Format check..."
    cargo fmt --all -- --check
    @echo "\n✅ Format check passed!"
    @echo "\n🔬 2️⃣ Clippy linting (with warnings as errors)..."
    cargo clippy --all-targets --all-features -- -D warnings
    @echo "\n✅ Clippy check passed!"
    @echo "\n🧪 3️⃣ Running all tests..."
    cargo test
    @echo "\n✅ All tests passed!"
    @echo "\n🎉 All CI checks passed! Ready for submission."
