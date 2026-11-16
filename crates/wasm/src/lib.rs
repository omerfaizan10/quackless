//! WebAssembly bindings for the browser workbench.

use quackless_core::{json, Database, Options};
use wasm_bindgen::prelude::*;

/// Row pairs a nested-loop join may compare per query in the browser.
pub const PAIR_BUDGET: u64 = 10_000_000;

#[wasm_bindgen]
pub struct Engine {
    db: Database,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        let mut db = Database::new();
        // well under a second of nested-loop work, so a cross product cannot hang the page
        db.pair_budget = Some(PAIR_BUDGET);
        Engine { db }
    }

    /// Run DDL (CREATE TABLE ...) or any statements; returns nothing on success.
    pub fn execute_script(&mut self, sql: &str) -> Result<(), JsError> {
        self.db.execute_script(sql, Options::default()).map(|_| ()).map_err(|e| JsError::new(&e))
    }

    /// Load CSV bytes (with a header row) into an existing table. Returns the row count.
    pub fn load_csv(&mut self, table: &str, bytes: &[u8]) -> Result<usize, JsError> {
        self.db.load_csv(table, bytes).map_err(|e| JsError::new(&e))
    }

    /// Run one statement. Returns the result, plans, per-operator stats and the
    /// execution trace as JSON (at most `max_rows` rows).
    pub fn query(&mut self, sql: &str, optimize: bool, max_rows: usize) -> Result<String, JsError> {
        let opts = Options { optimize, int_div: false, record: true };
        let r = self.db.execute(sql, opts).map_err(|e| JsError::new(&e))?;
        Ok(json::result(&r, max_rows))
    }

    /// Tables with row counts and column types, as JSON.
    pub fn tables(&self) -> String {
        let mut out = String::from("[");
        for (i, (name, rows, cols)) in self.db.tables().into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            json::esc(&name, &mut out);
            out.push_str(&format!(",\"rows\":{},\"columns\":[", rows));
            for (k, (c, t)) in cols.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                out.push('[');
                json::esc(c, &mut out);
                out.push(',');
                json::esc(t.name(), &mut out);
                out.push(']');
            }
            out.push_str("]}");
        }
        out.push(']');
        out
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
