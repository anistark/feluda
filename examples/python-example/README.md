# Python Example Project

This is a sample Python project used for testing Feluda's license analysis capabilities.

## Dependencies

This project includes dependencies with transient (indirect) dependencies:
- **flask**: Web framework (has transient dependencies like Werkzeug, Jinja2, click, etc.)
- **requests**: HTTP library (has transient dependencies like urllib3, certifi, charset-normalizer, etc.)
- **numpy**: Numerical computing library (has transient dependencies)
- **pytest**: Testing framework (has transient dependencies like pluggy, iniconfig, etc.)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/python-example
```

Or from within the example directory:

```sh
cd examples/python-example
feluda
```

## Setup (Optional)

To actually run this example:

```sh
pip install -r requirements.txt
python main.py
```
