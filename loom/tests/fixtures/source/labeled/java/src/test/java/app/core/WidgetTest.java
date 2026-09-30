package app.core;

public class WidgetTest {
    public void runsWithDisk() {
        Widget widget = new Widget("w");
        widget.run(new DiskSaver());
    }
}
