-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (18, -5, 'apple', NULL), (NULL, 7, 'apple', -7.39), (10, 11, 'kiwi', 69.23), (7, 8, 'banana', -21.45), (4, NULL, 'apple', 143.38), (12, 0, NULL, 89.86), (-4, 5, 'cherry', NULL), (NULL, 4, 'date', -38.95), (19, NULL, 'grape', 35.73), (19, 7, 'Apple', 21.80), (11, 0, 'fig', 30.15), (14, 14, NULL, -19.40), (3, 14, '', 49.87), (9, NULL, '', 88.81), (7, 0, 'date', 63.40);
INSERT INTO t2 VALUES (20, 17, 'kiwi'), (-3, 5, NULL), (16, NULL, NULL), (-4, 13, 'Apple'), (NULL, 13, 'fig'), (15, -2, 'grape'), (8, 1, 'date'), (19, -2, NULL), (NULL, 13, 'fig'), (-2, 16, 'kiwi'), (3, 17, 'fig'), (NULL, NULL, 'kiwi'), (NULL, 10, NULL), (5, 3, 'Apple'), (11, NULL, 'banana'), (-4, -1, NULL), (6, -3, 'kiwi'), (15, 14, ''), (11, 3, 'grape');
-- query
SELECT count(*) AS c0 FROM t1 LEFT JOIN t2 ON t1.a = t2.a WHERE (t2.a = (SELECT max(e) FROM t2 WHERE t2.e < (SELECT max(e) FROM t2 WHERE t2.e < b)));
-- expected
[[0]]
