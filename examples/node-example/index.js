const express = require('express');
const axios = require('axios');
const _ = require('lodash');
const moment = require('moment');

const app = express();

app.get('/', (req, res) => {
  res.json({
    message: 'Node.js example with transient dependencies',
    timestamp: moment().format()
  });
});

console.log('Node.js example with transient dependencies');
