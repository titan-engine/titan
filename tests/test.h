#pragma once
/* Minimal test harness. Each test file is one executable:
 *
 *   static void test_something(void) {
 *       REQUIRE(ptr != nullptr);   // stops this test on failure
 *       CHECK(ptr->x == 2);        // records the failure and continues
 *   }
 *   int main(void) { RUN(test_something); return TEST_RESULT(); }
 */
#include <stdio.h>

static int test_failures;

#define CHECK(cond)                                                                  \
    do {                                                                             \
        if (!(cond)) {                                                               \
            fprintf(stderr, "%s:%d: CHECK(%s) failed\n", __FILE__, __LINE__, #cond); \
            test_failures++;                                                         \
        }                                                                            \
    } while (0)

/* Like CHECK, but returns from the current test function on failure. Use it before dereferencing. */
#define REQUIRE(cond)                                                                  \
    do {                                                                               \
        if (!(cond)) {                                                                 \
            fprintf(stderr, "%s:%d: REQUIRE(%s) failed\n", __FILE__, __LINE__, #cond); \
            test_failures++;                                                           \
            return;                                                                    \
        }                                                                              \
    } while (0)

#define RUN(fn)                                                                              \
    do {                                                                                     \
        int failures_before = test_failures;                                                 \
        fn();                                                                                \
        fprintf(stderr, "%s %s\n", test_failures == failures_before ? "ok  " : "FAIL", #fn); \
    } while (0)

#define TEST_RESULT() (test_failures == 0 ? 0 : 1)
