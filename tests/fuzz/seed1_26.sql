-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (8, 7, 'banana', 74.85), (NULL, 10, 'cherry', NULL), (19, 12, 'banana', 76.67), (5, 17, 'fig', 44.68), (NULL, 6, 'kiwi', -3.06), (6, 0, 'kiwi', -48.64), (17, 10, '', 128.51), (1, 13, '', 3.08), (12, -2, NULL, -32.98), (14, NULL, '', 98.21), (-2, 10, 'fig', -31.62), (19, 3, '', 20.80), (2, 10, 'banana', 79.70), (NULL, 14, NULL, -17.50), (NULL, NULL, 'banana', 19.84), (9, 10, NULL, NULL), (16, 5, 'apple', 143.40), (14, -4, 'apple', -15.88), (9, 16, '', -22.72), (18, -1, NULL, 12.64), (9, 1, NULL, 97.78), (15, NULL, 'fig', 15.58), (20, 12, 'apple', 71.20), (-4, -1, 'grape', -27.07), (-4, 19, NULL, -36.62), (3, 14, 'fig', -4.73), (NULL, 17, 'apple', 46.43), (18, 6, 'cherry', 33.34);
INSERT INTO t2 VALUES (18, 0, 'fig'), (-4, 7, NULL), (19, 6, 'apple'), (-1, 14, 'fig');
-- query
SELECT (t1.a / (SELECT max(e) FROM t2 WHERE t2.e < b)) AS c0 FROM t1 JOIN t2 ON t1.a = t2.a;
-- expected
[[2.7142857142857144],["inf"],[null],[null],[-0.2857142857142857],["inf"]]
