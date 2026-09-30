namespace outer::inner {
    void helper() {}
}

namespace lib {
    class Engine {
    public:
        void start();
    };
}

void lib::Engine::start() {
    outer::inner::helper();
}

template <typename T>
T identity(T value) {
    return value;
}

void use() {
    identity<int>(1);
}
