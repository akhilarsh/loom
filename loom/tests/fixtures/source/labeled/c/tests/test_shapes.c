#include "shapes.h"

int test_area(void)
{
    Shape s = {2, 3};
    return area(&s) == 6;
}
