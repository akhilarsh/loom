#include "widget.h"

class Widget {
public:
    void run() {
        this->step();
        step();
    }
    void step() {}
    void skip();
};

void Widget::skip() {
    this->step();
}
