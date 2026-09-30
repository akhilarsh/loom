<?php
namespace App\Contracts;

interface Saver
{
    public function save($widget): void;
}
