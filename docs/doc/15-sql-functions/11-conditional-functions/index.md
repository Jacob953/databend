---
title: 'Conditional Functions'
---

SQL Conditional Functions Overview.

| Function                                                   | Description                                                                                                                                              | Example                                                                          | Result   |
|------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------|----------|
| **IF(cond1, expr1, [cond2, expr2, ...], expr_else)**       | If cond1 is TRUE, it returns expr1. Otherwise if cond2 is TRUE, it returns expr2, and so on.                                                             | **IF(1 > 2, 3, 4 < 5, 6, 7)**                                                    | 6        |
| **IFNULL(expr1, expr2)**                                   | Return expr1 if it is not NULL. Otherwise return expr2. They must have the same data type.                                                               | **IFNULL(0, NULL)**                                                              | 0        |
| **value [ NOT ] IN (value1, value2, ...)**                 | Check whether value is (or is not) one of the members of an explicit list.                                                                               | **1 not in (2, 3)**                                                              | 1(TRUE)  |
| **expr1 IS [ NOT ] DISTINCT FROM expr2**                   | Compares whether two expressions are equal (or not equal) with awareness of nullability, meaning it treats NULLs as known values for comparing equality. | **NULL is distinct from NULL**                                                   | 0(FALSE) |
| **IS_NOT_NULL(expr)**                                      | Check whether the value is not NULL.                                                                                                                     | **IS_NOT_NULL(1)**                                                               | 1(TRUE)  |
| **IS_NULL(expr)**                                          | Check whether the value is NULL.                                                                                                                         | **IS_NULL(1)**                                                                   | 0(FALSE) |
| **NULLIF(expr1, expr2)**                                   | Return NULL if two expressions are equal. Otherwise return expr1. They must have the same data type.                                                     | **NULLIF(0, NULL)**                                                              | 0        |
