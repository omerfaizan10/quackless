//! Scalar functions: dates, strings and math.

use crate::types::*;

fn str_arg(c: &Column) -> Result<Column, String> {
    if c.data_type() == DataType::Str {
        Ok(c.clone())
    } else {
        c.cast(DataType::Str)
    }
}

fn date_part(name: &str, d: i32) -> i64 {
    let (y, m, dd) = civil_from_days(d);
    match name {
        "year" => y as i64,
        "month" => m as i64,
        "day" => dd as i64,
        "quarter" => ((m - 1) / 3 + 1) as i64,
        // 1970-01-01 was a Thursday; Sunday = 0
        "dayofweek" => (d as i64 + 4).rem_euclid(7),
        "dayofyear" => (d - days_from_civil(y, 1, 1) + 1) as i64,
        "week" => {
            // ISO week number
            let wd = (d as i64 + 3).rem_euclid(7); // Monday = 0
            let thursday = d as i64 - wd + 3;
            let (ty, _, _) = civil_from_days(thursday as i32);
            (thursday - days_from_civil(ty, 1, 1) as i64) / 7 + 1
        }
        _ => 0,
    }
}

/// Postgres-style SUBSTRING(s FROM start FOR len), 1-based, characters not bytes.
pub fn substring(s: &str, start: i64, len: Option<i64>) -> String {
    let end = len.map(|l| start.saturating_add(l));
    let from = start.max(1);
    let mut out = String::new();
    for (k, ch) in s.chars().enumerate() {
        let pos = k as i64 + 1;
        if pos < from {
            continue;
        }
        if let Some(e) = end {
            if pos >= e {
                break;
            }
        }
        out.push(ch);
    }
    out
}

