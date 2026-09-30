namespace App.Util;

public static class Strings
{
    public static string Shout(string s)
    {
        return s.ToUpper() + "!";
    }

    public static string Clean(string s)
    {
        return s.Trim();
    }
}
