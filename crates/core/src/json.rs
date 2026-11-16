//! Minimal JSON writer for results and plans (keeps serde out of the core crate).

use crate::exec::{OpStats, PhysNode};
use crate::plan::LogicalPlan;
use crate::types::Value;
use crate::QueryResult;
use std::fmt::Write;

pub fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(x: f64, out: &mut String) {
    if x.is_finite() {
        let _ = write!(out, "{}", (x * 1000.0).round() / 1000.0);
    } else {
        out.push_str("null");
    }
}

fn value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            // beyond 2^53 a JS number loses digits, so send it as text
            if i.unsigned_abs() < (1u64 << 53) {
                let _ = write!(out, "{}", i);
            } else {
                esc(&i.to_string(), out)
            }
        }
        Value::Float(f) => {
            if f.is_finite() {
                let _ = write!(out, "{}", f);
            } else {
                esc(&v.to_string(), out)
            }
        }
        other => esc(&other.to_string(), out),
    }
}

pub fn logical(p: &LogicalPlan, out: &mut String) {
    let (op, detail) = p.node_label();
    out.push_str("{\"op\":");
    esc(&op, out);
    out.push_str(",\"detail\":");
    esc(&detail, out);
    out.push_str(",\"children\":[");
    for (i, c) in p.children().into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        logical(c, out);
    }
    out.push_str("]}");
}

fn physical(n: &PhysNode, stats: &[OpStats], out: &mut String) {
    let st = stats.get(n.id).cloned().unwrap_or_default();
    let child_ms: f64 = n.children.iter().map(|c| stats.get(c.id).map(|s| s.incl_ms).unwrap_or(0.0)).sum();
    let _ = write!(out, "{{\"id\":{},\"op\":", n.id);
    esc(&n.name, out);
    out.push_str(",\"detail\":");
    esc(&n.detail, out);
    let _ = write!(out, ",\"rows\":{},\"batches\":{},\"ms\":", st.rows, st.batches);
    num(st.incl_ms, out);
    out.push_str(",\"self_ms\":");
    num((st.incl_ms - child_ms).max(0.0), out);
    out.push_str(",\"children\":[");
    for (i, c) in n.children.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        physical(c, stats, out);
    }
    out.push_str("]}");
}

/// The full result as JSON, with at most `max_rows` rows.
pub fn result(r: &QueryResult, max_rows: usize) -> String {
    let mut o = String::with_capacity(4096);
    o.push_str("{\"columns\":[");
    for (i, n) in r.names.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"name\":");
        esc(n, &mut o);
        o.push_str(",\"type\":");
        esc(r.types[i].name(), &mut o);
        o.push('}');
    }
    let _ = write!(o, "],\"row_count\":{},\"rows\":[", r.rows);
    for row in 0..r.rows.min(max_rows) {
        if row > 0 {
            o.push(',');
        }
        o.push('[');
        for (ci, c) in r.columns.iter().enumerate() {
            if ci > 0 {
                o.push(',');
            }
            value(&c.get(row), &mut o);
        }
        o.push(']');
    }
    o.push_str("],\"plan_before\":");
    match &r.plan_before {
        Some(p) => logical(p, &mut o),
        None => o.push_str("null"),
    }
    o.push_str(",\"plan_after\":");
    match &r.plan_after {
        Some(p) => logical(p, &mut o),
        None => o.push_str("null"),
    }
    o.push_str(",\"physical\":");
    match &r.physical {
        Some(p) => physical(p, &r.stats, &mut o),
        None => o.push_str("null"),
    }
    // trace: [t_ms, op, cumulative rows], thinned to at most 4000 events
    o.push_str(",\"trace\":[");
    let step = (r.trace.len() / 4000).max(1);
    let mut first = true;
    for (i, e) in r.trace.iter().enumerate() {
        if i % step != 0 && i + 1 != r.trace.len() {
            continue;
        }
        if !first {
            o.push(',');
        }
        first = false;
        o.push('[');
        num(e.t_ms, &mut o);
        let _ = write!(o, ",{},{}]", e.op, e.rows);
    }
    o.push_str("],\"optimizer\":[");
    for (i, (rule, msg)) in r.opt_log.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"rule\":");
        esc(rule, &mut o);
        o.push_str(",\"change\":");
        esc(msg, &mut o);
        o.push('}');
    }
    o.push_str("],\"timing\":{\"parse_ms\":");
    num(r.parse_ms, &mut o);
    o.push_str(",\"plan_ms\":");
    num(r.plan_ms, &mut o);
    o.push_str(",\"optimize_ms\":");
    num(r.optimize_ms, &mut o);
    o.push_str(",\"exec_ms\":");
    num(r.exec_ms, &mut o);
    o.push_str("},\"affected\":");
    match r.affected {
        Some(n) => {
            let _ = write!(o, "{}", n);
        }
        None => o.push_str("null"),
    }
    o.push('}');
    o
}
