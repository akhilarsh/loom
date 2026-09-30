struct ops {
    int (*apply)(int);
};

int run(struct ops *table, int value) {
    return table->apply(value);
}
