package app.core;

import java.util.List;
import java.util.*;
import static app.util.Helpers.assist;

public class Widget {
    public void run() {
        this.step();
        this.ping();
        helper.assist(1);
        make().go();
        assist();
        new Widget();
    }

    void step() {}

    void step(int n) {}

    void ping() {}

    static class Inner {
        void pong() {
            this.pong();
            ping();
        }
    }
}
