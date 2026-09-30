package app.core;

import static app.util.Strings.shout;
import app.util.*;

public class Widget {
    public String name;

    public Widget(String name) {
        this.name = name;
    }

    public void run(Saver saver) {
        this.step(1);
        String label = shout(name);
        saver.save(this);
        ping();
        System.out.println(describe(label));
    }

    void step(int n) {
        ping();
    }

    void step(String s) {
        ping();
    }

    void ping() {
    }

    String describe(String label) {
        return Strings.clean(label);
    }

    class Inner {
        void ping() {
        }

        void pong() {
            ping();
        }
    }
}
