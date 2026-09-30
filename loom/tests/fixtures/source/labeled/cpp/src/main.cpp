#include <vector>
#include "shapes.h"
#include "widget.h"

using shapes::Widget;

namespace {

int helper(int v) {
    return v + 1;
}

}  // namespace

int shapes::total(const Shape &s) {
    return s.area();
}

int main() {
    Widget w;
    w.run();
    std::vector<int> v;
    v.push_back(helper(1));
    int n = v.size();
    return shapes::twice(n);
}
