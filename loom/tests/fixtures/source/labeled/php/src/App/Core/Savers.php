<?php
namespace App\Core;

class DiskSaver implements \App\Contracts\Saver
{
    public function save($widget): void
    {
        file_put_contents('saved.txt', 'x');
    }
}

class MemSaver implements \App\Contracts\Saver
{
    private array $seen = [];

    public function save($widget): void
    {
        array_push($this->seen, 1);
    }
}
