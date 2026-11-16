-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (19, NULL, 'Apple', 80.03), (2, 1, 'kiwi', NULL), (13, -3, 'date', 109.30), (1, -5, '', -41.88), (-4, NULL, 'cherry', NULL), (3, 8, 'banana', 38.78), (-4, 7, 'banana', NULL), (-5, 12, 'apple', -28.16), (-1, 13, 'banana', NULL), (20, -1, 'date', 116.61), (19, NULL, 'grape', 105.54), (8, 15, 'apple', -41.48), (11, -2, 'fig', 56.55), (5, 7, 'date', NULL), (8, NULL, 'apple', 75.96);
INSERT INTO t2 VALUES (-4, 11, 'Apple'), (15, 8, ''), (-5, 9, 'Apple'), (15, 18, 'date'), (NULL, NULL, 'Apple'), (2, 0, 'cherry'), (4, 8, NULL), (8, 3, NULL), (-5, NULL, 'date'), (10, 20, 'fig'), (NULL, NULL, 'Apple'), (-4, -3, 'apple'), (17, -3, 'date'), (-3, 17, 'kiwi'), (8, 7, 'banana'), (6, -4, NULL), (-3, 14, ''), (11, 0, 'date'), (20, NULL, 'Apple'), (-3, 1, 'Apple'), (11, 4, 'kiwi');
-- query
SELECT 1.5 AS c0 FROM t2 WHERE ((SELECT max(e) FROM t2 WHERE t2.e < -5) = (e % e));
-- expected
[]
