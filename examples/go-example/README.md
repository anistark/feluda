# Go Example Project

This is a sample Go project used for testing Feluda's license analysis capabilities.

## Dependencies

This project includes dependencies with transient (indirect) dependencies:
- **gin-gonic/gin**: Web framework (has transient dependencies like go-playground/validator, etc.)
- **spf13/cobra**: CLI framework (has transient dependencies like spf13/pflag, etc.)
- **stretchr/testify**: Testing toolkit (has transient dependencies like davecgh/go-spew, etc.)
- **uber-go/zap**: Logging library (has transient dependencies like uber-go/multierr, etc.)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/go-example
```

Or from within the example directory:

```sh
cd examples/go-example
feluda
```

## Setup (Optional)

To actually run this example:

```sh
go mod download
go run main.go
```
