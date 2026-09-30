<?php
namespace App\Support;

trait Loggable
{
    public function log(string $m): void
    {
        $this->format($m);
    }

    public function format(string $m): string
    {
        return $m;
    }
}
