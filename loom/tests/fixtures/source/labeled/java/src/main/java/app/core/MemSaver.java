package app.core;

import java.util.ArrayList;
import java.util.List;

public class MemSaver implements Saver {
    private final List<String> seen = new ArrayList<>();

    public void save(Widget widget) {
        seen.add(widget.name);
    }
}
