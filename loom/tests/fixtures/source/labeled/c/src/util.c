#include "shapes.h"

static int clamp(int v)
{
    return v > 100 ? 100 : v;
}

int scale(const Shape *s)
{
    return clamp(area(s));
}

#ifdef FAST
int mode(void)
{
    return 1;
}
#else
int mode(void)
{
    return 0;
}
#endif

int current(void)
{
    return mode();
}
