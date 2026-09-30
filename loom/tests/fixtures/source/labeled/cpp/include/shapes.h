#pragma once

namespace shapes {

class Shape {
public:
    virtual int area() const = 0;
};

class Circle : public Shape {
public:
    int area() const override { return 3; }
};

class Square : public Shape {
public:
    int area() const override { return 4; }
};

int total(const Shape &s);

}  // namespace shapes
