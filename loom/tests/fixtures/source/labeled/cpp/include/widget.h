#pragma once

namespace shapes {

using Count = int;

class Widget {
public:
    void run();
    void step(int n);
    void step(double d);
    void ping();
    Count value() const { return 1; }
};

template <typename T>
T twice(T x) {
    return x + x;
}

}  // namespace shapes
