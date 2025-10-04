# Node.js Example Project

This is a sample Node.js project used for testing Feluda's license analysis capabilities.

## Dependencies

This project includes dependencies with transient (indirect) dependencies:
- **express**: Web framework (has transient dependencies like body-parser, cookie, etc.)
- **axios**: HTTP client (has transient dependencies like follow-redirects, form-data, etc.)
- **lodash**: Utility library (standalone, but good for testing)
- **moment**: Date manipulation library (has transient dependencies)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/node-example
```

Or from within the example directory:

```sh
cd examples/node-example
feluda
```

## Setup (Optional)

To actually run this example:

```sh
npm install
node index.js
```
