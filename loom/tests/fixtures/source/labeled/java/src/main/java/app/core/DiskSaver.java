package app.core;

public class DiskSaver implements Saver {
    public void save(Widget widget) {
        System.out.println(widget.name);
    }
}
