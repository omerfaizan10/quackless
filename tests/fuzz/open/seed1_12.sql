-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (17, 3, 'fig', -43.61), (-1, 6, '', 142.61), (12, -2, '', -2.59), (-2, 20, 'fig', 57.88), (NULL, 12, 'grape', 112.78), (2, NULL, 'fig', NULL), (NULL, 17, 'fig', NULL), (-2, 20, 'grape', 61.44), (1, 0, 'banana', -2.44), (7, 19, NULL, 100.18), (-5, NULL, 'apple', -3.17), (8, 2, NULL, 18.18), (19, 18, '', 105.15), (2, 12, 'banana', 0.58), (10, 0, 'banana', -20.15), (13, 6, NULL, 132.87), (NULL, 2, 'date', -26.55), (20, 8, 'banana', 1.22), (12, NULL, 'cherry', 132.14), (13, 2, 'apple', 63.27), (12, -5, 'cherry', 88.18), (4, -1, 'date', 72.42), (0, 12, 'fig', 13.25), (19, -3, 'banana', 136.61), (NULL, 13, NULL, -32.43), (6, -3, 'banana', 129.22);
INSERT INTO t2 VALUES (NULL, 11, 'grape'), (5, NULL, 'Apple'), (5, 0, NULL), (6, 11, 'banana'), (-1, 12, 'grape'), (20, 9, 'kiwi'), (20, 14, 'grape'), (NULL, -2, 'kiwi'), (NULL, -3, NULL), (16, 11, 'apple'), (8, 19, 'kiwi'), (10, 13, 'grape'), (17, 13, NULL);
-- query
SELECT TRUE AS c0 FROM t1 WHERE (-1 BETWEEN b AND (1 % a));
-- expected
[[true],[true],[true],[true],[true]]
