//! W4 routes pass integration tests.
//!
//! Per-framework fixture tests that exercise the route extractor. Each test:
//!   - Indexes one of the fixture services under `tests/fixtures/routes/`.
//!   - Asserts `report.routes >= 2`.
//!   - Re-opens the redb store and asserts at least one `EdgeKind::Routes` edge
//!     whose source node has `NodeKind::Route`.
//!
//! One cross-cutting test indexes all three fixture services together and
//! asserts `routes >= 6`.

use std::fs;

use grafy::pipeline::Pipeline;
use grafy::store::{EdgeKind, NodeKind, NodeRecord, EDGES_TABLE, NODES_TABLE};
use postcard::from_bytes;
use redb::{Database, ReadableTable};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return (route_node_count, routes_edge_count) from the redb store at `db_path`.
fn count_routes(db_path: &std::path::Path) -> (u64, u64) {
    let db = Database::open(db_path).expect("open redb");
    let tx = db.begin_read().expect("begin read");

    let nodes_tbl = tx.open_table(NODES_TABLE).expect("open nodes table");
    let mut route_nodes = 0u64;
    for item in nodes_tbl.iter().expect("iter nodes") {
        let (_k, v) = item.expect("node item");
        if let Ok(rec) = from_bytes::<NodeRecord>(v.value()) {
            if rec.kind == NodeKind::Route {
                route_nodes += 1;
            }
        }
    }

    let edges_tbl = tx.open_table(EDGES_TABLE).expect("open edges table");
    let mut route_edges = 0u64;
    for item in edges_tbl.iter().expect("iter edges") {
        let (key, _val) = item.expect("edge item");
        let (_from, _to, kind) = key.value();
        if kind == EdgeKind::Routes as u8 {
            route_edges += 1;
        }
    }

    (route_nodes, route_edges)
}

// ---------------------------------------------------------------------------
// FastAPI fixture
// ---------------------------------------------------------------------------

#[test]
fn w4_fastapi_routes() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("main.py"),
        r#"from fastapi import FastAPI

app = FastAPI()

@app.get("/users/{id}")
def get_user(id: int):
    return {"id": id}

@app.post("/users")
def create_user():
    return {"created": True}
"#,
    )
    .expect("write main.py");

    let report = Pipeline::new(root).index().expect("index");
    assert!(
        report.routes >= 2,
        "expected ≥2 route nodes for FastAPI fixture, got {}. Report={report:?}",
        report.routes
    );

    let db_path = root.join(".grafy").join("index.redb");
    let (route_nodes, route_edges) = count_routes(&db_path);
    assert!(
        route_nodes >= 2,
        "expected ≥2 Route nodes in store, got {route_nodes}"
    );
    assert!(
        route_edges >= 1,
        "expected ≥1 Routes edge in store, got {route_edges}"
    );
}

// ---------------------------------------------------------------------------
// Gin fixture
// ---------------------------------------------------------------------------

#[test]
fn w4_gin_routes() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/gin-gonic/gin"

func getUser(c *gin.Context) {
    c.JSON(200, gin.H{"id": c.Param("id")})
}

func createUser(c *gin.Context) {
    c.JSON(201, gin.H{"created": true})
}

func main() {
    r := gin.Default()
    r.GET("/users/:id", getUser)
    r.POST("/users", createUser)
}
"#,
    )
    .expect("write main.go");

    let report = Pipeline::new(root).index().expect("index");
    assert!(
        report.routes >= 2,
        "expected ≥2 route nodes for Gin fixture, got {}. Report={report:?}",
        report.routes
    );

    let db_path = root.join(".grafy").join("index.redb");
    let (route_nodes, route_edges) = count_routes(&db_path);
    assert!(
        route_nodes >= 2,
        "expected ≥2 Route nodes in store, got {route_nodes}"
    );
    assert!(
        route_edges >= 1,
        "expected ≥1 Routes edge in store, got {route_edges}"
    );
}

// ---------------------------------------------------------------------------
// Express fixture
// ---------------------------------------------------------------------------

#[test]
fn w4_express_routes() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("app.js"),
        r#"const express = require('express');
const app = express();

function getUser(req, res) {
    res.json({ id: req.params.id });
}

function createUser(req, res) {
    res.json({ created: true });
}

app.get('/users/:id', getUser);
app.post('/users', createUser);
"#,
    )
    .expect("write app.js");

    let report = Pipeline::new(root).index().expect("index");
    assert!(
        report.routes >= 2,
        "expected ≥2 route nodes for Express fixture, got {}. Report={report:?}",
        report.routes
    );

    let db_path = root.join(".grafy").join("index.redb");
    let (route_nodes, route_edges) = count_routes(&db_path);
    assert!(
        route_nodes >= 2,
        "expected ≥2 Route nodes in store, got {route_nodes}"
    );
    assert!(
        route_edges >= 1,
        "expected ≥1 Routes edge in store, got {route_edges}"
    );
}

// ---------------------------------------------------------------------------
// Cross-cutting: all three frameworks together
// ---------------------------------------------------------------------------

#[test]
fn w4_all_frameworks_combined() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // FastAPI service.
    fs::write(
        root.join("fastapi_app.py"),
        r#"from fastapi import FastAPI

app = FastAPI()

@app.get("/items/{id}")
def get_item(id: int):
    return {"id": id}

@app.post("/items")
def create_item():
    return {"created": True}
"#,
    )
    .expect("write fastapi_app.py");

    // Gin service.
    fs::write(
        root.join("gin_main.go"),
        r#"package main

import "github.com/gin-gonic/gin"

func getItem(c *gin.Context) {}
func createItem(c *gin.Context) {}

func main() {
    r := gin.Default()
    r.GET("/items/:id", getItem)
    r.POST("/items", createItem)
}
"#,
    )
    .expect("write gin_main.go");

    // Express service.
    fs::write(
        root.join("express_app.js"),
        r#"const express = require('express');
const app = express();

function getItem(req, res) {}
function createItem(req, res) {}

app.get('/items/:id', getItem);
app.post('/items', createItem);
"#,
    )
    .expect("write express_app.js");

    let report = Pipeline::new(root).index().expect("index");
    assert!(
        report.routes >= 6,
        "expected ≥6 route nodes across all three frameworks, got {}. Report={report:?}",
        report.routes
    );
}
