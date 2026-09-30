#include <stdio.h>
#include "api.h"

typedef struct Pair {
    int left;
    int right;
} Pair;

enum mode { MODE_FAST, MODE_SLOW };

int add(int a, int b) {
    return a + b;
}

int twice(int value) {
    printf("%d\n", value);
    return add(value, value);
}
