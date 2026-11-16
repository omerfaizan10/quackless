-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (20, 9, NULL, 24.93), (16, -4, '', 1.61), (NULL, -4, 'banana', 28.40), (0, 19, 'grape', NULL), (-1, 11, 'fig', 115.83);
INSERT INTO t2 VALUES (0, NULL, 'banana'), (15, 8, NULL), (5, NULL, 'apple'), (-4, -1, 'date'), (7, -4, 'fig'), (3, 9, 'apple'), (NULL, 15, 'kiwi'), (18, 19, 'kiwi'), (19, 4, 'kiwi'), (13, 18, 'kiwi'), (3, NULL, NULL), (11, 10, 'grape'), (3, NULL, 'apple'), (7, 6, 'grape'), (8, 17, ''), (18, NULL, 'date');
-- query
SELECT 15 AS c0 FROM t1 WHERE ((a BETWEEN b AND 0) AND (-5 IN (b, (a % a), 15)));
-- expected
[]
