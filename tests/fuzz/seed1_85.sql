-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (9, 15, 'grape', 46.29), (5, 16, 'Apple', 19.07), (0, 14, NULL, 85.72), (4, 2, 'fig', 13.74), (3, -4, 'banana', NULL), (13, NULL, 'fig', 118.75), (13, -4, 'grape', -17.94), (-1, 16, 'fig', -43.97), (15, 3, 'fig', 144.13), (16, -1, 'grape', 98.37), (6, -4, 'apple', -23.06), (NULL, 8, 'fig', 104.04), (15, 13, 'banana', 105.48), (4, 6, NULL, 108.14), (-1, 2, 'fig', -42.61), (-4, 10, 'Apple', -41.58);
INSERT INTO t2 VALUES (13, 11, 'apple'), (NULL, 2, 'banana'), (13, 10, 'kiwi'), (NULL, 14, 'fig'), (-2, 11, 'cherry'), (-5, -5, 'Apple'), (NULL, NULL, 'date'), (12, NULL, NULL), (11, -4, 'banana'), (18, 4, 'grape'), (NULL, -1, ''), (1, 2, 'Apple'), (-5, NULL, 'date'), (17, NULL, 'apple'), (NULL, 18, 'banana'), (-4, 6, 'cherry'), (NULL, 11, ''), (0, 0, 'Apple'), (-2, 8, 'banana');
-- query
SELECT count(coalesce(b, (b % e))) AS c0 FROM t1, t2;
-- expected
[[285]]
