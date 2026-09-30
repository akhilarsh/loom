#include <stdio.h>
#include "shapes.h"

static int clamp(int v)
{
    return v < 0 ? 0 : v;
}

int area(const Shape *s)
{
    return clamp(s->w) * clamp(s->h);
}

int perimeter(const Shape *s)
{
    printf("perimeter\n");
    return 2 * (clamp(s->w) + clamp(s->h));
}

int apply(const Shape *s, int (*op)(const Shape *))
{
    return op(s);
}