pub fn call(name: &str, args: &[Column], dtype: DataType, n: usize) -> Result<Column, String> {
    let argn = |k: usize| -> Result<&Column, String> { args.get(k).ok_or_else(|| format!("{}() expects more arguments", name)) };
    match name {
        "year" | "month" | "day" | "quarter" | "dayofweek" | "dayofyear" | "week" => {
            let c = argn(0)?;
            let c = if c.data_type() == DataType::Date { c.clone() } else { c.cast(DataType::Date)? };
            let ColumnData::Date(v) = &c.data else { unreachable!() };
            Ok(Column::with_validity(ColumnData::Int(v.iter().map(|d| date_part(name, *d)).collect()), c.validity.clone()))
        }
        "upper" | "lower" | "trim" | "ltrim" | "rtrim" | "reverse" => {
            let c = str_arg(argn(0)?)?;
            let ColumnData::Str(s) = &c.data else { unreachable!() };
            let mut out = StrVec::with_capacity(n, s.bytes.len());
            for x in s.iter() {
                let r = match name {
                    "upper" => x.to_uppercase(),
                    "lower" => x.to_lowercase(),
                    "trim" => x.trim().to_string(),
                    "ltrim" => x.trim_start().to_string(),
                    "rtrim" => x.trim_end().to_string(),
                    _ => x.chars().rev().collect(),
                };
                out.push(&r);
            }
            Ok(Column::with_validity(ColumnData::Str(out), c.validity.clone()))
        }
        "length" => {
            let c = str_arg(argn(0)?)?;
            let ColumnData::Str(s) = &c.data else { unreachable!() };
            Ok(Column::with_validity(ColumnData::Int(s.iter().map(|x| x.chars().count() as i64).collect()), c.validity.clone()))
        }
        "substring" => {
            let c = str_arg(argn(0)?)?;
            let start = argn(1)?.cast(DataType::Int)?;
            let len = match args.get(2) {
                Some(l) => Some(l.cast(DataType::Int)?),
                None => None,
            };
            let mut b = ColumnBuilder::new(DataType::Str, n);
            let ColumnData::Str(s) = &c.data else { unreachable!() };
            let ColumnData::Int(st) = &start.data else { unreachable!() };
            for i in 0..n {
                let lv = len.as_ref().map(|l| (l.is_valid(i), match &l.data { ColumnData::Int(v) => v[i], _ => 0 }));
                if !c.is_valid(i) || !start.is_valid(i) || matches!(lv, Some((false, _))) {
                    b.push_null();
                    continue;
                }
                b.push(&Value::Str(substring(s.get(i), st[i], lv.map(|x| x.1))));
            }
            Ok(b.finish())
        }
        "concat" => {
            let cols: Vec<Column> = args.iter().map(str_arg).collect::<Result<_, _>>()?;
            let mut b = ColumnBuilder::new(DataType::Str, n);
            for i in 0..n {
                let mut s = String::new();
                for c in &cols {
                    if c.is_valid(i) {
                        if let ColumnData::Str(v) = &c.data {
                            s.push_str(v.get(i));
                        }
                    }
                }
                b.push(&Value::Str(s));
            }
            Ok(b.finish())
        }
        "replace" => {
            let (a, f, t) = (str_arg(argn(0)?)?, str_arg(argn(1)?)?, str_arg(argn(2)?)?);
            let mut b = ColumnBuilder::new(DataType::Str, n);
            for i in 0..n {
                match (a.get(i), f.get(i), t.get(i)) {
                    (Value::Str(x), Value::Str(y), Value::Str(z)) => b.push(&Value::Str(x.replace(&y, &z))),
                    _ => b.push_null(),
                }
            }
            Ok(b.finish())
        }
        "starts_with" | "ends_with" | "contains" | "instr" | "strpos" => {
            let (a, p) = (str_arg(argn(0)?)?, str_arg(argn(1)?)?);
            let mut b = ColumnBuilder::new(dtype, n);
            for i in 0..n {
                match (a.get(i), p.get(i)) {
                    (Value::Str(x), Value::Str(y)) => b.push(&match name {
                        "starts_with" => Value::Bool(x.starts_with(&y)),
                        "ends_with" => Value::Bool(x.ends_with(&y)),
                        "contains" => Value::Bool(x.contains(&y)),
                        _ => Value::Int(x.find(&y).map(|k| x[..k].chars().count() as i64 + 1).unwrap_or(0)),
                    }),
                    _ => b.push_null(),
                }
            }
            Ok(b.finish())
        }
        "abs" | "ceil" | "floor" | "sign" | "sqrt" | "ln" | "log10" | "log" | "exp" => {
            let c = argn(0)?;
            match &c.data {
                ColumnData::Int(v) if matches!(name, "abs" | "ceil" | "floor" | "sign") => {
                    let out = v
                        .iter()
                        .map(|x| match name {
                            "abs" => x.wrapping_abs(),
                            "sign" => x.signum(),
                            _ => *x,
                        })
                        .collect();
                    Ok(Column::with_validity(ColumnData::Int(out), c.validity.clone()))
                }
                _ => {
                    let f = c.cast(DataType::Float)?;
                    let ColumnData::Float(v) = &f.data else { unreachable!() };
                    let out: Vec<f64> = v
                        .iter()
                        .map(|x| match name {
                            "abs" => x.abs(),
                            "ceil" => x.ceil(),
                            "floor" => x.floor(),
                            "sign" => x.signum(),
                            "sqrt" => x.sqrt(),
                            "ln" | "log" => x.ln(),
                            "log10" => x.log10(),
                            _ => x.exp(),
                        })
                        .collect();
                    let col = Column::with_validity(ColumnData::Float(out), f.validity.clone());
                    if dtype == DataType::Int { col.cast(DataType::Int) } else { Ok(col) }
                }
            }
        }
        "power" => {
            let a = argn(0)?.cast(DataType::Float)?;
            let p = argn(1)?.cast(DataType::Float)?;
            let mut b = ColumnBuilder::new(DataType::Float, n);
            for i in 0..n {
                match (a.get(i), p.get(i)) {
                    (Value::Float(x), Value::Float(y)) => b.push(&Value::Float(x.powf(y))),
                    _ => b.push_null(),
                }
            }
            Ok(b.finish())
        }
        "round" => {
            let c = argn(0)?;
            if c.data_type() == DataType::Int {
                return Ok(c.clone());
            }
            let digits = match args.get(1) {
                Some(d) => d.cast(DataType::Int)?.get(0).as_f64().unwrap_or(0.0) as i32,
                None => 0,
            };
            let f = c.cast(DataType::Float)?;
            let ColumnData::Float(v) = &f.data else { unreachable!() };
            let m = 10f64.powi(digits);
            let out = v.iter().map(|x| (x * m).round() / m).collect();
            Ok(Column::with_validity(ColumnData::Float(out), f.validity.clone()))
        }
        "coalesce" | "greatest" | "least" | "nullif" => {
            let dt = if dtype == DataType::Null { DataType::Int } else { dtype };
            let cols: Vec<Column> = args.iter().map(|c| if c.data_type() == dt { Ok(c.clone()) } else { c.cast(dt) }).collect::<Result<_, _>>()?;
            let mut b = ColumnBuilder::new(dt, n);
            for i in 0..n {
                match name {
                    "coalesce" => match cols.iter().find(|c| c.is_valid(i)) {
                        Some(c) => b.push_from(c, i),
                        None => b.push_null(),
                    },
                    "nullif" => {
                        let (x, y) = (cols[0].get(i), cols[1].get(i));
                        if !x.is_null() && !y.is_null() && x.total_cmp(&y) == std::cmp::Ordering::Equal {
                            b.push_null()
                        } else {
                            b.push(&x)
                        }
                    }
                    _ => {
                        let vals: Vec<Value> = cols.iter().map(|c| c.get(i)).filter(|v| !v.is_null()).collect();
                        let pick = if name == "greatest" {
                            vals.into_iter().max_by(|a, b| a.total_cmp(b))
                        } else {
                            vals.into_iter().min_by(|a, b| a.total_cmp(b))
                        };
                        b.push(&pick.unwrap_or(Value::Null))
                    }
                }
            }
            Ok(b.finish())
        }
        other => Err(format!("Catalog error: unknown function {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_semantics() {
        assert_eq!(substring("25-989-741", 1, Some(2)), "25");
        assert_eq!(substring("abc", 0, Some(2)), "a");
        assert_eq!(substring("abc", 2, None), "bc");
    }

    #[test]
    fn date_parts() {
        let d = parse_date("1995-03-15").unwrap();
        assert_eq!(date_part("year", d), 1995);
        assert_eq!(date_part("month", d), 3);
        assert_eq!(date_part("quarter", d), 1);
        assert_eq!(date_part("dayofweek", parse_date("2024-01-07").unwrap()), 0); // Sunday
    }
}
