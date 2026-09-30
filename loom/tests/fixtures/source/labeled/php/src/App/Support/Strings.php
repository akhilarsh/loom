<?php
namespace App\Support;

class Strings
{
    public static function shout(string $s): string
    {
        return strtoupper($s) . '!';
    }

    public static function clean(string $s): string
    {
        return trim($s);
    }
}
