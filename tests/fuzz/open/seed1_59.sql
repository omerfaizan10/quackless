-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (8, -4, 'Apple', 53.53), (19, 20, NULL, 103.19), (16, -3, 'Apple', -31.05), (NULL, -1, 'grape', -32.80), (NULL, 7, 'Apple', 89.71), (3, 12, 'fig', NULL), (10, 3, 'date', 11.24), (10, 14, 'banana', 94.83), (-4, 1, NULL, NULL), (NULL, 16, 'grape', 78.39), (-1, 6, NULL, 145.43), (-4, 17, 'kiwi', 51.75), (2, 19, 'fig', NULL), (-2, 16, NULL, 57.62), (6, 13, 'banana', 145.57), (0, -1, 'date', NULL), (NULL, -5, 'kiwi', 99.88), (9, 17, 'cherry', -39.82), (18, 5, 'kiwi', 129.34), (NULL, 7, '', 3.29), (17, 19, NULL, -17.84), (-1, -5, 'kiwi', -33.89), (11, 4, 'cherry', 118.78), (20, 8, 'banana', 144.10), (5, NULL, 'cherry', NULL), (14, 11, 'Apple', -28.80), (11, 9, '', -42.16), (3, 18, 'kiwi', 87.04), (17, 12, NULL, 41.63), (NULL, NULL, 'grape', 32.05), (9, 8, 'date', -36.28), (4, 13, 'apple', -41.75);
INSERT INTO t2 VALUES (15, -2, 'apple'), (6, 3, 'date'), (-5, 15, NULL), (1, 18, 'grape'), (11, 18, 'fig'), (NULL, 5, 'fig'), (NULL, NULL, 'kiwi'), (5, 7, 'apple'), (0, 9, 'apple'), (17, 16, 'Apple'), (7, 14, 'cherry'), (15, NULL, 'banana'), (11, -3, 'kiwi'), (15, 18, 'grape');
-- query
SELECT (SELECT max(e) FROM t2 WHERE t2.e < (SELECT max(e) FROM t2 WHERE t2.e < t1.a)) AS c0 FROM t1 LEFT JOIN t2 ON t1.a = t2.a;
-- expected
[[-3],[-3],[-3],[-3],[-2],[-2],[3],[5],[5],[7],[7],[7],[14],[15],[15],[16],[16],[-3],[-3],[5],[7],[7],[15],[7],[7],[null],[null],[null],[null],[null],[null],[null],[null],[null]]
