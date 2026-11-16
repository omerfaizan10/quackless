-- Minimized fuzzer mismatch. Setup, query, then DuckDB's rows as JSON.
CREATE TABLE t1(a INTEGER, b INTEGER, c VARCHAR, d DOUBLE);
CREATE TABLE t2(a INTEGER, e INTEGER, f VARCHAR);
INSERT INTO t1 VALUES (8, NULL, NULL, 23.79), (13, 10, 'apple', -19.75), (-1, 7, 'apple', NULL), (9, 4, 'apple', -37.87), (8, 10, 'cherry', 90.00), (-1, 7, 'fig', 45.67), (NULL, 0, 'grape', -9.41), (5, NULL, 'cherry', NULL), (NULL, -4, 'kiwi', 121.08), (-3, 11, 'cherry', 21.12), (4, NULL, 'kiwi', 97.62), (6, 16, 'cherry', 92.54), (7, NULL, 'banana', 77.33), (-2, 1, 'apple', 17.19), (6, -5, 'grape', 129.28), (13, 2, NULL, -42.02), (6, 20, NULL, -8.09), (1, 3, 'kiwi', 38.41), (7, 13, 'grape', 123.51), (12, 13, NULL, 107.89), (13, -1, 'grape', 6.95), (0, 3, 'date', NULL), (-2, 8, 'apple', 24.88), (14, 12, 'date', 23.91), (1, 19, '', 143.12);
INSERT INTO t2 VALUES (NULL, 9, NULL), (12, 12, 'apple'), (NULL, 9, ''), (18, -5, NULL), (17, 12, 'Apple'), (NULL, 20, 'Apple'), (NULL, -2, 'cherry'), (11, 13, NULL), (-3, -1, 'apple'), (-4, 4, 'fig'), (9, 14, 'date'), (NULL, -4, 'fig'), (19, 13, ''), (NULL, 6, 'date'), (4, 17, 'Apple'), (18, 17, 'banana'), (9, 17, 'cherry'), (18, NULL, '');
-- query
SELECT c AS c0 FROM t1 WHERE ((6 IN (-5, (0 % b), a)) AND (a < 2));
-- expected
[]
