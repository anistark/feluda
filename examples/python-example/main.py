#!/usr/bin/env python3

from flask import Flask, jsonify
import requests
import numpy as np

app = Flask(__name__)

@app.route('/')
def hello():
    return jsonify({
        'message': 'Python example with transient dependencies',
        'numpy_version': np.__version__
    })

if __name__ == '__main__':
    print('Python example with transient dependencies')
    app.run(debug=True)
