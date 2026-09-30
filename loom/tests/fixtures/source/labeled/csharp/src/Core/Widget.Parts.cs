using App.Util;

namespace App.Core
{
    public partial class Widget
    {
        void Ping()
        {
        }

        void Audit()
        {
            Ping();
        }

        string Describe(string label)
        {
            return Strings.Clean(label);
        }

        class Inner
        {
            void Ping()
            {
            }

            void Pong()
            {
                Ping();
            }
        }
    }
}
