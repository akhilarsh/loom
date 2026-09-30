#include <string>
#include "widget.h"

namespace shapes {

void Widget::run() {
    this->ping();
    step(1);
    int t = twice<int>(2);
    std::string s = std::to_string(value());
}

void Widget::step(int n) {
    ping();
}

void Widget::step(double d) {
    this->ping();
}

void Widget::ping() {
}

}  // namespace shapes
