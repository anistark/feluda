# Configuration file for the Sphinx documentation builder.
#
# For the full list of built-in configuration values, see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Project information -----------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#project-information

project = 'Feluda'
copyright = '2025, The Feluda Maintainers'
author = 'The Feluda Maintainers'
release = 'v1.10.0'

# -- General configuration ---------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#general-configuration

extensions = [
    "sphinx_design",
    "sphinx_iconify",
    "sphinx_tabs.tabs"
]

templates_path = ['_templates']
exclude_patterns = []

language = 'en'

# -- Options for linkcheck ---------------------------------------------------
linkcheck_ignore = [
    r'https://crates\.io/',  # Bot protection causes false 404s
    r'https://api\.opensource\.org/',  # Frequent timeouts
]

# -- Options for HTML output -------------------------------------------------
# https://www.sphinx-doc.org/en/master/usage/configuration.html#options-for-html-output

html_theme = 'shibuya'
html_static_path = ['_static']
html_logo = '_static/felu.png'
html_favicon = '_static/favicon.png'
html_title = 'Feluda'

html_theme_options = {
    "logo_target": "/",
    "crate_url": "https://crates.io/crates/feluda",
    "github_url": "https://github.com/anistark/feluda",
    "discord_url": "https://discord.gg/5YrbwNRGaE",
}

def setup(app):
    app.add_css_file('custom.css')
    app.add_js_file('custom.js')
