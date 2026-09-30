#include <stdio.h>
#include "shapes.h"

int scale(const Shape *s);
int current(void);

int main(void)
{
    Shape s = {3, 4};
    int a = area(&s);
    int b = scale(&s);
    printf("%d %d %d\n", a, b, current());
    return apply(&s, perimeter);
}
