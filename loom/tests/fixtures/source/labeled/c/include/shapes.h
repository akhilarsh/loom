#ifndef SHAPES_H
#define SHAPES_H

typedef struct Shape {
    int w;
    int h;
} Shape;

int area(const Shape *s);
int perimeter(const Shape *s);
int apply(const Shape *s, int (*op)(const Shape *));

#endif
