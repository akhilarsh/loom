using System;
using System.Collections.Generic;

namespace App.Core
{
    public class DiskSaver : ISaver
    {
        public void Save(Widget widget)
        {
            Console.WriteLine(widget.Name);
        }
    }

    public class MemSaver : ISaver
    {
        private readonly List<string> seen = new List<string>();

        public void Save(Widget widget)
        {
            seen.Add(widget.Name);
        }
    }
}
