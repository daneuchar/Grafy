const express = require('express');
const app = express();

function getUser(req, res) {
    res.json({ id: req.params.id });
}

function createUser(req, res) {
    res.json({ created: true });
}

app.get('/users/:id', getUser);
app.post('/users', createUser);
