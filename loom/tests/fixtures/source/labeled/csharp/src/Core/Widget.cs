using System;
using App.Util;
using S = App.Util.Strings;

namespace App.Core
{
    public partial class Widget
    {
        public string Name;

        public void Run(ISaver saver)
        {
            Step(1);
            this.Ping();
            var label = S.Shout(Name);
            saver.Save(this);
            Console.WriteLine(Describe(label));
            Audit();
        }

        void Step(int n)
        {
            this.Ping();
        }

        void Step(string s)
        {
            this.Ping();
        }
    }
}
