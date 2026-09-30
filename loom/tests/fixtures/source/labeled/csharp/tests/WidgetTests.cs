using App.Core;

namespace App.Tests
{
    public class WidgetTests
    {
        public void RunsWithDisk()
        {
            var widget = new Widget();
            widget.Run(new DiskSaver());
        }
    }
}
