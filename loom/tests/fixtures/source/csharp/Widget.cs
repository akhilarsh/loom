using System;
using System.Collections.Generic;
using static System.Math;
using Alias = App.Core.Helper;

namespace App.Core
{
    public class Widget
    {
        public void Run()
        {
            this.Step();
            this.Ping();
            helper.Assist(1);
            Make().Go();
            Assist();
            new Widget();
            Convert<int>(1);
            helper.Cast<string>(2);
        }

        void Step() { }

        void Step(int n) { }

        void Ping() { }

        class Inner
        {
            void Pong()
            {
                this.Pong();
                Ping();
            }
        }
    }
}
